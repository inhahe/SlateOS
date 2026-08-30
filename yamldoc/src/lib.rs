//! Round-trip YAML editing for configuration files.
//!
//! `design.txt` says configuration is YAML, "processed with a library that
//! preserves comments and formatting". That second half is the whole reason
//! this crate exists. The obvious way to store settings — deserialize into a
//! struct, mutate it, serialize it back — is *lossy*: it discards every
//! comment, every blank line, every deliberate ordering and every choice of
//! quoting in the file, and hands the user back a machine-flattened version of
//! what they wrote. Do that once and nobody hand-edits a config again, because
//! their annotations do not survive the next time they touch the settings UI.
//!
//! So this is not a serializer. It is a *text editor with a YAML index*: the
//! document is kept as its original lines, and a write splices a new value
//! into the one line that holds it. Everything the edit did not name comes out
//! byte for byte as it went in.
//!
//! ```
//! # use yamldoc::Document;
//! let mut doc = Document::parse("# my settings\nfonts:\n  size: 13  # points\n");
//! assert_eq!(doc.get_i64(&["fonts", "size"]), Some(13));
//! doc.set_i64(&["fonts", "size"], 15);
//! assert_eq!(doc.to_text(), "# my settings\nfonts:\n  size: 15  # points\n");
//! ```
//!
//! # Paths
//!
//! Every accessor takes a path as a slice of keys — `&["fonts", "ui_font"]` —
//! rather than a dotted string. A dotted string would be shorter to write and
//! would silently do the wrong thing the first time a config key contained a
//! dot, which for a font family (`Noto Sans CJK.Bold`) or a hostname is not
//! hypothetical.
//!
//! # What is understood
//!
//! Block mappings and block sequences nested by indentation; plain, single-
//! and double-quoted scalars; `#` comments and blank lines anywhere. Enough
//! for a configuration file, which is what this is for.
//!
//! Anything else — anchors and aliases, tags, flow collections, multi-line
//! block scalars, multiple documents — is treated as **opaque text**: it is
//! not indexed, so it cannot be read or written through this API, but it
//! survives a round trip untouched. That is the right failure for a config
//! editor: a file using a feature this does not model is preserved rather
//! than corrupted.
//!
//! # Why parsing cannot fail
//!
//! [`Document::parse`] is infallible and every getter returns [`Option`].
//! There is no "malformed YAML" error to report because there is nothing this
//! crate could usefully do with one: a settings loader's answer to a key it
//! cannot read is to fall back to the default, which is exactly what `None`
//! says. A file of pure nonsense parses into a document with no readable keys
//! and re-serializes to itself.

#![no_std]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;
use core::ops::Range;

/// Indentation added per nesting level when a document gives no example of
/// its own to copy.
const DEFAULT_INDENT_STEP: usize = 2;

// ============================================================================
// Document
// ============================================================================

/// A YAML configuration file, held as text and edited in place.
///
/// Cloning is cheap relative to re-reading the file, and is the usual way to
/// keep a pristine copy for a revert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    /// The file's lines, without their terminators.
    lines: Vec<String>,
    /// The terminator the file uses, reproduced on output.
    newline: Newline,
    /// Whether the original text ended with a terminator. Preserved because
    /// adding one to a file that lacked it is still a diff.
    trailing_newline: bool,
}

