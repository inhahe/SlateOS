//! [`SyntaxHighlighter`]: a language's parser and highlight query, driven by
//! a code view through [`guitk::highlight::Highlighter`].
//!
//! # Parsing a slice at a time
//!
//! The view hands the highlighter every edit as the buffer journalled it; the
//! highlighter moves its tree to match (`Tree::edit` -- every node after an
//! edit shifts, which is what keeps colours at the right offsets however far
//! behind the parsing is) and marks the text as needing a parse. Parsing
//! happens in [`work`](Highlighter::work), for as long as the budget allows:
//! the runtime asks, every hundred or so steps, whether to stop, and a parse
//! stopped part-way resumes where it left off on the next call -- unless the
//! text changed in between, when it starts again from the moved tree, which
//! still spares it everything the edit did not touch.
//!
//! # A parse that never ends
//!
//! A grammar with a mistake in it can send the parser round in circles. The
//! budget keeps that from freezing the window -- the runtime stops each slice
//! on time -- but it would still take a slice of every frame for ever. So a
//! parse of one text is given up once it has run far longer than any real
//! file needs ([`parse_limit`]: five seconds, and a second more per megabyte);
//! the colours stay as the moved tree has them, and the next edit tries again.
//!
//! # From captures to colours
//!
//! The query's captures nest the way the tree does -- an escape inside a
//! string, a string inside an attribute -- and one node can be captured by
//! several patterns. Tree-sitter's own highlighter settles both the same way
//! this does: of the captures of one node the first pattern's wins (a
//! grammar's query lists the specific before the general), and inside a
//! node, a node within it wins. What comes out is sorted, flat and within the
//! range asked for, which is what the view draws.

use core::ops::Range;
use core::time::Duration;
use std::time::Instant;

use guitk::highlight::{Highlight, HighlightSpan, Highlighter};
use guitk::textbuffer::{Splice, TextBuffer};
use tree_sitter::{
    InputEdit, Node, ParseOptions, ParseState, Parser, Point, QueryCursor, StreamingIterator,
    TextProvider, Tree,
};

use crate::{Error, Language};

/// A language's parser and highlight query, for one text.
pub struct SyntaxHighlighter {
    language: &'static Language,
    parser: Parser,
    query: &'static tree_sitter::Query,
    /// What each of the query's captures colours, by capture index.
    kinds: Vec<Option<Highlight>>,
    /// The last complete parse, moved to match every edit since.
    tree: Option<Tree>,
    /// Whether the text changed since `tree` was parsed from it.
    stale: bool,
    /// Whether a parse of the text as it is was stopped part-way: the next
    /// call resumes it.
    halted: bool,
    /// How long parsing the text as it is has taken so far, over every
    /// slice.
    spent: Duration,
    /// Whether the parse of the text as it is was given up ([`parse_limit`]).
    abandoned: bool,
}

/// How long parsing a text of `len` bytes may take, over every slice, before
/// it is given up: five seconds, and a second more for every mebibyte.
#[must_use]
pub fn parse_limit(len: usize) -> Duration {
    let mebibytes = u64::try_from(len >> 20).unwrap_or(u64::MAX);
    Duration::from_secs(5).saturating_add(Duration::from_secs(mebibytes))
}

impl core::fmt::Debug for SyntaxHighlighter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SyntaxHighlighter")
            .field("language", &self.language.name())
            .field("parsed", &self.tree.is_some())
            .field("stale", &self.stale)
            .field("halted", &self.halted)
            .finish_non_exhaustive()
    }
}

impl SyntaxHighlighter {
    pub(crate) fn new(language: &'static Language) -> Result<Self, Error> {
        let query = language.query()?;
        let mut parser = Parser::new();
        parser
            .set_language(&language.ts_language())
            .map_err(|e| Error::Grammar {
                language: language.name(),
                message: e.to_string(),
            })?;
        let kinds = query
            .capture_names()
            .iter()
            .map(|name| Highlight::for_capture(name))
            .collect();
        Ok(Self {
            language,
            parser,
            query,
            kinds,
            tree: None,
            stale: true,
            halted: false,
            spent: Duration::ZERO,
            abandoned: false,
        })
    }

