use crate::buffer::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub offset: usize,
    pub anchor: usize,
}

impl Caret {
    pub fn new() -> Self {
        Self {
            offset: 0,
            anchor: 0,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.offset != self.anchor
    }

    pub fn selection_range(&self) -> (usize, usize) {
        if self.offset <= self.anchor {
            (self.offset, self.anchor)
        } else {
            (self.anchor, self.offset)
        }
    }

    pub fn collapse_to_offset(&mut self) {
        self.anchor = self.offset;
    }

    pub fn select_all(&mut self, doc_len: usize) {
        self.anchor = 0;
        self.offset = doc_len;
    }
}

impl Default for Caret {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ViewState {
    pub caret: Caret,
    pub first_visible_line: usize,
    pub scroll_x: i32,
    pub line_height: f32,
    pub char_width: f32,
    pub gutter_width: i32,
    pub client_width: i32,
    pub client_height: i32,
    pub word_wrap: bool,
    pub content_top: i32,
    pub content_bottom: i32,
    /// Right inset reserved for the vertical scrollbar (client-area child control).
    pub content_right: i32,
}

impl ViewState {
    pub fn new() -> Self {
        Self {
            caret: Caret::new(),
            first_visible_line: 0,
            scroll_x: 0,
            line_height: 16.0,
            char_width: 8.0,
            gutter_width: 56,
            client_width: 800,
            client_height: 600,
            word_wrap: false,
            content_top: 0,
            content_bottom: 0,
            content_right: 0,
        }
    }

    pub fn text_area_height(&self) -> i32 {
        (self.client_height - self.content_top - self.content_bottom).max(1)
    }

    pub fn text_area_width(&self) -> i32 {
        (self.client_width - self.text_left() - self.content_right).max(1)
    }

    pub fn editor_right(&self) -> i32 {
        (self.client_width - self.content_right).max(0)
    }

    pub fn wrap_cols(&self) -> usize {
        let avail = self.text_area_width() as f32;
        ((avail / self.char_width).floor() as usize).max(1)
    }

    /// Approximate max line width in pixels (byte length × char width; fine for scroll range).
    pub fn max_content_width_px(&self, doc: &Document) -> i32 {
        if self.word_wrap {
            return 0;
        }
        let mut max_bytes = 0usize;
        let lines = doc.lines();
        let line_count = lines.line_count();
        let doc_len = doc.len();
        for i in 0..line_count {
            let start = lines.line_start(i).unwrap_or(0);
            let end = lines.line_start(i + 1).unwrap_or(doc_len);
            let mut n = end.saturating_sub(start);
            // Drop trailing newline byte(s); over-subtracting is harmless for range.
            if n > 0 {
                n -= 1;
            }
            max_bytes = max_bytes.max(n);
        }
        ((max_bytes as f32) * self.char_width).ceil() as i32
    }

    pub fn max_scroll_x(&self, doc: &Document) -> i32 {
        (self.max_content_width_px(doc) - self.text_area_width()).max(0)
    }

    pub fn clamp_scroll_x(&mut self, doc: &Document) {
        let max = self.max_scroll_x(doc);
        self.scroll_x = self.scroll_x.clamp(0, max);
    }

    pub fn scroll_by_px(&mut self, delta: i32, doc: &Document) {
        self.scroll_x = (self.scroll_x + delta).clamp(0, self.max_scroll_x(doc));
    }

    pub fn visible_line_count(&self) -> usize {
        ((self.text_area_height() as f32 / self.line_height).ceil() as usize).max(1)
    }

    pub fn text_left(&self) -> i32 {
        self.gutter_width + 4
    }

    pub fn line_top(&self, line: usize) -> f32 {
        self.content_top as f32
            + (line.saturating_sub(self.first_visible_line)) as f32 * self.line_height
    }

    pub fn line_col_at(doc: &Document, offset: usize) -> (usize, usize) {
        let line = doc.lines().line_of_offset(offset);
        let start = doc.lines().line_start(line).unwrap_or(0);
        let col_bytes = offset.saturating_sub(start);
        let col = if let Ok(bytes) = doc.slice(start, offset) {
            String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|c| *c != '\r' && *c != '\n')
                .count()
        } else {
            col_bytes
        };
        (line + 1, col + 1)
    }

