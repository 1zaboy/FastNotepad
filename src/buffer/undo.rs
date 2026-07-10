#[derive(Debug, Clone)]
pub enum EditAction {
    Insert { pos: usize, text: Vec<u8> },
    Delete { pos: usize, text: Vec<u8> },
}

#[derive(Debug, Default)]
pub struct UndoStack {
    undo: Vec<EditAction>,
    redo: Vec<EditAction>,
    limit: usize,
}

impl UndoStack {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    pub fn push(&mut self, action: EditAction) {
        self.undo.push(action);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn pop_undo(&mut self) -> Option<EditAction> {
        let action = self.undo.pop()?;
        self.redo.push(action.clone());
        Some(action)
    }

    pub fn pop_redo(&mut self) -> Option<EditAction> {
        let action = self.redo.pop()?;
        self.undo.push(action.clone());
        Some(action)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redo_cleared_on_new_edit() {
        let mut stack = UndoStack::new(100);
        stack.push(EditAction::Insert {
            pos: 0,
            text: b"a".to_vec(),
        });
        let _ = stack.pop_undo();
        stack.push(EditAction::Insert {
            pos: 1,
            text: b"b".to_vec(),
        });
        assert!(!stack.can_redo());
    }
}
