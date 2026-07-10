use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetStockObject, DEFAULT_CHARSET, FONT_CHARSET, FW_NORMAL, LOGFONTW, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{
    ChooseFontW, CF_FORCEFONTEXIST, CF_INITTOLOGFONTSTRUCT, CF_SCREENFONTS, CHOOSEFONTW,
};
use windows::Win32::UI::Controls::BST_CHECKED;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowTextW,
    IsDialogMessageW, LoadCursorW, RegisterClassW, SendMessageW, ShowWindow, TranslateMessage,
    BM_GETCHECK, BM_SETCHECK, CS_HREDRAW, CS_VREDRAW, HMENU, IDC_ARROW, MSG, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_VISIBLE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAction {
    FindNext,
    Replace,
    ReplaceAll,
}

#[derive(Debug, Clone)]
pub struct FindReplaceResult {
    pub find: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub action: FindAction,
}

thread_local! {
    static RESULT: RefCell<Option<FindReplaceResult>> = const { RefCell::new(None) };
    static DONE: RefCell<bool> = const { RefCell::new(false) };
    static FIND_EDIT: RefCell<HWND> = const { RefCell::new(HWND(std::ptr::null_mut())) };
    static REPL_EDIT: RefCell<HWND> = const { RefCell::new(HWND(std::ptr::null_mut())) };
    static CASE_BTN: RefCell<HWND> = const { RefCell::new(HWND(std::ptr::null_mut())) };
    static WITH_REPLACE: RefCell<bool> = const { RefCell::new(false) };
}

pub fn show_find_replace(
    owner: HWND,
    find: &str,
    replace: &str,
    case_sensitive: bool,
    with_replace: bool,
) -> Option<FindReplaceResult> {
    unsafe {
        RESULT.with(|r| *r.borrow_mut() = None);
        DONE.with(|d| *d.borrow_mut() = false);
        WITH_REPLACE.with(|w| *w.borrow_mut() = with_replace);

        let hinstance = GetModuleHandleW(None).ok()?;
        let class_name = w!("FastNotepadFindReplace");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dlg_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let height = if with_replace { 200 } else { 150 };
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            if with_replace {
                w!("Find / Replace")
            } else {
                w!("Find")
            },
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE | WS_POPUP,
            300,
            220,
            420,
            height,
            owner,
            None,
            hinstance,
            None,
        )
        .ok()?;

        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Find:"),
            WS_CHILD | WS_VISIBLE,
            12,
            14,
            60,
            20,
            hwnd,
            None,
            hinstance,
            None,
        );
        let find_edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR(wide(find).as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(0x80),
            70,
            12,
            320,
            24,
            hwnd,
            HMENU(100usize as _),
            hinstance,
            None,
        )
        .ok()?;
        FIND_EDIT.with(|e| *e.borrow_mut() = find_edit);

        if with_replace {
            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Replace:"),
                WS_CHILD | WS_VISIBLE,
                12,
                48,
                60,
                20,
                hwnd,
                None,
                hinstance,
                None,
            );
            let repl = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                PCWSTR(wide(replace).as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(0x80),
                70,
                46,
                320,
                24,
                hwnd,
                HMENU(101usize as _),
                hinstance,
                None,
            )
            .ok()?;
            REPL_EDIT.with(|e| *e.borrow_mut() = repl);
        }

        let case_y = if with_replace { 80 } else { 48 };
        let case_btn = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Match case"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x00000003),
            70,
            case_y,
            120,
            22,
            hwnd,
            HMENU(102usize as _),
            hinstance,
            None,
        )
        .ok()?;
        CASE_BTN.with(|e| *e.borrow_mut() = case_btn);
        if case_sensitive {
            let _ = SendMessageW(
                case_btn,
                BM_SETCHECK,
                WPARAM(BST_CHECKED.0 as usize),
                LPARAM(0),
            );
        }

        let btn_y = if with_replace { 120 } else { 80 };
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Find Next"),
            WS_CHILD | WS_VISIBLE,
            70,
            btn_y,
            90,
            28,
            hwnd,
            HMENU(201usize as _),
            hinstance,
            None,
        );
        if with_replace {
            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("Replace"),
                WS_CHILD | WS_VISIBLE,
                170,
                btn_y,
                90,
                28,
                hwnd,
                HMENU(202usize as _),
                hinstance,
                None,
            );
            let _ = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("Replace All"),
                WS_CHILD | WS_VISIBLE,
                270,
                btn_y,
                100,
                28,
                hwnd,
                HMENU(203usize as _),
                hinstance,
                None,
            );
        }
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Cancel"),
            WS_CHILD | WS_VISIBLE,
            320,
            btn_y,
            70,
            28,
            hwnd,
            HMENU(204usize as _),
            hinstance,
            None,
        );

        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                if DONE.with(|d| *d.borrow()) {
                    break;
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if DONE.with(|d| *d.borrow()) {
                break;
            }
        }
        DONE.with(|d| *d.borrow_mut() = false);
        RESULT.with(|r| r.borrow().clone())
    }
}

unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id {
                201 | 202 | 203 => {
                    let find = read_edit(FIND_EDIT.with(|e| *e.borrow()));
                    let replace = if WITH_REPLACE.with(|w| *w.borrow()) {
                        read_edit(REPL_EDIT.with(|e| *e.borrow()))
                    } else {
                        String::new()
                    };
                    let case_btn = CASE_BTN.with(|e| *e.borrow());
                    let checked = SendMessageW(case_btn, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                        == BST_CHECKED.0 as isize;
                    let action = match id {
                        201 => FindAction::FindNext,
                        202 => FindAction::Replace,
                        _ => FindAction::ReplaceAll,
                    };
                    RESULT.with(|r| {
                        *r.borrow_mut() = Some(FindReplaceResult {
                            find,
                            replace,
                            case_sensitive: checked,
                            action,
                        });
                    });
                    DONE.with(|d| *d.borrow_mut() = true);
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                204 => {
                    RESULT.with(|r| *r.borrow_mut() = None);
                    DONE.with(|d| *d.borrow_mut() = true);
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        WM_CLOSE | WM_DESTROY => {
            DONE.with(|d| *d.borrow_mut() = true);
            if msg == WM_CLOSE {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn read_edit(hwnd: HWND) -> String {
    unsafe {
        let mut buf = vec![0u16; 1024];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

pub fn prompt_goto_line(owner: HWND) -> Option<usize> {
    unsafe {
        DONE.with(|d| *d.borrow_mut() = false);
        let hinstance = GetModuleHandleW(None).ok()?;
        let class_name = w!("FastNotepadGoto");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(goto_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Go to Line"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE | WS_POPUP,
            350,
            280,
            280,
            130,
            owner,
            None,
            hinstance,
            None,
        )
        .ok()?;
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!("1"),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            20,
            20,
            220,
            24,
            hwnd,
            HMENU(100usize as _),
            hinstance,
            None,
        )
        .ok()?;
        FIND_EDIT.with(|e| *e.borrow_mut() = edit);
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Go"),
            WS_CHILD | WS_VISIBLE,
            80,
            60,
            70,
            28,
            hwnd,
            HMENU(201usize as _),
            hinstance,
            None,
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if DONE.with(|d| *d.borrow()) {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DONE.with(|d| *d.borrow_mut() = false);
        let text = read_edit(FIND_EDIT.with(|e| *e.borrow()));
        text.trim().parse().ok()
    }
}

unsafe extern "system" fn goto_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND if (wparam.0 & 0xFFFF) == 201 => {
            DONE.with(|d| *d.borrow_mut() = true);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            DONE.with(|d| *d.borrow_mut() = true);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn choose_font(owner: HWND) -> Option<(String, i32)> {
    unsafe {
        let mut lf = LOGFONTW {
            lfHeight: -16,
            lfWeight: FW_NORMAL.0 as i32,
            lfCharSet: FONT_CHARSET(DEFAULT_CHARSET.0 as u8),
            ..Default::default()
        };
        let face = wide("Consolas");
        for (i, ch) in face.iter().take(31).enumerate() {
            lf.lfFaceName[i] = *ch;
        }
        let mut cf = CHOOSEFONTW {
            lStructSize: std::mem::size_of::<CHOOSEFONTW>() as u32,
            hwndOwner: owner,
            lpLogFont: &mut lf,
            Flags: CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_FORCEFONTEXIST,
            ..Default::default()
        };
        if !ChooseFontW(&mut cf as *mut _).as_bool() {
            return None;
        }
        let name_len = lf
            .lfFaceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(lf.lfFaceName.len());
        let face = String::from_utf16_lossy(&lf.lfFaceName[..name_len]);
        let size = if lf.lfHeight < 0 {
            -lf.lfHeight
        } else {
            lf.lfHeight
        };
        Some((face, size.max(8)))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
