//! A text buffer for editors: the text of a file of any size, held in
//! bounded chunks, with the byte and line index an editor asks of it on every
//! keystroke.
//!
//! # Why not a `String`, or a `Vec<String>` of lines
//!
//! `roadmap-detailed.md` §3.5 (*Code-Aware TextEdit Widget*) asks for "rope or
//! gap buffer backing (efficient for large files)". The two editors in the
//! tree each keep a file as `Vec<String>`, one per line: typing in a line is
//! cheap, but joining two lines moves every line after them, a paste of a
//! thousand lines inserts a thousand elements into the middle, and a file with
//! one enormous line (minified JavaScript, a log with no newlines) is a single
//! `String` that every keystroke rewrites. A single `String` has that last
//! problem for every file.
//!
//! Here the text is a list of chunks of at most [`MAX_CHUNK`] bytes, each
//! knowing how many newlines it holds, with the byte offset and the newline
//! count before each chunk kept alongside. An edit rewrites the one or two
//! chunks it touches -- a few kilobytes, whatever the file's size -- and the
//! lookups an editor makes constantly ("which line is this offset on", "where
//! does line 40 000 start") are a binary search over chunks and a scan of one.
//!
//! **Why chunks and not a tree.** A rope's balanced tree makes an edit
//! O(log n) where this is O(chunks after the edit) for the index; but that is
//! a list of integers -- a 100 MB file is 50 000 chunks, tens of microseconds
//! to walk -- and a flat list is far simpler to keep correct, which is what a
//! buffer holding someone's file has to be first. [`TextBuffer::apply`] makes
//! a batch of edits -- what an editor with several carets does on each
//! keystroke -- each against an index made right by the one before it.
//!
//! # Offsets
//!
//! Every position is a byte offset into the text, always on a character
//! boundary, as `str` indices are. An edit at an offset that is not one is
//! refused ([`EditError::NotACharBoundary`]) rather than rounded: a caller that
//! computed it has a bug, and moving the edit to a neighbouring character would
//! hide it by changing the user's text somewhere they did not type.
//!
//! Lines are separated by `'\n'` alone. A file with `"\r\n"` line endings keeps
//! its `'\r'`s as text at the ends of its lines; turning them into a line-ending
//! setting is the editor's job when it opens and saves the file, not the
//! buffer's.

use core::fmt;
use core::ops::Range;

/// The most a chunk holds, in bytes. A chunk may briefly hold up to three
/// bytes more, so a split never has to cut a character in half.
pub const MAX_CHUNK: usize = 4096;

/// The least a chunk holds after an edit, when it has a neighbour it can be
/// merged into -- so deleting a little at a time does not leave the file in
/// thousands of slivers.
pub const MIN_CHUNK: usize = 1024;

/// What a new chunk is filled to when text is cut into chunks: half full, so
/// the next few keystrokes in it do not split it again at once.
const FILL_CHUNK: usize = MAX_CHUNK / 2;

/// Why an edit or a query was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    /// An offset past the end of the text.
    OutOfRange {
        /// The offset asked for.
        offset: usize,
        /// The text's length.
        len: usize,
    },
    /// An offset inside a character's encoding.
    NotACharBoundary(usize),
    /// A range whose start is after its end.
    Backwards {
        /// Its start.
        start: usize,
        /// Its end.
        end: usize,
    },
    /// Two edits of one batch that overlap: one starts at `start`, inside
    /// another that ends at `end`.
    Overlapping {
        /// Where the second edit starts.
        start: usize,
        /// Where the first one, which it starts inside, ends.
        end: usize,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { offset, len } => {
                write!(f, "offset {offset} is past the end of a {len}-byte text")
            }
            Self::NotACharBoundary(offset) => {
                write!(f, "offset {offset} is inside a character")
            }
            Self::Backwards { start, end } => {
                write!(f, "the range {start}..{end} runs backwards")
            }
            Self::Overlapping { start, end } => write!(
                f,
                "an edit starting at {start} overlaps another that ends at {end}"
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// One edit: replace the text in `range` with `text`. An insertion has an
/// empty range; a deletion an empty text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// What is replaced, in offsets of the text *before* any edit of the batch.
    pub range: Range<usize>,
    /// What replaces it.
    pub text: String,
}

/// A chunk of the text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Chunk {
    text: String,
    /// How many `'\n'` it holds.
    newlines: usize,
}

impl Chunk {
    fn new(text: String) -> Self {
        let newlines = count_newlines(&text);
        Self { text, newlines }
    }
}

