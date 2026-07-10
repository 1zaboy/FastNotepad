pub mod buffer;
pub mod encoding;
pub mod file_io;
pub mod language;
pub mod settings;
pub mod tabs;
pub mod view;

pub use buffer::{Document, LineEnding, LineIndex, PieceTable};
pub use encoding::FileEncoding;
pub use language::{
    HighlightState, LanguageId, LanguageMode, LanguageRegistry, TokenKind, TokenSpan,
    FORMAT_MAX_BYTES,
};
pub use settings::AppSettings;
