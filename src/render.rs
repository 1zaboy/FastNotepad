use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush, DeleteDC,
    DeleteObject, FillRect, GetTextExtentPoint32W, IntersectClipRect, RestoreDC, SaveDC,
    SelectObject, SetBkMode, SetTextColor, TextOutW, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, FF_MODERN, FW_NORMAL, HBITMAP, HDC, HGDIOBJ, OUT_TT_PRECIS,
    SRCCOPY, TRANSPARENT,
};

use fast_notepad::buffer::Document;
use fast_notepad::encoding::FileEncoding;
use fast_notepad::language::{HighlightState, LanguageId, LanguageMode, TokenKind, TokenSpan};
use fast_notepad::tabs::{Tab, STATUS_HEIGHT, TAB_HEIGHT, TAB_MAX_WIDTH, TAB_MIN_WIDTH};
use fast_notepad::view::ViewState;

#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub bg: COLORREF,
    pub text: COLORREF,
    pub gutter_bg: COLORREF,
    pub gutter_text: COLORREF,
    pub selection: COLORREF,
    pub caret: COLORREF,
    pub tab_bg: COLORREF,
    pub tab_active: COLORREF,
    pub tab_text: COLORREF,
    pub status_bg: COLORREF,
    pub status_text: COLORREF,
    pub tok_keyword: COLORREF,
    pub tok_string: COLORREF,
    pub tok_number: COLORREF,
    pub tok_comment: COLORREF,
    pub tok_punctuation: COLORREF,
    pub tok_heading: COLORREF,
    pub tok_emphasis: COLORREF,
    pub tok_code: COLORREF,
    pub tok_link: COLORREF,
}

impl ThemeColors {
    pub fn light() -> Self {
        Self {
            bg: COLORREF(0x00FFFFFF),
            text: COLORREF(0x00000000),
            gutter_bg: COLORREF(0x00F0F0F0),
            gutter_text: COLORREF(0x00808080),
            selection: COLORREF(0x00FFD080),
            caret: COLORREF(0x00000000),
            tab_bg: COLORREF(0x00E8E8E8),
            tab_active: COLORREF(0x00FFFFFF),
            tab_text: COLORREF(0x00000000),
            status_bg: COLORREF(0x00F0F0F0),
            status_text: COLORREF(0x00000000),
            tok_keyword: COLORREF(0x00A00000),
            tok_string: COLORREF(0x00008000),
            tok_number: COLORREF(0x00808000),
            tok_comment: COLORREF(0x00808080),
            tok_punctuation: COLORREF(0x00404040),
            tok_heading: COLORREF(0x00A00000),
            tok_emphasis: COLORREF(0x00800080),
            tok_code: COLORREF(0x000000A0),
            tok_link: COLORREF(0x00FF0000),
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: COLORREF(0x001E1E1E),
            text: COLORREF(0x00D4D4D4),
            gutter_bg: COLORREF(0x00252525),
            gutter_text: COLORREF(0x00808080),
            selection: COLORREF(0x00264F78),
            caret: COLORREF(0x00FFFFFF),
            tab_bg: COLORREF(0x002D2D2D),
            tab_active: COLORREF(0x001E1E1E),
            tab_text: COLORREF(0x00D4D4D4),
            status_bg: COLORREF(0x000077BD),
            status_text: COLORREF(0x00FFFFFF),
            tok_keyword: COLORREF(0x00DCDCAA),
            tok_string: COLORREF(0x009CE07B),
            tok_number: COLORREF(0x00B5CEA8),
            tok_comment: COLORREF(0x006A9955),
            tok_punctuation: COLORREF(0x00D4D4D4),
            tok_heading: COLORREF(0x00569CD6),
            tok_emphasis: COLORREF(0x00C586C0),
            tok_code: COLORREF(0x00CE9178),
            tok_link: COLORREF(0x004FC1FF),
        }
    }

    pub fn color_for(&self, kind: TokenKind) -> COLORREF {
        match kind {
            TokenKind::Text => self.text,
            TokenKind::Keyword => self.tok_keyword,
            TokenKind::String => self.tok_string,
            TokenKind::Number => self.tok_number,
            TokenKind::Comment => self.tok_comment,
            TokenKind::Punctuation => self.tok_punctuation,
            TokenKind::Heading => self.tok_heading,
            TokenKind::Emphasis => self.tok_emphasis,
            TokenKind::Code => self.tok_code,
            TokenKind::Link => self.tok_link,
        }
    }
}