/// The text of a file, in chunks, with its index. See the module docs.
#[derive(Clone, Debug, Default)]
pub struct TextBuffer {
    /// The text, in order. Never holds an empty chunk.
    chunks: Vec<Chunk>,
    /// The byte offset each chunk starts at, and after the last one the
    /// text's length: `chunks.len() + 1` entries.
    starts: Vec<usize>,
    /// The newlines before each chunk, and after the last one the text's
    /// newline count: `chunks.len() + 1` entries.
    newlines_before: Vec<usize>,
}

impl TextBuffer {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        let mut buffer = Self::default();
        buffer.reindex_from(0);
        buffer
    }

    /// A buffer holding `text`.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut buffer = Self {
            chunks: cut(text),
            starts: Vec::new(),
            newlines_before: Vec::new(),
        };
        buffer.reindex_from(0);
        buffer
    }

    /// The text's length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many lines the text has: one more than its newlines, so an empty
    /// text has one line, empty, and a text ending in a newline has an empty
    /// last line after it -- where the caret goes after the last newline.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.newlines_before
            .last()
            .copied()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// The whole text.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.len());
        for chunk in &self.chunks {
            out.push_str(&chunk.text);
        }
        out
    }

    /// The text in `range`.
    ///
    /// # Errors
    ///
    /// When either end is past the text or inside a character, or the range
    /// runs backwards.
    pub fn slice(&self, range: Range<usize>) -> Result<String, EditError> {
        self.check_range(&range)?;
        let mut out = String::with_capacity(range.end.saturating_sub(range.start));
        for (piece_range, chunk) in self.pieces(range) {
            if let Some(piece) = chunk.text.get(piece_range) {
                out.push_str(piece);
            }
        }
        Ok(out)
    }

    /// Replace the text in `range` with `text`.
    ///
    /// # Errors
    ///
    /// As [`slice`](Self::slice); the text is unchanged when it fails.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), EditError> {
        self.apply(&[Edit {
            range,
            text: text.to_owned(),
        }])
    }

    /// Insert `text` at `offset`.
    ///
    /// # Errors
    ///
    /// As [`slice`](Self::slice).
    pub fn insert(&mut self, offset: usize, text: &str) -> Result<(), EditError> {
        self.replace(offset..offset, text)
    }

    /// Delete the text in `range`.
    ///
    /// # Errors
    ///
    /// As [`slice`](Self::slice).
    pub fn delete(&mut self, range: Range<usize>) -> Result<(), EditError> {
        self.replace(range, "")
    }

    /// Make every edit in `edits` at once, each range in offsets of the text
    /// as it is *before* the batch -- what an editor with a caret on each of
    /// several lines does when a key is pressed.
    ///
    /// The edits may be given in any order; two whose ranges overlap are
    /// refused, since there is no single answer to what the text between
    /// them becomes. Two ranges may meet end to start. At one offset, an
    /// insertion goes before a range that starts there, and two insertions go
    /// in the order given.
    ///
    /// # Errors
    ///
    /// As [`slice`](Self::slice), for any edit, and
    /// [`EditError::Overlapping`] for two that overlap. Nothing is changed
    /// when any edit is refused.
    pub fn apply(&mut self, edits: &[Edit]) -> Result<(), EditError> {
        for edit in edits {
            self.check_range(&edit.range)?;
        }
        // By where each starts; at one offset, an insertion (an empty range)
        // before a range starting there. Stable, so insertions at one offset
        // keep the order they were given.
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&i| {
            edits
                .get(i)
                .map_or((0, false), |e| (e.range.start, !e.range.is_empty()))
        });
        for pair in order.windows(2) {
            let [a, b] = pair else { continue };
            let (Some(first), Some(second)) = (edits.get(*a), edits.get(*b)) else {
                continue;
            };
            if second.range.start < first.range.end {
                return Err(EditError::Overlapping {
                    start: second.range.start,
                    end: first.range.end,
                });
            }
        }
        // Last first, so each edit's offsets -- which are the text's before
        // the batch -- are untouched by the ones already made. The index is
        // made right after each: an edit can re-cut or merge the chunks it
        // touches, and the next edit's offset has to find its chunk in the
        // chunks as they are now, not as they were.
        for &i in order.iter().rev() {
            let Some(edit) = edits.get(i) else { continue };
            let touched = self.splice(edit.range.clone(), &edit.text);
            if touched != usize::MAX {
                self.reindex_from(touched);
            }
        }
        Ok(())
    }

    /// The line `offset` is on, counting from 0.
    ///
    /// # Errors
    ///
    /// When `offset` is past the text. (Inside a character is allowed here:
    /// a character is on one line.)
    pub fn line_of(&self, offset: usize) -> Result<usize, EditError> {
        if offset > self.len() {
            return Err(EditError::OutOfRange {
                offset,
                len: self.len(),
            });
        }
        let (index, local) = self.chunk_at(offset);
        let before = self.newlines_before.get(index).copied().unwrap_or(0);
        let within = self
            .chunks
            .get(index)
            .and_then(|chunk| chunk.text.as_bytes().get(..local))
            .map_or(0, newlines_in);
        Ok(before.saturating_add(within))
    }

    /// The offset line `line` starts at; `None` past the last line.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return Some(0);
        }
        if line >= self.line_count() {
            return None;
        }
        // The chunk holding the `line`-th newline: the last whose newlines
        // before it are fewer than `line`.
        let index = self
            .newlines_before
            .partition_point(|&before| before < line)
            .checked_sub(1)?;
        let chunk = self.chunks.get(index)?;
        let wanted = line.checked_sub(*self.newlines_before.get(index)?)?;
        let at = chunk
            .text
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .nth(wanted.checked_sub(1)?)
            .map(|(at, _)| at)?;
        self.starts.get(index)?.checked_add(at)?.checked_add(1)
    }

    /// The offsets line `line` spans, not counting the newline that ends it;
    /// `None` past the last line.
    #[must_use]
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        let start = self.line_start(line)?;
        let end = match self.line_start(line.saturating_add(1)) {
            // The newline ending this line is the byte before the next one.
            Some(next) => next.saturating_sub(1),
            None => self.len(),
        };
        Some(start..end)
    }

    /// The text of line `line`, without its newline; `None` past the last
    /// line.
    #[must_use]
    pub fn line(&self, line: usize) -> Option<String> {
        let range = self.line_range(line)?;
        self.slice(range).ok()
    }

    /// `offset` as a line and a byte column within it.
    ///
    /// # Errors
    ///
    /// As [`line_of`](Self::line_of).
    pub fn point(&self, offset: usize) -> Result<(usize, usize), EditError> {
        let line = self.line_of(offset)?;
        let start = self.line_start(line).unwrap_or(0);
        Ok((line, offset.saturating_sub(start)))
    }

    /// The offset of byte column `column` on line `line`, the column held to
    /// the line's end; `None` past the last line.
    #[must_use]
    pub fn offset(&self, line: usize, column: usize) -> Option<usize> {
        let range = self.line_range(line)?;
        Some(
            range
                .start
                .saturating_add(column.min(range.end.saturating_sub(range.start))),
        )
    }

    /// The character starting at `offset`; `None` at the end of the text or
    /// inside a character.
    #[must_use]
    pub fn char_at(&self, offset: usize) -> Option<char> {
        self.chars(offset).next().map(|(_, c)| c)
    }

    /// The character ending at `offset`; `None` at the start of the text or
    /// inside a character.
    #[must_use]
    pub fn char_before(&self, offset: usize) -> Option<char> {
        self.chars_rev(offset).next().map(|(_, c)| c)
    }

    /// The characters from `offset` to the end, each with its offset. Empty
    /// when `offset` is past the text or inside a character.
    pub fn chars(&self, offset: usize) -> impl Iterator<Item = (usize, char)> + '_ {
        let valid = self.check_offset(offset).is_ok();
        let (first, local) = self.chunk_at(offset);
        let mut chunk_index = first;
        let mut position = local;
        core::iter::from_fn(move || {
            if !valid {
                return None;
            }
            loop {
                let chunk = self.chunks.get(chunk_index)?;
                if let Some(c) = chunk
                    .text
                    .get(position..)
                    .and_then(|rest| rest.chars().next())
                {
                    let at = self
                        .starts
                        .get(chunk_index)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(position);
                    position = position.saturating_add(c.len_utf8());
                    return Some((at, c));
                }
                chunk_index = chunk_index.saturating_add(1);
                position = 0;
            }
        })
    }

    /// The characters before `offset`, nearest first, each with its offset.
    /// Empty when `offset` is past the text or inside a character.
    pub fn chars_rev(&self, offset: usize) -> impl Iterator<Item = (usize, char)> + '_ {
        let valid = self.check_offset(offset).is_ok();
        let (first, local) = self.chunk_at(offset);
        // `None` once the start of the text is passed.
        let mut chunk_index = Some(first);
        let mut position = local;
        core::iter::from_fn(move || {
            if !valid {
                return None;
            }
            loop {
                let index = chunk_index?;
                let chunk = self.chunks.get(index)?;
                if let Some(c) = chunk
                    .text
                    .get(..position)
                    .and_then(|before| before.chars().next_back())
                {
                    position = position.saturating_sub(c.len_utf8());
                    let at = self
                        .starts
                        .get(index)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(position);
                    return Some((at, c));
                }
                chunk_index = index.checked_sub(1);
                position = chunk_index
                    .and_then(|i| self.chunks.get(i))
                    .map_or(0, |c| c.text.len());
            }
        })
    }

    // ------------------------------------------------------------------
    // Inside
    // ------------------------------------------------------------------

    /// Whether `offset` is on the text and on a character boundary.
    fn check_offset(&self, offset: usize) -> Result<(), EditError> {
        if offset > self.len() {
            return Err(EditError::OutOfRange {
                offset,
                len: self.len(),
            });
        }
        let (index, local) = self.chunk_at(offset);
        let on_boundary = self
            .chunks
            .get(index)
            .is_none_or(|chunk| chunk.text.is_char_boundary(local));
        if on_boundary {
            Ok(())
        } else {
            Err(EditError::NotACharBoundary(offset))
        }
    }

    fn check_range(&self, range: &Range<usize>) -> Result<(), EditError> {
        if range.start > range.end {
            return Err(EditError::Backwards {
                start: range.start,
                end: range.end,
            });
        }
        self.check_offset(range.start)?;
        self.check_offset(range.end)
    }

    /// The chunk `offset` is in, and where in it. The end of the text is the
    /// end of the last chunk; an empty text is `(0, 0)`.
    fn chunk_at(&self, offset: usize) -> (usize, usize) {
        if self.chunks.is_empty() {
            return (0, 0);
        }
        // The last chunk starting at or before `offset`.
        let index = self
            .starts
            .get(..self.chunks.len())
            .map_or(0, |starts| starts.partition_point(|&s| s <= offset))
            .saturating_sub(1);
        let start = self.starts.get(index).copied().unwrap_or(0);
        (index, offset.saturating_sub(start))
    }

    /// The pieces of chunks `range` covers: each chunk's local range, with
    /// the chunk.
    fn pieces(&self, range: Range<usize>) -> impl Iterator<Item = (Range<usize>, &Chunk)> + '_ {
        let (first, _) = self.chunk_at(range.start);
        self.chunks
            .iter()
            .enumerate()
            .skip(first)
            .map_while(move |(index, chunk)| {
                let start = self.starts.get(index).copied().unwrap_or(0);
                if start >= range.end && !(range.is_empty() && start == range.start) {
                    return None;
                }
                let from = range.start.saturating_sub(start).min(chunk.text.len());
                let to = range.end.saturating_sub(start).min(chunk.text.len());
                Some((from..to, chunk))
            })
    }

    /// Replace `range` with `text` in the chunks, against an index that is
    /// right for the text as it is; answer the lowest chunk whose index
    /// entries are now stale (`usize::MAX` for an edit that changed nothing),
    /// for the caller to reindex from.
    fn splice(&mut self, range: Range<usize>, text: &str) -> usize {
        if range.is_empty() && text.is_empty() {
            return usize::MAX;
        }
        if self.chunks.is_empty() {
            self.chunks = cut(text);
            return 0;
        }
        let (first, first_local) = self.chunk_at(range.start);
        let (last, last_local) = self.chunk_at(range.end);
        // The first chunk's text before the range, the new text, and the last
        // chunk's text after it, cut into chunks and put where the covered
        // chunks were.
        let head = self
            .chunks
            .get(first)
            .and_then(|c| c.text.get(..first_local))
            .unwrap_or("");
        let tail = self
            .chunks
            .get(last)
            .and_then(|c| c.text.get(last_local..))
            .unwrap_or("");
        let mut joined = String::with_capacity(
            head.len()
                .saturating_add(text.len())
                .saturating_add(tail.len()),
        );
        joined.push_str(head);
        joined.push_str(text);
        joined.push_str(tail);
        let replacement = if joined.len() <= MAX_CHUNK {
            if joined.is_empty() {
                Vec::new()
            } else {
                vec![Chunk::new(joined)]
            }
        } else {
            cut(&joined)
        };
        let end = last.saturating_add(1).min(self.chunks.len());
        self.chunks.splice(first..end, replacement);
        // A small chunk here may be merged into the one before `first`, whose
        // length -- not its start -- then changes: the index is stale from
        // the chunk after that one, which is `first - 1` at the lowest.
        self.mend_around(first);
        first.saturating_sub(1)
    }

    /// Merge a chunk left under [`MIN_CHUNK`] near `index` into a neighbour
    /// it fits in, so small deletions do not leave the text in slivers.
    fn mend_around(&mut self, index: usize) {
        let start = index.saturating_sub(1);
        let mut i = start;
        // At most a few chunks around an edit can have changed size.
        while i < self.chunks.len() && i <= index.saturating_add(2) {
            let small = self.chunks.get(i).is_some_and(|c| c.text.len() < MIN_CHUNK);
            let merged = small
                && [i.checked_sub(1), i.checked_add(1)]
                    .into_iter()
                    .flatten()
                    .find(|&n| {
                        let (Some(a), Some(b)) = (self.chunks.get(i), self.chunks.get(n)) else {
                            return false;
                        };
                        a.text.len().saturating_add(b.text.len()) <= MAX_CHUNK
                    })
                    .is_some_and(|neighbour| {
                        let (left, right) = if neighbour < i {
                            (neighbour, i)
                        } else {
                            (i, neighbour)
                        };
                        let taken = self.chunks.remove(right);
                        if let Some(keep) = self.chunks.get_mut(left) {
                            keep.text.push_str(&taken.text);
                            keep.newlines = keep.newlines.saturating_add(taken.newlines);
                        }
                        true
                    });
            if !merged {
                i = i.saturating_add(1);
            }
        }
    }

    /// Rebuild the index from chunk `from` on.
    fn reindex_from(&mut self, from: usize) {
        let from = from.min(self.chunks.len());
        self.starts.truncate(from);
        self.newlines_before.truncate(from);
        let (mut offset, mut newlines) = match (from.checked_sub(1), from) {
            (Some(prev), _) => (
                self.starts
                    .get(prev)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(self.chunks.get(prev).map_or(0, |c| c.text.len())),
                self.newlines_before
                    .get(prev)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(self.chunks.get(prev).map_or(0, |c| c.newlines)),
            ),
            (None, _) => (0, 0),
        };
        for chunk in self.chunks.get(from..).unwrap_or(&[]) {
            self.starts.push(offset);
            self.newlines_before.push(newlines);
            offset = offset.saturating_add(chunk.text.len());
            newlines = newlines.saturating_add(chunk.newlines);
        }
        self.starts.push(offset);
        self.newlines_before.push(newlines);
    }
}

