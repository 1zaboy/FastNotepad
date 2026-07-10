use std::fs;
use std::io::Write;
use std::time::Instant;

use fast_notepad::buffer::{Document, LineEnding, LineIndex, PieceTable};
use fast_notepad::encoding::{decode_to_utf8, detect_encoding, encode_chunk_from_utf8, FileEncoding};
use fast_notepad::file_io::{build_line_index_incremental, open_file, save_document};
use fast_notepad::view::{find_next, replace_all};

#[test]
fn encoding_roundtrips() {
    for enc in [
        FileEncoding::Utf8,
        FileEncoding::Utf8Bom,
        FileEncoding::Windows1251,
    ] {
        let src = "Hello Привет\n".as_bytes();
        let mut encoded = Vec::new();
        if enc.write_bom() {
            encoded.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        encoded.extend_from_slice(&encode_chunk_from_utf8(src, enc));
        let detected = detect_encoding(&encoded);
        if matches!(enc, FileEncoding::Utf8 | FileEncoding::Utf8Bom) {
            assert!(matches!(
                detected,
                FileEncoding::Utf8 | FileEncoding::Utf8Bom
            ));
        }
        let (decoded, _) = decode_to_utf8(&encoded, enc);
        assert_eq!(decoded, src);
    }
}

#[test]
fn document_edit_undo_redo() {
    let mut doc = Document::new();
    doc.insert(0, "hello").unwrap();
    doc.insert(5, " world").unwrap();
    assert_eq!(doc.slice(0, doc.len()).unwrap(), b"hello world");
    doc.undo().unwrap();
    assert_eq!(doc.slice(0, doc.len()).unwrap(), b"hello");
    doc.redo().unwrap();
    assert_eq!(doc.slice(0, doc.len()).unwrap(), b"hello world");
}

#[test]
fn open_save_roundtrip() {
    let dir = std::env::temp_dir().join("fast_notepad_tests");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("roundtrip.txt");
    fs::write(&path, "line1\nline2\n").unwrap();

    let opened = open_file(&path).unwrap();
    let mut doc = opened.document;
    while !build_line_index_incremental(&mut doc) {}
    assert!(doc.lines().line_count() >= 2);

    let out = dir.join("roundtrip_out.txt");
    save_document(&doc, &out, FileEncoding::Utf8).unwrap();
    let saved = fs::read_to_string(&out).unwrap();
    assert!(saved.contains("line1"));
    assert!(saved.contains("line2"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(&out);
}

#[test]
fn large_file_open_index_find_smoke() {
    let dir = std::env::temp_dir().join("fast_notepad_tests");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("large_smoke.txt");

    {
        let mut f = fs::File::create(&path).unwrap();
        let line = b"0123456789abcdef0123456789abcdef\n";
        let lines = (32 * 1024 * 1024) / line.len();
        for i in 0..lines {
            if i == lines / 2 {
                f.write_all(b"FIND_ME_MARKER\n").unwrap();
            } else {
                f.write_all(line).unwrap();
            }
        }
    }

    let started = Instant::now();
    let opened = open_file(&path).unwrap();
    let open_elapsed = started.elapsed();
    let mut doc = opened.document;

    let index_started = Instant::now();
    while !build_line_index_incremental(&mut doc) {}
    let index_elapsed = index_started.elapsed();

    assert!(doc.len() > 30 * 1024 * 1024);
    assert!(doc.lines().line_count() > 100_000);
    assert!(
        open_elapsed.as_secs_f64() < 2.0,
        "open too slow: {open_elapsed:?}"
    );
    assert!(
        index_elapsed.as_secs_f64() < 5.0,
        "index too slow: {index_elapsed:?}"
    );

    let found = find_next(&doc, "FIND_ME_MARKER", 0, true).expect("marker");
    assert_eq!(&doc.slice(found.0, found.1).unwrap(), b"FIND_ME_MARKER");

    // Edit in the middle of a huge file should stay responsive
    let edit_started = Instant::now();
    doc.insert(found.0, "X").unwrap();
    assert!(edit_started.elapsed().as_millis() < 500);

    let replaced = replace_all(&mut doc, "FIND_ME_MARKER", "DONE", true, 10);
    assert_eq!(replaced, 1);

    let _ = fs::remove_file(&path);
}

#[test]
fn piece_table_basic() {
    let mut table = PieceTable::from_utf8_bytes(b"abcdef".to_vec());
    table.insert(3, b"123").unwrap();
    assert_eq!(table.slice(0, table.len()).unwrap(), b"abc123def");
    let removed = table.delete(3, 6).unwrap();
    assert_eq!(removed, b"123");
    assert_eq!(table.slice(0, table.len()).unwrap(), b"abcdef");
}

#[test]
fn line_index_from_document_ops() {
    let table = PieceTable::from_utf8_bytes(b"a\nb\nc".to_vec());
    let lines = LineIndex::from_bytes(b"a\nb\nc");
    let mut doc = Document::from_table(table, lines, LineEnding::Lf);
    doc.insert(1, "\nX").unwrap();
    assert!(doc.lines().line_count() >= 3);
}
