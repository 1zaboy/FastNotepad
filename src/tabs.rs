use crate::buffer::Document;
use crate::encoding::FileEncoding;
use crate::language::LanguageId;
use crate::view::ViewState;

#[derive(Debug)]
pub struct Tab {
    pub document: Document,
    pub view: ViewState,
    pub encoding: FileEncoding,
    pub language: LanguageId,
    /// When true, Save As / path changes do not auto-redetect language.
    pub language_locked: bool,
    pub last_write_secs: Option<u64>,
    pub disk_change_notified: bool,
}

impl Tab {
    pub fn new_empty() -> Self {
        Self {
            document: Document::new(),
            view: ViewState::new(),
            encoding: FileEncoding::Utf8,
            language: LanguageId::Unknown,
            language_locked: false,
            last_write_secs: None,
            disk_change_notified: false,
        }
    }

    pub fn title(&self) -> String {
        let name = self
            .document
            .path()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        if self.document.is_dirty() {
            format!("*{name}")
        } else {
            name.to_string()
        }
    }

    pub fn is_blank(&self) -> bool {
        self.document.path().is_none() && !self.document.is_dirty() && self.document.is_empty()
    }
}

pub const TAB_HEIGHT: i32 = 28;
pub const STATUS_HEIGHT: i32 = 22;
/// Wider than the default Win32 non-client bars so they are easier to hit with the mouse.
pub const SCROLLBAR_THICKNESS: i32 = 12;
pub const TAB_MIN_WIDTH: i32 = 80;
pub const TAB_MAX_WIDTH: i32 = 160;

pub fn tab_at_x(x: i32, tab_count: usize, client_width: i32) -> Option<usize> {
    if tab_count == 0 {
        return None;
    }
    let width = (client_width / tab_count as i32).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let idx = (x / width) as usize;
    if idx < tab_count {
        Some(idx)
    } else {
        None
    }
}

pub fn close_button_hit(x: i32, tab_index: usize, tab_count: usize, client_width: i32) -> bool {
    if tab_count == 0 {
        return false;
    }
    let width = (client_width / tab_count as i32).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let left = tab_index as i32 * width;
    let close_left = left + width - 18;
    x >= close_left && x < left + width - 4
}