pub struct Renderer {
    font: windows::Win32::Graphics::Gdi::HFONT,
    pub face: String,
    pub size: i32,
    pub theme: ThemeColors,
    /// Reused across paints so fast scrolling does not allocate a new bitmap every frame.
    back_dc: HDC,
    back_bmp: HBITMAP,
    back_old: HGDIOBJ,
    back_w: i32,
    back_h: i32,
}

impl Renderer {
    pub fn new(face: &str, size: i32, dark: bool) -> anyhow::Result<Self> {
        let font = create_font(face, size);
        Ok(Self {
            font,
            face: face.to_string(),
            size,
            theme: if dark {
                ThemeColors::dark()
            } else {
                ThemeColors::light()
            },
            back_dc: HDC::default(),
            back_bmp: HBITMAP::default(),
            back_old: HGDIOBJ::default(),
            back_w: 0,
            back_h: 0,
        })
    }

    fn release_backbuffer(&mut self) {
        unsafe {
            if !self.back_dc.is_invalid() {
                if !self.back_old.is_invalid() {
                    SelectObject(self.back_dc, self.back_old);
                }
                if !self.back_bmp.is_invalid() {
                    let _ = DeleteObject(self.back_bmp);
                }
                let _ = DeleteDC(self.back_dc);
            }
            self.back_dc = HDC::default();
            self.back_bmp = HBITMAP::default();
            self.back_old = HGDIOBJ::default();
            self.back_w = 0;
            self.back_h = 0;
        }
    }