/// Which line terminator a document uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Newline {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    /// The characters to write.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty document, as when the config file does not exist yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            newline: Newline::Lf,
            trailing_newline: true,
        }
    }

    /// Read a document. Never fails — see the crate docs for why.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        // The terminator is taken from the first `\r\n` rather than by a
        // majority vote: mixed terminators mean something already went wrong
        // upstream, and this way the choice is at least reproducible.
        let newline = if text.contains("\r\n") {
            Newline::CrLf
        } else {
            Newline::Lf
        };
        let trailing_newline = text.is_empty() || text.ends_with('\n');
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect();
        // Splitting a newline-terminated file yields a final empty element
        // that is not a line; `trailing_newline` is what records it.
        if trailing_newline {
            lines.pop();
        }
        Self {
            lines,
            newline,
            trailing_newline,
        }
    }

    /// The document as text, ready to write back to the file.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push_str(self.newline.as_str());
            }
            out.push_str(line);
        }
        if self.trailing_newline && !self.lines.is_empty() {
            out.push_str(self.newline.as_str());
        }
        out
    }

    /// Whether the document has no lines at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    // -- reading -------------------------------------------------------------

    /// The scalar at `path`, with quoting and escapes resolved.
    ///
    /// `None` if the path does not exist, or names a nested block or a
    /// sequence rather than a scalar.
    #[must_use]
    pub fn get_str(&self, path: &[&str]) -> Option<String> {
        let rows = self.scan();
        let at = resolve(&rows, path)?;
        let Kind::Entry { value: Some(v), .. } = &rows.get(at)?.kind else {
            return None;
        };
        Some(unquote(self.lines.get(at)?.get(v.start..v.end)?))
    }

    /// The boolean at `path`.
    ///
    /// YAML 1.2 spells booleans `true`/`false`. The YAML 1.1 spellings
    /// (`yes`, `on`, `y`, …) are deliberately not accepted, because accepting
    /// them is how the country code `NO` becomes `false`.
    #[must_use]
    pub fn get_bool(&self, path: &[&str]) -> Option<bool> {
        match self.get_str(path)?.as_str() {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        }
    }

    /// The integer at `path`.
    #[must_use]
    pub fn get_i64(&self, path: &[&str]) -> Option<i64> {
        self.get_str(path)?.parse().ok()
    }

    /// The number at `path`, accepting an integer spelling.
    ///
    /// The infinities and NaN that YAML can spell (`.inf`, `.nan`) read as
    /// absent: a configuration value that is not a finite number is one no
    /// caller can clamp into a sane range, so the default is a better answer
    /// than a value that poisons every arithmetic it touches.
    #[must_use]
    pub fn get_f64(&self, path: &[&str]) -> Option<f64> {
        let value: f64 = self.get_str(path)?.parse().ok()?;
        value.is_finite().then_some(value)
    }

    /// The sequence at `path`, as resolved scalars.
    ///
    /// `None` if the path does not exist; an empty vector if it exists but
    /// holds no `- ` items.
    #[must_use]
    pub fn get_seq(&self, path: &[&str]) -> Option<Vec<String>> {
        let rows = self.scan();
        let at = resolve(&rows, path)?;
        let block = block_of(&rows, at);
        let indent = block_indent(&rows, block.clone());
        let mut out = Vec::new();
        for i in block {
            let Some(row) = rows.get(i) else { continue };
            let Kind::Item { value } = &row.kind else {
                continue;
            };
            if Some(row.indent) != indent {
                continue;
            }
            if let Some(raw) = self.lines.get(i).and_then(|l| l.get(value.start..value.end)) {
                out.push(unquote(raw));
            }
        }
        Some(out)
    }

    /// The keys of the mapping at `path`, in file order. An empty path asks
    /// for the document's top level.
    #[must_use]
    pub fn keys(&self, path: &[&str]) -> Vec<String> {
        let rows = self.scan();
        let block = if path.is_empty() {
            0..rows.len()
        } else {
            match resolve(&rows, path) {
                Some(at) => block_of(&rows, at),
                None => return Vec::new(),
            }
        };
        let indent = block_indent(&rows, block.clone());
        let mut out = Vec::new();
        for i in block {
            if let Some(row) = rows.get(i)
                && let Kind::Entry { key, .. } = &row.kind
                && Some(row.indent) == indent
            {
                out.push(key.clone());
            }
        }
        out
    }

    /// Whether `path` names anything — a scalar, a block or a sequence.
    #[must_use]
    pub fn contains(&self, path: &[&str]) -> bool {
        resolve(&self.scan(), path).is_some()
    }

    // -- writing -------------------------------------------------------------

    /// Set the string at `path`, creating any missing parent mappings.
    ///
    /// The value is quoted only when it has to be — when a plain spelling
    /// would read back as something else. That covers the empty string,
    /// leading or trailing space, anything starting with a YAML indicator,
    /// and anything that would otherwise parse as a number, boolean or null.
    pub fn set_str(&mut self, path: &[&str], value: &str) {
        let literal = quote_if_needed(value);
        self.set_literal(path, &literal);
    }

    /// Set the boolean at `path`.
    pub fn set_bool(&mut self, path: &[&str], value: bool) {
        self.set_literal(path, if value { "true" } else { "false" });
    }

    /// Set the integer at `path`.
    pub fn set_i64(&mut self, path: &[&str], value: i64) {
        let mut buf = String::new();
        // Writing to a `String` cannot fail; the result is discarded rather
        // than unwrapped so this stays panic-free.
        let _ = write!(buf, "{value}");
        self.set_literal(path, &buf);
    }

    /// Set the number at `path`.
    ///
    /// A whole number is written with a `.0`, so that it keeps reading back
    /// as a number rather than as an integer. `13` and `13.0` are different
    /// YAML types, and a file that flips between them produces a diff on
    /// every save even when nothing changed.
    pub fn set_f64(&mut self, path: &[&str], value: f64) {
        let mut buf = String::new();
        if value.is_finite() {
            let _ = write!(buf, "{value:?}");
        } else {
            // A non-finite value has no configuration meaning. Writing the
            // null that `get_f64` declines is honest; writing `.inf` would
            // look like a deliberate setting.
            buf.push_str("null");
        }
        self.set_literal(path, &buf);
    }

    /// Replace the sequence at `path` with `items`, creating the key if it is
    /// absent and discarding whatever was there before.
    pub fn set_seq(&mut self, path: &[&str], items: &[&str]) {
        // The whole path, not just the parents: a key cannot be both
        // `key: value` and a sequence, and `ensure_block` is what strips the
        // leftover inline scalar from a previous shape.
        self.ensure_block(path);
        let rows = self.scan();
        let Some(at) = resolve(&rows, path) else {
            return;
        };
        let block = block_of(&rows, at);
        let indent = block_indent(&rows, block.clone())
            .unwrap_or_else(|| child_indent(&rows, at, self.indent_step()));
        self.lines.drain(block.clone());
        let mut insert = block.start;
        for item in items {
            let mut line = String::new();
            for _ in 0..indent {
                line.push(' ');
            }
            line.push_str("- ");
            line.push_str(&quote_if_needed(item));
            if insert <= self.lines.len() {
                self.lines.insert(insert, line);
                insert = insert.saturating_add(1);
            }
        }
    }

    /// Remove `path` and everything nested under it.
    ///
    /// Returns whether anything was there. A comment sitting above the key is
    /// deliberately left behind: it is prose the user wrote, and guessing that
    /// it described the key rather than the section would delete their words
    /// on a hunch.
    pub fn remove(&mut self, path: &[&str]) -> bool {
        let rows = self.scan();
        let Some(at) = resolve(&rows, path) else {
            return false;
        };
        let end = block_of(&rows, at)
            .end
            .max(at.saturating_add(1))
            .min(self.lines.len());
        if at < end {
            self.lines.drain(at..end);
        }
        true
    }

    // -- writing internals ---------------------------------------------------

    /// Write `literal` — already in its final YAML spelling — at `path`.
    fn set_literal(&mut self, path: &[&str], literal: &str) {
        let Some((last, parents)) = path.split_last() else {
            return;
        };
        self.ensure_block(parents);
        let rows = self.scan();
        if let Some(at) = resolve(&rows, path) {
            // The key exists. Any nested block it owned is replaced by the
            // scalar: YAML cannot express a key that is both a mapping and a
            // value, so one of the two has to lose.
            let block = block_of(&rows, at);
            if block.start < block.end && block.end <= self.lines.len() {
                self.lines.drain(block);
            }
            self.rewrite_value(at, literal);
            return;
        }
        let (insert, indent) = self.insertion_point(&rows, parents);
        let mut line = String::new();
        for _ in 0..indent {
            line.push(' ');
        }
        line.push_str(&quote_key(last));
        line.push_str(": ");
        line.push_str(literal);
        if insert <= self.lines.len() {
            self.lines.insert(insert, line);
        }
    }

    /// Replace the value on the entry line at `at`, keeping its key, its
    /// spacing and any trailing comment.
    fn rewrite_value(&mut self, at: usize, literal: &str) {
        let rows = self.scan();
        let Some(row) = rows.get(at) else { return };
        let Kind::Entry {
            value, colon_at, ..
        } = &row.kind
        else {
            return;
        };
        let Some(line) = self.lines.get(at) else {
            return;
        };
        // With a value present, splice over exactly it. Without one, splice
        // in just past the colon, which turns `key:  # note` into
        // `key: value  # note` rather than displacing the comment.
        let (head, tail, space) = match value {
            Some(v) => (line.get(..v.start), line.get(v.end..), false),
            None => {
                let after = colon_at.saturating_add(1);
                (line.get(..after), line.get(after..), true)
            }
        };
        let (Some(head), Some(tail)) = (head, tail) else {
            return;
        };
        let mut next = String::with_capacity(line.len().saturating_add(literal.len()));
        next.push_str(head);
        if space {
            next.push(' ');
        }
        next.push_str(literal);
        next.push_str(tail);
        if let Some(slot) = self.lines.get_mut(at) {
            *slot = next;
        }
    }

    /// Strip the inline value from the entry at `at`, leaving a bare `key:`
    /// (and any trailing comment) for a block to be written under.
    fn clear_value(&mut self, at: usize) {
        let rows = self.scan();
        let Some(row) = rows.get(at) else { return };
        let Kind::Entry {
            value: Some(v),
            colon_at,
            ..
        } = &row.kind
        else {
            return;
        };
        let Some(line) = self.lines.get(at) else {
            return;
        };
        let after = colon_at.saturating_add(1);
        let (Some(head), Some(tail)) = (line.get(..after), line.get(v.end..)) else {
            return;
        };
        let mut next = String::with_capacity(line.len());
        next.push_str(head);
        next.push_str(tail);
        if let Some(slot) = self.lines.get_mut(at) {
            *slot = next;
        }
    }

    /// Make sure every key along `path` exists as a mapping, creating the
    /// missing ones as bare `key:` lines.
    ///
    /// A key that already exists but holds an inline scalar has that scalar
    /// stripped. YAML cannot spell a key that is both `fonts: 13` and the
    /// parent of `fonts.size`, so writing the child has to take the value
    /// away — and doing it here, rather than leaving `fonts: 13` with an
    /// indented `size:` under it, is the difference between a changed setting
    /// and a file that no longer parses.
    fn ensure_block(&mut self, path: &[&str]) {
        for depth in 1..=path.len() {
            let Some(prefix) = path.get(..depth) else {
                continue;
            };
            let rows = self.scan();
            if let Some(at) = resolve(&rows, prefix) {
                self.clear_value(at);
                continue;
            }
            let above = depth.saturating_sub(1);
            let (Some(parents), Some(key)) = (path.get(..above), path.get(above)) else {
                continue;
            };
            let (insert, indent) = self.insertion_point(&rows, parents);
            let mut line = String::new();
            for _ in 0..indent {
                line.push(' ');
            }
            line.push_str(&quote_key(key));
            line.push(':');
            if insert <= self.lines.len() {
                self.lines.insert(insert, line);
            }
        }
    }

    /// Where a new child of the mapping at `parents` belongs, and at what
    /// indentation.
    ///
    /// New keys land after the last real line of the block rather than at its
    /// start, so a blank line or a comment the user left as a separator before
    /// the *next* section stays a separator instead of ending up mid-section.
    fn insertion_point(&self, rows: &[Row], parents: &[&str]) -> (usize, usize) {
        if parents.is_empty() {
            let indent = block_indent(rows, 0..rows.len()).unwrap_or(0);
            return (last_real_line(rows, 0..rows.len()), indent);
        }
        let Some(at) = resolve(rows, parents) else {
            // `ensure_block` runs first, so this is unreachable in practice.
            // Appending at the end is the harmless answer if it is not.
            return (self.lines.len(), 0);
        };
        let block = block_of(rows, at);
        let indent = block_indent(rows, block.clone())
            .unwrap_or_else(|| child_indent(rows, at, self.indent_step()));
        if block.start >= block.end {
            return (at.saturating_add(1), indent);
        }
        (last_real_line(rows, block), indent)
    }

    /// How far this document indents one nesting level, copied from the first
    /// nested block it has. A file indented by four keeps being indented by
    /// four when this crate adds a key to it.
    fn indent_step(&self) -> usize {
        let rows = self.scan();
        let mut outer: Option<usize> = None;
        for row in &rows {
            if matches!(row.kind, Kind::Blank | Kind::Comment) {
                continue;
            }
            if let Some(prev) = outer
                && row.indent > prev
            {
                return row.indent.saturating_sub(prev);
            }
            outer = Some(row.indent);
        }
        DEFAULT_INDENT_STEP
    }

    /// Classify every line.
    ///
    /// Recomputed on demand rather than cached: the lines are the single
    /// source of truth, and a cached index that can disagree with them is the
    /// bug this design exists to make unrepresentable. Config files are a few
    /// dozen lines and edits happen when a user clicks Save.
    fn scan(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::with_capacity(self.lines.len());
        // Lines inside a `|`/`>` block scalar are arbitrary text that may
        // contain a colon; indexing them as mapping entries would invent keys
        // out of a user's prose. `opaque_below` holds the indentation the
        // block belongs to; anything deeper is its body.
        let mut opaque_below: Option<usize> = None;
        for line in &self.lines {
            let indent = leading_spaces(line);
            let body = line.get(indent..).unwrap_or("");
            if body.is_empty() {
                rows.push(Row {
                    indent,
                    kind: Kind::Blank,
                });
                continue;
            }
            if let Some(limit) = opaque_below {
                if indent > limit {
                    rows.push(Row {
                        indent,
                        kind: Kind::Opaque,
                    });
                    continue;
                }
                opaque_below = None;
            }
            if body.starts_with('#') {
                rows.push(Row {
                    indent,
                    kind: Kind::Comment,
                });
                continue;
            }
            if let Some(rest) = sequence_item(body) {
                let start = indent.saturating_add(body.len().saturating_sub(rest.len()));
                rows.push(Row {
                    indent,
                    kind: Kind::Item {
                        value: value_range(line, start),
                    },
                });
                continue;
            }
            if let Some((key, colon_at)) = split_key(body, indent) {
                let span = value_range(line, colon_at.saturating_add(1));
                let value = (span.start < span.end).then_some(span);
                if let Some(v) = &value
                    && let Some(text) = line.get(v.start..v.end)
                    && (text.starts_with('|') || text.starts_with('>'))
                {
                    opaque_below = Some(indent);
                }
                rows.push(Row {
                    indent,
                    kind: Kind::Entry {
                        key,
                        colon_at,
                        value,
                    },
                });
                continue;
            }
            rows.push(Row {
                indent,
                kind: Kind::Opaque,
            });
        }
        rows
    }
}

