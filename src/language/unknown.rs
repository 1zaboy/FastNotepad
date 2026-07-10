use super::{HighlightState, LanguageId, LanguageMode, TokenKind, TokenSpan};

pub struct UnknownMode;

impl LanguageMode for UnknownMode {
    fn id(&self) -> LanguageId {
        LanguageId::Unknown
    }

    fn display_name(&self) -> &'static str {
        "Plain Text"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &[]
    }

    fn sniff(&self, _sample: &[u8]) -> bool {
        false
    }

    fn highlight_line(&self, line: &str, _state: &mut HighlightState) -> Vec<TokenSpan> {
        if line.is_empty() {
            return Vec::new();
        }
        vec![TokenSpan {
            start: 0,
            end: line.len(),
            kind: TokenKind::Text,
        }]
    }

    fn format(&self, _text: &str) -> Result<String, String> {
        Err("No formatter for Plain Text".into())
    }
}
