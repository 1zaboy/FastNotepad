mod find_ui;
mod print_ui;

use std::cell::RefCell;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use anyhow::Result;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetDC, ReleaseDC, ScreenToClient, PAINTSTRUCT,
};
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesExW, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, FileOpenDialog, FileSaveDialog, IFileOpenDialog,
    IFileSaveDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, HDROP, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallWindowProcW, CheckMenuItem, CreateMenu, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos, GetMessageW,
    GetScrollInfo, GetWindowLongPtrW, LoadCursorW, MessageBoxW, MoveWindow, PostMessageW,
    PostQuitMessage, RegisterClassW, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
    GWLP_WNDPROC, HMENU, HTCLIENT, IDC_ARROW, IDC_HAND, IDC_IBEAM, IDCANCEL, IDNO, IDYES,
    MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO, MB_YESNOCANCEL,
    MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, SB_BOTTOM,
    SB_CTL, SB_LEFT, SB_LINEDOWN, SB_LINELEFT, SB_LINERIGHT, SB_LINEUP, SB_PAGEDOWN, SB_PAGELEFT,
    SB_PAGERIGHT, SB_PAGEUP, SB_RIGHT, SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP, SBS_HORZ, SBS_VERT,
    SCROLLINFO, SIF_PAGE, SIF_POS, SIF_RANGE, SIF_TRACKPOS, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DROPFILES, WM_ERASEBKGND,
    WM_HSCROLL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_TIMER, WM_VSCROLL, WNDCLASSW,
    WNDPROC, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use fast_notepad::buffer::LineEnding;
use fast_notepad::file_io::{build_line_index_incremental, open_file, save_document};
use fast_notepad::language::{LanguageId, LanguageRegistry, FORMAT_MAX_BYTES};
use fast_notepad::settings::AppSettings;
use fast_notepad::tabs::{
    close_button_hit, tab_at_x, Tab, SCROLLBAR_THICKNESS, STATUS_HEIGHT, TAB_HEIGHT,
};
use fast_notepad::view::{find_next, replace_all, replace_next, ViewState};
use crate::render::{invalidate, invalidate_editor, measure_metrics, status_text, Renderer};

const ID_FILE_NEW: usize = 1001;
const ID_FILE_OPEN: usize = 1002;
const ID_FILE_SAVE: usize = 1003;
const ID_FILE_SAVE_AS: usize = 1004;
const ID_FILE_PRINT: usize = 1005;
const ID_FILE_EXIT: usize = 1006;
const ID_FILE_CLOSE_TAB: usize = 1007;
const ID_FILE_NEW_TAB: usize = 1008;
const ID_RECENT_BASE: usize = 1200;
const ID_EDIT_UNDO: usize = 1101;
const ID_EDIT_REDO: usize = 1102;
const ID_EDIT_CUT: usize = 1103;
const ID_EDIT_COPY: usize = 1104;
const ID_EDIT_PASTE: usize = 1105;
const ID_EDIT_SELECT_ALL: usize = 1106;
const ID_EDIT_FIND: usize = 1107;
const ID_EDIT_REPLACE: usize = 1108;
const ID_EDIT_GOTO: usize = 1109;
const ID_FORMAT_FONT: usize = 1301;
const ID_FORMAT_DOCUMENT: usize = 1302;
const ID_VIEW_WRAP: usize = 1401;
const ID_VIEW_THEME: usize = 1402;
const ID_LANG_PLAIN: usize = 1501;
const ID_LANG_MARKDOWN: usize = 1502;
const ID_LANG_JSON: usize = 1503;
const ID_TIMER_INDEX: usize = 1;
const ID_TIMER_WATCH: usize = 2;
/// Deferred layout sync when SetScrollInfo re-enters the wndproc while RefCell is borrowed.
const WM_APP_SYNC_LAYOUT: u32 = WM_APP + 1;