    /// The language it highlights.
    #[must_use]
    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The tree as last parsed, moved to match the edits since.
    #[must_use]
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Whether the text changed since the tree was parsed from it.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Whether parsing the text as it is was given up, having run past
    /// [`parse_limit`]: a grammar that cannot finish it. The next edit tries
    /// again.
    #[must_use]
    pub fn is_abandoned(&self) -> bool {
        self.abandoned
    }
}

/// A splice as the runtime's edit.
fn input_edit(s: &Splice) -> InputEdit {
    let point = |(row, column): (usize, usize)| Point { row, column };
    InputEdit {
        start_byte: s.start,
        old_end_byte: s.old_end,
        new_end_byte: s.new_end,
        start_position: point(s.start_point),
        old_end_position: point(s.old_end_point),
        new_end_position: point(s.new_end_point),
    }
}

/// The text of a node, for a query's predicates (`#match?`, `#eq?`), a piece
/// of the buffer at a time.
struct BufferText<'a>(&'a TextBuffer);

impl<'a> TextProvider<&'a [u8]> for BufferText<'a> {
    type I = std::vec::IntoIter<&'a [u8]>;

    fn text(&mut self, node: Node<'_>) -> Self::I {
        self.0
            .bytes_in(node.byte_range())
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl Highlighter for SyntaxHighlighter {
    fn reset(&mut self, _text: &TextBuffer) {
        self.tree = None;
        self.parser.reset();
        self.halted = false;
        self.stale = true;
        self.spent = Duration::ZERO;
        self.abandoned = false;
    }

    fn edited(&mut self, _text: &TextBuffer, splices: &[Splice]) {
        if let Some(tree) = self.tree.as_mut() {
            for splice in splices {
                tree.edit(&input_edit(splice));
            }
        }
        // A parse stopped part-way was of the text before these edits: start
        // it again, from the moved tree.
        if self.halted {
            self.parser.reset();
            self.halted = false;
        }
        self.stale = true;
        self.spent = Duration::ZERO;
        self.abandoned = false;
    }

    fn work(&mut self, text: &TextBuffer, budget: Duration) -> bool {
        if !self.stale {
            return false;
        }
        let started = Instant::now();
        let deadline = started.checked_add(budget);
        let mut stopped = false;
        let mut progress = |_: &ParseState| {
            let late = deadline.is_none_or(|d| Instant::now() >= d);
            stopped |= late;
            late
        };
        let parsed = self.parser.parse_with_options(
            &mut |byte: usize, _: Point| text.bytes_from(byte),
            self.tree.as_ref(),
            Some(ParseOptions::new().progress_callback(&mut progress)),
        );
        match parsed {
            Some(tree) => {
                self.tree = Some(tree);
                self.stale = false;
                self.halted = false;
                false
            }
            None if stopped => {
                self.spent = self.spent.saturating_add(started.elapsed());
                if self.spent > parse_limit(text.len()) {
                    // Round in circles: give this text up (see the module
                    // docs); the next edit starts again.
                    self.parser.reset();
                    self.halted = false;
                    self.stale = false;
                    self.abandoned = true;
                    return false;
                }
                self.halted = true;
                true
            }
            // Refused for a reason that is not the budget -- which a parser
            // with a language set has none of. Stop asking rather than spin;
            // the moved tree keeps what colours it has.
            None => {
                self.parser.reset();
                self.halted = false;
                self.stale = false;
                false
            }
        }
    }

    fn highlights(&self, text: &TextBuffer, range: Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        if range.is_empty() {
            return Vec::new();
        }
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures = cursor.captures(self.query, tree.root_node(), BufferText(text));
        // Each node's first capture that colours anything, in the order the
        // cursor gives them: by position, and for one node by pattern.
        let mut found: Vec<(Range<usize>, Highlight)> = Vec::new();
        let mut last_node: Option<usize> = None;
        while let Some((m, index)) = captures.next() {
            let Some(capture) = m.captures.get(*index) else {
                continue;
            };
            let Some(Some(kind)) = usize::try_from(capture.index)
                .ok()
                .and_then(|i| self.kinds.get(i))
            else {
                continue;
            };
            let node = capture.node;
            if last_node == Some(node.id()) {
                continue;
            }
            last_node = Some(node.id());
            found.push((node.byte_range(), *kind));
        }
        flatten(found, &range)
    }
}

/// Nested spans made flat: sorted by start, the outer before the inner, the
/// inner winning where it is -- and cut to `within`.
fn flatten(mut spans: Vec<(Range<usize>, Highlight)>, within: &Range<usize>) -> Vec<HighlightSpan> {
    // Stable: spans the same (a node and its only child) keep the cursor's
    // order, which puts the parent first -- so the child wins, as it would
    // nested.
    spans.sort_by_key(|(r, _)| (r.start, core::cmp::Reverse(r.end)));
    let mut out: Vec<HighlightSpan> = Vec::new();
    let mut emit = |from: usize, to: usize, kind: Highlight| {
        let (from, to) = (from.max(within.start), to.min(within.end));
        if to <= from {
            return;
        }
        match out.last_mut() {
            Some(last) if last.range.end == from && last.highlight == kind => last.range.end = to,
            _ => out.push(HighlightSpan {
                range: from..to,
                highlight: kind,
            }),
        }
    };
    // The spans open at `pos`, innermost last: (end, kind).
    let mut open: Vec<(usize, Highlight)> = Vec::new();
    let mut pos = 0usize;
    for (range, kind) in spans {
        // Close what ends before this starts, colouring up to each end.
        while let Some(&(end, outer)) = open.last() {
            if end > range.start {
                break;
            }
            emit(pos, end, outer);
            pos = pos.max(end);
            open.pop();
        }
        if let Some(&(_, outer)) = open.last() {
            emit(pos, range.start, outer);
        }
        pos = pos.max(range.start);
        // A span running past the one it is in is held to it: trees nest.
        let end = open
            .last()
            .map_or(range.end, |&(outer_end, _)| range.end.min(outer_end));
        open.push((end, kind));
    }
    while let Some((end, kind)) = open.pop() {
        emit(pos, end, kind);
        pos = pos.max(end);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn spans(text: &str, language: &str) -> Vec<(String, Highlight)> {
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named(language).unwrap().highlighter().unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        h.highlights(&buffer, 0..buffer.len())
            .into_iter()
            .map(|s| (text[s.range].to_owned(), s.highlight))
            .collect()
    }

    /// **Rust is coloured as its query says**: keywords, a function's name,
    /// a type, a string, a number, a comment, a macro.
    #[test]
    fn rust_is_coloured_as_its_query_says() {
        let got = spans(
            "// note\nfn main() -> u8 { let s = \"hi\"; println!(\"{s}\"); 42 }\n",
            "rust",
        );
        for want in [
            ("// note", Highlight::Comment),
            ("fn", Highlight::Keyword),
            ("main", Highlight::Function),
            ("u8", Highlight::Type),
            ("let", Highlight::Keyword),
            ("\"hi\"", Highlight::String),
            ("println!", Highlight::Macro),
            ("42", Highlight::Constant),
        ] {
            assert!(
                got.iter()
                    .any(|(t, h)| t.as_str() == want.0 && *h == want.1),
                "{want:?} not in {got:?}"
            );
        }
    }

    /// **An escape inside a string is the escape's, the string on either
    /// side the string's**: the inner capture wins where it is.
    #[test]
    fn a_capture_inside_another_wins_where_it_is() {
        let got = spans("x = \"a\\nb\"\n", "python");
        let at = |s: &str| got.iter().find(|(t, _)| t == s).map(|(_, h)| *h);
        assert_eq!(at("\\n"), Some(Highlight::Escape), "{got:?}");
        assert!(
            got.iter()
                .any(|(t, h)| t.starts_with('"') && *h == Highlight::String),
            "{got:?}"
        );
        // Sorted, and none overlapping another.
        let buffer = TextBuffer::from_text("x = \"a\\nb\"\n");
        let mut h = Language::named("python").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let raw = h.highlights(&buffer, 0..buffer.len());
        for pair in raw.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start, "{raw:?}");
        }
    }

    /// **An edit moves the colours with the text at once**, before any
    /// parse -- and the parse after it colours the new text.
    #[test]
    fn an_edit_moves_the_colours_before_the_parse() {
        let mut buffer = TextBuffer::from_text("fn a() {}\n");
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        assert!(!h.work(&buffer, Duration::from_secs(5)));
        let _ = buffer.take_changes();
        buffer.insert(0, "// x\n").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        assert!(h.is_stale());
        // Not parsed yet: `fn` has moved five bytes along with its text.
        let moved = h.highlights(&buffer, 0..buffer.len());
        assert!(
            moved
                .iter()
                .any(|s| s.range == (5..7) && s.highlight == Highlight::Keyword),
            "{moved:?}"
        );
        assert!(
            !moved.iter().any(|s| s.highlight == Highlight::Comment),
            "the comment is not parsed yet"
        );
        assert!(!h.work(&buffer, Duration::from_secs(5)));
        let parsed = h.highlights(&buffer, 0..buffer.len());
        assert!(
            parsed
                .iter()
                .any(|s| s.range == (0..4) && s.highlight == Highlight::Comment),
            "{parsed:?}"
        );
    }

    /// **A parse that does not fit its budget stops and resumes** where it
    /// left off, until it is done -- and the result is the whole file's.
    #[test]
    fn a_parse_too_long_for_its_budget_resumes_until_done() {
        let mut text = String::new();
        for i in 0..3000 {
            text.push_str(&format!(
                "fn f{i}(x: u32) -> u32 {{ let y = x * {i}; y + 1 }}\n"
            ));
        }
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        let mut slices = 0;
        while h.work(&buffer, Duration::from_micros(200)) {
            slices += 1;
            assert!(slices < 100_000, "never finished");
        }
        assert!(
            slices > 1,
            "a file this size fit in 200 µs: the budget was not honoured"
        );
        let tree = h.tree().unwrap();
        assert!(!tree.root_node().has_error());
        assert_eq!(tree.root_node().named_child_count(), 3000);
    }

    /// **An edit during a stopped parse starts it again** from the moved
    /// tree, and what comes out is the edited text's.
    #[test]
    fn an_edit_during_a_stopped_parse_starts_it_again() {
        let mut text = String::new();
        for i in 0..2000 {
            text.push_str(&format!("x{i} = [{i}, \"{i}\"]\n"));
        }
        let mut buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("python").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        assert!(
            h.work(&buffer, Duration::from_micros(100)),
            "the first slice finished it"
        );
        let _ = buffer.take_changes();
        buffer.insert(0, "import os\n").unwrap();
        h.edited(&buffer, &buffer.clone().take_changes().splices.unwrap());
        while h.work(&buffer, Duration::from_secs(5)) {}
        let root = h.tree().unwrap().root_node();
        assert_eq!(root.named_child_count(), 2001);
        assert_eq!(root.named_child(0).unwrap().kind(), "import_statement");
    }

    /// **A parse is given up past its limit** -- five seconds and one a
    /// mebibyte -- and the next edit tries again.
    #[test]
    fn a_parse_past_its_limit_is_given_up_until_the_next_edit() {
        assert_eq!(parse_limit(0), Duration::from_secs(5));
        assert_eq!(parse_limit(3 << 20), Duration::from_secs(8));
        let mut buffer = TextBuffer::from_text("fn a() {}\n");
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        // As if the slices so far had taken the whole limit.
        h.spent = parse_limit(buffer.len());
        let mut calls = 0;
        while h.work(&buffer, Duration::ZERO) {
            calls += 1;
            assert!(calls < 10_000, "never gave up");
        }
        // A text this small parses inside one zero budget's first check, or
        // is given up: either way the work stops.
        assert!(h.is_abandoned() || h.tree().is_some());
        let _ = buffer.take_changes();
        buffer.insert(0, "x").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        assert!(!h.is_abandoned() && h.is_stale());
        while h.work(&buffer, Duration::from_secs(5)) {}
        assert!(h.tree().is_some() && !h.is_stale());
    }

    /// **Flattening**: nested spans become flat ones, the inner winning, cut
    /// to the range asked for; neighbours of one kind merge.
    #[test]
    fn nested_spans_are_flattened() {
        use Highlight::{Escape, Keyword, String as Str};
        let flat = flatten(
            vec![
                (0..10, Str),
                (3..5, Escape),
                (5..7, Escape),
                (12..14, Keyword),
            ],
            &(2..13),
        );
        let got: Vec<(Range<usize>, Highlight)> =
            flat.into_iter().map(|s| (s.range, s.highlight)).collect();
        assert_eq!(
            got,
            [(2..3, Str), (3..7, Escape), (7..10, Str), (12..13, Keyword)]
        );
        // A child as wide as its parent wins.
        let same = flatten(vec![(0..4, Str), (0..4, Keyword)], &(0..4));
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].highlight, Keyword);
        assert!(flatten(Vec::new(), &(0..9)).is_empty());
    }
}