    pub fn visual_rows_for_logical(&self, doc: &Document, logical: usize) -> Vec<(usize, usize)> {
        let start = doc.lines().line_start(logical).unwrap_or(0);
        let end = doc
            .lines()
            .line_start(logical + 1)
            .unwrap_or(doc.len());
        let Ok(bytes) = doc.slice(start, end) else {
            return vec![(start, end)];
        };
        let text = String::from_utf8_lossy(&bytes);
        let content: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        if !self.word_wrap || content.is_empty() {
            return vec![(start, start + content.len())];
        }
        let cols = self.wrap_cols();
        let mut rows = Vec::new();
        let mut byte_off = 0usize;
        let chars: Vec<(usize, char)> = content.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let row_start = start + byte_off;
            let mut count = 0usize;
            let mut end_byte = byte_off;
            while i < chars.len() && count < cols {
                end_byte = chars[i].0 + chars[i].1.len_utf8();
                count += 1;
                i += 1;
            }
            byte_off = end_byte;
            rows.push((row_start, start + end_byte));
        }
        if rows.is_empty() {
            rows.push((start, start));
        }
        rows
    }

    pub fn offset_from_point(&self, doc: &Document, x: i32, y: i32) -> usize {
        let local_y = (y - self.content_top).max(0);
        if self.word_wrap {
            return self.offset_from_point_wrapped(doc, x, local_y);
        }
        let line = self.first_visible_line
            + ((local_y as f32 / self.line_height).floor().max(0.0) as usize);
        let line = line.min(doc.lines().line_count().saturating_sub(1));
        self.offset_in_line(doc, line, x)
    }

    fn offset_from_point_wrapped(&self, doc: &Document, x: i32, local_y: i32) -> usize {
        let visual_origin = self.first_visible_line;
        let target = (local_y as f32 / self.line_height).floor().max(0.0) as usize;
        let mut seen = 0usize;
        for logical in 0..doc.lines().line_count() {
            let rows = self.visual_rows_for_logical(doc, logical);
            for (a, b) in rows {
                if seen >= visual_origin && seen - visual_origin == target {
                    let rel_x = (x - self.text_left() + self.scroll_x).max(0);
                    let col = (rel_x as f32 / self.char_width).round() as usize;
                    return self.offset_in_range(doc, a, b, col);
                }
                seen += 1;
            }
        }
        doc.len()
    }

    fn offset_in_line(&self, doc: &Document, line: usize, x: i32) -> usize {
        let start = doc.lines().line_start(line).unwrap_or(0);
        let end = doc
            .lines()
            .line_start(line + 1)
            .unwrap_or(doc.len());
        let rel_x = (x - self.text_left() + self.scroll_x).max(0);
        let col = (rel_x as f32 / self.char_width).round() as usize;
        self.offset_in_range(doc, start, end, col)
    }

    fn offset_in_range(&self, doc: &Document, start: usize, end: usize, col: usize) -> usize {
        if let Ok(bytes) = doc.slice(start, end) {
            let text = String::from_utf8_lossy(&bytes);
            let mut byte_off = 0usize;
            for (i, ch) in text.chars().enumerate() {
                if ch == '\r' || ch == '\n' {
                    break;
                }
                if i >= col {
                    break;
                }
                byte_off += ch.len_utf8();
            }
            return (start + byte_off).min(doc.len());
        }
        (start + col.min(end.saturating_sub(start))).min(doc.len())
    }

    pub fn ensure_caret_visible(&mut self, doc: &Document) {
        let line = doc.lines().line_of_offset(self.caret.offset);
        let visible = self.visible_line_count();
        if self.word_wrap {
            let mut visual_index = 0usize;
            let mut caret_visual = 0usize;
            for logical in 0..=line.min(doc.lines().line_count().saturating_sub(1)) {
                let rows = self.visual_rows_for_logical(doc, logical);
                if logical == line {
                    let off = self.caret.offset;
                    for (i, (a, b)) in rows.iter().enumerate() {
                        if off >= *a && off <= *b {
                            caret_visual = visual_index + i;
                            break;
                        }
                        caret_visual = visual_index + i;
                    }
                }
                visual_index += rows.len();
            }
            if caret_visual < self.first_visible_line {
                self.first_visible_line = caret_visual;
            } else if caret_visual >= self.first_visible_line + visible {
                self.first_visible_line = caret_visual + 1 - visible;
            }
            self.scroll_x = 0;
            return;
        }
        if line < self.first_visible_line {
            self.first_visible_line = line;
        } else if line >= self.first_visible_line + visible {
            self.first_visible_line = line + 1 - visible;
        }

        let (_, col_1based) = Self::line_col_at(doc, self.caret.offset);
        let caret_x = ((col_1based.saturating_sub(1)) as f32 * self.char_width) as i32;
        let avail = self.text_area_width();
        let margin = (self.char_width as i32).max(1);
        if caret_x < self.scroll_x {
            self.scroll_x = caret_x.saturating_sub(margin);
        } else if caret_x >= self.scroll_x + avail {
            self.scroll_x = caret_x - avail + margin;
        }
        self.clamp_scroll_x(doc);
    }