impl fmt::Display for TextBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in &self.chunks {
            f.write_str(&chunk.text)?;
        }
        Ok(())
    }
}

impl PartialEq for TextBuffer {
    /// Two buffers are equal when they hold the same text, however it is
    /// chunked.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.text() == other.text()
    }
}

impl Eq for TextBuffer {}

/// `text` cut into chunks of about [`FILL_CHUNK`] bytes each, each ending on
/// a character boundary.
fn cut(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::with_capacity(text.len().div_ceil(FILL_CHUNK));
    let mut rest = text;
    while !rest.is_empty() {
        let mut at = FILL_CHUNK.min(rest.len());
        // At most three steps: a character is four bytes at most, and the end
        // of `rest` is a boundary.
        while !rest.is_char_boundary(at) {
            at = at.saturating_add(1);
        }
        let (piece, after) = rest.split_at(at);
        chunks.push(Chunk::new(piece.to_owned()));
        rest = after;
    }
    chunks
}

fn count_newlines(text: &str) -> usize {
    newlines_in(text.as_bytes())
}

/// How many `'\n'` bytes `bytes` holds -- a slice that may end inside a
/// character, which is why this counts bytes rather than characters (a
/// newline is one byte in UTF-8, and never part of another character).
#[allow(
    clippy::naive_bytecount,
    reason = "the suggested `bytecount` crate is not a dependency; the loop \
              autovectorises, and a chunk is at most 4 KiB"
)]
fn newlines_in(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// Every invariant the index promises, checked against the text itself.
    fn check(buffer: &TextBuffer, model: &str) {
        assert_eq!(buffer.text(), model);
        assert_eq!(buffer.len(), model.len());
        assert_eq!(buffer.line_count(), model.matches('\n').count() + 1);
        assert_eq!(buffer.starts.len(), buffer.chunks.len() + 1);
        assert_eq!(buffer.newlines_before.len(), buffer.chunks.len() + 1);
        let mut offset = 0;
        let mut newlines = 0;
        for (i, chunk) in buffer.chunks.iter().enumerate() {
            assert!(!chunk.text.is_empty(), "chunk {i} is empty");
            assert!(
                chunk.text.len() <= MAX_CHUNK + 3,
                "chunk {i} is {} bytes",
                chunk.text.len()
            );
            assert_eq!(chunk.newlines, count_newlines(&chunk.text), "chunk {i}");
            assert_eq!(buffer.starts[i], offset, "chunk {i}'s start");
            assert_eq!(buffer.newlines_before[i], newlines, "chunk {i}'s newlines");
            offset += chunk.text.len();
            newlines += chunk.newlines;
        }
    }

    /// Line queries against a model's own lines.
    fn check_lines(buffer: &TextBuffer, model: &str) {
        let lines: Vec<&str> = model.split('\n').collect();
        assert_eq!(buffer.line_count(), lines.len());
        let mut start = 0;
        for (n, line) in lines.iter().enumerate() {
            assert_eq!(buffer.line_start(n), Some(start), "line {n}'s start");
            assert_eq!(buffer.line(n).as_deref(), Some(*line), "line {n}");
            assert_eq!(buffer.line_of(start), Ok(n), "the line of {start}");
            assert_eq!(
                buffer.line_of(start + line.len()),
                Ok(n),
                "the end of line {n}"
            );
            start += line.len() + 1;
        }
        assert_eq!(buffer.line_start(lines.len()), None);
        assert_eq!(buffer.line(lines.len()), None);
    }

    /// A small deterministic generator, so a failure repeats.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
        }
    }

    /// The nearest character boundary at or before `at` in `text`.
    fn floor(text: &str, mut at: usize) -> usize {
        at = at.min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    const PIECES: [&str; 8] = [
        "a",
        "hello\n",
        "\n",
        "é",
        "日本語\n",
        "\u{1F600}",
        "line one\nline two\nline three\n",
        "x",
    ];

    /// **The buffer is the text**: thousands of random insertions, deletions
    /// and replacements -- ASCII, accents, three- and four-byte characters,
    /// newlines; mostly a few characters where the caret is, now and then a
    /// large paste or a large deletion -- leave it equal to a plain `String`
    /// given the same edits, with every index entry right.
    #[test]
    fn random_edits_keep_the_buffer_equal_to_the_text() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let mut model: String = (0..3000).map(|i| PIECES[i % PIECES.len()]).collect();
        let mut buffer = TextBuffer::from_text(&model);
        let mut most_chunks = 0;
        for step in 0..4000 {
            let len = model.len();
            let a = floor(&model, rng.below(len + 1));
            // Mostly a few bytes, as typing and deleting are; now and then a
            // stretch that crosses chunks.
            let span = if rng.below(40) == 0 {
                rng.below(20_000)
            } else {
                rng.below(8)
            };
            let b = floor(&model, (a + span).min(len));
            let range = a.min(b)..a.max(b);
            let mut text = String::new();
            let pieces = if rng.below(40) == 0 {
                600
            } else {
                rng.below(4)
            };
            for _ in 0..pieces {
                text.push_str(PIECES[rng.below(PIECES.len())]);
            }
            match rng.below(3) {
                0 => {
                    buffer.insert(range.start, &text).unwrap();
                    model.insert_str(range.start, &text);
                }
                1 => {
                    buffer.delete(range.clone()).unwrap();
                    model.replace_range(range, "");
                }
                _ => {
                    buffer.replace(range.clone(), &text).unwrap();
                    model.replace_range(range, &text);
                }
            }
            check(&buffer, &model);
            most_chunks = most_chunks.max(buffer.chunks.len());
            if step % 200 == 0 {
                check_lines(&buffer, &model);
            }
        }
        check_lines(&buffer, &model);
        assert!(
            most_chunks > 10,
            "the text only ever spanned {most_chunks} chunks"
        );
    }

    /// **A batch is the edits made at once**, in offsets of the text before
    /// it, in any order -- what typing with several carets is.
    #[test]
    fn a_batch_is_made_in_the_offsets_before_it() {
        let mut buffer = TextBuffer::from_text("one\ntwo\nthree");
        buffer
            .apply(&[
                Edit {
                    range: 8..8,
                    text: "> ".into(),
                },
                Edit {
                    range: 0..0,
                    text: "> ".into(),
                },
                Edit {
                    range: 4..7,
                    text: "TWO".into(),
                },
            ])
            .unwrap();
        assert_eq!(buffer.text(), "> one\nTWO\n> three");
        check(&buffer, "> one\nTWO\n> three");

        // Two insertions at one offset go in the order given.
        let mut both = TextBuffer::from_text("ab");
        both.apply(&[
            Edit {
                range: 1..1,
                text: "1".into(),
            },
            Edit {
                range: 1..1,
                text: "2".into(),
            },
        ])
        .unwrap();
        assert_eq!(both.text(), "a12b");

        // An insertion at the start of a replaced range goes before its
        // replacement, whichever order the two are given in.
        for reverse in [false, true] {
            let mut edits = vec![
                Edit {
                    range: 1..3,
                    text: "R".into(),
                },
                Edit {
                    range: 1..1,
                    text: "I".into(),
                },
            ];
            if reverse {
                edits.reverse();
            }
            let mut buffer = TextBuffer::from_text("abcd");
            buffer.apply(&edits).unwrap();
            assert_eq!(buffer.text(), "aIRd", "reversed: {reverse}");
        }
    }

    /// **A batch whose edits re-cut the chunks between them is still made in
    /// the right places.** An edit that grows its chunk past the limit cuts it
    /// anew, and one that shrinks it merges it into a neighbour; the next
    /// edit's offset has to find its place in the chunks as they are then.
    /// Checked against a `String` given the same edits one at a time, highest
    /// offset first -- which is what a batch means.
    #[test]
    fn a_batch_across_recut_chunks_lands_in_the_right_places() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let base: String = (0..rng.below(20_000))
                .map(|_| PIECES[rng.below(PIECES.len())])
                .collect();
            let mut buffer = TextBuffer::from_text(&base);
            let mut model = base.clone();
            // Up to six disjoint edits, big and small, anywhere.
            let mut cuts: Vec<usize> = (0..12)
                .map(|_| floor(&base, rng.below(base.len() + 1)))
                .collect();
            cuts.sort_unstable();
            let mut edits = Vec::new();
            for pair in cuts.chunks(2) {
                let [a, b] = pair else { continue };
                let size = if rng.below(3) == 0 {
                    3000
                } else {
                    rng.below(5)
                };
                let text: String = (0..size).map(|_| PIECES[rng.below(PIECES.len())]).collect();
                edits.push(Edit {
                    range: *a..*b,
                    text,
                });
            }
            let mut expected = edits.clone();
            expected.sort_by_key(|e| std::cmp::Reverse(e.range.start));
            for edit in &expected {
                model.replace_range(edit.range.clone(), &edit.text);
            }
            // Handed over in a shuffled order.
            let shift = rng.below(edits.len().max(1));
            edits.rotate_left(shift);
            buffer.apply(&edits).unwrap();
            check(&buffer, &model);
        }
    }

    /// **Overlapping edits are refused, and nothing changes.**
    #[test]
    fn overlapping_edits_are_refused_whole() {
        let mut buffer = TextBuffer::from_text("abcdef");
        let refused = buffer.apply(&[
            Edit {
                range: 0..3,
                text: "x".into(),
            },
            Edit {
                range: 2..4,
                text: "y".into(),
            },
        ]);
        assert_eq!(refused, Err(EditError::Overlapping { start: 2, end: 3 }));
        assert_eq!(buffer.text(), "abcdef");
        // End to start is not an overlap.
        buffer
            .apply(&[
                Edit {
                    range: 0..3,
                    text: "x".into(),
                },
                Edit {
                    range: 3..6,
                    text: "y".into(),
                },
            ])
            .unwrap();
        assert_eq!(buffer.text(), "xy");
    }

    /// **An offset inside a character, or past the end, is refused** -- not
    /// rounded to a neighbour, which would change the text somewhere the user
    /// did not type.
    #[test]
    fn a_bad_offset_is_refused_not_rounded() {
        let mut buffer = TextBuffer::from_text("aé日");
        assert_eq!(buffer.insert(2, "x"), Err(EditError::NotACharBoundary(2)));
        assert_eq!(buffer.delete(0..5), Err(EditError::NotACharBoundary(5)));
        assert_eq!(
            buffer.insert(99, "x"),
            Err(EditError::OutOfRange { offset: 99, len: 6 })
        );
        // Written out, since a backwards range is what is being refused.
        let backwards = Range { start: 4, end: 1 };
        assert_eq!(
            buffer.slice(backwards),
            Err(EditError::Backwards { start: 4, end: 1 })
        );
        assert_eq!(buffer.text(), "aé日");
        assert_eq!(buffer.char_at(1), Some('é'));
        assert_eq!(buffer.char_at(2), None, "inside a character");
        assert_eq!(buffer.char_before(6), Some('日'));
    }

    /// **Lines, points and offsets agree**, including the empty last line
    /// after a final newline, and a column past a line's end is held to it.
    #[test]
    fn lines_points_and_offsets_agree() {
        let buffer = TextBuffer::from_text("ab\n\ncdé\n");
        check_lines(&buffer, "ab\n\ncdé\n");
        assert_eq!(buffer.line_count(), 4);
        assert_eq!(buffer.point(5), Ok((2, 1)));
        assert_eq!(buffer.offset(2, 1), Some(5));
        assert_eq!(buffer.offset(2, 99), Some(8), "held to the line's end");
        assert_eq!(buffer.offset(3, 0), Some(9));
        assert_eq!(buffer.offset(4, 0), None);
        assert_eq!(buffer.line_range(1), Some(3..3));

        let empty = TextBuffer::new();
        assert_eq!(empty.line_count(), 1);
        assert_eq!(empty.line(0).as_deref(), Some(""));
        assert_eq!(empty.line_of(0), Ok(0));
        assert!(empty.is_empty());
    }

    /// **The characters either side of an offset are walked across chunks**,
    /// forwards and back, each with its offset.
    #[test]
    fn characters_are_walked_across_chunks() {
        let text: String = "ab日\n".repeat(2000);
        let buffer = TextBuffer::from_text(&text);
        assert!(buffer.chunks.len() > 2);
        let forward: String = buffer.chars(0).map(|(_, c)| c).collect();
        assert_eq!(forward, text);
        let back: String = buffer.chars_rev(text.len()).map(|(_, c)| c).collect();
        assert_eq!(back, text.chars().rev().collect::<String>());
        for (at, c) in buffer.chars(3000).take(50) {
            assert_eq!(text[at..].chars().next(), Some(c), "at {at}");
        }
        for (at, c) in buffer.chars_rev(5000).take(50) {
            assert_eq!(text[at..].chars().next(), Some(c), "at {at}");
        }
        // `日` is bytes 2 to 4 of each repeat: 3 is inside it.
        assert_eq!(buffer.chars(3).count(), 0, "from inside a character");
        assert_eq!(
            buffer.chars_rev(3).count(),
            0,
            "back from inside a character"
        );
    }

    /// **An edit touches only the chunks around it**: typing a character into
    /// the middle of a large text rewrites one chunk, not the text.
    #[test]
    fn an_edit_rewrites_only_the_chunks_it_touches() {
        let text = "0123456789\n".repeat(100_000);
        let mut buffer = TextBuffer::from_text(&text);
        let before: Vec<*const u8> = buffer.chunks.iter().map(|c| c.text.as_ptr()).collect();
        let middle = text.len() / 2;
        buffer.insert(middle, "!").unwrap();
        let after: Vec<*const u8> = buffer.chunks.iter().map(|c| c.text.as_ptr()).collect();
        let rewritten = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert!(
            rewritten <= 3,
            "{rewritten} of {} chunks were rewritten for one character",
            before.len()
        );
        assert_eq!(buffer.len(), text.len() + 1);
    }

    /// **Deletions do not leave the text in slivers.** A deletion that
    /// crosses chunks joins what is left either side of it by itself; one
    /// inside a single chunk leaves that chunk small, and it is merged into a
    /// neighbour it fits. Deleting the middle of every chunk -- ten bytes
    /// left of each -- would otherwise leave a hundred ten-byte chunks.
    #[test]
    fn deletions_do_not_leave_slivers() {
        let mut model = "0123456789".repeat(20_000);
        let mut buffer = TextBuffer::from_text(&model);
        let chunks = buffer.chunks.len();
        assert!(chunks > 90);
        // From the last chunk to the first, each by where it is now.
        for i in (0..chunks).rev() {
            let (Some(&start), Some(chunk)) = (buffer.starts.get(i), buffer.chunks.get(i)) else {
                continue;
            };
            let len = chunk.text.len();
            if len > 20 {
                let range = start + 5..start + len - 5;
                buffer.delete(range.clone()).unwrap();
                model.replace_range(range, "");
            }
        }
        check(&buffer, &model);
        assert!(
            buffer.chunks.len() <= 2,
            "{} chunks for {} bytes",
            buffer.chunks.len(),
            model.len()
        );
    }

    /// **Equality is the text**, however it is chunked.
    #[test]
    fn equality_is_the_text() {
        let whole = TextBuffer::from_text(&"ab\n".repeat(5000));
        let mut built = TextBuffer::new();
        for _ in 0..5000 {
            let end = built.len();
            built.insert(end, "ab\n").unwrap();
        }
        assert_eq!(whole, built);
        assert_eq!(whole.to_string(), built.text());
    }

    /// A timing, not an assertion: run with `--ignored --nocapture` to see
    /// what an editor's commonest operations cost on a large file.
    #[test]
    #[ignore = "a measurement to read, not a check"]
    fn how_long_the_common_operations_take() {
        let text = "fn main() { println!(\"hello\"); }\n".repeat(300_000);
        let started = std::time::Instant::now();
        let mut buffer = TextBuffer::from_text(&text);
        println!(
            "load {} MB: {:?}",
            text.len() / 1_000_000,
            started.elapsed()
        );
        let started = std::time::Instant::now();
        for i in 0..10_000 {
            let at = (i * 997) % buffer.len();
            let at = buffer.line_start(buffer.line_of(at).unwrap()).unwrap();
            buffer.insert(at, "x").unwrap();
        }
        println!("10 000 scattered keystrokes: {:?}", started.elapsed());
        let started = std::time::Instant::now();
        let mut sum = 0;
        for line in (0..buffer.line_count()).step_by(97) {
            sum += buffer.line_start(line).unwrap();
        }
        println!(
            "{} line lookups: {:?} ({sum})",
            buffer.line_count() / 97,
            started.elapsed()
        );
    }
}
