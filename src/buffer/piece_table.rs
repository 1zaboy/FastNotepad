use std::sync::Arc;

use memmap2::Mmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceSource {
    Original { start: usize, len: usize },
    Add { start: usize, len: usize },
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    source: PieceSource,
    /// Length in logical UTF-8 bytes exposed by this piece.
    len: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("range out of bounds")]
    OutOfBounds,
    #[error("invalid utf-8 in original buffer")]
    InvalidUtf8,
}

/// Piece table backed by optional mmap original bytes and an append-only add buffer.
#[derive(Debug, Clone)]
pub struct PieceTable {
    original: Option<Arc<Mmap>>,
    add_buffer: Vec<u8>,
    pieces: Vec<Piece>,
    length: usize,
}

impl PieceTable {
    pub fn new_empty() -> Self {
        Self {
            original: None,
            add_buffer: Vec::new(),
            pieces: Vec::new(),
            length: 0,
        }
    }

    pub fn from_utf8_mmap(mmap: Arc<Mmap>) -> Self {
        Self::from_utf8_mmap_range(mmap, 0)
    }

    pub fn from_utf8_mmap_range(mmap: Arc<Mmap>, start: usize) -> Self {
        let total = mmap.len();
        let start = start.min(total);
        let len = total - start;
        let pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: PieceSource::Original { start, len },
                len,
            }]
        };
        Self {
            original: Some(mmap),
            add_buffer: Vec::new(),
            pieces,
            length: len,
        }
    }

    pub fn from_utf8_bytes(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self::new_empty();
        }
        let len = bytes.len();
        let mut table = Self::new_empty();
        table.add_buffer = bytes;
        table.pieces.push(Piece {
            source: PieceSource::Add { start: 0, len },
            len,
        });
        table.length = len;
        table
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    pub fn insert(&mut self, pos: usize, text: &[u8]) -> Result<(), ReadError> {
        if pos > self.length {
            return Err(ReadError::OutOfBounds);
        }
        if text.is_empty() {
            return Ok(());
        }
        std::str::from_utf8(text).map_err(|_| ReadError::InvalidUtf8)?;

        let add_start = self.add_buffer.len();
        self.add_buffer.extend_from_slice(text);
        let insert_len = text.len();

        let (piece_idx, offset_in_piece) = self.locate(pos)?;
        self.insert_piece(
            piece_idx,
            offset_in_piece,
            Piece {
                source: PieceSource::Add {
                    start: add_start,
                    len: insert_len,
                },
                len: insert_len,
            },
        );
        self.length += insert_len;
        Ok(())
    }

    pub fn delete(&mut self, start: usize, end: usize) -> Result<Vec<u8>, ReadError> {
        if start > end || end > self.length {
            return Err(ReadError::OutOfBounds);
        }
        if start == end {
            return Ok(Vec::new());
        }

        let deleted = self.slice(start, end)?;
        let (start_piece, start_off) = self.locate(start)?;
        let (end_piece, end_off) = self.locate(end)?;

        if start_piece == end_piece {
            let piece = &mut self.pieces[start_piece];
            match piece.source {
                PieceSource::Original { start: o, len } => {
                    if start_off == 0 && end_off == piece.len {
                        self.pieces.remove(start_piece);
                    } else if start_off == 0 {
                        piece.source = PieceSource::Original {
                            start: o + end_off,
                            len: len - end_off,
                        };
                        piece.len -= end_off;
                    } else if end_off == piece.len {
                        piece.source = PieceSource::Original {
                            start: o,
                            len: start_off,
                        };
                        piece.len = start_off;
                    } else {
                        let right = Piece {
                            source: PieceSource::Original {
                                start: o + end_off,
                                len: len - end_off,
                            },
                            len: len - end_off,
                        };
                        piece.source = PieceSource::Original {
                            start: o,
                            len: start_off,
                        };
                        piece.len = start_off;
                        self.pieces.insert(start_piece + 1, right);
                    }
                }
                PieceSource::Add { start: a, len } => {
                    if start_off == 0 && end_off == piece.len {
                        self.pieces.remove(start_piece);
                    } else if start_off == 0 {
                        piece.source = PieceSource::Add {
                            start: a + end_off,
                            len: len - end_off,
                        };
                        piece.len -= end_off;
                    } else if end_off == piece.len {
                        piece.source = PieceSource::Add {
                            start: a,
                            len: start_off,
                        };
                        piece.len = start_off;
                    } else {
                        let right = Piece {
                            source: PieceSource::Add {
                                start: a + end_off,
                                len: len - end_off,
                            },
                            len: len - end_off,
                        };
                        piece.source = PieceSource::Add {
                            start: a,
                            len: start_off,
                        };
                        piece.len = start_off;
                        self.pieces.insert(start_piece + 1, right);
                    }
                }
            }
        } else {
            let mut new_pieces = Vec::with_capacity(self.pieces.len());
            for (idx, piece) in self.pieces.iter().enumerate() {
                if idx < start_piece {
                    new_pieces.push(*piece);
                    continue;
                }
                if idx > end_piece {
                    new_pieces.push(*piece);
                    continue;
                }
                if idx == start_piece && start_off > 0 {
                    let trimmed = trim_piece_end(*piece, start_off)?;
                    if trimmed.len > 0 {
                        new_pieces.push(trimmed);
                    }
                }
                if idx == end_piece && end_off < piece.len {
                    let trimmed = trim_piece_start(*piece, end_off)?;
                    if trimmed.len > 0 {
                        new_pieces.push(trimmed);
                    }
                }
            }
            self.pieces = new_pieces;
        }

        self.length -= end - start;
        Ok(deleted)
    }

    pub fn slice(&self, start: usize, end: usize) -> Result<Vec<u8>, ReadError> {
        if start > end || end > self.length {
            return Err(ReadError::OutOfBounds);
        }
        if start == end {
            return Ok(Vec::new());
        }

        let mut out = Vec::with_capacity(end - start);
        let (mut piece_idx, mut offset) = self.locate(start)?;
        let mut remaining = end - start;

        while remaining > 0 {
            let piece = self.pieces[piece_idx];
            let available = piece.len - offset;
            let take = remaining.min(available);
            out.extend_from_slice(&self.read_piece_range(piece, offset, take)?);
            remaining -= take;
            offset += take;
            if offset == piece.len {
                piece_idx += 1;
                offset = 0;
            }
        }
        Ok(out)
    }

    pub fn slice_str(&self, start: usize, end: usize) -> Result<String, ReadError> {
        let bytes = self.slice(start, end)?;
        String::from_utf8(bytes).map_err(|_| ReadError::InvalidUtf8)
    }

    pub fn iter_pieces(&self) -> impl Iterator<Item = Result<Vec<u8>, ReadError>> + '_ {
        self.pieces.iter().map(|piece| self.read_piece(*piece))
    }

    fn locate(&self, pos: usize) -> Result<(usize, usize), ReadError> {
        if pos > self.length {
            return Err(ReadError::OutOfBounds);
        }
        if self.pieces.is_empty() {
            return Ok((0, 0));
        }

        let mut acc = 0usize;
        for (idx, piece) in self.pieces.iter().enumerate() {
            if pos < acc + piece.len {
                return Ok((idx, pos - acc));
            }
            acc += piece.len;
        }
        Ok((self.pieces.len().saturating_sub(1), self.pieces.last().map(|p| p.len).unwrap_or(0)))
    }

    fn insert_piece(&mut self, piece_idx: usize, offset: usize, inserted: Piece) {
        if self.pieces.is_empty() {
            self.pieces.push(inserted);
            return;
        }

        let current = self.pieces[piece_idx];
        if offset == 0 {
            self.pieces.insert(piece_idx, inserted);
            return;
        }
        if offset == current.len {
            self.pieces.insert(piece_idx + 1, inserted);
            return;
        }

        let right = split_piece(current, offset);
        self.pieces[piece_idx] = trim_piece_end(current, offset).unwrap();
        self.pieces.insert(piece_idx + 1, inserted);
        self.pieces.insert(piece_idx + 2, right);
    }

    fn read_piece(&self, piece: Piece) -> Result<Vec<u8>, ReadError> {
        self.read_piece_range(piece, 0, piece.len)
    }

    fn read_piece_range(&self, piece: Piece, offset: usize, len: usize) -> Result<Vec<u8>, ReadError> {
        match piece.source {
            PieceSource::Original { start, len: total } => {
                let mmap = self.original.as_ref().ok_or(ReadError::OutOfBounds)?;
                let end = start + total;
                let from = start + offset;
                let to = from + len;
                if to > end || to > mmap.len() {
                    return Err(ReadError::OutOfBounds);
                }
                Ok(mmap[from..to].to_vec())
            }
            PieceSource::Add { start, len: total } => {
                let end = start + total;
                let from = start + offset;
                let to = from + len;
                if to > end || to > self.add_buffer.len() {
                    return Err(ReadError::OutOfBounds);
                }
                Ok(self.add_buffer[from..to].to_vec())
            }
        }
    }
}