// ============================================================================
// Line classification
// ============================================================================

/// One line, indexed.
#[derive(Clone, Debug)]
struct Row {
    /// Leading spaces.
    indent: usize,
    /// What the line is.
    kind: Kind,
}

/// What a line is.
#[derive(Clone, Debug)]
enum Kind {
    /// Empty or whitespace only.
    Blank,
    /// A whole-line `#` comment.
    Comment,
    /// A mapping entry: `key:` with an optional inline value.
    Entry {
        /// The key, with its own quoting resolved.
        key: String,
        /// Byte index of the `:` that ends the key.
        colon_at: usize,
        /// Byte range of the value, excluding any trailing comment.
        value: Option<Range<usize>>,
    },
    /// A `- ` sequence item.
    Item {
        /// Byte range of the item, excluding any trailing comment.
        value: Range<usize>,
    },
    /// Something this crate does not model, carried through untouched.
    Opaque,
}

/// The text after a `- ` marker, or `None` if this is not a sequence item.
fn sequence_item(body: &str) -> Option<&str> {
    if body == "-" {
        return Some("");
    }
    body.strip_prefix("- ").map(str::trim_start)
}

/// Number of leading space characters.
fn leading_spaces(line: &str) -> usize {
    line.len().saturating_sub(line.trim_start_matches(' ').len())
}