    fn ensure_backbuffer(&mut self, hdc: HDC, w: i32, h: i32) {
        if w == self.back_w && h == self.back_h && !self.back_dc.is_invalid() {
            return;
        }
        self.release_backbuffer();
        unsafe {
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, w, h);
            let old = SelectObject(mem, bmp);
            self.back_dc = mem;
            self.back_bmp = bmp;
            self.back_old = old;
            self.back_w = w;
            self.back_h = h;
        }
    }

    pub fn set_theme(&mut self, dark: bool) {
        self.theme = if dark {
            ThemeColors::dark()
        } else {
            ThemeColors::light()
        };
    }

    pub fn set_font(&mut self, face: &str, size: i32) {
        unsafe {
            let _ = DeleteObject(self.font);
        }
        self.font = create_font(face, size);
        self.face = face.to_string();
        self.size = size;
    }

    pub fn paint_chrome(
        &self,
        hdc: HDC,
        tabs: &[Tab],
        active: usize,
        client_w: i32,
        client_h: i32,
        status: &str,
    ) {
        unsafe {
            // Tab strip
            let tab_brush = CreateSolidBrush(self.theme.tab_bg);
            let tab_rect = RECT {
                left: 0,
                top: 0,
                right: client_w,
                bottom: TAB_HEIGHT,
            };
            FillRect(hdc, &tab_rect, tab_brush);
            let _ = DeleteObject(tab_brush);

            let old_font = SelectObject(hdc, self.font);
            SetBkMode(hdc, TRANSPARENT);
            let count = tabs.len().max(1);
            let width = (client_w / count as i32).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
            for (i, tab) in tabs.iter().enumerate() {
                let left = i as i32 * width;
                let bg = if i == active {
                    self.theme.tab_active
                } else {
                    self.theme.tab_bg
                };
                let brush = CreateSolidBrush(bg);
                let r = RECT {
                    left,
                    top: 0,
                    right: left + width - 1,
                    bottom: TAB_HEIGHT,
                };
                FillRect(hdc, &r, brush);
                let _ = DeleteObject(brush);
                SetTextColor(hdc, self.theme.tab_text);
                let title = tab.title();
                let short = if title.chars().count() > 18 {
                    format!("{}…", title.chars().take(16).collect::<String>())
                } else {
                    title
                };
                let tw = wide(&short);
                let _ = TextOutW(hdc, left + 8, 6, &tw[..tw.len() - 1]);
                let close = wide("×");
                let _ = TextOutW(hdc, left + width - 16, 6, &close[..close.len() - 1]);
            }

            // Status bar
            let status_brush = CreateSolidBrush(self.theme.status_bg);
            let sr = RECT {
                left: 0,
                top: client_h - STATUS_HEIGHT,
                right: client_w,
                bottom: client_h,
            };
            FillRect(hdc, &sr, status_brush);
            let _ = DeleteObject(status_brush);
            SetTextColor(hdc, self.theme.status_text);
            let sw = wide(status);
            let _ = TextOutW(
                hdc,
                8,
                client_h - STATUS_HEIGHT + 4,
                &sw[..sw.len() - 1],
            );

            // Corner where vertical and horizontal scrollbars meet.
            if let Some(view) = tabs.get(active).map(|t| &t.view) {
                if view.content_right > 0 && view.content_bottom > STATUS_HEIGHT {
                    let corner_brush = CreateSolidBrush(self.theme.tab_bg);
                    let cr = RECT {
                        left: client_w - view.content_right,
                        top: client_h - view.content_bottom,
                        right: client_w,
                        bottom: client_h - STATUS_HEIGHT,
                    };
                    FillRect(hdc, &cr, corner_brush);
                    let _ = DeleteObject(corner_brush);
                }
            }

            SelectObject(hdc, old_font);
        }
    }

    pub fn paint_editor(
        &self,
        hdc: HDC,
        doc: &Document,
        view: &ViewState,
        mode: &dyn LanguageMode,
    ) {
        unsafe {
            let content_bottom_y = view.client_height - view.content_bottom;
            let editor_right = view.editor_right();
            let saved = SaveDC(hdc);
            IntersectClipRect(
                hdc,
                0,
                view.content_top,
                editor_right,
                content_bottom_y,
            );

            let brush = CreateSolidBrush(self.theme.bg);
            let mut rect = RECT {
                left: 0,
                top: view.content_top,
                right: editor_right,
                bottom: content_bottom_y,
            };
            FillRect(hdc, &rect, brush);
            let _ = DeleteObject(brush);

            let gutter_brush = CreateSolidBrush(self.theme.gutter_bg);
            rect.right = view.gutter_width.min(editor_right);
            FillRect(hdc, &rect, gutter_brush);
            let _ = DeleteObject(gutter_brush);

            let old_font = SelectObject(hdc, self.font);
            SetBkMode(hdc, TRANSPARENT);

            if view.word_wrap {
                self.paint_wrapped(hdc, doc, view, mode);
            } else {
                self.paint_unwrapped(hdc, doc, view, mode);
            }

            SelectObject(hdc, old_font);
            let _ = RestoreDC(hdc, saved);
        }
    }

    /// Paint the full client area into `hdc` via an offscreen bitmap (no flicker).
    pub fn paint_frame(
        &mut self,
        hdc: HDC,
        tabs: &[Tab],
        active: usize,
        doc: &Document,
        view: &ViewState,
        status: &str,
        mode: &dyn LanguageMode,
    ) {
        let w = view.client_width.max(1);
        let h = view.client_height.max(1);
        self.ensure_backbuffer(hdc, w, h);
        let mem = self.back_dc;

        // Clear full bitmap first — scrollbar gutters are not always filled by
        // paint_editor, and CreateCompatibleBitmap leaves uninitialized (often black) pixels.
        unsafe {
            let brush = CreateSolidBrush(self.theme.tab_bg);
            let full = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            FillRect(mem, &full, brush);
            let _ = DeleteObject(brush);
        }

        // Editor first, chrome on top so status/tabs are never covered by text.
        self.paint_editor(mem, doc, view, mode);
        self.paint_chrome(mem, tabs, active, w, h, status);

        unsafe {
            let _ = BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        }
    }

    unsafe fn paint_unwrapped(
        &self,
        hdc: HDC,
        doc: &Document,
        view: &ViewState,
        mode: &dyn LanguageMode,
    ) {
        let first = view.first_visible_line;
        let last = (first + view.visible_line_count() + 1).min(doc.lines().line_count());
        let content_bottom_y = view.client_height - view.content_bottom;

        if view.caret.has_selection() {
            self.paint_selection_unwrapped(hdc, doc, view, first, last);
        }

        let mut state = seed_highlight_state(doc, mode, first);

        for line in first..last {
            let y = view.line_top(line) as i32;
            if y >= content_bottom_y {
                break;
            }
            self.draw_gutter_number(hdc, view, line + 1, y);
            if let Ok(content) = doc.line_content(line) {
                let trimmed = content.trim_end_matches(['\r', '\n']);
                let spans = mode.highlight_line(trimmed, &mut state);
                self.draw_spans(
                    hdc,
                    trimmed,
                    &spans,
                    view.text_left() - view.scroll_x,
                    y,
                    view.char_width,
                    0,
                    trimmed.len(),
                );
            }
        }

        self.paint_caret_unwrapped(hdc, doc, view, first, last);
    }

    unsafe fn paint_wrapped(
        &self,
        hdc: HDC,
        doc: &Document,
        view: &ViewState,
        mode: &dyn LanguageMode,
    ) {
        let mut visual = 0usize;
        let first = view.first_visible_line;
        let visible = view.visible_line_count() + 2;
        let mut painted = 0usize;

        // First logical line that intersects the visible visual range.
        let mut seed_logical = 0usize;
        {
            let mut v = 0usize;
            for logical in 0..doc.lines().line_count() {
                let rows = view.visual_rows_for_logical(doc, logical);
                if v + rows.len() > first {
                    seed_logical = logical;
                    break;
                }
                v += rows.len();
            }
        }
        let mut state = seed_highlight_state(doc, mode, seed_logical);

        for logical in 0..doc.lines().line_count() {
            let line_start = doc.lines().line_start(logical).unwrap_or(0);
            let line_text = doc.line_content(logical).unwrap_or_default();
            let trimmed = line_text.trim_end_matches(['\r', '\n']);
            // State already advanced through seed_logical via seed_highlight_state;
            // continue from seed_logical onward.
            let spans = if logical >= seed_logical {
                mode.highlight_line(trimmed, &mut state)
            } else {
                Vec::new()
            };

            let rows = view.visual_rows_for_logical(doc, logical);
            for (a, b) in rows {
                if visual < first {
                    visual += 1;
                    continue;
                }
                if painted >= visible {
                    return;
                }
                let y = view.content_top as f32
                    + (visual - first) as f32 * view.line_height;
                let y = y as i32;
                if y >= view.client_height - view.content_bottom {
                    return;
                }
                if painted == 0 || a == line_start {
                    self.draw_gutter_number(hdc, view, logical + 1, y);
                }
                let local_a = a.saturating_sub(line_start).min(trimmed.len());
                let local_b = b.saturating_sub(line_start).min(trimmed.len());
                self.draw_spans(
                    hdc,
                    trimmed,
                    &spans,
                    view.text_left(),
                    y,
                    view.char_width,
                    local_a,
                    local_b,
                );
                let off = view.caret.offset;
                if off >= a && off <= b {
                    let col = if let Ok(bytes) = doc.slice(a, off) {
                        String::from_utf8_lossy(&bytes).chars().count()
                    } else {
                        off - a
                    };
                    let x = view.text_left() + (col as f32 * view.char_width) as i32;
                    let caret_brush = CreateSolidBrush(self.theme.caret);
                    let r = RECT {
                        left: x,
                        top: y,
                        right: x + 2,
                        bottom: y + view.line_height as i32,
                    };
                    FillRect(hdc, &r, caret_brush);
                    let _ = DeleteObject(caret_brush);
                }
                visual += 1;
                painted += 1;
            }
        }
    }

    unsafe fn draw_spans(
        &self,
        hdc: HDC,
        line: &str,
        spans: &[TokenSpan],
        base_x: i32,
        y: i32,
        char_width: f32,
        clip_start: usize,
        clip_end: usize,
    ) {
        if clip_start >= clip_end || line.is_empty() {
            return;
        }
        for span in spans {
            let start = span.start.max(clip_start);
            let end = span.end.min(clip_end).min(line.len());
            if start >= end {
                continue;
            }
            // Align to char boundaries
            let start = floor_char_boundary(line, start);
            let end = ceil_char_boundary(line, end);
            if start >= end {
                continue;
            }
            let Some(piece) = line.get(start..end) else {
                continue;
            };
            let col = line.get(..start).map(|s| s.chars().count()).unwrap_or(0);
            let x = base_x + (col as f32 * char_width) as i32;
            SetTextColor(hdc, self.theme.color_for(span.kind));
            let tw = wide(piece);
            let _ = TextOutW(hdc, x, y, &tw[..tw.len().saturating_sub(1)]);
        }
    }

    unsafe fn paint_selection_unwrapped(
        &self,
        hdc: HDC,
        doc: &Document,
        view: &ViewState,
        first: usize,
        last: usize,
    ) {
        let (sel_start, sel_end) = view.caret.selection_range();
        let start_line = doc.lines().line_of_offset(sel_start);
        let end_line = doc.lines().line_of_offset(sel_end);
        let sel_brush = CreateSolidBrush(self.theme.selection);
        for line in start_line..=end_line {
            if line < first || line >= last {
                continue;
            }
            let y = view.line_top(line) as i32;
            let line_start = doc.lines().line_start(line).unwrap_or(0);
            let line_end = doc
                .lines()
                .line_start(line + 1)
                .unwrap_or(doc.len());
            let a = sel_start.max(line_start);
            let b = sel_end.min(line_end);
            if a >= b {
                continue;
            }
            let x1 = view.text_left() - view.scroll_x
                + ((a - line_start) as f32 * view.char_width) as i32;
            let x2 = view.text_left() - view.scroll_x
                + ((b - line_start) as f32 * view.char_width) as i32;
            let r = RECT {
                left: x1.max(view.gutter_width),
                top: y,
                right: x2.max(x1 + 2),
                bottom: y + view.line_height as i32,
            };
            FillRect(hdc, &r, sel_brush);
        }
        let _ = DeleteObject(sel_brush);
    }

    unsafe fn paint_caret_unwrapped(
        &self,
        hdc: HDC,
        doc: &Document,
        view: &ViewState,
        first: usize,
        last: usize,
    ) {
        let caret_line = doc.lines().line_of_offset(view.caret.offset);
        if caret_line >= first && caret_line < last {
            let line_start = doc.lines().line_start(caret_line).unwrap_or(0);
            let col = if let Ok(bytes) = doc.slice(line_start, view.caret.offset) {
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .filter(|c| *c != '\r' && *c != '\n')
                    .count()
            } else {
                view.caret.offset.saturating_sub(line_start)
            };
            let x = view.text_left() - view.scroll_x + (col as f32 * view.char_width) as i32;
            let y = view.line_top(caret_line) as i32;
            let caret_brush = CreateSolidBrush(self.theme.caret);
            let r = RECT {
                left: x,
                top: y,
                right: x + 2,
                bottom: y + view.line_height as i32,
            };
            FillRect(hdc, &r, caret_brush);
            let _ = DeleteObject(caret_brush);
        }
    }

    unsafe fn draw_gutter_number(&self, hdc: HDC, view: &ViewState, number: usize, y: i32) {
        SetTextColor(hdc, self.theme.gutter_text);
        let number = format!("{number}");
        let number_w = wide(&number);
        let _ = TextOutW(hdc, 4, y, &number_w[..number_w.len().saturating_sub(1)]);
        let _ = view;
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.release_backbuffer();
        unsafe {
            let _ = DeleteObject(self.font);
        }
    }
}