fn split_piece(piece: Piece, offset: usize) -> Piece {
    match piece.source {
        PieceSource::Original { start, len } => Piece {
            source: PieceSource::Original {
                start: start + offset,
                len: len - offset,
            },
            len: piece.len - offset,
        },
        PieceSource::Add { start, len } => Piece {
            source: PieceSource::Add {
                start: start + offset,
                len: len - offset,
            },
            len: piece.len - offset,
        },
    }
}

fn trim_piece_end(piece: Piece, keep: usize) -> Result<Piece, ReadError> {
    if keep > piece.len {
        return Err(ReadError::OutOfBounds);
    }
    Ok(match piece.source {
        PieceSource::Original { start, .. } => Piece {
            source: PieceSource::Original {
                start,
                len: keep,
            },
            len: keep,
        },
        PieceSource::Add { start, .. } => Piece {
            source: PieceSource::Add {
                start,
                len: keep,
            },
            len: keep,
        },
    })
}

fn trim_piece_start(piece: Piece, skip: usize) -> Result<Piece, ReadError> {
    if skip > piece.len {
        return Err(ReadError::OutOfBounds);
    }
    Ok(split_piece(piece, skip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_delete_roundtrip() {
        let mut table = PieceTable::from_utf8_bytes(b"hello".to_vec());
        table.insert(5, b" world").unwrap();
        assert_eq!(table.len(), 11);
        assert_eq!(table.slice(0, table.len()).unwrap(), b"hello world");

        let removed = table.delete(5, 11).unwrap();
        assert_eq!(removed, b" world");
        assert_eq!(table.slice(0, table.len()).unwrap(), b"hello");
    }

    #[test]
    fn insert_in_middle_splits_piece() {
        let mut table = PieceTable::from_utf8_bytes(b"abcdef".to_vec());
        table.insert(3, b"123").unwrap();
        assert_eq!(table.slice(0, table.len()).unwrap(), b"abc123def");
        assert!(table.piece_count() >= 2);
    }

    #[test]
    fn delete_spanning_pieces() {
        let mut table = PieceTable::from_utf8_bytes(b"aaa".to_vec());
        table.insert(3, b"bbb").unwrap();
        table.insert(6, b"ccc").unwrap();
        // aaabbbccc — delete [2,7) => "abbbc", left "aacc"
        let removed = table.delete(2, 7).unwrap();
        assert_eq!(removed, b"abbbc");
        assert_eq!(table.slice(0, table.len()).unwrap(), b"aacc");
    }
}
