mod detect;
mod json;
mod markdown;
mod unknown;

pub use detect::detect_language;

use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Unknown,
    Json,
    Markdown,
}

impl LanguageId {
    pub fn display_name(self) -> &'static str {
        match self {
            LanguageId::Unknown => "Plain Text",
            LanguageId::Json => "JSON",
            LanguageId::Markdown => "Markdown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Text,
    Keyword,
    String,
    Number,
    Comment,
    Punctuation,
    Heading,
    Emphasis,
    Code,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Carry state between lines for multi-line constructs (JSON strings, MD fences).
#[derive(Debug, Clone, Default)]
pub struct HighlightState {
    pub in_string: bool,
    pub string_escape: bool,
    pub in_code_fence: bool,
    pub fence_marker: String,
}

pub trait LanguageMode: Send + Sync {
    fn id(&self) -> LanguageId;
    fn display_name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn sniff(&self, sample: &[u8]) -> bool;
    fn highlight_line(&self, line: &str, state: &mut HighlightState) -> Vec<TokenSpan>;
    fn format(&self, text: &str) -> Result<String, String>;
}

pub struct LanguageRegistry {
    modes: Vec<Arc<dyn LanguageMode>>,
}

impl LanguageRegistry {
    pub fn builtin() -> Self {
        Self {
            modes: vec![
                Arc::new(json::JsonMode),
                Arc::new(markdown::MarkdownMode),
                Arc::new(unknown::UnknownMode),
            ],
        }
    }

    pub fn get(&self, id: LanguageId) -> Arc<dyn LanguageMode> {
        self.modes
            .iter()
            .find(|m| m.id() == id)
            .cloned()
            .unwrap_or_else(|| Arc::new(unknown::UnknownMode))
    }

    pub fn detect(&self, path: Option<&Path>, sample: &[u8]) -> LanguageId {
        detect_language(path, sample, &self.modes)
    }

    pub fn all_ids(&self) -> impl Iterator<Item = LanguageId> + '_ {
        self.modes.iter().map(|m| m.id())
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Maximum document size (bytes) accepted by Format Document.
pub const FORMAT_MAX_BYTES: usize = 5_000_000;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_by_extension() {
        let reg = LanguageRegistry::builtin();
        assert_eq!(
            reg.detect(Some(Path::new("a.json")), b""),
            LanguageId::Json
        );
        assert_eq!(
            reg.detect(Some(Path::new("readme.md")), b""),
            LanguageId::Markdown
        );
        assert_eq!(
            reg.detect(Some(Path::new("notes.markdown")), b""),
            LanguageId::Markdown
        );
        assert_eq!(
            reg.detect(Some(Path::new("a.txt")), b"hello"),
            LanguageId::Unknown
        );
    }

    #[test]
    fn detect_json_by_sniff() {
        let reg = LanguageRegistry::builtin();
        assert_eq!(
            reg.detect(Some(Path::new("data")), b"  {\"a\":1}"),
            LanguageId::Json
        );
        assert_eq!(
            reg.detect(None, b"[1,2,3]"),
            LanguageId::Json
        );
    }

    #[test]
    fn unknown_default_untitled() {
        let reg = LanguageRegistry::builtin();
        assert_eq!(reg.detect(None, b""), LanguageId::Unknown);
        assert_eq!(reg.detect(None, b"hello world"), LanguageId::Unknown);
    }

    #[test]
    fn display_names() {
        assert_eq!(LanguageId::Unknown.display_name(), "Plain Text");
        let p: PathBuf = PathBuf::from("x.json");
        let _ = p;
    }
}