pub fn measure_metrics(hdc: HDC, view: &mut ViewState, face: &str, size: i32) {
    unsafe {
        let mem = CreateCompatibleDC(hdc);
        let font = create_font(face, size);
        let old = SelectObject(mem, font);
        let mut sz = SIZE::default();
        let sample = wide("M");
        let _ = GetTextExtentPoint32W(mem, &sample[..sample.len() - 1], &mut sz);
        view.char_width = sz.cx as f32;
        view.line_height = sz.cy as f32;
        SelectObject(mem, old);
        let _ = DeleteObject(font);
        let _ = DeleteDC(mem);
    }
    let _ = hdc;
}

fn create_font(face: &str, size: i32) -> windows::Win32::Graphics::Gdi::HFONT {
    let name = wide(face);
    unsafe {
        CreateFontW(
            size,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_TT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (DEFAULT_PITCH.0 | FF_MODERN.0) as u32,
            PCWSTR::from_raw(name.as_ptr()),
        )
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn invalidate(hwnd: HWND) {
    unsafe {
        // false = do not erase; we fully paint via double-buffer in WM_PAINT.
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
    }
}

/// Invalidate only the text area so tabs/status/scrollbars are not repainted on every scroll tick.
pub fn invalidate_editor(hwnd: HWND, view: &ViewState) {
    let rect = RECT {
        left: 0,
        top: view.content_top,
        right: view.editor_right(),
        bottom: (view.client_height - view.content_bottom).max(view.content_top),
    };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, Some(&rect), false);
    }
}

