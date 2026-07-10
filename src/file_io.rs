use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use memmap2::Mmap;

use crate::buffer::{Document, LineEnding, LineIndex, PieceTable};
use crate::encoding::{decode_to_utf8, detect_encoding, encode_chunk_from_utf8, strip_bom, FileEncoding};

const INDEX_CHUNK: usize = 8 * 1024 * 1024;

pub struct OpenedFile {
    pub document: Document,
    pub encoding: FileEncoding,
    pub had_decode_errors: bool,
}

pub fn open_file(path: &Path) -> Result<OpenedFile> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[..];
    let encoding = detect_encoding(bytes);

    let (document, had_decode_errors) =
        if matches!(encoding, FileEncoding::Utf8 | FileEncoding::Utf8Bom) {
            let bom_len = if matches!(encoding, FileEncoding::Utf8Bom) {
                3
            } else {
                0
            };
            let payload = strip_bom(bytes, encoding);
            let line_ending = LineEnding::detect(payload);
            let table = PieceTable::from_utf8_mmap_range(Arc::new(mmap), bom_len);
            let mut lines = LineIndex::new();
            lines.mark_incomplete();
            let mut doc = Document::from_table(table, lines, line_ending);
            doc.set_path(path.to_path_buf());
            (doc, false)
        } else {
            let (utf8, had_errors) = decode_to_utf8(bytes, encoding);
            let line_ending = LineEnding::detect(&utf8);
            let table = PieceTable::from_utf8_bytes(utf8);
            let lines = LineIndex::from_bytes(
                &table
                    .slice(0, table.len())
                    .unwrap_or_default(),
            );
            let mut doc = Document::from_table(table, lines, line_ending);
            doc.set_path(path.to_path_buf());
            (doc, had_errors)
        };

    Ok(OpenedFile {
        document,
        encoding,
        had_decode_errors,
    })
}

pub fn build_line_index_incremental(document: &mut Document) -> bool {
    if document.lines().is_complete() {
        return true;
    }

    let len = document.len();
    let start = document.lines().scan_cursor();
    if start >= len {
        document.lines_mut().mark_complete();
        return true;
    }

    let end = (start + INDEX_CHUNK).min(len);
    if let Ok(chunk) = document.slice(start, end) {
        document.lines_mut().scan_append(&chunk, start);
    }

    if end >= len {
        document.lines_mut().mark_complete();
        true
    } else {
        false
    }
}

pub fn save_document(document: &Document, path: &Path, encoding: FileEncoding) -> Result<()> {
    let temp_path = temp_save_path(path);
    {
        let file = File::create(&temp_path)
            .with_context(|| format!("create temp {}", temp_path.display()))?;
        let mut writer = BufWriter::new(file);

        if encoding.write_bom() {
            writer.write_all(&[0xEF, 0xBB, 0xBF])?;
        } else if matches!(encoding, FileEncoding::Utf16Le) {
            writer.write_all(&[0xFF, 0xFE])?;
        } else if matches!(encoding, FileEncoding::Utf16Be) {
            writer.write_all(&[0xFE, 0xFF])?;
        }

        let table = document.table();
        let mut pos = 0usize;
        let total = table.len();
        while pos < total {
            let end = (pos + INDEX_CHUNK).min(total);
            let chunk = table.slice(pos, end)?;
            let encoded = encode_chunk_from_utf8(&chunk, encoding);
            writer.write_all(&encoded)?;
            pos = end;
        }
        writer.flush()?;
    }

    if path.exists() {
        fs::remove_file(path).ok();
    }
    fs::rename(&temp_path, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

fn temp_save_path(path: &Path) -> PathBuf {
    let mut temp = path.to_path_buf();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("txt");
    temp.set_extension(format!("{ext}.tmp"));
    temp
}
