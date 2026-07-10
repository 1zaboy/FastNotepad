use super::{HighlightState, LanguageId, LanguageMode, TokenKind, TokenSpan};

pub struct MarkdownMode;

impl LanguageMode for MarkdownMode {
    fn id(&self) -> LanguageId {
        LanguageId::Markdown
    }

    fn display_name(&self) -> &'static str {
        "Markdown"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["md", "markdown"]
    }

    fn sniff(&self, _sample: &[u8]) -> bool {
        false
    }

    fn highlight_line(&self, line: &str, state: &mut HighlightState) -> Vec<TokenSpan> {
        highlight_md_line(line, state)
    }

    fn format(&self, text: &str) -> Result<String, String> {
        Ok(format_markdown(text))
    }
}

fn highlight_md_line(line: &str, state: &mut HighlightState) -> Vec<TokenSpan> {
    let trimmed_start = line.trim_start();
    let indent = line.len() - trimmed_start.len();

    // Fenced code block
    if state.in_code_fence {
        if is_closing_fence(trimmed_start, &state.fence_marker) {
            state.in_code_fence = false;
            state.fence_marker.clear();
            return vec![TokenSpan {
                start: 0,
                end: line.len(),
                kind: TokenKind::Code,
            }];
        }
        return vec![TokenSpan {
            start: 0,
            end: line.len(),
            kind: TokenKind::Code,
        }];
    }

    if let Some(marker) = opening_fence(trimmed_start) {
        state.in_code_fence = true;
        state.fence_marker = marker;
        return vec![TokenSpan {
            start: 0,
            end: line.len(),
            kind: TokenKind::Code,
        }];
    }

    // Heading
    if trimmed_start.starts_with('#') {
        let hashes = trimmed_start.bytes().take_while(|&b| b == b'#').count();
        if hashes > 0 && hashes <= 6 {
            let after = &trimmed_start[hashes..];
            if after.is_empty() || after.starts_with(' ') {
                return vec![TokenSpan {
                    start: 0,
                    end: line.len(),
                    kind: TokenKind::Heading,
                }];
            }
        }
    }

    highlight_inline(line, indent)
}

fn opening_fence(trimmed: &str) -> Option<String> {
    let bytes = trimmed.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    let ch = bytes[0];
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut n = 0;
    while n < bytes.len() && bytes[n] == ch {
        n += 1;
    }
    if n < 3 {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[..n]).into_owned())
}

fn is_closing_fence(trimmed: &str, marker: &str) -> bool {
    if marker.is_empty() {
        return false;
    }
    let ch = marker.as_bytes()[0];
    let need = marker.len();
    let bytes = trimmed.as_bytes();
    let mut n = 0;
    while n < bytes.len() && bytes[n] == ch {
        n += 1;
    }
    n >= need && bytes[n..].iter().all(|b| b.is_ascii_whitespace())
}

fn highlight_inline(line: &str, _indent: usize) -> Vec<TokenSpan> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        // Inline code `...`
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            push(&mut spans, start, i, TokenKind::Code);
            continue;
        }

        // Links [text](url) or ![alt](url)
        if bytes[i] == b'!' && bytes.get(i + 1) == Some(&b'[') {
            if let Some(end) = scan_link(bytes, i + 1) {
                push(&mut spans, i, end, TokenKind::Link);
                i = end;
                continue;
            }
        }
        if bytes[i] == b'[' {
            if let Some(end) = scan_link(bytes, i) {
                push(&mut spans, i, end, TokenKind::Link);
                i = end;
                continue;
            }
        }

        // Emphasis *...* or _..._ (simple, same-line)
        if bytes[i] == b'*' || bytes[i] == b'_' {
            let marker = bytes[i];
            let start = i;
            i += 1;
            // **bold** or *italic*
            let double = bytes.get(i) == Some(&marker);
            if double {
                i += 1;
            }
            let mut found = false;
            while i < bytes.len() {
                if double {
                    if bytes[i] == marker && bytes.get(i + 1) == Some(&marker) {
                        i += 2;
                        found = true;
                        break;
                    }
                } else if bytes[i] == marker {
                    i += 1;
                    found = true;
                    break;
                }
                i += 1;
            }
            if found {
                push(&mut spans, start, i, TokenKind::Emphasis);
                continue;
            }
            // unmatched — treat as text from start
            i = start + 1;
            push(&mut spans, start, i, TokenKind::Text);
            continue;
        }

        // Plain run until special
        let start = i;
        i += 1;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'`' || b == b'[' || b == b'*' || b == b'_' || (b == b'!' && bytes.get(i + 1) == Some(&b'['))
            {
                break;
            }
            i += 1;
        }
        push(&mut spans, start, i, TokenKind::Text);
    }

    spans
}

fn scan_link(bytes: &[u8], start: usize) -> Option<usize> {
    // start at '['
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] != b']' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    i += 1; // ]
    if bytes.get(i) != Some(&b'(') {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i] != b')' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    Some(i + 1)
}

fn push(spans: &mut Vec<TokenSpan>, start: usize, end: usize, kind: TokenKind) {
    if start < end {
        spans.push(TokenSpan { start, end, kind });
    }
}

pub fn format_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    let ends_crlf = text.contains("\r\n");
    let nl: &str = if ends_crlf { "\r\n" } else { "\n" };

    let lines: Vec<&str> = text.split('\n').collect();
    for (idx, raw) in lines.iter().enumerate() {
        let is_last = idx + 1 == lines.len();
        let content = raw.strip_suffix('\r').unwrap_or(raw);
        // Trailing empty segment from a final newline — skip; we re-add one ending.
        if is_last && content.is_empty() && text.ends_with('\n') {
            break;
        }

        let trimmed = content.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            // Collapse 3+ consecutive blank lines down to 2.
            if blank_run <= 2 {
                out.push_str(nl);
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push_str(nl);
        }
    }

    if out.is_empty() {
        return String::new();
    }
    if !out.ends_with('\n') {
        out.push_str(nl);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_and_code() {
        let m = MarkdownMode;
        let mut state = HighlightState::default();
        let h = m.highlight_line("# Title", &mut state);
        assert_eq!(h[0].kind, TokenKind::Heading);

        let c = m.highlight_line("use `code` here", &mut state);
        assert!(c.iter().any(|s| s.kind == TokenKind::Code));
    }

    #[test]
    fn fence_state() {
        let m = MarkdownMode;
        let mut state = HighlightState::default();
        let _ = m.highlight_line("```rust", &mut state);
        assert!(state.in_code_fence);
        let mid = m.highlight_line("fn main() {}", &mut state);
        assert_eq!(mid[0].kind, TokenKind::Code);
        let _ = m.highlight_line("```", &mut state);
        assert!(!state.in_code_fence);
    }

    #[test]
    fn link_span() {
        let m = MarkdownMode;
        let mut state = HighlightState::default();
        let spans = m.highlight_line("see [docs](https://x.com) now", &mut state);
        assert!(spans.iter().any(|s| s.kind == TokenKind::Link));
    }

    #[test]
    fn format_trims_and_collapses() {
        let m = MarkdownMode;
        let out = m.format("hello   \n\n\n\nworld  \n").unwrap();
        assert!(!out.contains("hello   "));
        // 3+ blanks -> at most 2 blank lines (three '\n' between content)
        assert!(!out.contains("\n\n\n\n"));
        assert!(out.contains("hello\n\n\nworld\n"));
        assert!(out.ends_with('\n'));
    }
}
