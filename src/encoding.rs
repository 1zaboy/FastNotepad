use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1251};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1251,
}

impl FileEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Windows1251 => "Windows-1251",
        }
    }

    pub fn encoding(self) -> &'static Encoding {
        match self {
            Self::Utf8 | Self::Utf8Bom => UTF_8,
            Self::Utf16Le => UTF_16LE,
            Self::Utf16Be => UTF_16BE,
            Self::Windows1251 => WINDOWS_1251,
        }
    }

    pub fn write_bom(self) -> bool {
        matches!(self, Self::Utf8Bom)
    }
}

pub fn detect_encoding(bytes: &[u8]) -> FileEncoding {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return FileEncoding::Utf8Bom;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return FileEncoding::Utf16Le;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return FileEncoding::Utf16Be;
    }

    if looks_like_utf16(bytes) {
        if bytes.len() >= 2 && bytes[0] == 0 && bytes[1] != 0 {
            return FileEncoding::Utf16Be;
        }
        return FileEncoding::Utf16Le;
    }

    if looks_like_windows1251(bytes) {
        return FileEncoding::Windows1251;
    }

    FileEncoding::Utf8
}

pub fn strip_bom(bytes: &[u8], encoding: FileEncoding) -> &[u8] {
    match encoding {
        FileEncoding::Utf8Bom if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) => &bytes[3..],
        FileEncoding::Utf16Le if bytes.starts_with(&[0xFF, 0xFE]) => &bytes[2..],
        FileEncoding::Utf16Be if bytes.starts_with(&[0xFE, 0xFF]) => &bytes[2..],
        _ => bytes,
    }
}

pub fn decode_to_utf8(bytes: &[u8], encoding: FileEncoding) -> (Vec<u8>, bool) {
    let payload = strip_bom(bytes, encoding);
    if matches!(encoding, FileEncoding::Utf8 | FileEncoding::Utf8Bom) {
        return (payload.to_vec(), std::str::from_utf8(payload).is_err());
    }

    let (decoded, _, had_errors) = encoding.encoding().decode(payload);
    (decoded.into_owned().into_bytes(), had_errors)
}

pub fn encode_chunk_from_utf8(text: &[u8], encoding: FileEncoding) -> Vec<u8> {
    if matches!(encoding, FileEncoding::Utf8 | FileEncoding::Utf8Bom) {
        return text.to_vec();
    }

    let src = std::str::from_utf8(text).unwrap_or("");
    encoding.encoding().encode(src).0.into_owned()
}

fn looks_like_utf16(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    let sample = bytes.len().min(4096);
    let mut zeros = 0usize;
    for &b in &bytes[..sample] {
        if b == 0 {
            zeros += 1;
        }
    }
    zeros * 10 > sample
}

fn looks_like_windows1251(bytes: &[u8]) -> bool {
    if std::str::from_utf8(bytes).is_ok() {
        return false;
    }
    let sample = bytes.len().min(8192);
    let mut cyrillic = 0usize;
    for &b in &bytes[..sample] {
        if (0xC0..=0xFF).contains(&b) {
            cyrillic += 1;
        }
    }
    cyrillic * 20 > sample
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bom() {
        assert_eq!(
            detect_encoding(&[0xEF, 0xBB, 0xBF, b'h']),
            FileEncoding::Utf8Bom
        );
        assert_eq!(
            detect_encoding(&[0xFF, 0xFE, b'a', 0]),
            FileEncoding::Utf16Le
        );
    }

    #[test]
    fn roundtrip_utf8() {
        let src = b"hello\nworld";
        let enc = encode_chunk_from_utf8(src, FileEncoding::Utf8);
        let (decoded, _) = decode_to_utf8(&enc, FileEncoding::Utf8);
        assert_eq!(decoded, src);
    }

    #[test]
    fn roundtrip_windows1251() {
        let src = "Привет".as_bytes();
        let enc = encode_chunk_from_utf8(src, FileEncoding::Windows1251);
        let (decoded, had_errors) = decode_to_utf8(&enc, FileEncoding::Windows1251);
        assert!(!had_errors);
        assert_eq!(decoded, src);
    }
}