pub fn status_text(
    doc: &Document,
    view: &ViewState,
    encoding: FileEncoding,
    language: LanguageId,
) -> String {
    let (line, col) = ViewState::line_col_at(doc, view.caret.offset);
    let ending = match doc.line_ending() {
        fast_notepad::buffer::LineEnding::Lf => "LF",
        fast_notepad::buffer::LineEnding::CrLf => "CRLF",
        fast_notepad::buffer::LineEnding::Mixed => "Mixed",
    };
    let wrap = if view.word_wrap { "Wrap" } else { "No Wrap" };
    format!(
        "Ln {line}, Col {col}  |  {}  |  {ending}  |  {wrap}  |  {}",
        encoding.label(),
        language.display_name()
    )
}

/// Seed lexer state from up to 64 lines before `first_line` (plan v1 approximation).
fn seed_highlight_state(
    doc: &Document,
    mode: &dyn LanguageMode,
    first_line: usize,
) -> HighlightState {
    let mut state = HighlightState::default();
    let start = first_line.saturating_sub(64);
    for line in start..first_line {
        if let Ok(content) = doc.line_content(line) {
            let trimmed = content.trim_end_matches(['\r', '\n']);
            let _ = mode.highlight_line(trimmed, &mut state);
        }
    }
    state
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}