/// Split `body` — a line with its indentation stripped — into its key and the
/// byte index of the colon that ends it, in the *whole line's* coordinates.
///
/// `None` when the line is not a mapping entry, which includes a plain scalar
/// holding a colon that is not followed by a space: YAML only treats `:` as a
/// key separator at end of line or before whitespace, which is what lets
/// `url: http://example.com` mean what it looks like.
fn split_key(body: &str, indent: usize) -> Option<(String, usize)> {
    let mut quote: Option<char> = None;
    for (i, ch) in body.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                // A comment reached before any colon means there is no key on
                // this line at all.
                '#' => return None,
                ':' => {
                    let after = body.get(i.saturating_add(1)..).unwrap_or("");
                    if !after.is_empty() && !after.starts_with(' ') {
                        continue;
                    }
                    let key = unquote(body.get(..i)?.trim_end());
                    if key.is_empty() {
                        return None;
                    }
                    return Some((key, indent.saturating_add(i)));
                }
                _ => {}
            },
        }
    }
    None
}

/// The byte range of the value starting at `from`, with leading whitespace
/// and any trailing `#` comment excluded.
///
/// A `#` only begins a comment when whitespace precedes it. By that rule
/// `color: #ff0000` really is a comment, while `color: "#ff0000"` and
/// `tag: a#b` are not — which is what YAML says, and what a user whose hex
/// colour vanished needs to be told.
fn value_range(line: &str, from: usize) -> Range<usize> {
    let Some(rest) = line.get(from..) else {
        return from..from;
    };
    let start = from.saturating_add(rest.len().saturating_sub(rest.trim_start().len()));
    let Some(body) = line.get(start..) else {
        return start..start;
    };
    let mut quote: Option<char> = None;
    let mut end = body.len();
    let mut prev_space = true;
    for (i, ch) in body.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '#' if prev_space => {
                    end = i;
                    break;
                }
                _ => {}
            },
        }
        prev_space = ch == ' ' || ch == '\t';
    }
    let trimmed = body.get(..end).unwrap_or("").trim_end();
    start..start.saturating_add(trimmed.len())
}

// ============================================================================
// Navigation
// ============================================================================

/// The line index of the entry named by `path`.
fn resolve(rows: &[Row], path: &[&str]) -> Option<usize> {
    let mut range = 0..rows.len();
    let mut found = None;
    for key in path {
        let at = find_entry(rows, range, key)?;
        found = Some(at);
        range = block_of(rows, at);
    }
    found
}

/// The first entry in `range` whose key is `key`, at the block's own
/// indentation — so a nested `size` is never mistaken for a top-level one.
fn find_entry(rows: &[Row], range: Range<usize>, key: &str) -> Option<usize> {
    let indent = block_indent(rows, range.clone())?;
    for i in range {
        if let Some(row) = rows.get(i)
            && row.indent == indent
            && let Kind::Entry { key: k, .. } = &row.kind
            && k == key
        {
            return Some(i);
        }
    }
    None
}

/// The indentation shared by the entries and items of the block spanning
/// `range` — that of its first real line. `None` for a block with none.
fn block_indent(rows: &[Row], range: Range<usize>) -> Option<usize> {
    for i in range {
        let row = rows.get(i)?;
        match row.kind {
            Kind::Blank | Kind::Comment | Kind::Opaque => {}
            Kind::Entry { .. } | Kind::Item { .. } => return Some(row.indent),
        }
    }
    None
}

/// The lines nested under the entry at `at`.
///
/// Interior blank lines and comments are inside the block; trailing ones are
/// not — so removing a key does not swallow the blank line separating it from
/// the next section, and adding one does not land after that separator.
fn block_of(rows: &[Row], at: usize) -> Range<usize> {
    let Some(row) = rows.get(at) else {
        return at..at;
    };
    let outer = row.indent;
    let start = at.saturating_add(1);
    let mut end = start;
    let mut i = start;
    while let Some(next) = rows.get(i) {
        match next.kind {
            Kind::Blank | Kind::Comment => {}
            _ if next.indent > outer => end = i.saturating_add(1),
            _ => break,
        }
        i = i.saturating_add(1);
    }
    start..end
}

/// One past the last non-blank, non-comment line of `range` — where a new
/// entry belongs.
fn last_real_line(rows: &[Row], range: Range<usize>) -> usize {
    let mut at = range.start;
    for i in range {
        if let Some(row) = rows.get(i)
            && !matches!(row.kind, Kind::Blank | Kind::Comment)
        {
            at = i.saturating_add(1);
        }
    }
    at
}

/// The indentation the first child of the entry at `at` should get.
fn child_indent(rows: &[Row], at: usize, step: usize) -> usize {
    rows.get(at)
        .map_or(step, |row| row.indent.saturating_add(step))
}

// ============================================================================
// Scalars
// ============================================================================

