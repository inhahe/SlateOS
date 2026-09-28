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
//! parse of one text is given up once it has done far more work than any
//! real file needs ([`parse_limit`]); the colours stay as the moved tree has
//! them, and the next edit tries again. The work is counted, not timed: the
//! runtime's own steps and the characters the lexers read, which a busy
//! machine cannot stretch the way it stretches a clock (§1439).
//!
//! # Languages inside languages
//!
//! A grammar's injection query names the stretches of its text written in
//! another language -- a Markdown code fence in the language its info string
//! names, a Rust macro's body in Rust. Those stretches are parsed with that
//! language's grammar (only them: the parser is given their ranges) and
//! coloured by its query, over the enclosing language's colours, to three
//! levels deep. Drawing starts each stretch it finds, all of them within
//! [`DRAW_BUDGET`] together; one that does not finish in it is carried on
//! by [`work`](Highlighter::work) a slice at a time -- the view keeps asking
//! while [`has_work`](Highlighter::has_work) says so -- and is coloured when
//! it is done. The parses are kept until the text changes, so drawing the
//! same screen again costs nothing; a stretch past its work limit
//! ([`parse_limit`]) stays in its host's colours.
//!
//! # From captures to colours
//!
//! The query's captures nest the way the tree does -- an escape inside a
//! string, a string inside an attribute -- and one node can be captured by
//! several patterns. Tree-sitter's own highlighter settles both the same way
//! this does: of the captures of one node the last pattern's wins -- a
//! grammar's query lists the general before the specific, `(identifier)
//! @variable` before a function's name -- and inside a node, a node within
//! it wins. (Tree-sitter's highlighter once let the first pattern win; the
//! queries vendored here are written, and tested upstream, for the last --
//! `highlight_tests.rs` runs those tests.) A capture
//! whose name no kind answers to, `@text.emphasis`, takes no part: the
//! node keeps what the other patterns said of it. What comes out is sorted,
//! flat and within the range asked for, which is what the view draws.

use core::cell::RefCell;
use core::ops::Range;
use core::time::Duration;
use std::collections::HashMap;
use std::time::Instant;

use guitk::highlight::{Highlight, HighlightSpan, Highlighter};
use guitk::textbuffer::{Splice, TextBuffer};
use tree_sitter::{
    InputEdit, Node, ParseOptions, ParseState, Parser, Point, QueryCursor, StreamingIterator,
    TextProvider, Tree,
};

use crate::{Compiled, Error, Injections, Language, Paint, ffi};

/// How many languages deep injections go: Markdown's code fence in Markdown
/// is two, a macro's body in that fence's Rust three.
const MAX_INJECTION_DEPTH: usize = 3;

/// How long drawing may spend parsing the injected stretches it finds, all
/// of them together; what does not fit is carried on by `work`.
pub const DRAW_BUDGET: Duration = Duration::from_millis(4);

/// The most injected text parsed at once: past this a stretch is left in
/// its host's colours, whatever the budget would allow.
const MAX_INJECTED_BYTES: usize = 1 << 20;

/// An injected stretch's key: its language's index, and its ranges'
/// bytes.
type StretchKey = (usize, Vec<(usize, usize)>);

/// An injected stretch's parse, for one revision of the text.
enum Stretch {
    /// Parsed.
    Done(Tree),
    /// Being parsed a slice at a time by `work`: the parser, holding the
    /// parse where it stopped, the work done so far and the most allowed.
    Pending {
        parser: Parser,
        used: u64,
        limit: u64,
    },
    /// Too big, past its work limit, or refused: in its host's colours until
    /// the text changes.
    Failed,
}

/// Parses of injected stretches for one revision of the text.
#[derive(Default)]
struct InjectionCache {
    revision: u64,
    /// Parsers not in use, by language index, for the next stretches.
    parsers: HashMap<usize, Vec<Parser>>,
    /// Each stretch's parse, by language and ranges.
    stretches: HashMap<StretchKey, Stretch>,
}

impl InjectionCache {
    /// Forget every stretch -- the text changed -- keeping the parsers of
    /// those being parsed, reset, for the next.
    fn forget(&mut self) {
        for (key, stretch) in self.stretches.drain() {
            if let Stretch::Pending { mut parser, .. } = stretch {
                parser.reset();
                self.parsers.entry(key.0).or_default().push(parser);
            }
        }
    }

    /// Whether any stretch is waiting for `work`.
    fn pending(&self) -> bool {
        self.stretches
            .values()
            .any(|s| matches!(s, Stretch::Pending { .. }))
    }
}

