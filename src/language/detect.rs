use std::path::Path;
use std::sync::Arc;

use super::{LanguageId, LanguageMode};

pub fn detect_language(
    path: Option<&Path>,
    sample: &[u8],
    modes: &[Arc<dyn LanguageMode>],
) -> LanguageId {
    if let Some(path) = path {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            for mode in modes {
                if mode
                    .extensions()
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&ext_lower))
                {
                    return mode.id();
                }
            }
        }
    }

    for mode in modes {
        if mode.id() == LanguageId::Unknown {
            continue;
        }
        if mode.sniff(sample) {
            return mode.id();
        }
    }

    LanguageId::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageRegistry;

    #[test]
    fn extension_beats_sniff() {
        let reg = LanguageRegistry::builtin();
        // .md file that looks like JSON still Markdown
        assert_eq!(
            reg.detect(Some(Path::new("x.md")), b"{\"a\":1}"),
            LanguageId::Markdown
        );
    }
}