/// Resolve a scalar's quoting and escapes.
fn unquote(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        let inner = raw.get(1..raw.len().saturating_sub(1)).unwrap_or("");
        return inner.replace("''", "'");
    }
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = raw.get(1..raw.len().saturating_sub(1)).unwrap_or("");
        return unescape(inner);
    }
    raw.to_owned()
}

/// Undo double-quoted escapes.
fn unescape(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let mut code: u32 = 0;
                let mut taken = 0u8;
                for _ in 0..4 {
                    let Some(digit) = chars.next().and_then(|c| c.to_digit(16)) else {
                        break;
                    };
                    code = code.saturating_mul(16).saturating_add(digit);
                    taken = taken.saturating_add(1);
                }
                // A truncated escape, or one naming a surrogate, becomes the
                // replacement character rather than disappearing: a corrupt
                // config should stay visibly corrupt, not quietly plausible.
                if taken == 4 {
                    out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                } else {
                    out.push('\u{FFFD}');
                }
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Spell `value` so that it reads back as exactly this string.
fn quote_if_needed(value: &str) -> String {
    if needs_quoting(value) {
        escape(value)
    } else {
        value.to_owned()
    }
}

/// Whether a plain (unquoted) spelling would change what the value means.
fn needs_quoting(value: &str) -> bool {
    if value.is_empty() || value.starts_with(' ') || value.ends_with(' ') {
        return true;
    }
    if value.chars().any(char::is_control) {
        return true;
    }
    // A leading indicator makes the scalar something else entirely: a
    // sequence item, a comment, an anchor, a flow collection, a block scalar.
    if value.starts_with([
        '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
        '`',
    ]) {
        return true;
    }
    if value.contains(": ") || value.contains(" #") || value.ends_with(':') {
        return true;
    }
    // Anything that would read back as a different *type*. A font family
    // named `NO`, or a version string `1.0`, must not return as a boolean or
    // a float.
    //
    // Note the deliberate asymmetry with `get_bool`, which refuses to read
    // `NO` as `false`: reading is liberal because the file may be anything a
    // user typed, but *writing* is the one moment this crate chooses the
    // spelling, and there it costs nothing to be unambiguous under YAML 1.1
    // as well. The file is not only read back by us — a user's `yq`, their
    // editor's highlighter, and the next tool to touch it all get a look.
    is_boolean_or_null_spelling(value) || looks_numeric(value)
}

/// Whether some YAML dialect would read this word as a boolean or a null.
///
/// The 1.1 spellings (`yes`, `on`, `y`, `n`, …) are included even though
/// [`Document::get_bool`] refuses them — see [`needs_quoting`].
fn is_boolean_or_null_spelling(value: &str) -> bool {
    // Only ASCII-case variants are possible spellings, and only three shapes
    // of them (all-lower, all-upper, capitalised) are ever used.
    matches!(
        value,
        "y" | "Y"
            | "n"
            | "N"
            | "yes"
            | "Yes"
            | "YES"
            | "no"
            | "No"
            | "NO"
            | "on"
            | "On"
            | "ON"
            | "off"
            | "Off"
            | "OFF"
            | "true"
            | "True"
            | "TRUE"
            | "false"
            | "False"
            | "FALSE"
            | "null"
            | "Null"
            | "NULL"
            | "~"
    )
}

/// Whether some YAML dialect would read this word as a number.
///
/// Wider than `str::parse`, which does not know YAML's underscore digit
/// separators (`1_000`), its hex and octal literals (`0x1f`, `0o17`), or its
/// infinities (`.inf`). A string setting that happens to spell one of those
/// has to be quoted or it comes back as an integer.
fn looks_numeric(value: &str) -> bool {
    if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
        return true;
    }
    let unsigned = value
        .strip_prefix(['+', '-'])
        .unwrap_or(value)
        .replace('_', "");
    if let Some(hex) = unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(oct) = unsigned.strip_prefix("0o").or_else(|| unsigned.strip_prefix("0O")) {
        return !oct.is_empty() && oct.chars().all(|c| ('0'..='7').contains(&c));
    }
    if matches!(
        unsigned.as_str(),
        ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN"
    ) {
        return true;
    }
    // `1_000` and `1_0.5` once the separators are gone.
    !unsigned.is_empty()
        && unsigned != value
        && (unsigned.parse::<i64>().is_ok() || unsigned.parse::<f64>().is_ok())
}

/// Double-quote and escape.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Spell a mapping key, quoting it when a plain spelling would not read back.
fn quote_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if plain {
        key.to_owned()
    } else {
        escape(key)
    }
}

impl core::fmt::Display for Document {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_text())
    }
}

impl core::str::FromStr for Document {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    use alloc::format;
    use alloc::vec;

    /// A settings file with every layout feature that has to survive: a banner
    /// comment, blank separators, a trailing comment on a value, four-space
    /// indentation, a sequence, and quoting the writer chose by hand.
    const SAMPLE: &str = "\
# SlateOS appearance settings.
# Edited by hand; the settings app must not eat these lines.

theme: dark

fonts:
    ui_font: Inter
    ui_size: 13      # points, not pixels
    mono_font: \"JetBrains Mono\"

    fallbacks:
        - Noto Sans
        - DejaVu Sans

transparency: 0.85
";

    // -- round trip ----------------------------------------------------------

    #[test]
    fn untouched_document_round_trips_byte_for_byte() {
        assert_eq!(Document::parse(SAMPLE).to_text(), SAMPLE);
    }

    #[test]
    fn round_trip_preserves_crlf_and_a_missing_final_newline() {
        for text in [
            "",
            "a: 1\n",
            "a: 1",
            "a: 1\r\nb: 2\r\n",
            "a: 1\r\nb: 2",
            "\n\n\n",
            "   \n\ta: not-a-key\n",
        ] {
            assert_eq!(Document::parse(text).to_text(), text, "round trip of {text:?}");
        }
    }

    #[test]
    fn an_edit_leaves_every_other_line_identical() {
        let mut doc = Document::parse(SAMPLE);
        doc.set_i64(&["fonts", "ui_size"], 15);
        let before = SAMPLE.lines().collect::<Vec<_>>();
        let after = doc.to_text();
        let after = after.lines().collect::<Vec<_>>();
        assert_eq!(before.len(), after.len(), "an edit changed the line count");
        for (i, (b, a)) in before.iter().zip(&after).enumerate() {
            if i == 7 {
                continue;
            }
            assert_eq!(b, a, "line {i} changed and should not have");
        }
        assert_eq!(after[7], "    ui_size: 15      # points, not pixels");
    }

