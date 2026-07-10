use super::{HighlightState, LanguageId, LanguageMode, TokenKind, TokenSpan};

pub struct JsonMode;

impl LanguageMode for JsonMode {
    fn id(&self) -> LanguageId {
        LanguageId::Json
    }

    fn display_name(&self) -> &'static str {
        "JSON"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn sniff(&self, sample: &[u8]) -> bool {
        let trimmed = trim_ascii_start(sample);
        matches!(trimmed.first(), Some(b'{') | Some(b'['))
    }

    fn highlight_line(&self, line: &str, state: &mut HighlightState) -> Vec<TokenSpan> {
        highlight_json_line(line, state)
    }

    fn format(&self, text: &str) -> Result<String, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {e}"))?;
        serde_json::to_string_pretty(&value).map_err(|e| format!("Format failed: {e}"))
    }
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    &bytes[i..]
}

fn highlight_json_line(line: &str, state: &mut HighlightState) -> Vec<TokenSpan> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if state.in_string {
            let start = i;
            while i < bytes.len() {
                if state.string_escape {
                    state.string_escape = false;
                    i += 1;
                    continue;
                }
                match bytes[i] {
                    b'\\' => {
                        state.string_escape = true;
                        i += 1;
                    }
                    b'"' => {
                        i += 1;
                        state.in_string = false;
                        break;
                    }
                    _ => i += 1,
                }
            }
            push_span(&mut spans, start, i, TokenKind::String);
            continue;
        }

        match bytes[i] {
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'"' => {
                state.in_string = true;
                state.string_escape = false;
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if state.string_escape {
                        state.string_escape = false;
                        i += 1;
                        continue;
                    }
                    match bytes[i] {
                        b'\\' => {
                            state.string_escape = true;
                            i += 1;
                        }
                        b'"' => {
                            i += 1;
                            state.in_string = false;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                // Distinguish keys (followed by :) from string values — approximate:
                // look ahead past whitespace for ':'
                let kind = if peek_is_key(bytes, i) {
                    TokenKind::Keyword
                } else {
                    TokenKind::String
                };
                push_span(&mut spans, start, i, kind);
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                push_span(&mut spans, i, i + 1, TokenKind::Punctuation);
                i += 1;
            }
            b't' | b'f' | b'n' => {
                if let Some(end) = match_literal(bytes, i, b"true")
                    .or_else(|| match_literal(bytes, i, b"false"))
                    .or_else(|| match_literal(bytes, i, b"null"))
                {
                    push_span(&mut spans, i, end, TokenKind::Keyword);
                    i = end;
                } else {
                    push_span(&mut spans, i, i + 1, TokenKind::Text);
                    i += 1;
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i = scan_number(bytes, i);
                push_span(&mut spans, start, i, TokenKind::Number);
            }
            _ => {
                push_span(&mut spans, i, i + 1, TokenKind::Text);
                i += 1;
            }
        }
    }

    spans
}

fn peek_is_key(bytes: &[u8], mut i: usize) -> bool {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    bytes.get(i) == Some(&b':')
}

fn match_literal(bytes: &[u8], i: usize, lit: &[u8]) -> Option<usize> {
    if bytes[i..].starts_with(lit) {
        let end = i + lit.len();
        let ok_boundary = bytes
            .get(end)
            .map(|c| !c.is_ascii_alphanumeric() && *c != b'_')
            .unwrap_or(true);
        if ok_boundary {
            return Some(end);
        }
    }
    None
}

fn scan_number(bytes: &[u8], mut i: usize) -> usize {
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if matches!(bytes.get(i), Some(b'e') | Some(b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

fn push_span(spans: &mut Vec<TokenSpan>, start: usize, end: usize, kind: TokenKind) {
    if start < end {
        spans.push(TokenSpan { start, end, kind });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_object_and_array() {
        let m = JsonMode;
        assert!(m.sniff(b"  {\"a\":1}"));
        assert!(m.sniff(b"[1]"));
        assert!(!m.sniff(b"hello"));
    }

    #[test]
    fn highlight_keys_strings_numbers() {
        let m = JsonMode;
        let mut state = HighlightState::default();
        let spans = m.highlight_line(r#"{"a": 12, "b": "hi"}"#, &mut state);
        assert!(spans.iter().any(|s| s.kind == TokenKind::Keyword));
        assert!(spans.iter().any(|s| s.kind == TokenKind::Number));
        assert!(spans.iter().any(|s| s.kind == TokenKind::String));
        assert!(!state.in_string);
    }

    #[test]
    fn multiline_string_state() {
        let m = JsonMode;
        let mut state = HighlightState::default();
        let _ = m.highlight_line(r#"{"x": "hello"#, &mut state);
        assert!(state.in_string);
        let spans = m.highlight_line(r#"world"}"#, &mut state);
        assert!(spans.iter().any(|s| s.kind == TokenKind::String));
        assert!(!state.in_string);
    }

    #[test]
    fn format_pretty() {
        let m = JsonMode;
        let out = m.format(r#"{"a":1,"b":[2]}"#).unwrap();
        assert!(out.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["a"], 1);
    }

    #[test]
    fn format_invalid() {
        let m = JsonMode;
        assert!(m.format("{bad").is_err());
    }
}
