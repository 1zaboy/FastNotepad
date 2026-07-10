/// Line-start offsets in logical UTF-8 document coordinates.
#[derive(Debug, Clone, Default)]
pub struct LineIndex {
    starts: Vec<usize>,
    complete: bool,
    scan_offset: usize,
}

impl LineIndex {
    pub fn new() -> Self {
        Self {
            starts: vec![0],
            complete: true,
            scan_offset: 0,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut index = Self::new();
        index.scan_append(bytes, 0);
        index.scan_offset = bytes.len();
        index.complete = true;
        index
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn scan_cursor(&self) -> usize {
        self.scan_offset
    }

    pub fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    pub fn mark_complete(&mut self) {
        self.complete = true;
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.starts.get(line).copied()
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        }
    }

    pub fn scan_append(&mut self, chunk: &[u8], base_offset: usize) {
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'\n' {
                self.starts.push(base_offset + i + 1);
            }
        }
        self.scan_offset = base_offset + chunk.len();
    }

    pub fn apply_insert(&mut self, pos: usize, text: &[u8]) {
        if pos < self.scan_offset {
            self.scan_offset += text.len();
        }

        let mut split_at = Vec::new();
        for (i, &b) in text.iter().enumerate() {
            if b == b'\n' {
                split_at.push(pos + i + 1);
            }
        }

        if split_at.is_empty() {
            for start in &mut self.starts {
                if *start > pos {
                    *start += text.len();
                }
            }
            return;
        }

        let mut new_starts = Vec::with_capacity(self.starts.len() + split_at.len());
        for &start in &self.starts {
            if start <= pos {
                new_starts.push(start);
            } else {
                new_starts.push(start + text.len());
            }
        }
        new_starts.extend(split_at);
        new_starts.sort_unstable();
        new_starts.dedup();
        if new_starts.is_empty() {
            new_starts.push(0);
        }
        self.starts = new_starts;
    }

    pub fn apply_delete(&mut self, start: usize, deleted: &[u8]) {
        let len = deleted.len();
        if start < self.scan_offset {
            self.scan_offset = self.scan_offset.saturating_sub(len);
        }

        let delete_end = start + len;
        let mut new_starts = Vec::with_capacity(self.starts.len());
        for &line_start in &self.starts {
            if line_start <= start {
                new_starts.push(line_start);
            } else if line_start >= delete_end {
                new_starts.push(line_start - len);
            }
        }
        new_starts.sort_unstable();
        new_starts.dedup();
        if new_starts.is_empty() {
            new_starts.push(0);
        }
        self.starts = new_starts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines() {
        let index = LineIndex::from_bytes(b"a\nb\nc");
        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_start(0), Some(0));
        assert_eq!(index.line_start(1), Some(2));
        assert_eq!(index.line_start(2), Some(4));
    }

    #[test]
    fn insert_updates_lines() {
        let mut index = LineIndex::from_bytes(b"abc");
        index.apply_insert(1, b"\nX\n");
        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_start(0), Some(0));
        assert_eq!(index.line_start(1), Some(2));
        assert_eq!(index.line_start(2), Some(4));
    }

    #[test]
    fn delete_updates_lines() {
        let mut index = LineIndex::from_bytes(b"a\nb\nc");
        index.apply_delete(1, b"\nb");
        assert_eq!(index.line_count(), 2);
        assert_eq!(index.line_start(0), Some(0));
        assert_eq!(index.line_start(1), Some(2));
    }
}