    #[test]
    fn nonsense_is_preserved_rather_than_corrupted() {
        // Anchors, tags, flow collections and a second document: all features
        // this crate does not model. None of them may be damaged.
        let text = "--- !!map\nlist: [1, 2, 3]\nbase: &anchor {a: 1}\nuse: *anchor\n...\n";
        let doc = Document::parse(text);
        assert_eq!(doc.to_text(), text);
        // A flow collection is a value like any other as far as the text goes.
        assert_eq!(doc.get_str(&["list"]).as_deref(), Some("[1, 2, 3]"));
    }

    #[test]
    fn a_block_scalar_body_is_not_mined_for_keys() {
        let text = "motd: |\n  Welcome to SlateOS.\n  note: this is prose, not a key\nafter: yes\n";
        let doc = Document::parse(text);
        assert_eq!(doc.keys(&[]), vec!["motd".to_owned(), "after".to_owned()]);
        assert!(!doc.contains(&["note"]));
        assert!(!doc.contains(&["motd", "note"]));
        assert_eq!(doc.to_text(), text);
    }

    // -- reading -------------------------------------------------------------

    #[test]
    fn reads_scalars_of_each_type() {
        let doc = Document::parse(SAMPLE);
        assert_eq!(doc.get_str(&["theme"]).as_deref(), Some("dark"));
        assert_eq!(doc.get_str(&["fonts", "ui_font"]).as_deref(), Some("Inter"));
        assert_eq!(doc.get_i64(&["fonts", "ui_size"]), Some(13));
        assert_eq!(doc.get_f64(&["transparency"]), Some(0.85));
        // An integer spelling is a legal number.
        assert_eq!(Document::parse("x: 2\n").get_f64(&["x"]), Some(2.0));
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_value() {
        let doc = Document::parse("a: value  # a note\nb: 3 # another\n");
        assert_eq!(doc.get_str(&["a"]).as_deref(), Some("value"));
        assert_eq!(doc.get_i64(&["b"]), Some(3));
    }

    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        // The bug this guards against loses the user's accent colour.
        let doc = Document::parse("accent: \"#ff8800\"\ntag: a#b\nempty: #ff8800\n");
        assert_eq!(doc.get_str(&["accent"]).as_deref(), Some("#ff8800"));
        assert_eq!(doc.get_str(&["tag"]).as_deref(), Some("a#b"));
        // Preceded by a space, it really is a comment, and the key has no value.
        assert_eq!(doc.get_str(&["empty"]), None);
        assert!(doc.contains(&["empty"]));
    }

    #[test]
    fn a_colon_inside_a_value_does_not_split_a_key() {
        let doc = Document::parse("url: http://example.com:8080/x\n");
        assert_eq!(
            doc.get_str(&["url"]).as_deref(),
            Some("http://example.com:8080/x")
        );
    }

    #[test]
    fn quoting_and_escapes_resolve() {
        let doc = Document::parse(
            "a: \"JetBrains Mono\"\nb: 'it''s'\nc: \"tab\\there\"\nd: \"\\u2713 ok\"\ne: \"\"\n",
        );
        assert_eq!(doc.get_str(&["a"]).as_deref(), Some("JetBrains Mono"));
        assert_eq!(doc.get_str(&["b"]).as_deref(), Some("it's"));
        assert_eq!(doc.get_str(&["c"]).as_deref(), Some("tab\there"));
        assert_eq!(doc.get_str(&["d"]).as_deref(), Some("\u{2713} ok"));
        assert_eq!(doc.get_str(&["e"]).as_deref(), Some(""));
    }

    #[test]
    fn booleans_use_yaml_1_2_spellings_only() {
        let doc = Document::parse("a: true\nb: FALSE\nc: yes\nd: NO\ne: on\n");
        assert_eq!(doc.get_bool(&["a"]), Some(true));
        assert_eq!(doc.get_bool(&["b"]), Some(false));
        // The Norway problem: `NO` is a country, not a boolean.
        assert_eq!(doc.get_bool(&["c"]), None);
        assert_eq!(doc.get_bool(&["d"]), None);
        assert_eq!(doc.get_bool(&["e"]), None);
        assert_eq!(doc.get_str(&["d"]).as_deref(), Some("NO"));
    }

    #[test]
    fn non_finite_numbers_read_as_absent() {
        let doc = Document::parse("a: .inf\nb: -.inf\nc: .nan\nd: inf\ne: NaN\n");
        for key in ["a", "b", "c", "d", "e"] {
            assert_eq!(doc.get_f64(&[key]), None, "{key} should not be a number");
        }
    }

    #[test]
    fn a_nested_key_is_never_mistaken_for_a_top_level_one() {
        let doc = Document::parse("size: 1\nfonts:\n  size: 2\n");
        assert_eq!(doc.get_i64(&["size"]), Some(1));
        assert_eq!(doc.get_i64(&["fonts", "size"]), Some(2));
        assert!(!doc.contains(&["fonts", "fonts"]));
    }

    #[test]
    fn a_block_is_not_a_scalar() {
        let doc = Document::parse(SAMPLE);
        assert!(doc.contains(&["fonts"]));
        assert_eq!(doc.get_str(&["fonts"]), None);
        assert_eq!(doc.get_i64(&["fonts"]), None);
    }

    #[test]
    fn reads_sequences_and_their_keys() {
        let doc = Document::parse(SAMPLE);
        assert_eq!(
            doc.get_seq(&["fonts", "fallbacks"]),
            Some(vec!["Noto Sans".to_owned(), "DejaVu Sans".to_owned()])
        );
        assert_eq!(
            doc.keys(&[]),
            vec!["theme".to_owned(), "fonts".to_owned(), "transparency".to_owned()]
        );
        assert_eq!(
            doc.keys(&["fonts"]),
            vec![
                "ui_font".to_owned(),
                "ui_size".to_owned(),
                "mono_font".to_owned(),
                "fallbacks".to_owned(),
            ]
        );
        assert_eq!(doc.keys(&["nope"]), Vec::<String>::new());
    }