/// Original SCROLLBAR class procedure (shared by both bars after subclassing).
static SCROLLBAR_ORIG_PROC: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn scrollbar_with_hand_cursor(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_SETCURSOR {
        if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
            SetCursor(cursor);
            return LRESULT(1);
        }
    }
    let orig = SCROLLBAR_ORIG_PROC.load(Ordering::Relaxed);
    if orig != 0 {
        let prev: WNDPROC = std::mem::transmute(orig);
        CallWindowProcW(prev, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn install_scrollbar_hand_cursor(hwnd: HWND) {
    unsafe {
        let prev = SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            scrollbar_with_hand_cursor as *const () as isize,
        );
        let _ = SCROLLBAR_ORIG_PROC.compare_exchange(0, prev, Ordering::SeqCst, Ordering::SeqCst);
    }
}

pub struct AppState {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub settings: AppSettings,
    pub renderer: Renderer,
    pub languages: LanguageRegistry,
    pub find_query: String,
    pub replace_query: String,
    pub find_case: bool,
    pub mouse_down: bool,
    pub hwnd: HWND,
    pub hwnd_vscroll: HWND,
    pub hwnd_hscroll: HWND,
    pub hmenu: HMENU,
    pub recent_menu: HMENU,
}

impl AppState {
    fn new(
        hwnd: HWND,
        hwnd_vscroll: HWND,
        hwnd_hscroll: HWND,
        hmenu: HMENU,
        recent_menu: HMENU,
    ) -> Result<Self> {
        let settings = AppSettings::load();
        let mut tab = Tab::new_empty();
        tab.view.word_wrap = settings.word_wrap;
        tab.view.content_top = TAB_HEIGHT;
        tab.view.content_bottom = STATUS_HEIGHT + SCROLLBAR_THICKNESS;
        tab.view.content_right = SCROLLBAR_THICKNESS;
        Ok(Self {
            tabs: vec![tab],
            active: 0,
            renderer: Renderer::new(&settings.font_face, settings.font_size, settings.dark_theme)?,
            languages: LanguageRegistry::builtin(),
            settings,
            find_query: String::new(),
            replace_query: String::new(),
            find_case: false,
            mouse_down: false,
            hwnd,
            hwnd_vscroll,
            hwnd_hscroll,
            hmenu,
            recent_menu,
        })
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn update_chrome_layout(&mut self) {
        let h_inset = if self.settings.word_wrap {
            STATUS_HEIGHT
        } else {
            STATUS_HEIGHT + SCROLLBAR_THICKNESS
        };
        let r_inset = SCROLLBAR_THICKNESS;
        for tab in &mut self.tabs {
            tab.view.content_top = TAB_HEIGHT;
            tab.view.content_bottom = h_inset;
            tab.view.content_right = r_inset;
            tab.view.word_wrap = self.settings.word_wrap;
        }
    }

    /// Copy viewport metrics so a new/replacement tab paints at the real client size
    /// instead of ViewState defaults (800×600).
    fn apply_view_metrics(tab: &mut Tab, src: &ViewState) {
        tab.view.client_width = src.client_width;
        tab.view.client_height = src.client_height;
        tab.view.char_width = src.char_width;
        tab.view.line_height = src.line_height;
        tab.view.word_wrap = src.word_wrap;
        tab.view.content_top = src.content_top;
        tab.view.content_bottom = src.content_bottom;
        tab.view.content_right = src.content_right;
    }

    fn update_title(&self) {
        let title = format!("{} - Fast Notepad", self.tab().title());
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide(&title).as_ptr()));
        }
    }

    fn vert_scroll_info(&self) -> SCROLLINFO {
        let tab = self.tab();
        let lines = if tab.view.word_wrap {
            tab.view.total_visual_rows(&tab.document)
        } else {
            tab.document.lines().line_count().max(1)
        };
        let page = tab.view.visible_line_count().max(1);
        SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: lines.saturating_sub(1) as i32,
            nPage: page as u32,
            nPos: tab.view.first_visible_line as i32,
            nTrackPos: 0,
        }
    }

    fn horz_scroll_info(&self) -> SCROLLINFO {
        let tab = self.tab();
        if tab.view.word_wrap {
            return SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: 0,
                nPage: 1,
                nPos: 0,
                nTrackPos: 0,
            };
        }
        let max_x = tab.view.max_content_width_px(&tab.document);
        let page = tab.view.text_area_width().max(1);
        // Win32: max thumb position = nMax - nPage + 1 == content_width - page.
        SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: max_x.saturating_sub(1).max(0),
            nPage: page as u32,
            nPos: tab.view.scroll_x,
            nTrackPos: 0,
        }
    }

    fn refresh_scrollbars(&self) {
        // Prefer refresh_scrollbars_outside_borrow from wndproc paths.
        // AppState methods may still call this while RefCell is borrowed; WM_SIZE
        // defers via WM_APP_SYNC_LAYOUT if SetScrollInfo re-enters.
        apply_scroll(self.hwnd_vscroll, &self.vert_scroll_info(), true);
        apply_scroll(self.hwnd_hscroll, &self.horz_scroll_info(), true);
        let show_h = !self.settings.word_wrap;
        unsafe {
            let _ = ShowWindow(self.hwnd_hscroll, if show_h { SW_SHOW } else { SW_HIDE });
        }
    }

    fn after_edit(&mut self) {
        {
            let tab = &mut self.tabs[self.active];
            let doc_len = tab.document.len();
            tab.view.caret.offset = tab.view.caret.offset.min(doc_len);
            tab.view.ensure_caret_visible(&tab.document);
        }
        self.update_title();
        invalidate(self.hwnd);
        self.refresh_scrollbars();
    }

    fn insert_text(&mut self, text: &str) {
        let tab = self.tab_mut();
        if tab.view.caret.has_selection() {
            let (a, b) = tab.view.caret.selection_range();
            let _ = tab.document.delete(a, b);
            tab.view.caret.offset = a;
            tab.view.caret.collapse_to_offset();
        }
        let pos = tab.view.caret.offset;
        let _ = tab.document.insert(pos, text);
        tab.view.caret.offset = pos + text.len();
        tab.view.caret.collapse_to_offset();
        self.after_edit();
    }

    fn delete_selection_or_char(&mut self, backward: bool) {
        let tab = self.tab_mut();
        if tab.view.caret.has_selection() {
            let (a, b) = tab.view.caret.selection_range();
            let _ = tab.document.delete(a, b);
            tab.view.caret.offset = a;
            tab.view.caret.collapse_to_offset();
        } else if backward {
            if tab.view.caret.offset > 0 {
                let end = tab.view.caret.offset;
                let start = prev_char_boundary(&tab.document, end);
                let _ = tab.document.delete(start, end);
                tab.view.caret.offset = start;
                tab.view.caret.collapse_to_offset();
            }
        } else if tab.view.caret.offset < tab.document.len() {
            let start = tab.view.caret.offset;
            let end = next_char_boundary(&tab.document, start);
            let _ = tab.document.delete(start, end);
        }
        self.after_edit();
    }

    fn selected_text(&self) -> Option<String> {
        let tab = self.tab();
        if !tab.view.caret.has_selection() {
            return None;
        }
        let (a, b) = tab.view.caret.selection_range();
        let bytes = tab.document.slice(a, b).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn copy_selection(&self) {
        if let Some(text) = self.selected_text() {
            let _ = set_clipboard_text(self.hwnd, &text);
        }
    }

    fn cut_selection(&mut self) {
        if self.tab().view.caret.has_selection() {
            self.copy_selection();
            let tab = self.tab_mut();
            let (a, b) = tab.view.caret.selection_range();
            let _ = tab.document.delete(a, b);
            tab.view.caret.offset = a;
            tab.view.caret.collapse_to_offset();
            self.after_edit();
        }
    }

    fn paste(&mut self) {
        if let Some(text) = get_clipboard_text(self.hwnd) {
            self.insert_text(&text);
        }
    }

    fn move_caret(&mut self, new_offset: usize, extend: bool) {
        let tab = self.tab_mut();
        tab.view.caret.offset = new_offset.min(tab.document.len());
        if !extend {
            tab.view.caret.collapse_to_offset();
        }
        tab.view.ensure_caret_visible(&tab.document);
        invalidate(self.hwnd);
        self.refresh_scrollbars();
    }

    fn line_start(&self, line: usize) -> usize {
        self.tab().document.lines().line_start(line).unwrap_or(0)
    }

    fn line_end_content(&self, line: usize) -> usize {
        let tab = self.tab();
        let start = self.line_start(line);
        let end = tab
            .document
            .lines()
            .line_start(line + 1)
            .unwrap_or(tab.document.len());
        let mut e = end;
        if e > start {
            if let Ok(bytes) = tab.document.slice(start, end) {
                if bytes.ends_with(b"\r\n") {
                    e -= 2;
                } else if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
                    e -= 1;
                }
            }
        }
        e
    }

    fn new_tab(&mut self) {
        let mut tab = Tab::new_empty();
        Self::apply_view_metrics(&mut tab, &self.tab().view);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.update_chrome_layout();
        self.update_title();
        sync_language_menu(self);
        invalidate(self.hwnd);
        self.refresh_scrollbars();
    }

    /// Close a tab without prompting. Caller must handle dirty confirmation
    /// *outside* any RefCell borrow — MessageBox pumps the queue and re-enters wndproc.
    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // Preserve viewport size before remove — replacement empty tab must not
        // fall back to ViewState defaults (800×600), or paint draws a small tile.
        let metrics = self.tabs[index].view.clone();
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let mut tab = Tab::new_empty();
            Self::apply_view_metrics(&mut tab, &metrics);
            self.tabs.push(tab);
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if index < self.active {
            self.active -= 1;
        }
        self.update_chrome_layout();
        self.update_title();
        invalidate(self.hwnd);
        // Do not call refresh_scrollbars here — SetScrollInfo can re-enter wndproc
        // while RefCell is still borrowed by the caller.
    }

    fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.update_title();
            invalidate(self.hwnd);
            self.refresh_scrollbars();
            sync_language_menu(self);
        }
    }

    fn open_path(&mut self, path: &Path) -> Result<(), String> {
        let opened = open_file(path).map_err(|e| e.to_string())?;
        let metrics = self.tab().view.clone();
        let replace_blank = self.tab().is_blank();

        let mut tab = Tab::new_empty();
        tab.document = opened.document;
        tab.encoding = opened.encoding;
        Self::apply_view_metrics(&mut tab, &metrics);
        tab.last_write_secs = file_write_secs(path);
        tab.disk_change_notified = false;
        if !tab.document.lines().is_complete() {
            let _ = build_line_index_incremental(&mut tab.document);
        }
        tab.language = detect_language_for(&tab.document, Some(path), &self.languages);
        tab.language_locked = false;

        if replace_blank {
            let idx = self.active;
            self.tabs[idx] = tab;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
        self.settings.add_recent(path);
        self.rebuild_recent_menu();
        self.update_title();
        sync_language_menu(self);
        invalidate(self.hwnd);
        self.refresh_scrollbars();
        if opened.had_decode_errors {
            show_info(
                self.hwnd,
                "Encoding",
                "File opened with some decoding replacements.",
            );
        }
        Ok(())
    }

    fn rebuild_recent_menu(&self) {
        unsafe {
            while windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(self.recent_menu) > 0 {
                let _ = windows::Win32::UI::WindowsAndMessaging::DeleteMenu(
                    self.recent_menu,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::MF_BYPOSITION,
                );
            }
            if self.settings.recent.is_empty() {
                let _ = AppendMenuW(self.recent_menu, MF_STRING, 0, w!("(empty)"));
            } else {
                for (i, path) in self.settings.recent.iter().enumerate() {
                    let label = format!("&{} {}", i + 1, path);
                    let _ = AppendMenuW(
                        self.recent_menu,
                        MF_STRING,
                        ID_RECENT_BASE + i,
                        PCWSTR(wide(&label).as_ptr()),
                    );
                }
            }
        }
    }

    fn find_next_match(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let case = self.find_case;
        let q = self.find_query.clone();
        let tab = &mut self.tabs[self.active];
        let start = if tab.view.caret.has_selection() {
            tab.view.caret.selection_range().1
        } else {
            tab.view.caret.offset
        };
        if let Some((a, b)) = find_next(&tab.document, &q, start, case)
            .or_else(|| find_next(&tab.document, &q, 0, case))
        {
            tab.view.caret.anchor = a;
            tab.view.caret.offset = b;
            tab.view.ensure_caret_visible(&tab.document);
            invalidate(self.hwnd);
            self.refresh_scrollbars();
        } else {
            show_info(self.hwnd, "Find", "Text not found.");
        }
    }

    fn do_replace_next(&mut self) {
        let q = self.find_query.clone();
        let r = self.replace_query.clone();
        let case = self.find_case;
        let tab = self.tab_mut();
        let from = tab.view.caret.selection_range().0;
        if let Some((a, b)) = replace_next(&mut tab.document, &q, &r, from, case) {
            tab.view.caret.anchor = a;
            tab.view.caret.offset = b;
            self.after_edit();
        } else {
            show_info(self.hwnd, "Replace", "Text not found.");
        }
    }

    fn do_replace_all(&mut self) {
        let q = self.find_query.clone();
        let r = self.replace_query.clone();
        let case = self.find_case;
        let tab = self.tab_mut();
        let n = replace_all(&mut tab.document, &q, &r, case, 100_000);
        self.after_edit();
        show_info(self.hwnd, "Replace All", &format!("Replaced {n} occurrence(s)."));
    }

    fn open_path_replace_active(&mut self, path: &Path) -> Result<(), String> {
        let opened = open_file(path).map_err(|e| e.to_string())?;
        let locked = self.tab().language_locked;
        let prev_lang = self.tab().language;
        {
            let tab = self.tab_mut();
            tab.document = opened.document;
            tab.encoding = opened.encoding;
            tab.view.caret = Default::default();
            tab.view.first_visible_line = 0;
            tab.last_write_secs = file_write_secs(path);
            tab.disk_change_notified = false;
            if !tab.document.lines().is_complete() {
                let _ = build_line_index_incremental(&mut tab.document);
            }
        }
        let language = if locked {
            prev_lang
        } else {
            detect_language_for(&self.tab().document, Some(path), &self.languages)
        };
        {
            let tab = self.tab_mut();
            tab.language = language;
            tab.language_locked = locked;
        }
        self.update_title();
        sync_language_menu(self);
        invalidate(self.hwnd);
        Ok(())
    }
}

