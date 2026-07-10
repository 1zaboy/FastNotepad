use std::path::{Path, PathBuf};

use super::line_index::LineIndex;
use super::piece_table::{PieceTable, ReadError};
use super::undo::{EditAction, UndoStack};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Mixed,
}

impl LineEnding {
    pub fn detect(bytes: &[u8]) -> Self {
        let mut saw_lf = false;
        let mut saw_crlf = false;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                saw_crlf = true;
                i += 2;
            } else if bytes[i] == b'\n' {
                saw_lf = true;
                i += 1;
            } else {
                i += 1;
            }
        }
        match (saw_crlf, saw_lf) {
            (true, false) => LineEnding::CrLf,
            (false, true) => LineEnding::Lf,
            (true, true) => LineEnding::Mixed,
            (false, false) => LineEnding::Lf,
        }
    }

    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            LineEnding::Lf | LineEnding::Mixed => b"\n",
            LineEnding::CrLf => b"\r\n",
        }
    }
}

#[derive(Debug)]
pub struct Document {
    table: PieceTable,
    lines: LineIndex,
    undo: UndoStack,
    path: Option<PathBuf>,
    dirty: bool,
    line_ending: LineEnding,
}

impl Document {
    pub fn new() -> Self {
        Self {
            table: PieceTable::new_empty(),
            lines: LineIndex::new(),
            undo: UndoStack::new(512),
            path: None,
            dirty: false,
            line_ending: LineEnding::Lf,
        }
    }

    pub fn from_table(table: PieceTable, lines: LineIndex, line_ending: LineEnding) -> Self {
        Self {
            table,
            lines,
            undo: UndoStack::new(512),
            path: None,
            dirty: false,
            line_ending,
        }
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    pub fn clear_path(&mut self) {
        self.path = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn set_line_ending(&mut self, ending: LineEnding) {
        self.line_ending = ending;
    }

    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    pub fn lines_mut(&mut self) -> &mut LineIndex {
        &mut self.lines
    }

    pub fn table(&self) -> &PieceTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut PieceTable {
        &mut self.table
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn slice(&self, start: usize, end: usize) -> Result<Vec<u8>, ReadError> {
        self.table.slice(start, end)
    }

    pub fn line_content(&self, line: usize) -> Result<String, ReadError> {
        let start = self.lines.line_start(line).unwrap_or(0);
        let end = self
            .lines
            .line_start(line + 1)
            .unwrap_or(self.table.len());
        let bytes = self.table.slice(start, end)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn insert(&mut self, pos: usize, text: &str) -> Result<(), ReadError> {
        let bytes = text.as_bytes();
        self.table.insert(pos, bytes)?;
        self.lines.apply_insert(pos, bytes);
        self.undo.push(EditAction::Insert {
            pos,
            text: bytes.to_vec(),
        });
        self.dirty = true;
        Ok(())
    }

    pub fn delete(&mut self, start: usize, end: usize) -> Result<(), ReadError> {
        let removed = self.table.delete(start, end)?;
        self.lines.apply_delete(start, &removed);
        self.undo.push(EditAction::Delete {
            pos: start,
            text: removed,
        });
        self.dirty = true;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), ReadError> {
        let Some(action) = self.undo.pop_undo() else {
            return Ok(());
        };
        match action {
            EditAction::Insert { pos, text } => {
                self.table.delete(pos, pos + text.len())?;
                self.lines.apply_delete(pos, &text);
            }
            EditAction::Delete { pos, text } => {
                self.table.insert(pos, &text)?;
                self.lines.apply_insert(pos, &text);
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), ReadError> {
        let Some(action) = self.undo.pop_redo() else {
            return Ok(());
        };
        match action {
            EditAction::Insert { pos, text } => {
                self.table.insert(pos, &text)?;
                self.lines.apply_insert(pos, &text);
            }
            EditAction::Delete { pos, text } => {
                self.table.delete(pos, pos + text.len())?;
                self.lines.apply_delete(pos, &text);
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.table = PieceTable::new_empty();
        self.lines = LineIndex::new();
        self.undo.clear();
        self.path = None;
        self.dirty = false;
        self.line_ending = LineEnding::Lf;
    }

    pub fn replace_all(&mut self, content: PieceTable, lines: LineIndex, line_ending: LineEnding) {
        self.table = content;
        self.lines = lines;
        self.undo.clear();
        self.dirty = false;
        self.line_ending = line_ending;
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}