    #[test]
    fn a_sequence_item_may_be_quoted_or_commented() {
        let doc = Document::parse("l:\n  - plain   # note\n  - \"quoted: yes\"\n  -\n");
        assert_eq!(
            doc.get_seq(&["l"]),
            Some(vec![
                "plain".to_owned(),
                "quoted: yes".to_owned(),
                String::new(),
            ])
        );
    }

    #[test]
    fn missing_paths_read_as_none() {
        let doc = Document::parse(SAMPLE);
        assert_eq!(doc.get_str(&["nope"]), None);
        assert_eq!(doc.get_str(&["fonts", "nope"]), None);
        assert_eq!(doc.get_str(&["theme", "nope"]), None);
        assert_eq!(doc.get_seq(&["nope"]), None);
        assert!(!doc.contains(&[]));
    }

    // -- writing -------------------------------------------------------------

    #[test]
    fn writing_a_value_keeps_its_comment_and_spacing() {
        let mut doc = Document::parse("size: 13      # points\n");
        doc.set_i64(&["size"], 15);
        assert_eq!(doc.to_text(), "size: 15      # points\n");
    }

    #[test]
    fn writing_into_a_commented_empty_key_does_not_displace_the_comment() {
        let mut doc = Document::parse("size:  # points\n");
        doc.set_i64(&["size"], 15);
        assert_eq!(doc.to_text(), "size: 15  # points\n");
        assert_eq!(doc.get_i64(&["size"]), Some(15));
    }

    #[test]
    fn a_new_top_level_key_is_appended() {
        let mut doc = Document::parse("a: 1\n");
        doc.set_str(&["b"], "two");
        assert_eq!(doc.to_text(), "a: 1\nb: two\n");
    }

    #[test]
    fn a_new_key_lands_inside_its_block_not_after_the_separator() {
        let mut doc = Document::parse("fonts:\n  a: 1\n\n# next section\nother: 2\n");
        doc.set_i64(&["fonts", "b"], 2);
        assert_eq!(
            doc.to_text(),
            "fonts:\n  a: 1\n  b: 2\n\n# next section\nother: 2\n"
        );
    }

    #[test]
    fn a_new_key_copies_the_documents_own_indent_width() {
        let mut doc = Document::parse("fonts:\n    a: 1\n");
        doc.set_i64(&["fonts", "b"], 2);
        assert_eq!(doc.to_text(), "fonts:\n    a: 1\n    b: 2\n");
        // A document with no nesting to copy falls back to two.
        let mut flat = Document::parse("a: 1\n");
        flat.set_i64(&["fonts", "b"], 2);
        assert_eq!(flat.to_text(), "a: 1\nfonts:\n  b: 2\n");
    }

    #[test]
    fn missing_parents_are_created() {
        let mut doc = Document::new();
        doc.set_str(&["fonts", "mono", "family"], "JetBrains Mono");
        // Plain, not quoted: an interior space is legal in a plain scalar, and
        // quoting what does not need it is its own kind of churn in the file.
        assert_eq!(doc.to_text(), "fonts:\n  mono:\n    family: JetBrains Mono\n");
        assert_eq!(
            doc.get_str(&["fonts", "mono", "family"]).as_deref(),
            Some("JetBrains Mono")
        );
    }

    #[test]
    fn nesting_under_a_scalar_takes_the_scalar_away() {
        // `fonts: 13` cannot also be the parent of `fonts.size`; leaving the
        // 13 in place would emit a file that no longer parses.
        let mut doc = Document::parse("fonts: 13\nafter: 1\n");
        doc.set_i64(&["fonts", "size"], 15);
        assert_eq!(doc.to_text(), "fonts:\n  size: 15\nafter: 1\n");
        assert_eq!(doc.get_i64(&["fonts", "size"]), Some(15));
    }

    #[test]
    fn writing_a_scalar_over_a_block_takes_the_block_away() {
        let mut doc = Document::parse("fonts:\n  size: 13\n  name: x\nafter: 1\n");
        doc.set_str(&["fonts"], "default");
        assert_eq!(doc.to_text(), "fonts: default\nafter: 1\n");
    }

    #[test]
    fn set_str_quotes_only_what_would_read_back_wrong() {
        let mut doc = Document::new();
        doc.set_str(&["a"], "Inter");
        doc.set_str(&["b"], "JetBrains Mono");
        doc.set_str(&["c"], "");
        doc.set_str(&["d"], "NO");
        doc.set_str(&["e"], "1.0");
        doc.set_str(&["f"], "#ff8800");
        doc.set_str(&["g"], " padded ");
        doc.set_str(&["h"], "a: b");
        doc.set_str(&["i"], "say \"hi\"");
        doc.set_str(&["j"], "two\nlines");
        doc.set_str(&["k"], "0x1f");
        doc.set_str(&["l"], "1_000");
        // Quoted where a plain spelling would change the value's meaning, and
        // plain everywhere else — an interior space or an interior quote is
        // legal in a plain scalar, so `b` and `i` stay bare.
        assert_eq!(
            doc.to_text(),
            "a: Inter\nb: JetBrains Mono\nc: \"\"\nd: \"NO\"\ne: \"1.0\"\nf: \"#ff8800\"\n\
             g: \" padded \"\nh: \"a: b\"\ni: say \"hi\"\nj: \"two\\nlines\"\n\
             k: \"0x1f\"\nl: \"1_000\"\n"
        );
        // Every one of them has to survive the round trip as itself.
        for (key, want) in [
            ("a", "Inter"),
            ("b", "JetBrains Mono"),
            ("c", ""),
            ("d", "NO"),
            ("e", "1.0"),
            ("f", "#ff8800"),
            ("g", " padded "),
            ("h", "a: b"),
            ("i", "say \"hi\""),
            ("j", "two\nlines"),
            ("k", "0x1f"),
            ("l", "1_000"),
        ] {
            let reread = Document::parse(&doc.to_text());
            assert_eq!(reread.get_str(&[key]).as_deref(), Some(want), "key {key}");
        }
    }

    #[test]
    fn a_whole_number_still_writes_as_a_number() {
        let mut doc = Document::new();
        doc.set_f64(&["a"], 1.0);
        doc.set_f64(&["b"], 0.85);
        doc.set_f64(&["c"], f64::NAN);
        assert_eq!(doc.to_text(), "a: 1.0\nb: 0.85\nc: null\n");
        let reread = Document::parse(&doc.to_text());
        assert_eq!(reread.get_f64(&["a"]), Some(1.0));
        assert_eq!(reread.get_f64(&["c"]), None);
    }