    pub fn scroll_by_lines(&mut self, delta: i32, doc: &Document) {
        let max_first = if self.word_wrap {
            self.total_visual_rows(doc).saturating_sub(1)
        } else {
            doc.lines().line_count().saturating_sub(1)
        };
        let next = (self.first_visible_line as i32 + delta).max(0) as usize;
        self.first_visible_line = next.min(max_first);
    }

    pub fn total_visual_rows(&self, doc: &Document) -> usize {
        if !self.word_wrap {
            return doc.lines().line_count().max(1);
        }
        let mut total = 0usize;
        for line in 0..doc.lines().line_count() {
            total += self.visual_rows_for_logical(doc, line).len();
        }
        total.max(1)
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn find_next(doc: &Document, query: &str, from: usize, case_sensitive: bool) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }

    let query_bytes = if case_sensitive {
        query.as_bytes().to_vec()
    } else {
        query.to_lowercase().into_bytes()
    };
    let qlen = query_bytes.len();
    if qlen == 0 {
        return None;
    }

    const CHUNK: usize = 1024 * 1024;
    let overlap = qlen.saturating_sub(1);
    let mut pos = from.min(doc.len());

    while pos < doc.len() {
        let end = (pos + CHUNK).min(doc.len());
        let bytes = doc.slice(pos, end).ok()?;
        let hay = if case_sensitive {
            bytes
        } else {
            String::from_utf8_lossy(&bytes).to_lowercase().into_bytes()
        };

        if let Some(rel) = find_bytes(&hay, &query_bytes) {
            let abs = pos + rel;
            return Some((abs, abs + qlen));
        }

        if end >= doc.len() {
            break;
        }
        pos = end.saturating_sub(overlap);
    }
    None
}

pub fn replace_next(
    doc: &mut Document,
    query: &str,
    replacement: &str,
    from: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    let (a, b) = find_next(doc, query, from, case_sensitive)?;
    doc.delete(a, b).ok()?;
    doc.insert(a, replacement).ok()?;
    Some((a, a + replacement.len()))
}

pub fn replace_all(
    doc: &mut Document,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    limit: usize,
) -> usize {
    if query.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut from = 0usize;
    while count < limit {
        let Some((a, b)) = find_next(doc, query, from, case_sensitive) else {
            break;
        };
        if doc.delete(a, b).is_err() {
            break;
        }
        if doc.insert(a, replacement).is_err() {
            break;
        }
        from = a + replacement.len();
        count += 1;
    }
    count
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Document, LineEnding, LineIndex, PieceTable};

    #[test]
    fn finds_across_chunks() {
        let mut text = "a".repeat(100);
        text.push_str("needle");
        text.push_str(&"b".repeat(100));
        let table = PieceTable::from_utf8_bytes(text.into_bytes());
        let lines = LineIndex::from_bytes(&table.slice(0, table.len()).unwrap());
        let doc = Document::from_table(table, lines, LineEnding::Lf);
        let found = find_next(&doc, "needle", 0, true).unwrap();
        assert_eq!(&doc.slice(found.0, found.1).unwrap(), b"needle");
    }

    #[test]
    fn line_col_basic() {
        let table = PieceTable::from_utf8_bytes(b"ab\ncd".to_vec());
        let lines = LineIndex::from_bytes(b"ab\ncd");
        let doc = Document::from_table(table, lines, LineEnding::Lf);
        assert_eq!(ViewState::line_col_at(&doc, 0), (1, 1));
        assert_eq!(ViewState::line_col_at(&doc, 4), (2, 2));
    }

    #[test]
    fn replace_all_works() {
        let table = PieceTable::from_utf8_bytes(b"foo bar foo".to_vec());
        let lines = LineIndex::from_bytes(b"foo bar foo");
        let mut doc = Document::from_table(table, lines, LineEnding::Lf);
        let n = replace_all(&mut doc, "foo", "baz", true, 100);
        assert_eq!(n, 2);
        assert_eq!(doc.slice(0, doc.len()).unwrap(), b"baz bar baz");
    }
}