pub fn run() -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hinstance = GetModuleHandleW(None)?;
        let class_name = w!("FastNotepadWindow");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_IBEAM)?,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let (menu, recent) = build_menu()?;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Untitled - Fast Notepad"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1100,
            750,
            None,
            menu,
            hinstance,
            None,
        )?;

        let hwnd_vscroll = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SCROLLBAR"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SBS_VERT as u32),
            0,
            0,
            SCROLLBAR_THICKNESS,
            100,
            hwnd,
            None,
            hinstance,
            None,
        )?;
        let hwnd_hscroll = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SCROLLBAR"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SBS_HORZ as u32),
            0,
            0,
            100,
            SCROLLBAR_THICKNESS,
            hwnd,
            None,
            hinstance,
            None,
        )?;

        install_scrollbar_hand_cursor(hwnd_vscroll);
        install_scrollbar_hand_cursor(hwnd_hscroll);

        let state = Rc::new(RefCell::new(AppState::new(
            hwnd,
            hwnd_vscroll,
            hwnd_hscroll,
            menu,
            recent,
        )?));
        {
            let mut s = state.borrow_mut();
            s.rebuild_recent_menu();
            s.update_chrome_layout();
            sync_view_menu(&s);
            sync_language_menu(&s);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(state) as isize);
        DragAcceptFiles(hwnd, true);

        let hdc = GetDC(hwnd);
        {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<AppState>;
            if !ptr.is_null() {
                {
                    let mut s = (*ptr).borrow_mut();
                    let face = s.settings.font_face.clone();
                    let size = s.settings.font_size;
                    for tab in &mut s.tabs {
                        measure_metrics(hdc, &mut tab.view, &face, size);
                    }
                }
                sync_layout(hwnd, &*ptr);
            }
        }
        ReleaseDC(hwnd, hdc);

        let _ = SetTimer(hwnd, ID_TIMER_INDEX, 16, None);
        let _ = SetTimer(hwnd, ID_TIMER_WATCH, 1000, None);
        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn build_menu() -> Result<(HMENU, HMENU)> {
    unsafe {
        let menu = CreateMenu()?;
        let file = CreatePopupMenu()?;
        let edit = CreatePopupMenu()?;
        let format = CreatePopupMenu()?;
        let view = CreatePopupMenu()?;
        let recent = CreatePopupMenu()?;

        AppendMenuW(file, MF_STRING, ID_FILE_NEW, w!("&New\tCtrl+N"))?;
        AppendMenuW(file, MF_STRING, ID_FILE_NEW_TAB, w!("New &Tab\tCtrl+T"))?;
        AppendMenuW(file, MF_STRING, ID_FILE_OPEN, w!("&Open...\tCtrl+O"))?;
        AppendMenuW(file, MF_STRING, ID_FILE_SAVE, w!("&Save\tCtrl+S"))?;
        AppendMenuW(file, MF_STRING, ID_FILE_SAVE_AS, w!("Save &As..."))?;
        AppendMenuW(file, MF_STRING, ID_FILE_CLOSE_TAB, w!("Close Tab\tCtrl+W"))?;
        AppendMenuW(file, MF_POPUP, recent.0 as usize, w!("Recent"))?;
        AppendMenuW(file, MF_SEPARATOR, 0, PCWSTR::null())?;
        AppendMenuW(file, MF_STRING, ID_FILE_PRINT, w!("&Print...\tCtrl+P"))?;
        AppendMenuW(file, MF_SEPARATOR, 0, PCWSTR::null())?;
        AppendMenuW(file, MF_STRING, ID_FILE_EXIT, w!("E&xit"))?;

        AppendMenuW(edit, MF_STRING, ID_EDIT_UNDO, w!("&Undo\tCtrl+Z"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_REDO, w!("&Redo\tCtrl+Y"))?;
        AppendMenuW(edit, MF_SEPARATOR, 0, PCWSTR::null())?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_CUT, w!("Cu&t\tCtrl+X"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_COPY, w!("&Copy\tCtrl+C"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_PASTE, w!("&Paste\tCtrl+V"))?;
        AppendMenuW(edit, MF_SEPARATOR, 0, PCWSTR::null())?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_SELECT_ALL, w!("Select &All\tCtrl+A"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_FIND, w!("&Find...\tCtrl+F"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_REPLACE, w!("&Replace...\tCtrl+H"))?;
        AppendMenuW(edit, MF_STRING, ID_EDIT_GOTO, w!("&Go to Line...\tCtrl+G"))?;

        AppendMenuW(format, MF_STRING, ID_FORMAT_FONT, w!("&Font..."))?;
        AppendMenuW(
            format,
            MF_STRING,
            ID_FORMAT_DOCUMENT,
            w!("Format &Document\tCtrl+Shift+F"),
        )?;

        let language = CreatePopupMenu()?;
        AppendMenuW(language, MF_STRING, ID_LANG_PLAIN, w!("&Plain Text"))?;
        AppendMenuW(language, MF_STRING, ID_LANG_MARKDOWN, w!("&Markdown"))?;
        AppendMenuW(language, MF_STRING, ID_LANG_JSON, w!("&JSON"))?;

        AppendMenuW(view, MF_STRING, ID_VIEW_WRAP, w!("&Word Wrap"))?;
        AppendMenuW(view, MF_STRING, ID_VIEW_THEME, w!("&Dark Theme"))?;

        AppendMenuW(menu, MF_POPUP, file.0 as usize, w!("&File"))?;
        AppendMenuW(menu, MF_POPUP, edit.0 as usize, w!("&Edit"))?;
        AppendMenuW(menu, MF_POPUP, format.0 as usize, w!("F&ormat"))?;
        AppendMenuW(menu, MF_POPUP, view.0 as usize, w!("&View"))?;
        AppendMenuW(menu, MF_POPUP, language.0 as usize, w!("&Language"))?;
        Ok((menu, recent))
    }
}

fn sync_view_menu(s: &AppState) {
    unsafe {
        let wrap_flag = if s.settings.word_wrap {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let theme_flag = if s.settings.dark_theme {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = CheckMenuItem(
            s.hmenu,
            ID_VIEW_WRAP as u32,
            (MF_BYCOMMAND | wrap_flag).0,
        );
        let _ = CheckMenuItem(
            s.hmenu,
            ID_VIEW_THEME as u32,
            (MF_BYCOMMAND | theme_flag).0,
        );
    }
}

fn sync_language_menu(s: &AppState) {
    unsafe {
        let lang = s.tab().language;
        for (id, want) in [
            (ID_LANG_PLAIN, LanguageId::Unknown),
            (ID_LANG_MARKDOWN, LanguageId::Markdown),
            (ID_LANG_JSON, LanguageId::Json),
        ] {
            let flag = if lang == want {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            };
            let _ = CheckMenuItem(s.hmenu, id as u32, (MF_BYCOMMAND | flag).0);
        }
    }
}

fn apply_scroll(hwnd: HWND, info: &SCROLLINFO, redraw: bool) {
    unsafe {
        SetScrollInfo(hwnd, SB_CTL, info, redraw);
    }
}

/// Reposition child scrollbars inside the client area.
fn layout_scrollbars(state: &AppState, client_w: i32, client_h: i32) {
    let thick = SCROLLBAR_THICKNESS;
    let top = TAB_HEIGHT;
    let status_top = (client_h - STATUS_HEIGHT).max(top);
    let show_h = !state.settings.word_wrap;
    let h_bar_top = if show_h {
        (status_top - thick).max(top)
    } else {
        status_top
    };
    let v_left = (client_w - thick).max(0);
    let v_height = (h_bar_top - top).max(0);
    let h_width = v_left.max(0);

    unsafe {
        let _ = MoveWindow(state.hwnd_vscroll, v_left, top, thick, v_height, true);
        if show_h {
            let _ = MoveWindow(state.hwnd_hscroll, 0, h_bar_top, h_width, thick, true);
            let _ = ShowWindow(state.hwnd_hscroll, SW_SHOW);
        } else {
            let _ = ShowWindow(state.hwnd_hscroll, SW_HIDE);
        }
        let _ = ShowWindow(state.hwnd_vscroll, SW_SHOW);
    }
}

/// Update client size + scrollbars without holding RefCell across SetScrollInfo.
/// If the cell is already borrowed (re-entrant Win32 call), defer via PostMessage.
fn sync_layout(hwnd: HWND, state_cell: &RefCell<AppState>) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    let client_w = rect.right - rect.left;
    let client_h = rect.bottom - rect.top;
    let scroll = match state_cell.try_borrow_mut() {
        Ok(mut s) => {
            for tab in &mut s.tabs {
                tab.view.client_width = client_w;
                tab.view.client_height = client_h;
                tab.view.clamp_scroll_x(&tab.document);
            }
            s.update_chrome_layout();
            layout_scrollbars(&s, client_w, client_h);
            Some((
                s.hwnd_vscroll,
                s.hwnd_hscroll,
                s.vert_scroll_info(),
                s.horz_scroll_info(),
            ))
        }
        Err(_) => {
            unsafe {
                let _ = PostMessageW(hwnd, WM_APP_SYNC_LAYOUT, WPARAM(0), LPARAM(0));
            }
            None
        }
    };
    if let Some((v_hwnd, h_hwnd, v_info, h_info)) = scroll {
        apply_scroll(v_hwnd, &v_info, true);
        apply_scroll(h_hwnd, &h_info, true);
        invalidate(hwnd);
    }
}

/// Apply scrollbar state after dropping any RefCell borrow.
fn refresh_scrollbars_outside_borrow(state_cell: &RefCell<AppState>) {
    refresh_scrollbars_outside_borrow_ex(state_cell, true);
}

fn refresh_scrollbars_outside_borrow_ex(state_cell: &RefCell<AppState>, redraw_bars: bool) {
    let (v_hwnd, h_hwnd, v_info, h_info, show_h) = {
        let s = state_cell.borrow();
        (
            s.hwnd_vscroll,
            s.hwnd_hscroll,
            s.vert_scroll_info(),
            s.horz_scroll_info(),
            !s.settings.word_wrap,
        )
    };
    apply_scroll(v_hwnd, &v_info, redraw_bars);
    apply_scroll(h_hwnd, &h_info, redraw_bars);
    unsafe {
        let _ = ShowWindow(h_hwnd, if show_h { SW_SHOW } else { SW_HIDE });
    }
}

fn invalidate_editor_from_state(state_cell: &RefCell<AppState>) {
    let s = state_cell.borrow();
    let tab = s.tab();
    invalidate_editor(s.hwnd, &tab.view);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<AppState>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state_cell = &*ptr;

    match msg {
        WM_PAINT => {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let client_w = client.right - client.left;
            let client_h = client.bottom - client.top;
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            // try_borrow: modal dialogs / SetScrollInfo can re-enter while RefCell is held.
            if let Ok(mut s) = state_cell.try_borrow_mut() {
                for tab in &mut s.tabs {
                    tab.view.client_width = client_w;
                    tab.view.client_height = client_h;
                }
                let active = s.active;
                let status = {
                    let tab = &s.tabs[active];
                    status_text(&tab.document, &tab.view, tab.encoding, tab.language)
                };
                let lang = s.tabs[active].language;
                let AppState {
                    ref tabs,
                    ref mut renderer,
                    ref languages,
                    ..
                } = &mut *s;
                let mode = languages.get(lang);
                let tab = &tabs[active];
                renderer.paint_frame(
                    hdc,
                    tabs,
                    active,
                    &tab.document,
                    &tab.view,
                    &status,
                    mode.as_ref(),
                );
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE | WM_APP_SYNC_LAYOUT => {
            sync_layout(hwnd, state_cell);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == ID_TIMER_INDEX {
                let needs_refresh = {
                    let mut s = state_cell.borrow_mut();
                    if !s.tab().document.lines().is_complete() {
                        let _ = build_line_index_incremental(&mut s.tab_mut().document);
                        true
                    } else {
                        false
                    }
                };
                if needs_refresh {
                    refresh_scrollbars_outside_borrow(state_cell);
                    invalidate(hwnd);
                }
            } else if wparam.0 == ID_TIMER_WATCH {
                poll_disk_changes(hwnd, state_cell);
            }
            LRESULT(0)
        }
        WM_VSCROLL => {
            let code = (wparam.0 & 0xFFFF) as i32;
            let tracking = code == SB_THUMBTRACK.0;
            {
                let mut s = state_cell.borrow_mut();
                let bar = HWND(lparam.0 as *mut _);
                let vscroll = s.hwnd_vscroll;
                let tab = s.tab_mut();
                let lines = if tab.view.word_wrap {
                    tab.view.total_visual_rows(&tab.document)
                } else {
                    tab.document.lines().line_count().max(1)
                };
                let page = tab.view.visible_line_count().max(1);
                let mut pos = tab.view.first_visible_line as i32;
                match code {
                    c if c == SB_LINEUP.0 => pos -= 1,
                    c if c == SB_LINEDOWN.0 => pos += 1,
                    c if c == SB_PAGEUP.0 => pos -= page as i32,
                    c if c == SB_PAGEDOWN.0 => pos += page as i32,
                    c if c == SB_TOP.0 => pos = 0,
                    c if c == SB_BOTTOM.0 => pos = lines as i32,
                    c if c == SB_THUMBPOSITION.0 || c == SB_THUMBTRACK.0 => {
                        let mut info = SCROLLINFO {
                            cbSize: size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_TRACKPOS,
                            ..Default::default()
                        };
                        let target = if bar.0.is_null() { vscroll } else { bar };
                        let _ = GetScrollInfo(target, SB_CTL, &mut info);
                        pos = info.nTrackPos;
                    }
                    _ => {}
                }
                pos = pos.clamp(0, lines.saturating_sub(1) as i32);
                tab.view.first_visible_line = pos as usize;
            }
            // During thumb drag, skip bar redraw (thumb already follows the mouse).
            refresh_scrollbars_outside_borrow_ex(state_cell, !tracking);
            invalidate_editor_from_state(state_cell);
            LRESULT(0)
        }
        WM_HSCROLL => {
            let code = (wparam.0 & 0xFFFF) as i32;
            let tracking = code == SB_THUMBTRACK.0;
            {
                let mut s = state_cell.borrow_mut();
                let bar = HWND(lparam.0 as *mut _);
                let hscroll = s.hwnd_hscroll;
                let tab = s.tab_mut();
                if tab.view.word_wrap {
                    tab.view.scroll_x = 0;
                } else {
                    let page = tab.view.text_area_width().max(1);
                    let max_x = tab.view.max_scroll_x(&tab.document);
                    let step = (tab.view.char_width as i32).max(1) * 4;
                    let mut pos = tab.view.scroll_x;
                    match code {
                        c if c == SB_LINELEFT.0 => pos -= step,
                        c if c == SB_LINERIGHT.0 => pos += step,
                        c if c == SB_PAGELEFT.0 => pos -= page,
                        c if c == SB_PAGERIGHT.0 => pos += page,
                        c if c == SB_LEFT.0 => pos = 0,
                        c if c == SB_RIGHT.0 => pos = max_x,
                        c if c == SB_THUMBPOSITION.0 || c == SB_THUMBTRACK.0 => {
                            let mut info = SCROLLINFO {
                                cbSize: size_of::<SCROLLINFO>() as u32,
                                fMask: SIF_TRACKPOS,
                                ..Default::default()
                            };
                            let target = if bar.0.is_null() { hscroll } else { bar };
                            let _ = GetScrollInfo(target, SB_CTL, &mut info);
                            pos = info.nTrackPos;
                        }
                        _ => {}
                    }
                    tab.view.scroll_x = pos.clamp(0, max_x);
                }
            }
            refresh_scrollbars_outside_borrow_ex(state_cell, !tracking);
            invalidate_editor_from_state(state_cell);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as i32;
            let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
            {
                let mut s = state_cell.borrow_mut();
                let tab = s.tab_mut();
                if shift && !tab.view.word_wrap {
                    let step = (tab.view.char_width as i32).max(1) * 6;
                    let dir = if delta > 0 { -step } else { step };
                    tab.view.scroll_by_px(dir, &tab.document);
                } else {
                    let lines = if delta > 0 { -3 } else { 3 };
                    tab.view.scroll_by_lines(lines, &tab.document);
                }
            }
            refresh_scrollbars_outside_borrow(state_cell);
            invalidate_editor_from_state(state_cell);
            LRESULT(0)
        }
        WM_MOUSEHWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as i32;
            {
                let mut s = state_cell.borrow_mut();
                let tab = s.tab_mut();
                if !tab.view.word_wrap {
                    // Positive delta = tilt right → scroll content right (increase scroll_x).
                    let step = (tab.view.char_width as i32).max(1) * 6;
                    let dir = if delta > 0 { step } else { -step };
                    tab.view.scroll_by_px(dir, &tab.document);
                }
            }
            refresh_scrollbars_outside_borrow(state_cell);
            invalidate_editor_from_state(state_cell);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let hit = (lparam.0 & 0xFFFF) as u32;
            if hit == HTCLIENT {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let _ = ScreenToClient(hwnd, &mut pt);
                    let s = state_cell.borrow();
                    let view = &s.tab().view;
                    let over_vscroll = pt.x >= view.editor_right();
                    let over_hscroll = !s.settings.word_wrap
                        && pt.y >= view.client_height - view.content_bottom
                        && pt.y < view.client_height - STATUS_HEIGHT;
                    let idc = if over_vscroll || over_hscroll {
                        IDC_HAND
                    } else if pt.y < TAB_HEIGHT {
                        IDC_ARROW
                    } else {
                        IDC_IBEAM
                    };
                    if let Ok(cursor) = LoadCursorW(None, idc) {
                        SetCursor(cursor);
                    }
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DROPFILES => {
            handle_drop_files(hwnd, state_cell, HDROP(wparam.0 as *mut _));
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_MBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            if y < TAB_HEIGHT {
                let (count, w) = {
                    let s = state_cell.borrow();
                    (s.tabs.len(), s.tab().view.client_width)
                };
                if msg == WM_MBUTTONDOWN {
                    if let Some(idx) = tab_at_x(x, count, w) {
                        request_close_tab(hwnd, state_cell, idx);
                    }
                } else if let Some(idx) = tab_at_x(x, count, w) {
                    if close_button_hit(x, idx, count, w) {
                        request_close_tab(hwnd, state_cell, idx);
                    } else {
                        state_cell.borrow_mut().switch_tab(idx);
                    }
                }
                return LRESULT(0);
            }
            let mut s = state_cell.borrow_mut();
            let offset = {
                let tab = s.tab();
                tab.view.offset_from_point(&tab.document, x, y)
            };
            let extend = key_down(VK_SHIFT.0 as i32);
            s.move_caret(offset, extend);
            s.mouse_down = true;
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let mut s = state_cell.borrow_mut();
            if s.mouse_down {
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let offset = {
                    let tab = s.tab();
                    tab.view.offset_from_point(&tab.document, x, y)
                };
                s.move_caret(offset, true);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            state_cell.borrow_mut().mouse_down = false;
            let _ = ReleaseCapture();
            LRESULT(0)
        }
        WM_CHAR => {
            let ch = wparam.0 as u32;
            if ch < 32 && ch != b'\t' as u32 && ch != b'\r' as u32 {
                return LRESULT(0);
            }
            let mut s = state_cell.borrow_mut();
            if ch == b'\r' as u32 {
                let ending = match s.tab().document.line_ending() {
                    LineEnding::CrLf => "\r\n",
                    _ => "\n",
                };
                s.insert_text(ending);
            } else if let Some(c) = char::from_u32(ch) {
                if !c.is_control() || c == '\t' {
                    s.insert_text(&c.to_string());
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            handle_keydown(hwnd, state_cell, wparam.0 as u32);
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(hwnd, state_cell, wparam.0 & 0xFFFF);
            LRESULT(0)
        }
        WM_CLOSE => {
            if confirm_discard_all(hwnd, state_cell) {
                state_cell.borrow().settings.save();
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const RefCell<AppState>;
            if !ptr.is_null() {
                drop(Rc::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_keydown(hwnd: HWND, state_cell: &RefCell<AppState>, vk: u32) {
    let ctrl = key_down(VK_CONTROL.0 as i32);
    let shift = key_down(VK_SHIFT.0 as i32);

    if ctrl {
        match vk {
            0x41 => {
                let mut s = state_cell.borrow_mut();
                let len = s.tab().document.len();
                s.tab_mut().view.caret.select_all(len);
                invalidate(hwnd);
            }
            0x43 => state_cell.borrow().copy_selection(),
            0x58 => state_cell.borrow_mut().cut_selection(),
            0x56 => state_cell.borrow_mut().paste(),
            0x5A => {
                let mut s = state_cell.borrow_mut();
                let _ = s.tab_mut().document.undo();
                s.after_edit();
            }
            0x59 => {
                let mut s = state_cell.borrow_mut();
                let _ = s.tab_mut().document.redo();
                s.after_edit();
            }
            0x4F => handle_command(hwnd, state_cell, ID_FILE_OPEN),
            0x53 => handle_command(hwnd, state_cell, ID_FILE_SAVE),
            0x4E => handle_command(hwnd, state_cell, ID_FILE_NEW),
            0x54 => handle_command(hwnd, state_cell, ID_FILE_NEW_TAB),
            0x57 => handle_command(hwnd, state_cell, ID_FILE_CLOSE_TAB),
            0x46 => {
                if shift {
                    handle_command(hwnd, state_cell, ID_FORMAT_DOCUMENT);
                } else {
                    handle_command(hwnd, state_cell, ID_EDIT_FIND);
                }
            }
            0x48 => handle_command(hwnd, state_cell, ID_EDIT_REPLACE),
            0x47 => handle_command(hwnd, state_cell, ID_EDIT_GOTO),
            0x50 => handle_command(hwnd, state_cell, ID_FILE_PRINT),
            0x09 => {
                // Ctrl+Tab
                let mut s = state_cell.borrow_mut();
                let next = if shift {
                    if s.active == 0 {
                        s.tabs.len() - 1
                    } else {
                        s.active - 1
                    }
                } else {
                    (s.active + 1) % s.tabs.len()
                };
                s.switch_tab(next);
            }
            _ => {}
        }
        return;
    }

    let mut s = state_cell.borrow_mut();
    match vk {
        v if v == VK_LEFT.0 as u32 => {
            let next = {
                let tab = s.tab();
                if tab.view.caret.has_selection() && !shift {
                    tab.view.caret.selection_range().0
                } else {
                    prev_char_boundary(&tab.document, tab.view.caret.offset)
                }
            };
            s.move_caret(next, shift);
        }
        v if v == VK_RIGHT.0 as u32 => {
            let next = {
                let tab = s.tab();
                if tab.view.caret.has_selection() && !shift {
                    tab.view.caret.selection_range().1
                } else {
                    next_char_boundary(&tab.document, tab.view.caret.offset)
                }
            };
            s.move_caret(next, shift);
        }
        v if v == VK_UP.0 as u32 => {
            let line = s.tab().document.lines().line_of_offset(s.tab().view.caret.offset);
            if line > 0 {
                let col = s.tab().view.caret.offset - s.line_start(line);
                let prev_start = s.line_start(line - 1);
                let prev_end = s.line_end_content(line - 1);
                s.move_caret(prev_start + col.min(prev_end.saturating_sub(prev_start)), shift);
            }
        }
        v if v == VK_DOWN.0 as u32 => {
            let line = s.tab().document.lines().line_of_offset(s.tab().view.caret.offset);
            if line + 1 < s.tab().document.lines().line_count() {
                let col = s.tab().view.caret.offset - s.line_start(line);
                let next_start = s.line_start(line + 1);
                let next_end = s.line_end_content(line + 1);
                s.move_caret(next_start + col.min(next_end.saturating_sub(next_start)), shift);
            }
        }
        v if v == VK_HOME.0 as u32 => {
            let line = s.tab().document.lines().line_of_offset(s.tab().view.caret.offset);
            let start = s.line_start(line);
            s.move_caret(start, shift);
        }
        v if v == VK_END.0 as u32 => {
            let line = s.tab().document.lines().line_of_offset(s.tab().view.caret.offset);
            let end = s.line_end_content(line);
            s.move_caret(end, shift);
        }
        v if v == VK_PRIOR.0 as u32 => {
            let active = s.active;
            let page = s.tabs[active].view.visible_line_count();
            let max_first = if s.tabs[active].view.word_wrap {
                s.tabs[active]
                    .view
                    .total_visual_rows(&s.tabs[active].document)
                    .saturating_sub(1)
            } else {
                s.tabs[active]
                    .document
                    .lines()
                    .line_count()
                    .saturating_sub(1)
            };
            let next = s.tabs[active].view.first_visible_line.saturating_sub(page);
            s.tabs[active].view.first_visible_line = next.min(max_first);
            let line = s.tabs[active]
                .document
                .lines()
                .line_of_offset(s.tabs[active].view.caret.offset);
            let target = line.saturating_sub(page);
            let start = s.line_start(target);
            s.move_caret(start, shift);
        }
        v if v == VK_NEXT.0 as u32 => {
            let active = s.active;
            let page = s.tabs[active].view.visible_line_count();
            let max_first = if s.tabs[active].view.word_wrap {
                s.tabs[active]
                    .view
                    .total_visual_rows(&s.tabs[active].document)
                    .saturating_sub(1)
            } else {
                s.tabs[active]
                    .document
                    .lines()
                    .line_count()
                    .saturating_sub(1)
            };
            let next = (s.tabs[active].view.first_visible_line + page).min(max_first);
            s.tabs[active].view.first_visible_line = next;
            let line = s.tabs[active]
                .document
                .lines()
                .line_of_offset(s.tabs[active].view.caret.offset);
            let target = (line + page).min(max_first);
            let start = s.line_start(target);
            s.move_caret(start, shift);
        }
        v if v == VK_BACK.0 as u32 => s.delete_selection_or_char(true),
        v if v == VK_DELETE.0 as u32 => s.delete_selection_or_char(false),
        v if v == VK_TAB.0 as u32 => s.insert_text("\t"),
        _ => {}
    }
}

fn request_close_tab(hwnd: HWND, state_cell: &RefCell<AppState>, index: usize) {
    let dirty = {
        let s = state_cell.borrow();
        s.tabs
            .get(index)
            .map(|t| t.document.is_dirty())
            .unwrap_or(false)
    };
    if dirty {
        // MessageBox must not run under RefCell borrow — it pumps WM_PAINT/WM_TIMER.
        let answer = unsafe {
            MessageBoxW(
                hwnd,
                w!("Save changes to this tab before closing?"),
                w!("Fast Notepad"),
                MB_YESNOCANCEL | MB_ICONINFORMATION,
            )
        };
        match answer {
            IDYES => {
                state_cell.borrow_mut().active = index;
                handle_command(hwnd, state_cell, ID_FILE_SAVE);
                let still_dirty = state_cell
                    .borrow()
                    .tabs
                    .get(index)
                    .map(|t| t.document.is_dirty())
                    .unwrap_or(true);
                if still_dirty {
                    return;
                }
            }
            IDCANCEL => return,
            _ => {} // IDNO — discard changes
        }
    }
    state_cell.borrow_mut().close_tab(index);
    refresh_scrollbars_outside_borrow(state_cell);
}

enum DiskChangeAction {
    WarnDirty,
    AskReload { path: PathBuf, current: u64 },
}

fn poll_disk_changes(hwnd: HWND, state_cell: &RefCell<AppState>) {
    let action = {
        let mut s = state_cell.borrow_mut();
        let path = s.tab().document.path().map(|p| p.to_path_buf());
        let Some(path) = path else {
            return;
        };
        let Some(current) = file_write_secs(&path) else {
            return;
        };
        let tab = s.tab_mut();
        let Some(known) = tab.last_write_secs else {
            tab.last_write_secs = Some(current);
            return;
        };
        if current == known || tab.disk_change_notified {
            return;
        }
        tab.disk_change_notified = true;
        let dirty = tab.document.is_dirty();
        if dirty {
            DiskChangeAction::WarnDirty
        } else {
            DiskChangeAction::AskReload { path, current }
        }
    };

    match action {
        DiskChangeAction::WarnDirty => {
            show_warn(
                hwnd,
                "File Changed",
                "The file has changed on disk and you have unsaved edits.",
            );
        }
        DiskChangeAction::AskReload { path, current } => {
            let answer = unsafe {
                MessageBoxW(
                    hwnd,
                    w!("File changed on disk. Reload?"),
                    w!("Fast Notepad"),
                    MB_YESNO | MB_ICONWARNING,
                )
            };
            if answer == IDYES {
                let _ = state_cell.borrow_mut().open_path_replace_active(&path);
                refresh_scrollbars_outside_borrow(state_cell);
            } else {
                let mut s = state_cell.borrow_mut();
                s.tab_mut().last_write_secs = Some(current);
                s.tab_mut().disk_change_notified = false;
            }
        }
    }
}

fn handle_command(hwnd: HWND, state_cell: &RefCell<AppState>, id: usize) {
    if (ID_RECENT_BASE..ID_RECENT_BASE + 10).contains(&id) {
        let idx = id - ID_RECENT_BASE;
        let path = state_cell
            .borrow()
            .settings
            .recent
            .get(idx)
            .cloned();
        if let Some(path) = path {
            let p = PathBuf::from(&path);
            if !p.exists() {
                show_error(hwnd, "Recent file no longer exists.");
                let mut s = state_cell.borrow_mut();
                s.settings.recent.retain(|x| x != &path);
                s.settings.save();
                s.rebuild_recent_menu();
                return;
            }
            if let Err(err) = state_cell.borrow_mut().open_path(&p) {
                show_error(hwnd, &err);
            }
        }
        return;
    }

    match id {
        ID_FILE_NEW | ID_FILE_NEW_TAB => {
            state_cell.borrow_mut().new_tab();
        }
        ID_FILE_CLOSE_TAB => {
            let idx = state_cell.borrow().active;
            request_close_tab(hwnd, state_cell, idx);
        }
        ID_FILE_OPEN => {
            if let Some(path) = open_file_dialog(hwnd) {
                if let Err(err) = state_cell.borrow_mut().open_path(&path) {
                    show_error(hwnd, &err);
                }
            }
        }
        ID_FILE_SAVE => {
            let path = state_cell.borrow().tab().document.path().map(|p| p.to_path_buf());
            if let Some(path) = path {
                do_save(hwnd, state_cell, &path);
            } else {
                handle_command(hwnd, state_cell, ID_FILE_SAVE_AS);
            }
        }
        ID_FILE_SAVE_AS => {
            if let Some(path) = save_file_dialog(hwnd) {
                do_save(hwnd, state_cell, &path);
            }
        }
        ID_FILE_PRINT => {
            let s = state_cell.borrow();
            let _ = print_ui::print_document(hwnd, &s.tab().document, &s.tab().view, &s.renderer);
        }
        ID_FILE_EXIT => {
            if confirm_discard_all(hwnd, state_cell) {
                state_cell.borrow().settings.save();
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
        ID_EDIT_UNDO => {
            let mut s = state_cell.borrow_mut();
            let _ = s.tab_mut().document.undo();
            s.after_edit();
        }
        ID_EDIT_REDO => {
            let mut s = state_cell.borrow_mut();
            let _ = s.tab_mut().document.redo();
            s.after_edit();
        }
        ID_EDIT_CUT => state_cell.borrow_mut().cut_selection(),
        ID_EDIT_COPY => state_cell.borrow().copy_selection(),
        ID_EDIT_PASTE => state_cell.borrow_mut().paste(),
        ID_EDIT_SELECT_ALL => {
            let mut s = state_cell.borrow_mut();
            let len = s.tab().document.len();
            s.tab_mut().view.caret.select_all(len);
            invalidate(hwnd);
        }
        ID_EDIT_FIND | ID_EDIT_REPLACE => {
            let with_replace = id == ID_EDIT_REPLACE;
            let (find, repl, case) = {
                let s = state_cell.borrow();
                (
                    s.find_query.clone(),
                    s.replace_query.clone(),
                    s.find_case,
                )
            };
            if let Some(result) = find_ui::show_find_replace(hwnd, &find, &repl, case, with_replace)
            {
                let mut s = state_cell.borrow_mut();
                s.find_query = result.find;
                s.replace_query = result.replace;
                s.find_case = result.case_sensitive;
                match result.action {
                    find_ui::FindAction::FindNext => s.find_next_match(),
                    find_ui::FindAction::Replace => s.do_replace_next(),
                    find_ui::FindAction::ReplaceAll => s.do_replace_all(),
                }
            }
        }
        ID_EDIT_GOTO => {
            if let Some(line) = find_ui::prompt_goto_line(hwnd) {
                let mut s = state_cell.borrow_mut();
                let idx = line.saturating_sub(1);
                let start = s.tab().document.lines().line_start(idx).unwrap_or(0);
                s.move_caret(start, false);
            }
        }
        ID_FORMAT_FONT => {
            if let Some((face, size)) = find_ui::choose_font(hwnd) {
                let mut s = state_cell.borrow_mut();
                s.settings.font_face = face.clone();
                s.settings.font_size = size;
                s.settings.save();
                s.renderer.set_font(&face, size);
                let hdc = unsafe { GetDC(hwnd) };
                for tab in &mut s.tabs {
                    measure_metrics(hdc, &mut tab.view, &face, size);
                }
                unsafe {
                    ReleaseDC(hwnd, hdc);
                }
                invalidate(hwnd);
            }
        }
        ID_FORMAT_DOCUMENT => {
            format_active_document(hwnd, state_cell);
        }
        ID_LANG_PLAIN => set_language(hwnd, state_cell, LanguageId::Unknown),
        ID_LANG_MARKDOWN => set_language(hwnd, state_cell, LanguageId::Markdown),
        ID_LANG_JSON => set_language(hwnd, state_cell, LanguageId::Json),
        ID_VIEW_WRAP => {
            let mut s = state_cell.borrow_mut();
            s.settings.word_wrap = !s.settings.word_wrap;
            let wrap = s.settings.word_wrap;
            s.settings.save();
            for tab in &mut s.tabs {
                tab.view.word_wrap = wrap;
                tab.view.scroll_x = 0;
            }
            s.update_chrome_layout();
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
            }
            layout_scrollbars(&s, rect.right - rect.left, rect.bottom - rect.top);
            sync_view_menu(&s);
            s.refresh_scrollbars();
            invalidate(hwnd);
        }
        ID_VIEW_THEME => {
            let mut s = state_cell.borrow_mut();
            s.settings.dark_theme = !s.settings.dark_theme;
            let dark = s.settings.dark_theme;
            s.settings.save();
            s.renderer.set_theme(dark);
            sync_view_menu(&s);
            invalidate(hwnd);
        }
        _ => {}
    }
}

fn do_save(hwnd: HWND, state_cell: &RefCell<AppState>, path: &Path) {
    let encoding = state_cell.borrow().tab().encoding;
    let result = {
        let s = state_cell.borrow();
        save_document(&s.tab().document, path, encoding)
    };
    match result {
        Ok(()) => {
            let mut s = state_cell.borrow_mut();
            s.tab_mut().document.set_path(path.to_path_buf());
            s.tab_mut().document.set_dirty(false);
            s.tab_mut().last_write_secs = file_write_secs(path);
            s.tab_mut().disk_change_notified = false;
            if !s.tab().language_locked {
                let lang = detect_language_for(&s.tab().document, Some(path), &s.languages);
                s.tab_mut().language = lang;
            }
            s.settings.add_recent(path);
            s.rebuild_recent_menu();
            s.update_title();
            sync_language_menu(&s);
            invalidate(hwnd);
        }
        Err(err) => show_error(hwnd, &format!("Failed to save file:\n{err}")),
    }
}

fn set_language(hwnd: HWND, state_cell: &RefCell<AppState>, language: LanguageId) {
    let mut s = state_cell.borrow_mut();
    s.tab_mut().language = language;
    s.tab_mut().language_locked = true;
    sync_language_menu(&s);
    invalidate(hwnd);
}

fn format_active_document(hwnd: HWND, state_cell: &RefCell<AppState>) {
    let (language, len, ending) = {
        let s = state_cell.borrow();
        let tab = s.tab();
        (tab.language, tab.document.len(), tab.document.line_ending())
    };

    if language == LanguageId::Unknown {
        show_info(hwnd, "Format Document", "No formatter for Plain Text.");
        return;
    }
    if len > FORMAT_MAX_BYTES {
        show_error(
            hwnd,
            &format!(
                "Document is too large to format ({} bytes; limit is {}).",
                len, FORMAT_MAX_BYTES
            ),
        );
        return;
    }

    let text = {
        let s = state_cell.borrow();
        match s.tab().document.slice(0, len) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(err) => {
                show_error(hwnd, &format!("Failed to read document:\n{err}"));
                return;
            }
        }
    };

    let formatted = {
        let s = state_cell.borrow();
        let mode = s.languages.get(language);
        match mode.format(&text) {
            Ok(out) => apply_line_ending(&out, ending),
            Err(err) => {
                show_error(hwnd, &err);
                return;
            }
        }
    };

    if formatted == text {
        return;
    }

    let mut s = state_cell.borrow_mut();
    let doc = &mut s.tab_mut().document;
    if let Err(err) = doc.delete(0, doc.len()) {
        show_error(hwnd, &format!("Format failed:\n{err}"));
        return;
    }
    if let Err(err) = doc.insert(0, &formatted) {
        show_error(hwnd, &format!("Format failed:\n{err}"));
        return;
    }
    s.after_edit();
}

fn detect_language_for(
    doc: &fast_notepad::Document,
    path: Option<&Path>,
    registry: &LanguageRegistry,
) -> LanguageId {
    let n = doc.len().min(4096);
    let sample = doc.slice(0, n).unwrap_or_default();
    registry.detect(path, &sample)
}

fn apply_line_ending(text: &str, ending: LineEnding) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match ending {
        LineEnding::CrLf => normalized.replace('\n', "\r\n"),
        LineEnding::Lf | LineEnding::Mixed => normalized,
    }
}

fn confirm_discard_all(hwnd: HWND, state_cell: &RefCell<AppState>) -> bool {
    let dirty_tabs: Vec<usize> = state_cell
        .borrow()
        .tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| t.document.is_dirty())
        .map(|(i, _)| i)
        .collect();
    for idx in dirty_tabs {
        state_cell.borrow_mut().active = idx;
        state_cell.borrow_mut().update_title();
        let answer = unsafe {
            MessageBoxW(
                hwnd,
                w!("Save changes before continuing?"),
                w!("Fast Notepad"),
                MB_YESNOCANCEL | MB_ICONINFORMATION,
            )
        };
        match answer {
            IDYES => {
                handle_command(hwnd, state_cell, ID_FILE_SAVE);
                if state_cell.borrow().tabs[idx].document.is_dirty() {
                    return false;
                }
            }
            IDNO => {}
            _ => return false,
        }
    }
    true
}

fn open_file_dialog(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
        if dialog.Show(hwnd).is_err() {
            return None;
        }
        let item = dialog.GetResult().ok()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = path.to_string().ok()?;
        CoTaskMemFree(Some(path.0 as _));
        Some(PathBuf::from(s))
    }
}

fn save_file_dialog(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM);
        let _ = dialog.SetFileName(w!("Untitled.txt"));
        if dialog.Show(hwnd).is_err() {
            return None;
        }
        let item = dialog.GetResult().ok()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = path.to_string().ok()?;
        CoTaskMemFree(Some(path.0 as _));
        Some(PathBuf::from(s))
    }
}

fn file_write_secs(path: &Path) -> Option<u64> {
    unsafe {
        let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
        let wpath = wide(&path.to_string_lossy());
        GetFileAttributesExW(
            PCWSTR(wpath.as_ptr()),
            GetFileExInfoStandard,
            &mut data as *mut _ as *mut _,
        )
        .ok()?;
        let ft = data.ftLastWriteTime;
        Some(((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64)
    }
}

fn set_clipboard_text(hwnd: HWND, text: &str) -> Result<()> {
    unsafe {
        OpenClipboard(hwnd)?;
        EmptyClipboard()?;
        let data: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = data.len() * 2;
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
        let ptr = GlobalLock(hmem);
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, ptr as *mut u8, bytes);
            let _ = GlobalUnlock(hmem);
        }
        SetClipboardData(
            CF_UNICODETEXT.0 as u32,
            windows::Win32::Foundation::HANDLE(hmem.0),
        )?;
        CloseClipboard()?;
    }
    Ok(())
}

fn get_clipboard_text(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(hwnd).ok()?;
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let ptr = GlobalLock(windows::Win32::Foundation::HGLOBAL(handle.0));
        if ptr.is_null() {
            let _ = CloseClipboard();
            return None;
        }
        let mut len = 0usize;
        let p = ptr as *const u16;
        while *p.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(p, len);
        let text = String::from_utf16_lossy(slice);
        let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(handle.0));
        let _ = CloseClipboard();
        Some(text)
    }
}

fn show_error(hwnd: HWND, msg: &str) {
    unsafe {
        let _ = MessageBoxW(
            hwnd,
            PCWSTR(wide(msg).as_ptr()),
            w!("Error"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn show_info(hwnd: HWND, title: &str, msg: &str) {
    unsafe {
        let _ = MessageBoxW(
            hwnd,
            PCWSTR(wide(msg).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn show_warn(hwnd: HWND, title: &str, msg: &str) {
    unsafe {
        let _ = MessageBoxW(
            hwnd,
            PCWSTR(wide(msg).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
}

fn handle_drop_files(hwnd: HWND, state_cell: &RefCell<AppState>, hdrop: HDROP) {
    unsafe {
        let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            let written = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
            if written == 0 {
                continue;
            }
            let path = PathBuf::from(String::from_utf16_lossy(&buf[..written]));
            if !path.is_file() {
                continue;
            }
            if let Err(err) = state_cell.borrow_mut().open_path(&path) {
                show_error(hwnd, &err);
            }
        }
        DragFinish(hdrop);
    }
}

fn key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

fn prev_char_boundary(doc: &fast_notepad::buffer::Document, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let start = offset.saturating_sub(4);
    if let Ok(bytes) = doc.slice(start, offset) {
        let s = String::from_utf8_lossy(&bytes);
        if let Some((i, _)) = s.char_indices().next_back() {
            return start + i;
        }
    }
    offset - 1
}

fn next_char_boundary(doc: &fast_notepad::buffer::Document, offset: usize) -> usize {
    if offset >= doc.len() {
        return doc.len();
    }
    let end = (offset + 4).min(doc.len());
    if let Ok(bytes) = doc.slice(offset, end) {
        let s = String::from_utf8_lossy(&bytes);
        if let Some(c) = s.chars().next() {
            return offset + c.len_utf8();
        }
    }
    (offset + 1).min(doc.len())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