    #[test]
    fn set_bool_and_read_back() {
        let mut doc = Document::parse("dark: false\n");
        doc.set_bool(&["dark"], true);
        assert_eq!(doc.to_text(), "dark: true\n");
        assert_eq!(doc.get_bool(&["dark"]), Some(true));
    }

    #[test]
    fn a_key_needing_quotes_gets_them() {
        let mut doc = Document::new();
        doc.set_str(&["a b: c"], "x");
        assert_eq!(doc.to_text(), "\"a b: c\": x\n");
        assert_eq!(
            Document::parse(&doc.to_text()).get_str(&["a b: c"]).as_deref(),
            Some("x")
        );
    }

    // -- sequences -----------------------------------------------------------

    #[test]
    fn set_seq_replaces_the_items_and_keeps_the_surroundings() {
        let mut doc = Document::parse(SAMPLE);
        doc.set_seq(&["fonts", "fallbacks"], &["Cantarell", "Noto Sans"]);
        assert_eq!(
            doc.get_seq(&["fonts", "fallbacks"]),
            Some(vec!["Cantarell".to_owned(), "Noto Sans".to_owned()])
        );
        // The block's own indent width is copied, not guessed.
        assert!(doc.to_text().contains("\n        - Cantarell\n"));
        assert!(doc.to_text().starts_with("# SlateOS appearance settings."));
        assert_eq!(doc.get_f64(&["transparency"]), Some(0.85));
    }

    #[test]
    fn set_seq_creates_a_missing_key_and_empties_cleanly() {
        let mut doc = Document::parse("a: 1\n");
        doc.set_seq(&["l"], &["one", "two"]);
        assert_eq!(doc.to_text(), "a: 1\nl:\n  - one\n  - two\n");
        doc.set_seq(&["l"], &[]);
        assert_eq!(doc.to_text(), "a: 1\nl:\n");
        assert_eq!(doc.get_seq(&["l"]), Some(Vec::new()));
    }

    #[test]
    fn set_seq_over_a_scalar_does_not_emit_both() {
        let mut doc = Document::parse("l: single\nafter: 1\n");
        doc.set_seq(&["l"], &["x"]);
        assert_eq!(doc.to_text(), "l:\n  - x\nafter: 1\n");
        assert_eq!(doc.get_seq(&["l"]), Some(vec!["x".to_owned()]));
    }

    #[test]
    fn set_seq_quotes_items_that_need_it() {
        let mut doc = Document::new();
        doc.set_seq(&["l"], &["plain", "", "1.5", "- dash", "y"]);
        assert_eq!(
            doc.to_text(),
            "l:\n  - plain\n  - \"\"\n  - \"1.5\"\n  - \"- dash\"\n  - \"y\"\n"
        );
        assert_eq!(
            Document::parse(&doc.to_text()).get_seq(&["l"]),
            Some(vec![
                "plain".to_owned(),
                String::new(),
                "1.5".to_owned(),
                "- dash".to_owned(),
                "y".to_owned(),
            ])
        );
    }

    // -- removal -------------------------------------------------------------

    #[test]
    fn remove_takes_the_key_and_its_children_only() {
        let mut doc = Document::parse(SAMPLE);
        assert!(doc.remove(&["fonts"]));
        assert!(!doc.contains(&["fonts"]));
        assert_eq!(doc.get_str(&["theme"]).as_deref(), Some("dark"));
        assert_eq!(doc.get_f64(&["transparency"]), Some(0.85));
        assert!(doc.to_text().starts_with("# SlateOS appearance settings."));
        assert!(!doc.to_text().contains("Inter"));
    }

    #[test]
    fn remove_reports_a_missing_key() {
        let mut doc = Document::parse("a: 1\n");
        assert!(!doc.remove(&["b"]));
        assert!(!doc.remove(&[]));
        assert_eq!(doc.to_text(), "a: 1\n");
    }

    #[test]
    fn remove_leaves_the_separator_between_sections() {
        let mut doc = Document::parse("a:\n  x: 1\n\nb: 2\n");
        assert!(doc.remove(&["a"]));
        assert_eq!(doc.to_text(), "\nb: 2\n");
    }

    #[test]
    fn remove_leaves_a_comment_above_the_key() {
        // Deleting the user's prose on a hunch about what it described is a
        // worse failure than leaving an orphaned line.
        let mut doc = Document::parse("# about a\na: 1\nb: 2\n");
        assert!(doc.remove(&["a"]));
        assert_eq!(doc.to_text(), "# about a\nb: 2\n");
    }

    // -- whole-file behaviour -------------------------------------------------

    #[test]
    fn a_settings_save_cycle_is_stable() {
        // Two saves in a row with the same values must not produce a second
        // diff — otherwise every launch dirties the user's file.
        let mut doc = Document::parse(SAMPLE);
        let write = |d: &mut Document| {
            d.set_str(&["fonts", "ui_font"], "Cantarell");
            d.set_i64(&["fonts", "ui_size"], 14);
            d.set_f64(&["transparency"], 0.9);
            d.set_bool(&["shadows"], true);
            d.set_seq(&["fonts", "fallbacks"], &["Noto Sans"]);
        };
        write(&mut doc);
        let once = doc.to_text();
        write(&mut doc);
        assert_eq!(doc.to_text(), once, "a second identical save changed the file");
        // And the comments are all still there.
        assert!(once.contains("# SlateOS appearance settings."));
        assert!(once.contains("# points, not pixels"));
    }

    #[test]
    fn crlf_survives_an_edit_that_adds_a_line() {
        let mut doc = Document::parse("a: 1\r\n");
        doc.set_i64(&["b"], 2);
        assert_eq!(doc.to_text(), "a: 1\r\nb: 2\r\n");
    }

    #[test]
    fn a_file_without_a_final_newline_keeps_not_having_one() {
        let mut doc = Document::parse("a: 1");
        doc.set_i64(&["a"], 2);
        assert_eq!(doc.to_text(), "a: 2");
    }

    #[test]
    fn display_and_from_str_agree_with_the_inherent_methods() {
        let doc: Document = SAMPLE.parse().unwrap();
        assert_eq!(format!("{doc}"), doc.to_text());
        assert_eq!(doc, Document::parse(SAMPLE));
        assert!(Document::new().is_empty());
        assert!(!doc.is_empty());
    }
}
