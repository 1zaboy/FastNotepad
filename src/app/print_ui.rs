use anyhow::Result;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteDC, DeleteObject, GetDeviceCaps, SelectObject, TextOutW, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_MODERN, FW_NORMAL, HORZRES,
    LOGPIXELSY, OUT_TT_PRECIS, VERTRES,
};
use windows::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
use windows::Win32::UI::Controls::Dialogs::{PrintDlgW, PD_RETURNDC, PRINTDLGW};

use crate::render::Renderer;
use fast_notepad::buffer::Document;
use fast_notepad::view::ViewState;

pub fn print_document(
    hwnd: HWND,
    doc: &Document,
    view: &ViewState,
    renderer: &Renderer,
) -> Result<()> {
    unsafe {
        let mut pd = PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            hwndOwner: hwnd,
            Flags: PD_RETURNDC,
            nFromPage: 1,
            nToPage: 1,
            nMinPage: 1,
            nMaxPage: 1,
            nCopies: 1,
            ..Default::default()
        };
        if !PrintDlgW(&mut pd as *mut _).as_bool() {
            return Ok(());
        }
        let hdc = pd.hDC;
        let page_w = GetDeviceCaps(hdc, HORZRES);
        let page_h = GetDeviceCaps(hdc, VERTRES);
        let dpi = GetDeviceCaps(hdc, LOGPIXELSY);
        let font_px = ((renderer.size as f32) * (dpi as f32) / 72.0).round() as i32;
        let face = wide(&renderer.face);
        let font = CreateFontW(
            font_px,
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
            PCWSTR(face.as_ptr()),
        );
        let old = SelectObject(hdc, font);
        let line_h = font_px + 4;
        let lines_per_page = ((page_h - 80) / line_h).max(1) as usize;
        let margin = 40;

        let doc_name = wide("Fast Notepad");
        let di = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(doc_name.as_ptr()),
            ..Default::default()
        };
        if StartDocW(hdc, &di) <= 0 {
            SelectObject(hdc, old);
            let _ = DeleteObject(font);
            return Ok(());
        }

        let total_lines = doc.lines().line_count().max(1);
        let mut line = 0usize;
        while line < total_lines {
            if StartPage(hdc) <= 0 {
                break;
            }
            for row in 0..lines_per_page {
                if line >= total_lines {
                    break;
                }
                if let Ok(content) = doc.line_content(line) {
                    let trimmed = content.trim_end_matches(['\r', '\n']);
                    let max_chars =
                        ((page_w - margin * 2) as f32 / (font_px as f32 * 0.55)).floor() as usize;
                    let text: String = if view.word_wrap && trimmed.chars().count() > max_chars {
                        trimmed.chars().take(max_chars).collect()
                    } else {
                        trimmed.to_string()
                    };
                    let tw = wide(&text);
                    let _ = TextOutW(
                        hdc,
                        margin,
                        margin + row as i32 * line_h,
                        &tw[..tw.len().saturating_sub(1)],
                    );
                }
                line += 1;
            }
            let _ = EndPage(hdc);
        }
        let _ = EndDoc(hdc);
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
        let _ = DeleteDC(hdc);
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