/// How one slice of a parse ended.
enum Slice {
    /// Done: the tree.
    Done(Tree),
    /// Stopped at the deadline, with more to do.
    Late,
    /// Past its work limit: to be given up.
    Over,
    /// Refused, for a reason that is not the deadline -- which a parser with
    /// a language set has none of.
    Refused,
}

/// Parse `text` with `parser` -- carrying on where it stopped, if it did --
/// until it is done or `deadline` (none: no deadline) passes, adding its
/// work to `used` and stopping it past `limit` ([`parse_limit`]).
fn slice(
    parser: &mut Parser,
    text: &TextBuffer,
    old: Option<&Tree>,
    deadline: Option<Instant>,
    used: &mut u64,
    limit: u64,
) -> Slice {
    let (before, lexed) = (*used, ffi::advances());
    // The work so far, this slice's included: the steps the callback has
    // been called for, and the characters lexed since the slice began.
    let done = |checks: u64| {
        before
            .saturating_add(checks.saturating_mul(STEPS_PER_CHECK))
            .saturating_add(ffi::advances().wrapping_sub(lexed))
    };
    let mut checks: u64 = 0;
    let (mut late, mut over) = (false, false);
    let mut progress = |_: &ParseState| {
        checks = checks.saturating_add(1);
        over = done(checks) > limit;
        late = deadline.is_some_and(|d| Instant::now() >= d);
        over || late
    };
    let parsed = parser.parse_with_options(
        &mut |byte: usize, _: Point| text.bytes_from(byte),
        old,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    *used = done(checks);
    match parsed {
        Some(tree) => Slice::Done(tree),
        None if over => Slice::Over,
        None if late => Slice::Late,
        None => Slice::Refused,
    }
}

/// A language's parser and highlight query, for one text.
pub struct SyntaxHighlighter {
    language: &'static Language,
    parser: Parser,
    /// The language's queries.
    compiled: &'static Compiled,
    /// Parses of the text's injected stretches. In a cell because they are
    /// begun while drawing, which asks through `&self` -- a cache of what the
    /// text says, remade when the text changes.
    injected: RefCell<InjectionCache>,
    /// How long drawing may parse the stretches it finds: [`DRAW_BUDGET`].
    draw_budget: Duration,
    /// The last complete parse, moved to match every edit since.
    tree: Option<Tree>,
    /// Whether the text changed since `tree` was parsed from it.
    stale: bool,
    /// Whether a parse of the text as it is was stopped part-way: the next
    /// call resumes it.
    halted: bool,
    /// The work parsing the text as it is has done so far, over every slice,
    /// in [`parse_limit`]'s units.
    used: u64,
    /// Whether the parse of the text as it is was given up ([`parse_limit`]).
    abandoned: bool,
}

/// The runtime's steps between two calls of a parse's progress callback
/// (`OP_COUNT_PER_PARSER_TIMEOUT_CHECK` in its `parser.c`).
const STEPS_PER_CHECK: u64 = 100;

/// The work any parse may do, however short its text.
const WORK_FLOOR: u64 = 2_000_000;

/// The work a parse may do for each byte of its text, on top.
const WORK_PER_BYTE: u64 = 200;

/// How much work parsing a text of `len` bytes may do, over every slice,
/// before it is given up: the runtime's steps and the characters its lexers
/// step over ([`ffi::advances`]), together -- two million, and two hundred a
/// byte.
///
/// Measured (§1439): real files in every language here, each language fed
/// the others' files and random bytes, all take under ten a byte. What
/// passes the limit is a grammar going round in circles, or a scanner that
/// reads the rest of a line again for every token of it -- Markdown's does,
/// on a line of thousands of `*`s: five thousand a byte on ten thousand.
#[must_use]
pub fn parse_limit(len: usize) -> u64 {
    let len = u64::try_from(len).unwrap_or(u64::MAX);
    WORK_FLOOR.saturating_add(len.saturating_mul(WORK_PER_BYTE))
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
        let compiled = language.compiled()?;
        let mut parser = Parser::new();
        parser
            .set_language(&language.ts_language())
            .map_err(|e| Error::Grammar {
                language: language.name(),
                message: e.to_string(),
            })?;
        Ok(Self {
            language,
            parser,
            compiled,
            injected: RefCell::default(),
            draw_budget: DRAW_BUDGET,
            tree: None,
            stale: true,
            halted: false,
            used: 0,
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
        self.injected.get_mut().forget();
        self.halted = false;
        self.stale = true;
        self.used = 0;
        self.abandoned = false;
    }

    fn edited(&mut self, _text: &TextBuffer, splices: &[Splice]) {
        if let Some(tree) = self.tree.as_mut() {
            for splice in splices {
                tree.edit(&input_edit(splice));
            }
        }
        // A parse stopped part-way was of the text before these edits: start
        // it again, from the moved tree. So were the injected stretches.
        if self.halted {
            self.parser.reset();
            self.halted = false;
        }
        self.injected.get_mut().forget();
        self.stale = true;
        self.used = 0;
        self.abandoned = false;
    }

    fn work(&mut self, text: &TextBuffer, budget: Duration) -> bool {
        // A budget too long to add to the clock has no deadline.
        let deadline = Instant::now().checked_add(budget);
        if self.stale {
            let limit = parse_limit(text.len());
            match slice(
                &mut self.parser,
                text,
                self.tree.as_ref(),
                deadline,
                &mut self.used,
                limit,
            ) {
                Slice::Done(tree) => {
                    self.tree = Some(tree);
                    self.stale = false;
                    self.halted = false;
                }
                Slice::Over => {
                    // Round in circles, or as good as: give this text up
                    // (see the module docs); the next edit starts again.
                    self.parser.reset();
                    self.halted = false;
                    self.stale = false;
                    self.abandoned = true;
                }
                Slice::Late => {
                    self.halted = true;
                    return true;
                }
                // Stop asking rather than spin; the moved tree keeps what
                // colours it has.
                Slice::Refused => {
                    self.parser.reset();
                    self.halted = false;
                    self.stale = false;
                }
            }
        }
        self.carry_on_injections(text, deadline)
    }

    fn has_work(&self) -> bool {
        // Borrowed only while drawing, which does not ask this.
        self.stale || self.injected.try_borrow().is_ok_and(|c| c.pending())
    }

    fn highlights(&self, text: &TextBuffer, range: Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        if range.is_empty() {
            return Vec::new();
        }
        let deadline = Instant::now().checked_add(self.draw_budget);
        let mut found = Vec::new();
        self.collect(
            self.compiled,
            tree,
            text,
            range.clone(),
            (0, deadline),
            &mut found,
        );
        flatten(found, &range)
    }
}

impl SyntaxHighlighter {
    /// The captures of `tree` over `range`, in `compiled`'s language, then
    /// those of each stretch it injects -- after them, so an injected
    /// language's colours win over its host's where both colour a stretch.
    /// `depth` is how deep in injections this is, and `deadline` when
    /// drawing stops parsing the stretches it finds (see [`DRAW_BUDGET`]).
    fn collect(
        &self,
        compiled: &Compiled,
        tree: &Tree,
        text: &TextBuffer,
        range: Range<usize>,
        (depth, deadline): (usize, Option<Instant>),
        found: &mut Vec<(Range<usize>, Option<Highlight>)>,
    ) {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures =
            cursor.captures(&compiled.highlights, tree.root_node(), BufferText(text));
        // A node's captures come one after another, by pattern: of those
        // that paint, the last wins. (A capture of another node between
        // them, starting where it does, parts them -- then each is a span
        // of its own, stacked in that order, as tree-sitter's are.)
        let mut last_node: Option<usize> = None;
        while let Some((m, index)) = captures.next() {
            let Some(capture) = m.captures.get(*index) else {
                continue;
            };
            let paint = usize::try_from(capture.index)
                .ok()
                .and_then(|i| compiled.paints.get(i))
                .copied()
                .unwrap_or(Paint::Skip);
            let kind = match paint {
                Paint::Kind(kind) => Some(kind),
                Paint::Plain => None,
                Paint::Skip => continue,
            };
            let node = capture.node;
            if last_node == Some(node.id()) {
                if let Some(last) = found.last_mut() {
                    last.1 = kind;
                }
                continue;
            }
            last_node = Some(node.id());
            found.push((node.byte_range(), kind));
        }
        if depth >= MAX_INJECTION_DEPTH {
            return;
        }
        let Some(injections) = compiled.injections.as_ref() else {
            return;
        };
        for (language, ranges) in injections_in(injections, tree, text, range.clone()) {
            let (Some(first), Some(last)) = (ranges.first(), ranges.last()) else {
                continue;
            };
            let within = range.start.max(first.start_byte)..range.end.min(last.end_byte);
            if within.is_empty() {
                continue;
            }
            let Ok(inner) = language.compiled() else {
                continue;
            };
            let Some(sub) = self.injected_tree(language, &ranges, text, deadline) else {
                continue;
            };
            self.collect(
                inner,
                &sub,
                text,
                within,
                (depth.saturating_add(1), deadline),
                found,
            );
        }
    }

    /// The tree of `ranges` of `text` in `language`, if it has one yet:
    /// parsed once for each revision of the text -- begun here, until
    /// `deadline`, and carried on by `work` if it does not finish.
    fn injected_tree(
        &self,
        language: &'static Language,
        ranges: &[tree_sitter::Range],
        text: &TextBuffer,
        deadline: Option<Instant>,
    ) -> Option<Tree> {
        let mut cache = self.injected.try_borrow_mut().ok()?;
        if cache.revision != text.revision() {
            cache.forget();
            cache.revision = text.revision();
        }
        let key = (
            language.index,
            ranges
                .iter()
                .map(|r| (r.start_byte, r.end_byte))
                .collect::<Vec<_>>(),
        );
        match cache.stretches.get(&key) {
            Some(Stretch::Done(tree)) => return Some(tree.clone()),
            Some(Stretch::Pending { .. } | Stretch::Failed) => return None,
            None => {}
        }
        let bytes: usize = ranges
            .iter()
            .map(|r| r.end_byte.saturating_sub(r.start_byte))
            .fold(0, usize::saturating_add);
        if bytes > MAX_INJECTED_BYTES {
            cache.stretches.insert(key, Stretch::Failed);
            return None;
        }
        let InjectionCache {
            parsers, stretches, ..
        } = &mut *cache;
        let pool = parsers.entry(language.index).or_default();
        let mut parser = pool.pop().unwrap_or_else(|| {
            let mut parser = Parser::new();
            // A grammar the runtime refuses leaves the parser without a
            // language, and it then refuses to parse: the stretch fails.
            let _ = parser.set_language(&language.ts_language());
            parser
        });
        if parser.set_included_ranges(ranges).is_err() {
            pool.push(parser);
            stretches.insert(key, Stretch::Failed);
            return None;
        }
        let limit = parse_limit(bytes);
        let mut used = 0;
        match slice(&mut parser, text, None, deadline, &mut used, limit) {
            Slice::Done(tree) => {
                pool.push(parser);
                stretches.insert(key, Stretch::Done(tree.clone()));
                Some(tree)
            }
            Slice::Late => {
                stretches.insert(
                    key,
                    Stretch::Pending {
                        parser,
                        used,
                        limit,
                    },
                );
                None
            }
            Slice::Over | Slice::Refused => {
                parser.reset();
                pool.push(parser);
                stretches.insert(key, Stretch::Failed);
                None
            }
        }
    }

    /// Carry on the injected stretches drawing began and did not finish,
    /// each until it is done or `deadline` passes: whether any is left.
    fn carry_on_injections(&mut self, text: &TextBuffer, deadline: Option<Instant>) -> bool {
        let cache = self.injected.get_mut();
        if cache.revision != text.revision() {
            // Begun on another text: nothing to carry on.
            cache.forget();
            return false;
        }
        let InjectionCache {
            parsers, stretches, ..
        } = cache;
        let mut left = false;
        for (key, stretch) in stretches.iter_mut() {
            let Stretch::Pending {
                parser,
                used,
                limit,
            } = stretch
            else {
                continue;
            };
            if deadline.is_some_and(|d| Instant::now() >= d) {
                left = true;
                continue;
            }
            let outcome = slice(parser, text, None, deadline, used, *limit);
            let next = match outcome {
                Slice::Done(tree) => Stretch::Done(tree),
                Slice::Late => {
                    left = true;
                    continue;
                }
                Slice::Over | Slice::Refused => Stretch::Failed,
            };
            if let Stretch::Pending { mut parser, .. } = core::mem::replace(stretch, next) {
                parser.reset();
                parsers.entry(key.0).or_default().push(parser);
            }
        }
        left
    }
}

/// The stretches `injections` finds in `tree` over `range`: each with its
/// language and the ranges its text is in. A match with
/// `injection.combined` is one document with every other match of its
/// pattern and language (among those in `range`).
fn injections_in(
    injections: &Injections,
    tree: &Tree,
    text: &TextBuffer,
    range: Range<usize>,
) -> Vec<(&'static Language, Vec<tree_sitter::Range>)> {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range);
    let mut matches = cursor.matches(&injections.query, tree.root_node(), BufferText(text));
    let mut out: Vec<(&'static Language, Vec<tree_sitter::Range>)> = Vec::new();
    let mut combined: Vec<(usize, &'static Language, Vec<tree_sitter::Range>)> = Vec::new();
    while let Some(m) = matches.next() {
        let settings = injections.query.property_settings(m.pattern_index);
        let setting = |key: &str| settings.iter().find(|p| &*p.key == key);
        let mut name: Option<String> = setting("injection.language")
            .and_then(|p| p.value.as_deref())
            .map(str::to_owned);
        let include_children = setting("injection.include-children").is_some();
        let is_combined = setting("injection.combined").is_some();
        let mut ranges = Vec::new();
        for capture in m.captures {
            if Some(capture.index) == injections.language {
                name = text.slice(capture.node.byte_range()).ok();
            } else if Some(capture.index) == injections.content {
                content_ranges(capture.node, include_children, &mut ranges);
            }
        }
        let Some(language) = name.as_deref().and_then(Language::for_injection) else {
            continue;
        };
        if ranges.is_empty() {
            continue;
        }
        if is_combined {
            match combined
                .iter_mut()
                .find(|(pattern, l, _)| *pattern == m.pattern_index && *l == language)
            {
                Some((_, _, all)) => all.extend(ranges),
                None => combined.push((m.pattern_index, language, ranges)),
            }
        } else {
            out.push((language, ranges));
        }
    }
    for (_, language, mut ranges) in combined {
        ranges.sort_by_key(|r| r.start_byte);
        out.push((language, ranges));
    }
    out
}

/// The ranges of `node`'s text an injection covers: all of it with
/// `injection.include-children`, otherwise the text between its named
/// children. An anonymous child -- punctuation, a keyword -- is the node's
/// own text, which its grammar split into tokens only to find where the
/// node ends: Markdown's block grammar lexes a paragraph's backticks and
/// brackets so, and the inline grammar it injects needs them. Neovim, whose
/// queries these are, reads the default so; Helix spells it
/// `injection.include-unnamed-children`.
fn content_ranges(node: Node<'_>, include_children: bool, out: &mut Vec<tree_sitter::Range>) {
    let range = |start_byte: usize, end_byte: usize, start_point: Point, end_point: Point| {
        tree_sitter::Range {
            start_byte,
            end_byte,
            start_point,
            end_point,
        }
    };
    if include_children || node.named_child_count() == 0 {
        out.push(node.range());
        return;
    }
    let (mut at, mut at_point) = (node.start_byte(), node.start_position());
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.start_byte() > at {
            out.push(range(
                at,
                child.start_byte(),
                at_point,
                child.start_position(),
            ));
        }
        at = child.end_byte();
        at_point = child.end_position();
    }
    if node.end_byte() > at {
        out.push(range(at, node.end_byte(), at_point, node.end_position()));
    }
}

/// Stacked spans made flat -- cut to `within`. The spans are in the order
/// the captures came, which is by where they start; each goes on top of
/// those still open where it starts, and the one on top is what shows. So a
/// node inside another wins where it is, and of two spans that start
/// together the later pattern's is on top over its whole length, whether
/// it is the shorter or the longer: TOML's `(pair (bare_key)) @property`
/// colours a key over `(bare_key) @type` that way. A span stays open, and
/// hidden, under one above it until that one closes. This is tree-sitter's
/// highlighter's stack, event for event. A span of no kind paints plainly:
/// nothing is emitted for it, and what it covers shows the text's own ink.
fn flatten(
    mut spans: Vec<(Range<usize>, Option<Highlight>)>,
    within: &Range<usize>,
) -> Vec<HighlightSpan> {
    // Stable, so spans that start together keep the order they came in:
    // the host's by pattern, then each injected language's over them.
    spans.sort_by_key(|(r, _)| r.start);
    let mut out: Vec<HighlightSpan> = Vec::new();
    let mut emit = |from: usize, to: usize, kind: Option<Highlight>| {
        let (from, to) = (from.max(within.start), to.min(within.end));
        let Some(kind) = kind.filter(|_| to > from) else {
            return;
        };
        match out.last_mut() {
            Some(last) if last.range.end == from && last.highlight == kind => last.range.end = to,
            _ => out.push(HighlightSpan {
                range: from..to,
                highlight: kind,
            }),
        }
    };
    // The spans open at `pos`, the top last: (end, kind).
    let mut open: Vec<(usize, Option<Highlight>)> = Vec::new();
    let mut pos = 0usize;
    for (range, kind) in spans {
        // Close what ends before this starts, from the top down, colouring
        // up to each end; one still open stops it, whatever is under it (a
        // span under one that outlasts it is closed with it, having shown
        // nowhere past it).
        while let Some(&(end, top)) = open.last() {
            if end > range.start {
                break;
            }
            emit(pos, end, top);
            pos = pos.max(end);
            open.pop();
        }
        if let Some(&(_, top)) = open.last() {
            emit(pos, range.start, top);
        }
        pos = pos.max(range.start);
        open.push((range.end, kind));
    }
    while let Some((end, kind)) = open.pop() {
        emit(pos, end, kind);
        pos = pos.max(end);
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;

    /// The highlights of all of `buffer` once there is no work left, as a
    /// view gets them: it draws, and while the highlighter has work -- the
    /// document's parse, then the stretches each draw found and did not
    /// finish -- it works and draws again. Each round finishes what the last
    /// draw found, and injections go three deep, so a highlighter that still
    /// has work after a few rounds is broken: that fails, rather than hangs.
    fn settled(h: &mut SyntaxHighlighter, buffer: &TextBuffer) -> Vec<HighlightSpan> {
        for _ in 0..=MAX_INJECTION_DEPTH + 1 {
            while h.work(buffer, Duration::from_secs(5)) {}
            let drawn = h.highlights(buffer, 0..buffer.len());
            if !h.has_work() {
                return drawn;
            }
        }
        panic!("the highlighter still has work after every round of it");
    }

    fn spans(text: &str, language: &str) -> Vec<(String, Highlight)> {
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named(language).unwrap().highlighter().unwrap();
        h.reset(&buffer);
        settled(&mut h, &buffer)
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

    /// **A parse is given up past its limit** -- two million steps and
    /// characters, and two hundred a byte -- and the next edit tries again.
    #[test]
    fn a_parse_past_its_limit_is_given_up_until_the_next_edit() {
        assert_eq!(parse_limit(0), 2_000_000);
        assert_eq!(parse_limit(1 << 20), 2_000_000 + 200 * (1 << 20));
        assert_eq!(parse_limit(usize::MAX), u64::MAX);
        // Long enough that the runtime checks its progress at least once.
        let mut text = String::new();
        for i in 0..200 {
            text.push_str(&format!("fn a{i}() {{}}\n"));
        }
        let mut buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        // As if the slices so far had done the whole limit's work: however
        // long the budget, the first check gives it up.
        h.used = parse_limit(buffer.len());
        assert!(!h.work(&buffer, Duration::from_mins(1)), "not given up");
        assert!(h.is_abandoned() && h.tree().is_none() && !h.is_stale());
        let _ = buffer.take_changes();
        buffer.insert(0, "x").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        assert!(!h.is_abandoned() && h.is_stale());
        while h.work(&buffer, Duration::from_secs(5)) {}
        assert!(h.tree().is_some() && !h.is_stale());
    }

    /// **Work is what is limited, the lexers' included**: on a line of three
    /// thousand `*`s, Markdown's scanner reads the rest of the line again for
    /// every one of them -- four and a half million characters, for a few
    /// thousand of the runtime's steps -- and the parse is given up for
    /// that, however long the budget, on any machine.
    #[test]
    fn a_scanner_rereading_its_line_is_given_up_for_its_work() {
        let text = "*".repeat(3000);
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("markdown").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_mins(1)) {}
        assert!(h.is_abandoned(), "finished, in {} units", h.used);
        assert!(h.used > parse_limit(buffer.len()));
        // A file as long of ordinary text is nowhere near it.
        let prose = "Some words, *emphasis*, and `code`.\n".repeat(90);
        let buffer = TextBuffer::from_text(&prose);
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_mins(1)) {}
        assert!(!h.is_abandoned() && h.tree().is_some());
        assert!(
            h.used < 20 * u64::try_from(prose.len()).unwrap(),
            "{}",
            h.used
        );
    }

    /// **Flattening**: nested spans become flat ones, the inner winning, cut
    /// to the range asked for; neighbours of one kind merge; a plain span
    /// (`@none`) leaves a gap its enclosing colour does not show through.
    #[test]
    fn nested_spans_are_flattened() {
        use Highlight::{Escape, Keyword, String as Str};
        let flat = flatten(
            vec![
                (0..10, Some(Str)),
                (3..5, Some(Escape)),
                (5..7, Some(Escape)),
                (12..14, Some(Keyword)),
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
        let same = flatten(vec![(0..4, Some(Str)), (0..4, Some(Keyword))], &(0..4));
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].highlight, Keyword);
        assert!(flatten(Vec::new(), &(0..9)).is_empty());
        // Plain inside a string: the string on either side, nothing within;
        // and a keyword inside the plain part is still a keyword.
        let plain = flatten(
            vec![(0..10, Some(Str)), (2..8, None), (4..6, Some(Keyword))],
            &(0..10),
        );
        let got: Vec<(Range<usize>, Highlight)> =
            plain.into_iter().map(|s| (s.range, s.highlight)).collect();
        assert_eq!(got, [(0..2, Str), (4..6, Keyword), (8..10, Str)]);
    }

    /// **Spans that start together stack in the order they came**, the
    /// later on top over its whole length -- longer or shorter -- and one
    /// under a longer one stays hidden until that one closes: TOML's key,
    /// `(bare_key) @type` then `(pair (bare_key)) @property`, is a property.
    #[test]
    fn spans_that_start_together_stack_in_their_order() {
        use Highlight::{Keyword, Property, String as Str, Type};
        let flat =
            |spans: Vec<(Range<usize>, Option<Highlight>)>| -> Vec<(Range<usize>, Highlight)> {
                flatten(spans, &(0..30))
                    .into_iter()
                    .map(|s| (s.range, s.highlight))
                    .collect()
            };
        // `title = "x"`: the key's type under the pair's property.
        assert_eq!(
            flat(vec![
                (0..5, Some(Type)),
                (0..22, Some(Property)),
                (8..20, Some(Str)),
            ]),
            [(0..8, Property), (8..20, Str), (20..22, Property)]
        );
        // The other way round, the shorter on top where it is.
        assert_eq!(
            flat(vec![(0..22, Some(Property)), (0..5, Some(Type))]),
            [(0..5, Type), (5..22, Property)]
        );
        // A span after the hidden one's end but inside the one over it
        // goes on top of that one; the hidden one never shows again.
        assert_eq!(
            flat(vec![
                (0..5, Some(Type)),
                (0..20, Some(Property)),
                (10..12, Some(Keyword)),
                (24..26, Some(Str)),
            ]),
            [
                (0..10, Property),
                (10..12, Keyword),
                (12..20, Property),
                (24..26, Str)
            ]
        );
    }

    /// **A stretch in another language is coloured as that language**: a
    /// `macro_rules!` body is a token tree to the Rust grammar, where `let`
    /// is only a word, and Rust's injection query says it is Rust -- parsed
    /// as Rust, `let` is a keyword and `1` a number.
    #[test]
    fn an_injected_stretch_is_coloured_as_its_language() {
        let text = "macro_rules! m { () => { let x = 1; } }\n";
        let got = spans(text, "rust");
        let at = |s: &str| got.iter().find(|(t, _)| t == s).map(|(_, h)| *h);
        assert_eq!(at("let"), Some(Highlight::Keyword), "{got:?}");
        assert_eq!(at("1"), Some(Highlight::Constant), "{got:?}");
        // The parse is kept for the text as it is: asking again parses
        // nothing more.
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let first = h.highlights(&buffer, 0..buffer.len());
        let parses = h.injected.borrow().stretches.len();
        assert!(parses > 0, "nothing was injected");
        assert_eq!(h.highlights(&buffer, 0..buffer.len()), first);
        assert_eq!(h.injected.borrow().stretches.len(), parses);
    }

    /// **Markdown is coloured block by block, inline, and in the languages
    /// it holds**: the block grammar colours a heading and a list, the
    /// inline grammar it injects into each paragraph colours a code span and
    /// a link -- the paragraph's own backticks and brackets, which the block
    /// grammar lexed as tokens of its own, handed to it with the rest --
    /// front matter is YAML, its keys keys (a later pattern than the one
    /// that makes every scalar a string), and a fenced block is coloured as
    /// the language its fence names, not as the literal text around it,
    /// which the query gaps with `@none` for it.
    #[test]
    fn markdown_is_coloured_block_inline_and_by_its_fences() {
        let text = "---\ntitle: Notes\n---\n\n# Title\n\n- see `x` and \
                    [docs](https://a.b)\n\n```rust\nlet n = 1;\n```\n";
        let got = spans(text, "markdown");
        let at = |s: &str| got.iter().find(|(t, _)| t.trim() == s).map(|(_, h)| *h);
        for (s, want) in [
            // Front matter, injected as YAML.
            ("title", Highlight::Property),
            // The block grammar.
            ("#", Highlight::Punctuation),
            ("Title", Highlight::Heading),
            ("-", Highlight::Punctuation),
            // The inline grammar, injected into the list item's paragraph.
            ("x", Highlight::String),
            ("docs", Highlight::Link),
            ("https://a.b", Highlight::Link),
            // The fence, and the Rust inside it.
            ("```", Highlight::Punctuation),
            ("let", Highlight::Keyword),
            ("1", Highlight::Constant),
        ] {
            assert_eq!(at(s), Some(want), "{s}: {got:?}");
        }
        // What Rust leaves plain in the fence is plain, not the literal
        // colour of the block around it.
        assert!(
            !got.iter()
                .any(|(t, h)| t.contains('=') && *h == Highlight::String),
            "{got:?}"
        );
    }

    /// **A stretch too long to parse while drawing is carried on by
    /// `work`**, which the highlighter says it has, and coloured when it is
    /// done: a Rust fence of five hundred lines in Markdown, drawn with no
    /// time to parse, is uncoloured at first -- and a short paragraph beside
    /// it, which parses before the first check of the clock, is not.
    #[test]
    fn a_stretch_too_long_to_draw_is_parsed_by_work_then_coloured() {
        let mut text = String::from("Some `code` here.\n\n```rust\n");
        for i in 0..500 {
            text.push_str(&format!("let x{i} = {i};\n"));
        }
        text.push_str("```\n");
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("markdown").unwrap().highlighter().unwrap();
        h.draw_budget = Duration::ZERO;
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        assert!(!h.has_work(), "the document itself is parsed");
        let kinds = |h: &SyntaxHighlighter| -> Vec<Highlight> {
            h.highlights(&buffer, 0..buffer.len())
                .into_iter()
                .map(|s| s.highlight)
                .collect()
        };
        let first = kinds(&h);
        assert!(
            first.contains(&Highlight::String),
            "the paragraph's code span"
        );
        assert!(
            !first.contains(&Highlight::Keyword),
            "the fence parsed at once"
        );
        assert!(h.has_work(), "the fence left for `work` was not reported");
        let mut slices = 0;
        while h.work(&buffer, Duration::from_micros(200)) {
            slices += 1;
            assert!(slices < 100_000, "never finished");
        }
        assert!(!h.has_work());
        assert!(
            kinds(&h).contains(&Highlight::Keyword),
            "the fence was not coloured"
        );
    }

    /// **An edit forgets the stretches being parsed**: they are of the text
    /// before it. The next draw begins them again on the text as it is.
    #[test]
    fn an_edit_forgets_the_stretches_being_parsed() {
        let mut text = String::from("```rust\n");
        for i in 0..500 {
            text.push_str(&format!("let x{i} = {i};\n"));
        }
        text.push_str("```\n");
        let mut buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("markdown").unwrap().highlighter().unwrap();
        h.draw_budget = Duration::ZERO;
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let _ = h.highlights(&buffer, 0..buffer.len());
        assert!(h.injected.borrow().pending());
        let _ = buffer.take_changes();
        buffer.insert(0, "x\n\n").unwrap();
        h.edited(&buffer, &buffer.clone().take_changes().splices.unwrap());
        assert!(!h.injected.borrow().pending(), "a stretch of the old text");
        assert!(h.injected.borrow().stretches.is_empty());
        // Its parser is kept for the next stretch in its language.
        let rust = Language::named("rust").unwrap().index;
        assert_eq!(
            h.injected.borrow().parsers.get(&rust).map(Vec::len),
            Some(1)
        );
        while h.work(&buffer, Duration::from_secs(5)) {}
        let _ = h.highlights(&buffer, 0..buffer.len());
        while h.work(&buffer, Duration::from_secs(5)) {}
        let kinds: Vec<Highlight> = h
            .highlights(&buffer, 0..buffer.len())
            .into_iter()
            .map(|s| s.highlight)
            .collect();
        assert!(kinds.contains(&Highlight::Keyword));
    }

    /// **A budget too long to add to the clock is no deadline** -- not one
    /// already passed: a whole parse happens in one call, however many
    /// times the runtime checks the clock on the way.
    #[test]
    fn a_budget_past_the_clock_is_no_deadline() {
        let mut text = String::new();
        for i in 0..200 {
            text.push_str(&format!("fn f{i}() {{}}\n"));
        }
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("rust").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        assert!(!h.work(&buffer, Duration::MAX));
        assert!(h.tree().is_some() && !h.is_stale());
    }

    /// **A language another's text names is found by name or alias**, in
    /// any case, with a fence's decorations off.
    #[test]
    fn an_injected_language_is_found_by_name_or_alias() {
        let found = |n: &str| Language::for_injection(n).map(Language::name);
        assert_eq!(found("rust"), Some("Rust"));
        assert_eq!(found("RS"), Some("Rust"));
        assert_eq!(found(" py "), Some("Python"));
        assert_eq!(found("{.yml}"), Some("YAML"));
        assert_eq!(found("cobol"), None);
    }
}
