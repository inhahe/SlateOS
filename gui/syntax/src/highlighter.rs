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
//! levels deep -- in the text it was given, where its colours show over
//! its host's, whichever starts where; what it was not given, a diff line's
//! `+` or a template's `${...}`, keeps its host's colours. (A query may
//! move a stretch's ends, `#offset!`, and name its language by a file's,
//! `@injection.filename`: Neovim's, as a diff's hunks need.) Drawing starts
//! each stretch it finds, all of them within
//! [`DRAW_BUDGET`] together; one that does not finish in it is carried on
//! by [`work`](Highlighter::work) a slice at a time -- the view keeps asking
//! while [`has_work`](Highlighter::has_work) says so -- and is coloured when
//! it is done. The parses are kept -- across an edit that does not touch
//! them, moved with the text as the host's tree is, so a keystroke re-parses
//! only the stretch it was typed in -- and drawing the
//! same screen again costs nothing; a stretch past its work limit
//! ([`parse_limit`]) stays in its host's colours.
//!
//! # Where a name was declared
//!
//! A grammar may say, besides how to colour each kind of node, where its
//! names are declared and used -- a third query, `locals.scm`: which nodes
//! are scopes (a function, a block), which declare a name in the scope
//! around them, and which may use one. A use is then coloured as its
//! declaration is -- a parameter's uses as the parameter -- and a name found
//! declared, a *local*, is left alone by the patterns marked
//! `(#is-not? local)`: `module` is Node's where nothing declares it, and a
//! plain variable where the code does. That is tree-sitter's highlighter's
//! reading, scope for scope ([`LocalsPass`]).
//!
//! A use may be thousands of lines below its declaration, where drawing --
//! a screen at a time -- does not look. So each tree is indexed once, by a
//! pass of the locals query over the whole text, which
//! [`work`](Highlighter::work) makes a slice at a time after the parse, as
//! it makes the parse; an injected stretch's tree is indexed as it is
//! parsed. Until the pass over a new tree is done, drawing uses the last
//! index, moved with each edit as the tree is; what an edit touched is left
//! out of it until then. A declaration's colour is found the first time a
//! use of it is drawn, and kept.
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
//!
//! Before all that, a pattern may set its captures' priority, Neovim's
//! `(#set! priority 95)` -- 100 where it sets none -- and over any one byte
//! the highest priority shows: a diff's `+` under its line's colour. Then
//! an injected language's colours over its host's, in the text it was given
//! -- where Neovim, whose queries these mostly are, puts them too. Below
//! both, the order above: tree-sitter's highlighter's, event for event.

use core::cell::RefCell;
use core::ops::Range;
use core::time::Duration;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use guitk::highlight::{Brackets, Highlight, HighlightSpan, Highlighter};
use guitk::textbuffer::{Splice, TextBuffer};
use tree_sitter::{
    InputEdit, Node, ParseOptions, ParseState, Parser, Point, QueryCursor, StreamingIterator,
    TextProvider, Tree,
};

use crate::{Compiled, DEFAULT_PRIORITY, Error, Injections, Language, LocalsQuery, Paint, ffi};

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
    /// Parsed: the tree, and where its names are declared and used -- once
    /// the pass of its language's locals query over it, which `work`
    /// carries on as it does a parse, is done.
    Done {
        tree: Tree,
        locals: Option<Arc<Locals>>,
        pass: Option<LocalsPass>,
        /// Its language's queries: a pass the text moved under is begun
        /// again with them.
        compiled: &'static Compiled,
    },
    /// Being parsed a slice at a time by `work`: the parser, holding the
    /// parse where it stopped, the work done so far and the most allowed,
    /// and its language's queries.
    Pending {
        parser: Parser,
        used: u64,
        limit: u64,
        compiled: &'static Compiled,
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

    /// Move with an edit, as the host's tree moves: a stretch the edit did
    /// not touch keeps its parse and its locals -- its text is the same --
    /// moved along with the text; one it touched is forgotten, and so is one
    /// being parsed, whose parse read the text before (its parser kept for
    /// the next).
    fn edit(&mut self, s: &Splice) {
        let touched = |&(start, end): &(usize, usize)| end >= s.start && start <= s.old_end;
        let shift = |p: usize| {
            if p > s.old_end {
                p.saturating_sub(s.old_end).saturating_add(s.new_end)
            } else {
                p
            }
        };
        for ((language, ranges), stretch) in core::mem::take(&mut self.stretches) {
            let stretch = match stretch {
                Stretch::Pending { mut parser, .. } => {
                    parser.reset();
                    self.parsers.entry(language).or_default().push(parser);
                    continue;
                }
                kept => kept,
            };
            if ranges.iter().any(touched) {
                continue;
            }
            let moved = match stretch {
                Stretch::Done {
                    mut tree,
                    locals,
                    compiled,
                    ..
                } => {
                    tree.edit(&input_edit(s));
                    let locals = locals.map(|mut locals| {
                        Arc::make_mut(&mut locals).edit(s);
                        locals
                    });
                    // A pass being made read the tree before the edit:
                    // begin it again.
                    let pass = if locals.is_none() {
                        compiled
                            .locals
                            .as_ref()
                            .map(|query| LocalsPass::new(tree.clone(), compiled, query))
                    } else {
                        None
                    };
                    Stretch::Done {
                        tree,
                        locals,
                        pass,
                        compiled,
                    }
                }
                failed => failed,
            };
            let ranges = ranges
                .into_iter()
                .map(|(start, end)| (shift(start), shift(end)))
                .collect();
            self.stretches.insert((language, ranges), moved);
        }
    }

    /// Whether any stretch is waiting for `work`: its parse, or its locals
    /// pass.
    fn pending(&self) -> bool {
        self.stretches.values().any(|s| {
            matches!(
                s,
                Stretch::Pending { .. } | Stretch::Done { pass: Some(_), .. }
            )
        })
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

/// A colour a capture paints: a kind of code, or none -- the text's own ink
/// (`@none`).
type Colour = Option<Highlight>;

/// A node, as the locals index knows it: where it is, and its kind. Not its
/// id, which dies with its tree -- an index outlives the tree it was made
/// from, moved with each edit, until the pass over the next is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NodeKey {
    start: usize,
    end: usize,
    kind: u16,
}

impl NodeKey {
    fn of(node: Node<'_>) -> Self {
        Self {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: node.kind_id(),
        }
    }

    /// Whether `s` touched it: changed the text it covers, or text against
    /// either end of it.
    fn touched_by(&self, s: &Splice) -> bool {
        self.end >= s.start && self.start <= s.old_end
    }

    /// Move it with `s`, which did not touch it: along by as much as the
    /// text grew or shrank, if it is after the edit.
    fn shift(&mut self, s: &Splice) {
        if self.start > s.old_end {
            self.start = self
                .start
                .saturating_sub(s.old_end)
                .saturating_add(s.new_end);
            self.end = self.end.saturating_sub(s.old_end).saturating_add(s.new_end);
        }
    }
}

/// What the locals query found a node to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Local {
    /// The declaration of this definition.
    Declares(usize),
    /// A use of this definition.
    Uses(usize),
}

/// Where a language's names are declared and used in one tree, as its
/// locals query says: the index drawing colours names by (see the module
/// docs).
#[derive(Clone)]
pub(crate) struct Locals {
    /// The language's queries, which say a declaration's colour.
    compiled: &'static Compiled,
    /// Each definition's node, whose colour is the definition's. None for
    /// one whose node the query went on to take for a scope as well -- which
    /// leaves the definition without a colour, as tree-sitter's highlighter
    /// leaves it -- and for one an edit touched.
    definitions: Vec<Option<NodeKey>>,
    /// The nodes that declare or use a definition, sorted.
    nodes: Vec<(NodeKey, Local)>,
    /// Each definition's colour, found the first time a use of it is drawn:
    /// none if its node paints nothing.
    colours: Vec<OnceLock<Option<Colour>>>,
}

impl Locals {
    /// What the index says of `node`, in `tree`: whether it is a local -- a
    /// name found declared -- and, for a use, its declaration's colour. A use
    /// of a declaration with no colour is no local, as it is to
    /// tree-sitter's highlighter.
    fn of(&self, node: Node<'_>, tree: &Tree, text: &TextBuffer) -> (bool, Option<Colour>) {
        let key = NodeKey::of(node);
        let local = self
            .nodes
            .binary_search_by(|(k, _)| k.cmp(&key))
            .ok()
            .and_then(|i| self.nodes.get(i))
            .map(|&(_, local)| local);
        match local {
            Some(Local::Declares(_)) => (true, None),
            Some(Local::Uses(d)) => {
                let colour = self.colour(d, tree, text);
                (colour.is_some(), colour)
            }
            None => (false, None),
        }
    }

    /// Definition `d`'s colour: what the highlight query paints its node,
    /// settled as a local's is, found in `tree` the first time it is asked
    /// for.
    fn colour(&self, d: usize, tree: &Tree, text: &TextBuffer) -> Option<Colour> {
        let key = self.definitions.get(d).copied().flatten()?;
        let memo = self.colours.get(d)?;
        *memo.get_or_init(|| {
            let path = path_to(tree, key)?;
            // Every pattern that captures the node matches from at most
            // `depth` levels above it -- one more for a pattern of siblings
            // -- so the query need look no higher: from the root, it would
            // step past everything before the node, a file's worth.
            let up = self.compiled.depth.saturating_add(1);
            let from = *path.get(path.len().saturating_sub(up.saturating_add(1)))?;
            settle_node(self.compiled, from, key, text)
        })
    }

    /// Move with an edit, as the tree moves: what follows it shifts, and
    /// what it touched is forgotten until the pass over the edited text.
    fn edit(&mut self, s: &Splice) {
        for definition in &mut self.definitions {
            if definition.is_some_and(|k| k.touched_by(s)) {
                *definition = None;
            } else if let Some(k) = definition {
                k.shift(s);
            }
        }
        // What is left keeps its order: all of it is before the edit, or
        // after it and moved alike.
        self.nodes.retain(|(k, _)| !k.touched_by(s));
        for (k, _) in &mut self.nodes {
            k.shift(s);
        }
    }
}

/// The nodes from `tree`'s root down to the node `key` names, that last:
/// none if the tree has no such node.
fn path_to(tree: &Tree, key: NodeKey) -> Option<Vec<Node<'_>>> {
    let mut cursor = tree.walk();
    let mut path = vec![cursor.node()];
    loop {
        let node = cursor.node();
        if NodeKey::of(node) == key {
            return Some(path);
        }
        if node.start_byte() > key.start || node.end_byte() < key.end {
            return None;
        }
        cursor.goto_first_child_for_byte(key.start)?;
        path.push(cursor.node());
    }
}

/// What the highlight query of `compiled` paints the node `key` names,
/// settled as a local's is, from the matches found under `from` -- the
/// node itself or an ancestor.
fn settle_node(
    compiled: &Compiled,
    from: Node<'_>,
    key: NodeKey,
    text: &TextBuffer,
) -> Option<Colour> {
    if key.start >= key.end {
        return None;
    }
    // Every capture of the node is in a match that meets its range,
    // whatever else the match takes in.
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(key.start..key.end);
    let mut captures = cursor.captures(&compiled.highlights, from, BufferText(text));
    let mut settling = Settling {
        local: true,
        ..Settling::default()
    };
    while let Some((m, index)) = captures.next() {
        let Some(capture) = m.captures.get(*index) else {
            continue;
        };
        if NodeKey::of(capture.node) == key {
            settling.take(
                compiled.paint(capture.index),
                compiled.is_non_local(m.pattern_index),
                compiled.priority(m.pattern_index, capture.index),
            );
        }
    }
    settling.paint
}

/// A scope open during a locals pass.
struct Scope {
    /// Where it ends.
    end: usize,
    /// Whether a name not declared in it is looked for in the scope around
    /// it.
    inherits: bool,
    /// Its definitions, by name: each name's in the order declared, each
    /// with where the value its declaration gives it ends -- a use before
    /// then is not of it.
    names: HashMap<Box<[u8]>, Vec<(usize, usize)>>,
}

/// One node's locals captures, while they come: what it declares or uses
/// so far. (tree-sitter's highlighter lets a later capture of the node
/// undo an earlier: a declaration undoes a use, a scope a declaration.)
struct Taking {
    id: usize,
    key: NodeKey,
    declares: Option<usize>,
    uses: Option<usize>,
}

impl Taking {
    /// Put what the node turned out to be into the index.
    fn finish(self, definitions: &mut [Option<NodeKey>], nodes: &mut Vec<(NodeKey, Local)>) {
        if let Some(d) = self.declares {
            if let Some(slot) = definitions.get_mut(d) {
                *slot = Some(self.key);
            }
            nodes.push((self.key, Local::Declares(d)));
        } else if let Some(d) = self.uses {
            nodes.push((self.key, Local::Uses(d)));
        }
    }
}

/// How many nodes a locals pass takes between two looks at the clock.
const NODES_PER_CHECK: u32 = 64;

/// A pass of a language's locals query over one tree, a slice at a time:
/// the scopes open where it is, and what it has found. It reads the
/// captures as tree-sitter's highlighter does, in the order they come --
/// by where their nodes start -- keeping a stack of scopes: a scope is
/// opened by its capture and closed by the first node to start past its
/// end; a declaration goes into the innermost open scope; a use is of the
/// last declaration of its name, before it, in the innermost scope that has
/// one -- looking outward only through scopes that inherit.
pub(crate) struct LocalsPass {
    tree: Tree,
    compiled: &'static Compiled,
    query: &'static LocalsQuery,
    /// Where the next slice starts: the captures of every node that starts
    /// before here are taken.
    resume: usize,
    /// The scopes open where the pass stopped, innermost last. The first is
    /// the whole text's, which nothing closes and which inherits nothing.
    stack: Vec<Scope>,
    definitions: Vec<Option<NodeKey>>,
    nodes: Vec<(NodeKey, Local)>,
}

impl LocalsPass {
    fn new(tree: Tree, compiled: &'static Compiled, query: &'static LocalsQuery) -> Self {
        Self {
            tree,
            compiled,
            query,
            resume: 0,
            stack: vec![Scope {
                end: usize::MAX,
                inherits: false,
                names: HashMap::new(),
            }],
            definitions: Vec::new(),
            nodes: Vec::new(),
        }
    }

    /// Carry the pass on until it is done -- answering what it found -- or
    /// `deadline` (none: no deadline) passes, when it stops between two
    /// nodes that start apart, to resume there.
    fn advance(&mut self, text: &TextBuffer, deadline: Option<Instant>) -> Option<Locals> {
        let Self {
            tree,
            query,
            resume,
            stack,
            definitions,
            nodes,
            ..
        } = self;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(*resume..usize::MAX);
        let mut captures = cursor.captures(&query.query, tree.root_node(), BufferText(text));
        let mut taking: Option<Taking> = None;
        let mut since_check: u32 = 0;
        while let Some((m, index)) = captures.next() {
            let Some(capture) = m.captures.get(*index) else {
                continue;
            };
            let node = capture.node;
            let start = node.start_byte();
            if start < *resume {
                // Taken by an earlier slice.
                continue;
            }
            if taking.as_ref().is_none_or(|t| t.id != node.id()) {
                let before = taking.take().map(|t| {
                    let at = t.key.start;
                    t.finish(definitions, nodes);
                    at
                });
                if before.is_some_and(|at| at < start) {
                    since_check = since_check.saturating_add(1);
                    if since_check >= NODES_PER_CHECK {
                        since_check = 0;
                        if deadline.is_some_and(|d| Instant::now() >= d) {
                            *resume = start;
                            return None;
                        }
                    }
                }
                // Close the scopes this node starts past. (One starting
                // exactly where a scope ends is still in it, as it is to
                // tree-sitter's highlighter.)
                while stack.len() > 1 && stack.last().is_some_and(|s| start > s.end) {
                    stack.pop();
                }
                taking = Some(Taking {
                    id: node.id(),
                    key: NodeKey::of(node),
                    declares: None,
                    uses: None,
                });
            }
            let Some(t) = taking.as_mut() else {
                continue;
            };
            let which = Some(capture.index);
            if which == query.scope {
                t.declares = None;
                let inherits = query
                    .query
                    .property_settings(m.pattern_index)
                    .iter()
                    .rfind(|p| &*p.key == "local.scope-inherits")
                    .is_none_or(|p| p.value.as_deref().is_none_or(|v| v == "true"));
                stack.push(Scope {
                    end: node.end_byte(),
                    inherits,
                    names: HashMap::new(),
                });
            } else if which == query.definition {
                t.uses = None;
                let value_end = m
                    .captures
                    .iter()
                    .rfind(|c| Some(c.index) == query.definition_value)
                    .map_or(0, |c| c.node.end_byte());
                let d = definitions.len();
                definitions.push(None);
                if let Some(scope) = stack.last_mut() {
                    scope
                        .names
                        .entry(name_of(text, node))
                        .or_default()
                        .push((d, value_end));
                }
                t.declares = Some(d);
            } else if which == query.reference && t.declares.is_none() {
                let name = name_of(text, node);
                for scope in stack.iter().rev() {
                    let found = scope.names.get(&name).and_then(|declared| {
                        declared
                            .iter()
                            .rev()
                            .find(|&&(_, value_end)| start >= value_end)
                    });
                    if let Some(&(d, _)) = found {
                        t.uses = Some(d);
                        break;
                    }
                    if !scope.inherits {
                        break;
                    }
                }
            }
        }
        if let Some(t) = taking.take() {
            t.finish(definitions, nodes);
        }
        let mut nodes = core::mem::take(nodes);
        nodes.sort_by_key(|&(key, _)| key);
        let definitions = core::mem::take(definitions);
        Some(Locals {
            compiled: self.compiled,
            colours: definitions.iter().map(|_| OnceLock::new()).collect(),
            definitions,
            nodes,
        })
    }
}

/// A node's text, as bytes: a name, as a locals pass compares names.
fn name_of(text: &TextBuffer, node: Node<'_>) -> Box<[u8]> {
    text.bytes_in(node.byte_range())
        .flat_map(|piece| piece.iter().copied())
        .collect()
}

/// Begin the locals pass over an injected stretch's new `tree`, if its
/// language has a locals query, and carry it on until `deadline`: what it
/// found, if it is done -- otherwise the pass, for `work` to carry on.
fn begin_locals(
    tree: &Tree,
    compiled: &'static Compiled,
    text: &TextBuffer,
    deadline: Option<Instant>,
) -> (Option<Arc<Locals>>, Option<LocalsPass>) {
    let Some(query) = compiled.locals.as_ref() else {
        return (None, None);
    };
    let mut pass = LocalsPass::new(tree.clone(), compiled, query);
    match pass.advance(text, deadline) {
        Some(locals) => (Some(Arc::new(locals)), None),
        None => (None, Some(pass)),
    }
}

/// How one node's highlight captures settle, as tree-sitter's highlighter
/// settles them: of those that paint, the last pattern's wins -- save that
/// a local, a name found declared, is not painted by a pattern marked
/// `(#is-not? local)` unless it is the node's first. A capture of a higher
/// priority wins over the patterns after it, as Neovim has it. (A capture
/// of a name no kind answers to paints nothing and takes no part: see the
/// module docs.)
#[derive(Clone, Copy, Debug, Default)]
struct Settling {
    /// Whether the node is a local.
    local: bool,
    /// Whether a capture of it has come.
    seen: bool,
    /// What the winning capture that paints said, so far.
    paint: Option<Colour>,
    /// That capture's priority.
    priority: u16,
}

impl Settling {
    /// The node's next capture: what it paints, whether its pattern is
    /// marked `(#is-not? local)`, and its priority.
    fn take(&mut self, paint: Paint, non_local: bool, priority: u16) {
        let first = !self.seen;
        self.seen = true;
        if self.local && non_local && !first {
            return;
        }
        let colour = match paint {
            Paint::Kind(kind) => Some(kind),
            Paint::Plain => None,
            Paint::Skip => return,
        };
        // The first always: no priority is below 0, where it starts.
        if priority >= self.priority {
            self.paint = Some(colour);
            self.priority = priority;
        }
    }
}

/// A span a node paints, and what ranks it against the others over the
/// same bytes ([`flatten`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    range: Range<usize>,
    colour: Colour,
    /// Its capture's priority: `(#set! priority N)`, or
    /// [`DEFAULT_PRIORITY`].
    priority: u16,
    /// How deep in injections its language is: 0 for the text's own.
    depth: usize,
}

/// One node's highlight captures, while they come.
struct Painting {
    id: usize,
    range: Range<usize>,
    /// For a use of a declared name, its declaration's colour: what the node
    /// shows, whatever its captures say.
    uses: Option<Colour>,
    settling: Settling,
}

impl Painting {
    /// The span the node paints, if it paints one, `depth` deep in
    /// injections.
    fn span(self, depth: usize) -> Option<Span> {
        let Self {
            range,
            uses,
            settling,
            ..
        } = self;
        uses.or(settling.paint).map(|colour| Span {
            range,
            colour,
            priority: if settling.paint.is_some() {
                settling.priority
            } else {
                DEFAULT_PRIORITY
            },
            depth,
        })
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
    /// Where the text's names are declared and used, for `tree` -- or for
    /// the tree before it, moved with every edit since, until the pass over
    /// this one is done.
    locals: Option<Locals>,
    /// The pass of the locals query over `tree`, while it is being made.
    pass: Option<LocalsPass>,
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
            locals: None,
            pass: None,
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
        self.locals = None;
        self.pass = None;
        self.parser.reset();
        self.injected.get_mut().forget();
        self.halted = false;
        self.stale = true;
        self.used = 0;
        self.abandoned = false;
    }

    fn edited(&mut self, text: &TextBuffer, splices: &[Splice]) {
        if let Some(tree) = self.tree.as_mut() {
            for splice in splices {
                tree.edit(&input_edit(splice));
            }
        }
        if let Some(locals) = self.locals.as_mut() {
            for splice in splices {
                locals.edit(splice);
            }
        }
        // A pass over the tree before these edits read the text before them.
        self.pass = None;
        // A parse stopped part-way was of the text before these edits: start
        // it again, from the moved tree. The injected stretches move with
        // the text, but for those the edits touched.
        if self.halted {
            self.parser.reset();
            self.halted = false;
        }
        let cache = self.injected.get_mut();
        for splice in splices {
            cache.edit(splice);
        }
        cache.revision = text.revision();
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
                    self.pass = self
                        .compiled
                        .locals
                        .as_ref()
                        .map(|query| LocalsPass::new(tree.clone(), self.compiled, query));
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
        let mut left = false;
        if let Some(pass) = self.pass.as_mut() {
            match pass.advance(text, deadline) {
                Some(locals) => {
                    self.locals = Some(locals);
                    self.pass = None;
                }
                None => left = true,
            }
        }
        self.carry_on_injections(text, deadline) || left
    }

    fn has_work(&self) -> bool {
        // Borrowed only while drawing, which does not ask this.
        self.stale || self.pass.is_some() || self.injected.try_borrow().is_ok_and(|c| c.pending())
    }

    /// Paired by the tree: a bracket is a token of its own, and its partner
    /// the matching token among its node's children -- so a bracket in a
    /// string or a comment, part of a token that is not one, is none, and
    /// one in code pairs over those. A language injected there pairs its
    /// own. Where the parse is behind the text, or the partner is not where
    /// the tree would have it (broken code), it cannot say.
    fn brackets(&self, text: &TextBuffer, offset: usize) -> Brackets {
        let Some(tree) = self.tree.as_ref() else {
            return Brackets::Unknown;
        };
        if self.is_stale() {
            return Brackets::Unknown;
        }
        let candidates = [
            text.char_at(offset).map(|c| (offset, c)),
            text.chars_rev(offset).next(),
        ];
        let mut unknown = false;
        for (at, c) in candidates.into_iter().flatten() {
            if partner_of(c).is_none() {
                continue;
            }
            match self.bracket_in(self.compiled, tree, text, (at, c), 0) {
                pair @ Brackets::Pair(..) => return pair,
                Brackets::Unknown => unknown = true,
                Brackets::Unpaired => {}
            }
        }
        if unknown {
            Brackets::Unknown
        } else {
            Brackets::Unpaired
        }
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
            (tree, self.locals.as_ref()),
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
    /// those of each stretch it injects, each cut to the text the stretch
    /// was given: an injected language colours only what it read, and a
    /// hole in that -- a diff line's marker, a template's `${...}` -- keeps
    /// its host's colours. `locals` is where the tree's names are declared
    /// and used, if its language says; `depth` is how deep in injections
    /// this is, and `deadline` when drawing stops parsing the stretches it
    /// finds (see [`DRAW_BUDGET`]).
    fn collect(
        &self,
        compiled: &'static Compiled,
        (tree, locals): (&Tree, Option<&Locals>),
        text: &TextBuffer,
        range: Range<usize>,
        (depth, deadline): (usize, Option<Instant>),
        found: &mut Vec<Span>,
    ) {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures =
            cursor.captures(&compiled.highlights, tree.root_node(), BufferText(text));
        // A node's captures come one after another, by pattern, and settle
        // together (`Settling`). (A capture of another node between them,
        // starting where it does, parts them -- then each is a span of its
        // own, stacked in that order, as tree-sitter's are.)
        let mut painting: Option<Painting> = None;
        while let Some((m, index)) = captures.next() {
            let Some(capture) = m.captures.get(*index) else {
                continue;
            };
            let node = capture.node;
            if painting.as_ref().is_none_or(|p| p.id != node.id()) {
                found.extend(painting.take().and_then(|p| p.span(depth)));
                let (local, uses) = locals.map_or((false, None), |l| l.of(node, tree, text));
                painting = Some(Painting {
                    id: node.id(),
                    range: node.byte_range(),
                    uses,
                    settling: Settling {
                        local,
                        ..Settling::default()
                    },
                });
            }
            if let Some(p) = painting.as_mut() {
                p.settling.take(
                    compiled.paint(capture.index),
                    compiled.is_non_local(m.pattern_index),
                    compiled.priority(m.pattern_index, capture.index),
                );
            }
        }
        found.extend(painting.take().and_then(|p| p.span(depth)));
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
            let Some((sub, sub_locals)) =
                self.injected_tree(language, inner, &ranges, text, deadline)
            else {
                continue;
            };
            let mut spans = Vec::new();
            self.collect(
                inner,
                (&sub, sub_locals.as_deref()),
                text,
                within,
                (depth.saturating_add(1), deadline),
                &mut spans,
            );
            clip(spans, &ranges, found);
        }
    }

    /// The tree of `ranges` of `text` in `language` -- whose queries are
    /// `compiled` -- if it has one yet, and where its names are declared and
    /// used, once that is known: parsed and indexed once for each revision
    /// of the text -- begun here, until `deadline`, and carried on by `work`
    /// if not finished.
    fn injected_tree(
        &self,
        language: &'static Language,
        compiled: &'static Compiled,
        ranges: &[tree_sitter::Range],
        text: &TextBuffer,
        deadline: Option<Instant>,
    ) -> Option<(Tree, Option<Arc<Locals>>)> {
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
            Some(Stretch::Done { tree, locals, .. }) => {
                return Some((tree.clone(), locals.clone()));
            }
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
                let (locals, pass) = begin_locals(&tree, compiled, text, deadline);
                stretches.insert(
                    key,
                    Stretch::Done {
                        tree: tree.clone(),
                        locals: locals.clone(),
                        pass,
                        compiled,
                    },
                );
                Some((tree, locals))
            }
            Slice::Late => {
                stretches.insert(
                    key,
                    Stretch::Pending {
                        parser,
                        used,
                        limit,
                        compiled,
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

    /// The bracket `c` at `at` as `tree`, in `compiled`'s language, reads
    /// it -- or as the stretch injected over it does, `depth` injections
    /// down, when that stretch has been parsed.
    fn bracket_in(
        &self,
        compiled: &'static Compiled,
        tree: &Tree,
        text: &TextBuffer,
        (at, c): (usize, char),
        depth: usize,
    ) -> Brackets {
        let end = at.saturating_add(c.len_utf8());
        if let Some(injections) = compiled
            .injections
            .as_ref()
            .filter(|_| depth < MAX_INJECTION_DEPTH)
        {
            for (language, ranges) in injections_in(injections, tree, text, at..end) {
                if !ranges
                    .iter()
                    .any(|r| r.start_byte <= at && end <= r.end_byte)
                {
                    continue;
                }
                let key: StretchKey = (
                    language.index,
                    ranges.iter().map(|r| (r.start_byte, r.end_byte)).collect(),
                );
                let found = self.injected.try_borrow().ok().and_then(|cache| {
                    if cache.revision != text.revision() {
                        return None;
                    }
                    match cache.stretches.get(&key) {
                        Some(Stretch::Done { tree, compiled, .. }) => {
                            Some((tree.clone(), *compiled))
                        }
                        _ => None,
                    }
                });
                return match found {
                    Some((sub, inner)) => {
                        self.bracket_in(inner, &sub, text, (at, c), depth.saturating_add(1))
                    }
                    None => Brackets::Unknown,
                };
            }
        }
        let Some(node) = tree.root_node().descendant_for_byte_range(at, end) else {
            return Brackets::Unknown;
        };
        // Part of a token that is not a bracket -- a string's text, a
        // comment, a character -- it is none.
        if node.is_named() || node.child_count() > 0 {
            return Brackets::Unpaired;
        }
        let Some(partner) = partner_of(c) else {
            return Brackets::Unpaired;
        };
        let token = |n: Node<'_>| -> Option<String> {
            (!n.is_named() && n.child_count() == 0)
                .then(|| text.slice(n.byte_range()).ok())
                .flatten()
        };
        // The partner is the matching token among the same node's children:
        // what is between them sits in children of its own.
        let opens = matches!(c, '(' | '[' | '{');
        let mut sibling = if opens {
            node.next_sibling()
        } else {
            node.prev_sibling()
        };
        while let Some(n) = sibling {
            if let Some(t) = token(n) {
                if opens && t.starts_with(partner) {
                    return Brackets::Pair(at, n.start_byte());
                }
                if !opens && t.ends_with(partner) {
                    return Brackets::Pair(at, n.end_byte().saturating_sub(partner.len_utf8()));
                }
            }
            sibling = if opens {
                n.next_sibling()
            } else {
                n.prev_sibling()
            };
        }
        Brackets::Unknown
    }

    /// Carry on the injected stretches drawing began and did not finish --
    /// their parses and their locals passes -- each until it is done or
    /// `deadline` passes: whether any is left.
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
            let late = deadline.is_some_and(|d| Instant::now() >= d);
            match stretch {
                Stretch::Done {
                    locals,
                    pass: pass @ Some(_),
                    ..
                } => {
                    if late {
                        left = true;
                        continue;
                    }
                    if let Some(found) = pass.as_mut().and_then(|p| p.advance(text, deadline)) {
                        *locals = Some(Arc::new(found));
                        *pass = None;
                    } else {
                        left = true;
                    }
                }
                Stretch::Pending {
                    parser,
                    used,
                    limit,
                    compiled,
                } => {
                    if late {
                        left = true;
                        continue;
                    }
                    let compiled = *compiled;
                    let next = match slice(parser, text, None, deadline, used, *limit) {
                        Slice::Done(tree) => {
                            let (locals, pass) = begin_locals(&tree, compiled, text, deadline);
                            left |= pass.is_some();
                            Stretch::Done {
                                tree,
                                locals,
                                pass,
                                compiled,
                            }
                        }
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
                Stretch::Done { pass: None, .. } | Stretch::Failed => {}
            }
        }
        left
    }
}

/// The stretches `injections` finds in `tree` over `range`: each with its
/// language and the ranges its text is in. A match with
/// `injection.combined` is one document with every other match of its
/// pattern and language (among those in `range`). A stretch's language is
/// named -- `injection.language`, set or captured -- or is the language of a
/// file a capture names (`injection.filename`: a diff's file); a
/// `#offset!` moves its ranges' ends, as Neovim's does, whose directive it
/// is.
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
        let mut language: Option<&'static Language> = setting("injection.language")
            .and_then(|p| p.value.as_deref())
            .and_then(Language::for_injection);
        let include_children = setting("injection.include-children").is_some();
        let is_combined = setting("injection.combined").is_some();
        let offset = content_offset(&injections.query, m.pattern_index, injections.content);
        let mut ranges = Vec::new();
        for capture in m.captures {
            let named = || text.slice(capture.node.byte_range()).ok();
            if Some(capture.index) == injections.language {
                language = named().as_deref().and_then(Language::for_injection);
            } else if Some(capture.index) == injections.filename {
                language = named().as_deref().and_then(language_of_file);
            } else if Some(capture.index) == injections.content {
                let first = ranges.len();
                content_ranges(capture.node, include_children, &mut ranges);
                if let Some(offset) = offset {
                    let moved: Vec<tree_sitter::Range> = ranges
                        .drain(first..)
                        .filter_map(|r| offset_range(text, &r, offset))
                        .collect();
                    ranges.extend(moved);
                }
            }
        }
        let Some(language) = language else {
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

/// The bracket that pairs with `c`, if `c` is one.
fn partner_of(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        ')' => Some('('),
        '[' => Some(']'),
        ']' => Some('['),
        '{' => Some('}'),
        '}' => Some('{'),
        _ => None,
    }
}

/// A pattern's `(#offset! @injection.content start-row start-column
/// end-row end-column)`: how far to move its content's ends.
fn content_offset(
    query: &tree_sitter::Query,
    pattern: usize,
    content: Option<u32>,
) -> Option<[i64; 4]> {
    query.general_predicates(pattern).iter().find_map(|p| {
        if &*p.operator != "offset!" {
            return None;
        }
        let (tree_sitter::QueryPredicateArg::Capture(capture), numbers) = p.args.split_first()?
        else {
            return None;
        };
        if Some(*capture) != content {
            return None;
        }
        let mut offset = [0i64; 4];
        for (slot, arg) in offset.iter_mut().zip(numbers) {
            let tree_sitter::QueryPredicateArg::String(n) = arg else {
                return None;
            };
            *slot = n.parse().ok()?;
        }
        Some(offset)
    })
}

/// The language of the file a diff's header names -- `b/src/main.rs` --
/// by its name: the name alone where a tab ends it, as one does before
/// `diff -u`'s timestamp (and git's after a name with a space in it), and
/// out of the quotes git puts round a name with an odd character in it.
/// (Neovim, whose query this is, reads the header's whole text, so misses a
/// timestamped one.)
fn language_of_file(header: &str) -> Option<&'static Language> {
    let name = header.split_once('\t').map_or(header, |(name, _)| name);
    let name = name
        .strip_prefix('"')
        .and_then(|n| n.strip_suffix('"'))
        .unwrap_or(name);
    Language::for_file(std::path::Path::new(name))
}

/// `range` moved by an `#offset!`'s rows and columns ([`moved`]), or none
/// if its start would pass its end -- where Neovim leaves the capture
/// whole, a text too short to trim having nothing in it to colour.
fn offset_range(
    text: &TextBuffer,
    range: &tree_sitter::Range,
    [start_row, start_column, end_row, end_column]: [i64; 4],
) -> Option<tree_sitter::Range> {
    let at = |byte| -> Option<Point> {
        let (row, column) = text.point(byte).ok()?;
        Some(Point { row, column })
    };
    let start_byte = moved(
        text,
        (range.start_byte, range.start_point),
        start_row,
        start_column,
    )?;
    let end_byte = moved(text, (range.end_byte, range.end_point), end_row, end_column)?;
    (start_byte < end_byte).then_some(tree_sitter::Range {
        start_byte,
        end_byte,
        start_point: at(start_byte)?,
        end_point: at(end_byte)?,
    })
}

/// The offset `rows` rows and `columns` columns from `byte`, which is at
/// `point`: on the row `rows` away, at the point's column, then `columns`
/// on -- back, if negative -- as Neovim, whose directive `#offset!` is,
/// moves one: a column past a line's end goes on into the next line, the
/// end of the line one column whichever it is, `\n` or `\r\n`. So the
/// diff query's `0 1 0 1` takes in a line's end, all of it, and nothing of
/// the next line. A column inside a character is taken to the character's
/// far side; the text's ends hold it. `None` past the text's last row.
fn moved(
    text: &TextBuffer,
    (byte, point): (usize, Point),
    rows: i64,
    columns: i64,
) -> Option<usize> {
    let (mut at, columns) = if rows == 0 {
        (byte, columns)
    } else {
        let row = usize::try_from(i64::try_from(point.row).ok()?.checked_add(rows)?).ok()?;
        let column = i64::try_from(point.column).ok()?.checked_add(columns)?;
        (text.line_start(row)?, column)
    };
    let mut left = usize::try_from(columns.unsigned_abs()).unwrap_or(usize::MAX);
    if columns > 0 {
        let mut chars = text.chars(at).peekable();
        while left > 0 {
            let Some((from, c)) = chars.next() else {
                break;
            };
            at = from.saturating_add(c.len_utf8());
            if c == '\r' && chars.next_if(|&(_, next)| next == '\n').is_some() {
                at = at.saturating_add(1);
                left = left.saturating_sub(1);
            } else {
                left = left.saturating_sub(c.len_utf8());
            }
        }
    } else {
        let mut chars = text.chars_rev(at).peekable();
        while left > 0 {
            let Some((from, c)) = chars.next() else {
                break;
            };
            at = from;
            if c == '\n'
                && let Some((from, _)) = chars.next_if(|&(_, before)| before == '\r')
            {
                at = from;
                left = left.saturating_sub(1);
            } else {
                left = left.saturating_sub(c.len_utf8());
            }
        }
    }
    Some(at)
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

/// `spans`, an injected stretch's, cut to its `ranges` -- in order and
/// apart, as the parser was given them -- into `out`: a span over a hole
/// in the stretch, the text between two of its ranges, colours only the
/// ranges on either side of it.
fn clip(spans: Vec<Span>, ranges: &[tree_sitter::Range], out: &mut Vec<Span>) {
    for span in spans {
        let first = ranges.partition_point(|r| r.end_byte <= span.range.start);
        for r in ranges.get(first..).unwrap_or_default() {
            if r.start_byte >= span.range.end {
                break;
            }
            let (start, end) = (
                span.range.start.max(r.start_byte),
                span.range.end.min(r.end_byte),
            );
            if start < end {
                out.push(Span {
                    range: start..end,
                    ..span.clone()
                });
            }
        }
    }
}

/// Stacked spans made flat -- cut to `within`. Over each byte, of the spans
/// over it, the one that shows is the one of the highest priority; of
/// those, the one deepest in injections -- an injected language's colours
/// over its host's, in the text it was given ([`clip`]); and of those, the
/// one that started last, of spans that start together the one that came
/// last -- the host's by pattern, then each injected language's. So a node
/// inside another wins where it is, and of two spans that start together
/// the later pattern's is on top over its whole length, whether it is the
/// shorter or the longer: TOML's `(pair (bare_key)) @property` colours a
/// key over `(bare_key) @type` that way. A span stays hidden under one
/// above it until that one closes. Within one language and priority this
/// is tree-sitter's highlighter's stack, event for event; the priority and
/// the depth are Neovim's. A span of no kind paints plainly: nothing is
/// emitted for it, and what it covers shows the text's own ink.
fn flatten(mut spans: Vec<Span>, within: &Range<usize>) -> Vec<HighlightSpan> {
    // Stable, so spans that start together keep the order they came in.
    spans.sort_by_key(|s| s.range.start);
    // Where each span opens and closes: (offset, opens, span).
    let mut edges: Vec<(usize, bool, usize)> = Vec::with_capacity(spans.len().saturating_mul(2));
    for (i, s) in spans.iter().enumerate() {
        if s.range.start < s.range.end {
            edges.push((s.range.start, true, i));
            edges.push((s.range.end, false, i));
        }
    }
    edges.sort_unstable_by_key(|&(at, opens, _)| (at, opens));
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
    // The spans open between one edge and the next, ranked: the last shows.
    let mut open: std::collections::BTreeSet<(u16, usize, usize)> =
        std::collections::BTreeSet::new();
    let mut from = 0usize;
    for group in edges.chunk_by(|a, b| a.0 == b.0) {
        let Some(&(at, _, _)) = group.first() else {
            continue;
        };
        if let Some(top) = open.last().and_then(|&(_, _, i)| spans.get(i)) {
            emit(from, at, top.colour);
        }
        for &(_, opens, i) in group {
            let Some(s) = spans.get(i) else {
                continue;
            };
            if opens {
                open.insert((s.priority, s.depth, i));
            } else {
                open.remove(&(s.priority, s.depth, i));
            }
        }
        from = at;
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

    /// `spans` -- each of the default priority, the text's own -- made
    /// flat.
    fn flat_of(spans: Vec<(Range<usize>, Colour)>, within: &Range<usize>) -> Vec<HighlightSpan> {
        flatten(
            spans
                .into_iter()
                .map(|(range, colour)| Span {
                    range,
                    colour,
                    priority: DEFAULT_PRIORITY,
                    depth: 0,
                })
                .collect(),
            within,
        )
    }

    /// The highlights of all of `text` in `language`, settled.
    fn highlighted(text: &str, language: &str) -> Vec<HighlightSpan> {
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named(language).unwrap().highlighter().unwrap();
        h.reset(&buffer);
        settled(&mut h, &buffer)
    }

    /// The colour over the first byte of the `nth` (from 0) `needle` in
    /// `text`, as `spans` paint it.
    fn colour_at(
        spans: &[HighlightSpan],
        text: &str,
        needle: &str,
        nth: usize,
    ) -> Option<Highlight> {
        let at = text.match_indices(needle).nth(nth).unwrap().0;
        spans
            .iter()
            .find(|s| s.range.contains(&at))
            .map(|s| s.highlight)
    }

    /// JavaScript's tree of `text`, and its compiled queries.
    fn javascript(text: &str) -> (Tree, &'static Compiled) {
        let language = Language::named("javascript").unwrap();
        let mut parser = Parser::new();
        parser.set_language(&language.ts_language()).unwrap();
        (
            parser.parse(text, None).unwrap(),
            language.compiled().unwrap(),
        )
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
        let flat = flat_of(
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
        let same = flat_of(vec![(0..4, Some(Str)), (0..4, Some(Keyword))], &(0..4));
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].highlight, Keyword);
        assert!(flat_of(Vec::new(), &(0..9)).is_empty());
        // Plain inside a string: the string on either side, nothing within;
        // and a keyword inside the plain part is still a keyword.
        let plain = flat_of(
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
                flat_of(spans, &(0..30))
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

    /// **Bash is coloured as its query says**: a keyword, a command's name,
    /// a string, a comment, a variable's name, an option.
    #[test]
    fn bash_is_coloured_as_its_query_says() {
        let got = spans(
            "# note\nif true; then\n  NAME=x\n  echo \"hi\" -n\nfi\n",
            "bash",
        );
        let at = |s: &str| got.iter().find(|(t, _)| t == s).map(|(_, h)| *h);
        for (s, want) in [
            ("# note", Highlight::Comment),
            ("if", Highlight::Keyword),
            ("then", Highlight::Keyword),
            ("NAME", Highlight::Property),
            ("echo", Highlight::Function),
            ("\"hi\"", Highlight::String),
            ("-n", Highlight::Constant),
            ("fi", Highlight::Keyword),
        ] {
            assert_eq!(at(s), Some(want), "{s}: {got:?}");
        }
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

    /// **A use of a declared name is coloured as its declaration is**: a
    /// parameter wherever its function uses it, and no further out; a
    /// function held in a constant, wherever the constant is used. And a
    /// name found declared is no longer what `(#is-not? local)` takes it
    /// for: `module` is a variable in the block that declares one, and
    /// Node's `module` outside it.
    #[test]
    fn a_use_is_coloured_as_its_declaration() {
        let text = "function f(alpha) {\n  alpha;\n  { let module = 1; module; }\n  module;\n}\nalpha;\nconst g = () => 1;\ng;\n";
        let spans = highlighted(text, "javascript");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("alpha", 0), Some(Highlight::Parameter));
        assert_eq!(at("alpha", 1), Some(Highlight::Parameter), "{spans:?}");
        assert_eq!(at("alpha", 2), Some(Highlight::Variable));
        assert_eq!(at("module", 0), Some(Highlight::Variable));
        assert_eq!(at("module", 1), Some(Highlight::Variable));
        assert_eq!(at("module", 2), Some(Highlight::Builtin));
        assert_eq!(at("g ", 0), Some(Highlight::Function));
        assert_eq!(at("g;", 0), Some(Highlight::Function), "{spans:?}");
    }

    /// **A use is of the last declaration of its name before it**: `var`
    /// may declare a name twice, and a use takes the second's colour -- a
    /// use before a declaration is not of it.
    #[test]
    fn a_use_is_of_the_last_declaration_before_it() {
        let text = "zeta;\nvar zeta = 1;\nzeta;\nvar zeta = () => 2;\nzeta;\n";
        let spans = highlighted(text, "javascript");
        let at = |nth| colour_at(&spans, text, "zeta", nth);
        assert_eq!(at(0), Some(Highlight::Variable), "before any");
        assert_eq!(at(2), Some(Highlight::Variable), "after the first");
        assert_eq!(at(3), Some(Highlight::Function));
        assert_eq!(at(4), Some(Highlight::Function), "after the second");
    }

    /// **A locals pass stopped and resumed finds what one pass does** --
    /// the same declarations, the same uses of the same ones -- however
    /// many slices it took.
    #[test]
    fn a_locals_pass_in_slices_finds_what_one_pass_does() {
        let mut text = String::new();
        for i in 0..300 {
            text.push_str(&format!(
                "function f{i}(a, {{ b }}, [c]) {{\n  let d = a + b;\n  {{ let a = c; a; d; }}\n  return (e) => a + e + d + x{i};\n}}\nlet x{i} = f{i};\n"
            ));
        }
        let buffer = TextBuffer::from_text(&text);
        let (tree, compiled) = javascript(&text);
        let query = compiled.locals.as_ref().unwrap();
        let whole = LocalsPass::new(tree.clone(), compiled, query)
            .advance(&buffer, None)
            .unwrap();
        let mut pass = LocalsPass::new(tree, compiled, query);
        let past = Instant::now();
        let mut slices = 1;
        let sliced = loop {
            if let Some(found) = pass.advance(&buffer, Some(past)) {
                break found;
            }
            slices += 1;
            assert!(slices < 1_000_000, "never finished");
        };
        assert!(
            slices > 10,
            "{slices} slices: the deadline was not honoured"
        );
        assert_eq!(sliced.definitions, whole.definitions);
        assert_eq!(sliced.nodes, whole.nodes);
        // A resumed slice is handed again the captures of the nodes it
        // starts inside -- the scopes open there -- which it must not take
        // twice: a scope opened twice, one that does not inherit, hides
        // from a use what was declared before the slice began.
        let closed = no_inheriting(&tree_of(&text));
        let whole_closed = LocalsPass::new(tree_of(&text), compiled, closed)
            .advance(&buffer, None)
            .unwrap();
        let mut pass = LocalsPass::new(tree_of(&text), compiled, closed);
        let past = Instant::now();
        let sliced_closed = loop {
            if let Some(found) = pass.advance(&buffer, Some(past)) {
                break found;
            }
        };
        assert_eq!(sliced_closed.nodes, whole_closed.nodes);
        assert!(
            whole_closed
                .nodes
                .iter()
                .any(|(_, l)| matches!(l, Local::Uses(_)))
        );
        let uses = whole
            .nodes
            .iter()
            .filter(|(_, l)| matches!(l, Local::Uses(_)))
            .count();
        // Each function's: the parameter `a` twice, `c`, the inner `a`, `d`
        // twice, the arrow's `e`. (`{ b }` declares nothing to JavaScript's
        // query, and `x{i}` is declared only after its use.)
        assert_eq!(uses, 300 * 7, "{:?}", &whole.nodes[..20]);
    }

    /// JavaScript's tree of `text`.
    fn tree_of(text: &str) -> Tree {
        javascript(text).0
    }

    /// A locals query for JavaScript whose blocks do not inherit, and whose
    /// declarations give their names values: what JavaScript's own does not
    /// try.
    fn no_inheriting(tree: &Tree) -> &'static LocalsQuery {
        let query = tree_sitter::Query::new(
            &tree.language(),
            "((statement_block) @local.scope (#set! local.scope-inherits false))
             (variable_declarator
               name: (identifier) @local.definition
               value: (_) @local.definition-value)
             (identifier) @local.reference",
        )
        .unwrap();
        Box::leak(Box::new(LocalsQuery {
            scope: query.capture_index_for_name("local.scope"),
            definition: query.capture_index_for_name("local.definition"),
            definition_value: query.capture_index_for_name("local.definition-value"),
            reference: query.capture_index_for_name("local.reference"),
            query,
        }))
    }

    /// **A scope that does not inherit keeps out the names around it, and
    /// a declaration's own value is not a use of it** --
    /// `local.scope-inherits` and `local.definition-value`, which
    /// JavaScript's query does not use; tried here with one that does.
    #[test]
    fn a_scope_that_does_not_inherit_and_a_declarations_value_are_honoured() {
        let text = "let x = 1;\nx;\n{ x; }\nlet y = y;\ny;\n";
        let (tree, compiled) = javascript(text);
        let query = no_inheriting(&tree);
        let buffer = TextBuffer::from_text(text);
        let found = LocalsPass::new(tree, compiled, query)
            .advance(&buffer, None)
            .unwrap();
        let at = |needle: &str, nth: usize| {
            let at = text.match_indices(needle).nth(nth).unwrap().0;
            found
                .nodes
                .iter()
                .find(|(k, _)| k.start == at)
                .map(|&(_, l)| l)
        };
        assert_eq!(at("x", 0), Some(Local::Declares(0)));
        assert_eq!(at("x", 1), Some(Local::Uses(0)));
        assert_eq!(at("x", 2), None, "inside a scope that does not inherit");
        assert_eq!(at("y", 0), Some(Local::Declares(1)));
        assert_eq!(at("y", 1), None, "inside its own declaration's value");
        assert_eq!(at("y", 2), Some(Local::Uses(1)));
    }

    /// **An edit moves the index with the text, at once**: before the pass
    /// over the edited text, a use after the edit keeps its declaration's
    /// colour where it has moved to, and a use the edit touched is a use of
    /// nothing until the pass says what it is.
    #[test]
    fn an_edit_moves_the_locals_with_the_text() {
        let original = "function f(alpha) {\n  alpha;\n  alpha;\n}\n";
        let mut buffer = TextBuffer::from_text(original);
        let mut h = Language::named("javascript")
            .unwrap()
            .highlighter()
            .unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let second = original.match_indices("alpha").nth(2).unwrap().0;
        let _ = buffer.take_changes();
        // A line before everything; a letter on the end of the second use.
        buffer.insert(second + "alpha".len(), "z").unwrap();
        buffer.insert(0, "// x\n").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        let text = format!(
            "// x\n{}z{}",
            &original[..second + 5],
            &original[second + 5..]
        );
        let colour = |h: &SyntaxHighlighter, nth| {
            colour_at(&h.highlights(&buffer, 0..buffer.len()), &text, "alpha", nth)
        };
        assert!(h.has_work());
        assert_eq!(colour(&h, 1), Some(Highlight::Parameter), "moved");
        assert_eq!(colour(&h, 2), Some(Highlight::Variable), "touched");
        while h.work(&buffer, Duration::from_secs(5)) {}
        assert_eq!(colour(&h, 1), Some(Highlight::Parameter));
        assert_eq!(colour(&h, 2), Some(Highlight::Variable), "`alphaz`");
    }

    /// **An edit against a name's end forgets it until the pass**: deleting
    /// the blank between `alpha` and `b` runs them into one name, `alphab`,
    /// which the moved tree still shows as `alpha` until the parse -- and
    /// which is not the parameter, so it is not coloured as one meanwhile.
    #[test]
    fn an_edit_against_a_names_end_forgets_it() {
        let original = "function f(alpha) {\n  alpha b;\n}\n";
        let mut buffer = TextBuffer::from_text(original);
        let mut h = Language::named("javascript")
            .unwrap()
            .highlighter()
            .unwrap();
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let blank = original.find(" b;").unwrap();
        let spans = h.highlights(&buffer, 0..buffer.len());
        assert_eq!(
            colour_at(&spans, original, "alpha", 1),
            Some(Highlight::Parameter),
            "before the edit"
        );
        let _ = buffer.take_changes();
        buffer.delete(blank..blank + 1).unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        let text = "function f(alpha) {\n  alphab;\n}\n";
        let before = h.highlights(&buffer, 0..buffer.len());
        assert_eq!(
            colour_at(&before, text, "alpha", 1),
            Some(Highlight::Variable)
        );
        while h.work(&buffer, Duration::from_secs(5)) {}
        let after = h.highlights(&buffer, 0..buffer.len());
        assert_eq!(
            colour_at(&after, text, "alphab", 0),
            Some(Highlight::Variable)
        );
    }

    /// **An injected stretch is indexed as it is parsed**: a JavaScript
    /// fence in Markdown colours its parameter's use as the parameter.
    #[test]
    fn an_injected_stretch_has_its_own_locals() {
        let text = "# Code\n\n```js\nfunction f(alpha) {\n  return alpha;\n}\n```\n";
        let spans = highlighted(text, "markdown");
        assert_eq!(
            colour_at(&spans, text, "alpha", 1),
            Some(Highlight::Parameter),
            "{spans:?}"
        );
    }

    /// **A highlighter may be handed to another thread**: nothing in it --
    /// the locals index and its colours included -- is tied to the one that
    /// made it.
    #[test]
    fn a_highlighter_may_move_between_threads() {
        fn send<T: Send>() {}
        send::<SyntaxHighlighter>();
        send::<Locals>();
    }

    /// JavaScript's highlighter, with `highlights` and `locals` for its
    /// queries in place of its own: for what its own do not try.
    fn with_queries(highlights: &str, locals: &str) -> SyntaxHighlighter {
        with_queries_for("javascript", highlights, locals)
    }

    /// `language`'s highlighter, with `highlights` and `locals` for its
    /// queries in place of its own.
    fn with_queries_for(language: &str, highlights: &str, locals: &str) -> SyntaxHighlighter {
        let language = Language::named(language).unwrap();
        let grammar = language.ts_language();
        let highlights_source = highlights;
        let highlights = tree_sitter::Query::new(&grammar, highlights).unwrap();
        let locals = tree_sitter::Query::new(&grammar, locals).unwrap();
        let compiled: &'static Compiled = Box::leak(Box::new(Compiled::new(
            highlights,
            highlights_source,
            None,
            Some(LocalsQuery {
                scope: locals.capture_index_for_name("local.scope"),
                definition: locals.capture_index_for_name("local.definition"),
                definition_value: locals.capture_index_for_name("local.definition-value"),
                reference: locals.capture_index_for_name("local.reference"),
                query: locals,
            }),
        )));
        let mut h = language.highlighter().unwrap();
        h.compiled = compiled;
        h
    }

    /// **A local's first capture paints, whatever its pattern says of
    /// locals; only the later ones marked `(#is-not? local)` leave it
    /// alone** -- as in tree-sitter's highlighter, which takes a node's
    /// first capture before it asks. A name not declared is painted by the
    /// last of them.
    #[test]
    fn a_locals_first_capture_paints_whatever_its_pattern_says() {
        let mut h = with_queries(
            "((identifier) @variable.builtin (#is-not? local))
             ((identifier) @constant (#is-not? local))",
            "(variable_declarator name: (identifier) @local.definition)
             (identifier) @local.reference",
        );
        let text = "let x = y;\n";
        let buffer = TextBuffer::from_text(text);
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        assert_eq!(colour_at(&spans, text, "x", 0), Some(Highlight::Builtin));
        assert_eq!(colour_at(&spans, text, "y", 0), Some(Highlight::Constant));
    }

    /// **A scope stays open for a node that starts just where it ends**, as
    /// it does in tree-sitter's highlighter, which closes a scope only for
    /// a node starting past its end: in `{ let a = 1; }{ a; }` the second
    /// block opens inside the first, and its `a` is the first's.
    #[test]
    fn a_scope_stays_open_for_a_node_starting_where_it_ends() {
        let text = "{ let a = 1; }{ a; }\n{ let b = 1; } { b; }\n";
        let (tree, compiled) = javascript(text);
        let query = compiled.locals.as_ref().unwrap();
        let buffer = TextBuffer::from_text(text);
        let found = LocalsPass::new(tree, compiled, query)
            .advance(&buffer, None)
            .unwrap();
        let at = |needle: &str, nth: usize| {
            let at = text.match_indices(needle).nth(nth).unwrap().0;
            found
                .nodes
                .iter()
                .find(|(k, _)| k.start == at)
                .map(|&(_, l)| l)
        };
        assert_eq!(at("a", 1), Some(Local::Uses(0)));
        assert_eq!(at("b", 1), None, "a blank between them closes the first");
    }

    /// **The view keeps working while a pass is unfinished**: a file too
    /// big to index in one slice is indexed over several, the highlighter
    /// saying it has work until the pass is done -- and the colours are
    /// then the whole file's.
    #[test]
    fn the_view_keeps_working_while_a_pass_is_unfinished() {
        let mut text = String::new();
        for i in 0..2000 {
            text.push_str(&format!(
                "function f{i}(alpha) {{\n  return alpha + {i};\n}}\n"
            ));
        }
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("javascript")
            .unwrap()
            .highlighter()
            .unwrap();
        h.reset(&buffer);
        let mut passing = 0;
        let mut rounds = 0;
        while h.work(&buffer, Duration::from_micros(300)) {
            rounds += 1;
            assert!(rounds < 1_000_000, "never finished");
            if h.pass.is_some() {
                passing += 1;
                assert!(h.has_work(), "a pass is unfinished");
            }
        }
        assert!(
            passing > 0,
            "the pass fit in one slice: the budget was not honoured"
        );
        assert!(h.pass.is_none() && h.locals.is_some() && !h.has_work());
        let last = text.match_indices("alpha").count() - 1;
        let spans = h.highlights(&buffer, 0..buffer.len());
        assert_eq!(
            colour_at(&spans, &text, "alpha", last),
            Some(Highlight::Parameter)
        );
    }

    /// **A declaration's colour found from near it is the one found from
    /// the root**: for every declaration in a file of functions, blocks,
    /// destructured parameters and arrow functions, the query started a
    /// pattern's depth above the node settles as the one started at the
    /// root does.
    #[test]
    fn a_colour_found_near_its_node_is_the_one_found_from_the_root() {
        let mut text = String::new();
        for i in 0..50 {
            text.push_str(&format!(
                "function f{i}(a, {{ b: c }}, [d], ...e) {{\n  let v = a + 1;\n  const g = () => v;\n  let h = function () {{}};\n  {{ var K_{i} = [c, d]; }}\n  module.x{i} = (y) => y;\n}}\n"
            ));
        }
        let buffer = TextBuffer::from_text(&text);
        let (tree, compiled) = javascript(&text);
        let found = LocalsPass::new(tree.clone(), compiled, compiled.locals.as_ref().unwrap())
            .advance(&buffer, None)
            .unwrap();
        let mut kinds = Vec::new();
        for (d, key) in found.definitions.iter().enumerate() {
            let key = key.unwrap();
            let near = found.colour(d, &tree, &buffer);
            let far = settle_node(compiled, tree.root_node(), key, &buffer);
            assert_eq!(near, far, "{:?}", &text[key.start..key.end]);
            if !kinds.contains(&near) {
                kinds.push(near);
            }
        }
        // Parameters, variables, functions and constants among them.
        assert!(kinds.len() >= 4, "{kinds:?}");
    }

    /// **An injected stretch's pass that does not fit a draw is carried on
    /// by `work`**, as its parse is, and its uses are coloured when it is
    /// done.
    #[test]
    fn a_stretchs_pass_is_carried_on_by_work() {
        let mut text = String::from("# Code\n\n```js\n");
        for i in 0..1500 {
            text.push_str(&format!("function f{i}(alpha) {{ return alpha + {i}; }}\n"));
        }
        text.push_str("```\n");
        let buffer = TextBuffer::from_text(&text);
        let mut h = Language::named("markdown").unwrap().highlighter().unwrap();
        h.draw_budget = Duration::ZERO;
        h.reset(&buffer);
        let mut carried = false;
        let mut rounds = 0;
        loop {
            let _ = h.highlights(&buffer, 0..buffer.len());
            carried |= h
                .injected
                .borrow()
                .stretches
                .values()
                .any(|s| matches!(s, Stretch::Done { pass: Some(_), .. }));
            if !h.has_work() {
                break;
            }
            while h.work(&buffer, Duration::from_micros(300)) {
                rounds += 1;
                assert!(rounds < 1_000_000, "never finished");
                carried |= h
                    .injected
                    .borrow()
                    .stretches
                    .values()
                    .any(|s| matches!(s, Stretch::Done { pass: Some(_), .. }));
            }
        }
        assert!(carried, "the stretch's pass never waited for work");
        let last = text.match_indices("alpha").count() - 1;
        assert_eq!(
            colour_at(
                &h.highlights(&buffer, 0..buffer.len()),
                &text,
                "alpha",
                last
            ),
            Some(Highlight::Parameter)
        );
    }

    /// **A node the query takes for a declaration and then for a scope is
    /// no declaration** -- tree-sitter's highlighter drops the declaration's
    /// colour when a later capture of its node opens a scope -- so neither
    /// it nor a use of its name is a local.
    #[test]
    fn a_declaration_its_node_then_opens_a_scope_for_is_no_local() {
        let mut h = with_queries(
            "(identifier) @variable
             ((identifier) @constant (#is-not? local))",
            "(variable_declarator name: (identifier) @local.definition)
             (variable_declarator name: (identifier) @local.scope)
             (identifier) @local.reference",
        );
        let text = "let x = 1;\nx;\nlet y = 2;\n";
        let buffer = TextBuffer::from_text(text);
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        assert_eq!(colour_at(&spans, text, "x", 0), Some(Highlight::Constant));
        assert_eq!(colour_at(&spans, text, "x", 1), Some(Highlight::Constant));
        // Without the scope pattern it would be a local: its first capture
        // paints it, the second leaves it alone.
        let mut plain = with_queries(
            "(identifier) @variable
             ((identifier) @constant (#is-not? local))",
            "(variable_declarator name: (identifier) @local.definition)
             (identifier) @local.reference",
        );
        plain.reset(&buffer);
        let spans = settled(&mut plain, &buffer);
        assert_eq!(colour_at(&spans, text, "x", 0), Some(Highlight::Variable));
        assert_eq!(colour_at(&spans, text, "x", 1), Some(Highlight::Variable));
    }

    /// **HTML is coloured with its scripts and styles in their own
    /// languages**: a `<script>`'s JavaScript -- its names by where each was
    /// declared too -- and a `<style>`'s CSS, among HTML's tags and
    /// attributes.
    #[test]
    fn html_is_coloured_with_its_scripts_and_styles_in_their_languages() {
        let text = "<p class=\"x\">Hi</p>\n<script>\nfunction f(alpha) { return alpha; }\n</script>\n<style>\nb { color: red; }\n</style>\n";
        let spans = highlighted(text, "html");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("p class", 0), Some(Highlight::Tag), "{spans:?}");
        assert_eq!(at("class", 0), Some(Highlight::Attribute));
        // The value, inside its quotes, which are the attribute's own.
        assert_eq!(at("x\"", 0), Some(Highlight::String));
        assert_eq!(at("script>", 0), Some(Highlight::Tag));
        assert_eq!(at("function", 0), Some(Highlight::Keyword));
        assert_eq!(at("alpha", 1), Some(Highlight::Parameter));
        assert_eq!(at("color", 0), Some(Highlight::Property));
    }

    /// **A JavaScript template tagged `html` is coloured as HTML**, the
    /// template's substitutions as JavaScript.
    #[test]
    fn a_template_tagged_html_is_coloured_as_html() {
        let text = "const t = html`<b class=\"x\">${name}</b>`;\n";
        let spans = highlighted(text, "javascript");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("b class", 0), Some(Highlight::Tag), "{spans:?}");
        assert_eq!(at("class", 0), Some(Highlight::Attribute));
        assert_eq!(at("name", 0), Some(Highlight::Variable));
    }

    /// **TypeScript's parameters are coloured as parameters, and a type's
    /// `<...>` as brackets** -- its query after JavaScript's, the specific
    /// after the general. In the order `tree-sitter.json` lists them,
    /// TypeScript's first, JavaScript's patterns win on the same nodes:
    /// every parameter a variable, and the `<` an operator (§1441).
    #[test]
    fn typescripts_query_goes_after_javascripts() {
        let text = "function f(alpha: Map<string, number>): number {\n  return alpha.size;\n}\n";
        let spans = highlighted(text, "typescript");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("alpha", 0), Some(Highlight::Parameter), "{spans:?}");
        assert_eq!(at("alpha", 1), Some(Highlight::Parameter), "a use");
        assert_eq!(at("<", 0), Some(Highlight::Punctuation));
        assert_eq!(at("Map", 0), Some(Highlight::Type));
        assert_eq!(at("string", 0), Some(Highlight::Type));
        assert_eq!(at("function", 0), Some(Highlight::Keyword));
        let upstream = format!(
            "{}\n{}",
            include_str!("../grammars/typescript/highlights.scm"),
            include_str!("../grammars/javascript/highlights.scm")
        );
        let mut h = with_queries_for("typescript", &upstream, crate::grammars::typescript::LOCALS);
        let buffer = TextBuffer::from_text(text);
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("alpha", 0), Some(Highlight::Variable));
        assert_eq!(at("<", 0), Some(Highlight::Operator));
    }

    /// **TSX is TypeScript with JSX's tags and attributes**, and its
    /// parameters are parameters too.
    #[test]
    fn tsx_is_coloured_with_jsx_and_types() {
        let text =
            "function App(props: Props) {\n  return <div className=\"x\">{props.name}</div>;\n}\n";
        let spans = highlighted(text, "tsx");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("div", 0), Some(Highlight::Tag), "{spans:?}");
        assert_eq!(at("className", 0), Some(Highlight::Attribute));
        assert_eq!(at("props", 0), Some(Highlight::Parameter));
        assert_eq!(at("props", 1), Some(Highlight::Parameter));
        assert_eq!(at("Props", 0), Some(Highlight::Type));
    }

    /// **A C++ raw string is coloured in the language its delimiter names**
    /// -- `R"js(...)js"` as JavaScript -- and C++ as C's query and its own
    /// say.
    #[test]
    fn a_cpp_raw_string_is_coloured_as_its_delimiter_says() {
        let text = "class Foo {\npublic:\n  const char *s = R\"js(let alpha = 1;)js\";\n};\n";
        let spans = highlighted(text, "c++");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("class", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("public", 0), Some(Highlight::Keyword));
        assert_eq!(at("let", 0), Some(Highlight::Keyword));
        assert_eq!(at("1;", 0), Some(Highlight::Number));
    }

    /// **Ada is coloured as its query says** -- which names its captures as
    /// Neovim does (`@keyword.function`, `@repeat`, `@include`), each
    /// painting its kind: keywords, a subprogram's and a package's names,
    /// strings, numbers, comments. (Its grammar ships no highlight tests.)
    #[test]
    fn ada_is_coloured_as_its_query_says() {
        let text = "-- The disk's driver.\nwith Ada.Text_IO;\npackage body Ahci is\n   procedure Reset (Port : in out Port_Type) is\n   begin\n      if Port.Busy then\n         raise Device_Error with \"busy\";\n      end if;\n      for I in 1 .. 10 loop\n         null;\n      end loop;\n   end Reset;\nend Ahci;\n";
        let spans = highlighted(text, "ada");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("-- The", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("with Ada", 0), Some(Highlight::Keyword));
        assert_eq!(at("Ada.Text_IO", 0), Some(Highlight::Module));
        assert_eq!(at("package", 0), Some(Highlight::Keyword));
        assert_eq!(at("Ahci", 0), Some(Highlight::Function));
        assert_eq!(at("Reset", 0), Some(Highlight::Function));
        assert_eq!(at("if Port", 0), Some(Highlight::Keyword));
        assert_eq!(at("raise", 0), Some(Highlight::Keyword));
        assert_eq!(at("\"busy\"", 0), Some(Highlight::String));
        assert_eq!(at("10", 0), Some(Highlight::Number));
        assert_eq!(at("loop", 0), Some(Highlight::Keyword));
    }

    /// **JavaScript's regular expressions and doc comments are coloured
    /// inside**, by the grammars its injection query names: a regex's
    /// escapes and operators; a JSDoc tag and its type over the comment,
    /// the rest of which stays a comment.
    #[test]
    fn regexes_and_doc_comments_are_coloured_inside() {
        let text = "/**\n * @param {string} name\n */\nfunction f(name) {\n  return /\\d+x?/g.test(name);\n}\n";
        let spans = highlighted(text, "javascript");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("/**", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("@param", 0), Some(Highlight::Keyword));
        assert_eq!(at("string", 0), Some(Highlight::Type));
        assert_eq!(at("\\d", 0), Some(Highlight::Escape));
        assert_eq!(at("+x", 0), Some(Highlight::Operator));
        assert_eq!(at("?/", 0), Some(Highlight::Operator));
    }

    /// The ranges of the stretches parsed and kept in `h`'s cache, sorted.
    fn kept_stretches(h: &SyntaxHighlighter) -> Vec<Vec<(usize, usize)>> {
        let mut kept: Vec<Vec<(usize, usize)>> = h
            .injected
            .borrow()
            .stretches
            .iter()
            .filter(|(_, s)| matches!(s, Stretch::Done { .. }))
            .map(|((_, ranges), _)| ranges.clone())
            .collect();
        kept.sort();
        kept
    }

    /// **An edit keeps the stretches it did not touch**: typing in one code
    /// fence forgets that fence's parse alone -- the other keeps its parse
    /// and its locals, moved with the text, and is coloured at once, the
    /// JavaScript parameter's use as the parameter, before any work.
    #[test]
    fn an_edit_keeps_the_stretches_it_did_not_touch() {
        let original =
            "```rust\nfn a() {}\n```\n\n```js\nfunction f(alpha) { return alpha; }\n```\n";
        let mut buffer = TextBuffer::from_text(original);
        let mut h = Language::named("markdown").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        let _ = settled(&mut h, &buffer);
        let before = kept_stretches(&h);
        assert_eq!(before.len(), 2, "{before:?}");
        let _ = buffer.take_changes();
        let at = original.find("a()").unwrap();
        buffer.insert(at, "x").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        // The Rust fence was typed in; the JavaScript one moved one byte on.
        let moved: Vec<(usize, usize)> = before[1].iter().map(|&(a, b)| (a + 1, b + 1)).collect();
        assert_eq!(kept_stretches(&h), [moved]);
        let text = buffer.text();
        h.draw_budget = Duration::ZERO;
        let spans = h.highlights(&buffer, 0..buffer.len());
        // Exactly where the words now are: a tree kept but not moved would
        // colour a byte early.
        let word = |needle: &str, nth: usize| {
            let at = text.match_indices(needle).nth(nth).unwrap().0;
            at..at + needle.len()
        };
        assert!(
            spans
                .iter()
                .any(|s| s.range == word("function", 0) && s.highlight == Highlight::Keyword),
            "{spans:?}"
        );
        assert!(
            spans
                .iter()
                .any(|s| s.range == word("alpha", 1) && s.highlight == Highlight::Parameter),
            "{spans:?}"
        );
    }

    /// **Go is coloured as its query says** -- its grammar ships no
    /// highlight tests -- keywords, a function's name and a call's, a type,
    /// a string, a number, a comment.
    #[test]
    fn go_is_coloured_as_its_query_says() {
        let text = "// Package main runs.\npackage main\n\nimport \"fmt\"\n\nfunc add(a int, b int) int {\n\treturn a + b + 42\n}\n\nfunc main() {\n\tfmt.Println(add(1, 2))\n}\n";
        let spans = highlighted(text, "go");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("// Package", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("package", 0), Some(Highlight::Keyword));
        assert_eq!(at("\"fmt\"", 0), Some(Highlight::String));
        assert_eq!(at("func add", 0), Some(Highlight::Keyword));
        assert_eq!(at("add(", 0), Some(Highlight::Function));
        assert_eq!(at("int,", 0), Some(Highlight::Type));
        assert_eq!(at("return", 0), Some(Highlight::Keyword));
        assert_eq!(at("42", 0), Some(Highlight::Number));
        assert_eq!(at("Println", 0), Some(Highlight::Function));
        // In the published order the general patterns come last, and win:
        // the function's name a variable, the method a property (§1442).
        let mut h = with_queries_for(
            "go",
            include_str!("../grammars/go/highlights.scm"),
            "(identifier) @local.reference",
        );
        let buffer = TextBuffer::from_text(text);
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("add(", 0), Some(Highlight::Variable));
        assert_eq!(at("Println", 0), Some(Highlight::Property));
    }

    /// **An INI file -- a desktop entry -- is coloured as its query says**:
    /// a section's name, a setting's name, a comment.
    #[test]
    fn ini_is_coloured_as_its_query_says() {
        let text = "# Files, the manager.\n[Desktop Entry]\nName=Files\nExec=files %U\n";
        let spans = highlighted(text, "ini");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("# Files", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("Desktop Entry", 0), Some(Highlight::Type));
        assert_eq!(at("Name", 0), Some(Highlight::Property));
        assert_eq!(at("=Files", 0), Some(Highlight::Operator));
    }

    /// **A diff's lines are coloured as a change's**: a `+` line inserted, a
    /// `-` line deleted, the files' header lines likewise, the hunk's
    /// location an attribute -- and a line both have, uncoloured.
    #[test]
    fn a_diff_is_coloured_as_its_query_says() {
        let text = "diff --git a/x.txt b/x.txt\n--- a/x.txt\n+++ b/x.txt\n@@ -1,2 +1,2 @@\n same\n-old\n+new\n";
        let spans = highlighted(text, "diff");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        // A line's marker is the line's colour -- its punctuation's
        // `(#set! priority 95)` puts it under the line's 100 -- and so is
        // the rest of the line.
        assert_eq!(at("-old", 0), Some(Highlight::Deleted), "{spans:?}");
        assert_eq!(at("old\n", 0), Some(Highlight::Deleted));
        assert_eq!(at("+new", 0), Some(Highlight::Inserted));
        assert_eq!(at("new\n", 0), Some(Highlight::Inserted));
        // The files' header lines: their markers and the blank a change's
        // colour, their names paths.
        assert_eq!(at("--- a", 0), Some(Highlight::Deleted));
        assert_eq!(at(" a/x.txt\n+++", 0), Some(Highlight::Deleted));
        assert_eq!(at("a/x.txt\n+++", 0), Some(Highlight::String));
        assert_eq!(at(" b/x.txt\n@@", 0), Some(Highlight::Inserted));
        assert_eq!(at("@@", 0), Some(Highlight::Attribute));
        assert_eq!(at(" same", 0), None);
    }

    /// **A Makefile is coloured as its query says**: a variable's name, a
    /// variable make itself uses, a reference to one, a standard target, a
    /// comment.
    #[test]
    fn a_makefile_is_coloured_as_its_query_says() {
        let text = "# Build it.\nCC := gcc\nOBJS = main.o\nall: $(OBJS)\n\t$(CC) -o app $(OBJS)\n";
        let spans = highlighted(text, "make");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("# Build", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("CC :=", 0), Some(Highlight::Constant));
        assert_eq!(at("OBJS =", 0), Some(Highlight::Constant));
        assert_eq!(at("OBJS)", 0), Some(Highlight::Constant));
        assert_eq!(at("all:", 0), Some(Highlight::Macro));
    }

    /// **A diff's hunks are coloured in their file's language**: a Rust
    /// file's diff paints its lines as Rust -- the lines kept and added as
    /// the new file, the ones taken away as the old -- each line's marker
    /// left out, over the change's colours.
    #[test]
    fn a_diffs_hunks_are_coloured_in_their_files_language() {
        let text = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    let x = 1; // one\n+    let y = 2; // two\n }\n";
        let spans = highlighted(text, "diff");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("fn main", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("let x", 0), Some(Highlight::Keyword));
        assert_eq!(at("let y", 0), Some(Highlight::Keyword));
        assert_eq!(at("// two", 0), Some(Highlight::Comment));
        // The comment ends with its line: the next line's `}` is Rust's.
        assert_eq!(at("}\n", 0), Some(Highlight::Punctuation));
        // The markers stay the diff's: their lines' colours.
        assert_eq!(at("-    let", 0), Some(Highlight::Deleted));
        assert_eq!(at("+    let", 0), Some(Highlight::Inserted));
    }

    /// **A diff's hunk is one document for each side**: the lines the new
    /// file has -- kept and added -- are read together, so a comment opened
    /// on one line goes on over the next; the old file's lines likewise; and
    /// neither side reads the other's lines.
    #[test]
    fn a_hunks_new_and_old_lines_are_each_one_document() {
        let text = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n /* one\n-let two\n+let three\n */\n@@ -9 +9 @@\n-/* gone\n+fn kept() {}\n@@ -20 +20 @@\n+/* new\n-fn old() {}\n";
        let (spans, stretches) = diff_stretches(text);
        // One document for each side of each hunk, each of its lines.
        let lines = |needles: &[&str]| -> Vec<(usize, usize)> {
            needles
                .iter()
                .map(|n| {
                    // Past the newline before it and its marker, to
                    // past its own newline.
                    let at = text.find(n).unwrap();
                    (at + 2, at + n.len() + 1)
                })
                .collect()
        };
        let mut expected = vec![
            lines(&["\n /* one", "\n+let three", "\n */"]),
            lines(&["\n /* one", "\n-let two", "\n */"]),
            lines(&["\n-/* gone"]),
            lines(&["\n+fn kept() {}"]),
            lines(&["\n+/* new"]),
            lines(&["\n-fn old() {}"]),
        ];
        expected.sort();
        assert_eq!(stretches, expected);
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("let three", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("let two", 0), Some(Highlight::Comment));
        assert_eq!(at("fn kept", 0), Some(Highlight::Keyword));
        assert_eq!(at("fn old", 0), Some(Highlight::Keyword));
    }

    /// **A diff's plain code keeps the change's colour**: a name its
    /// language does not colour shows the line's, inserted or deleted, the
    /// language's colours over it where it has them.
    #[test]
    fn a_hunks_uncoloured_code_keeps_the_changes_colour() {
        let text = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-let old = 1;\n+let new = 2;\n";
        let spans = highlighted(text, "diff");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("let new", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("new =", 0), Some(Highlight::Inserted));
        assert_eq!(at("old =", 0), Some(Highlight::Deleted));
    }

    /// **A diff with `\r\n` line ends is read a line at a time**: each
    /// line's end, both its bytes, ends the line in its language too -- a
    /// comment stops at it rather than running on into the next line.
    #[test]
    fn a_crlf_diffs_lines_end_where_they_do() {
        let text = "--- a/x.rs\r\n+++ b/x.rs\r\n@@ -1,3 +1,3 @@\r\n fn main() { // start\r\n+    let y = 2;\r\n }\r\n";
        let spans = highlighted(text, "diff");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("// start", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("let y", 0), Some(Highlight::Keyword));
    }

    /// **A diff's file is known by its name alone**: a `diff -u` header's
    /// timestamp, after a tab, is not part of it; nor are the quotes git
    /// puts round an odd name; `/dev/null` -- a file added or deleted --
    /// has no language.
    #[test]
    fn a_diff_headers_file_is_known_by_its_name() {
        let named = |header| language_of_file(header).map(Language::name);
        assert_eq!(named("b/src/main.rs"), Some("Rust"));
        assert_eq!(named("a.c\t2024-01-01 12:00:00.000000000 +0000"), Some("C"));
        assert_eq!(named("b/my file.py"), Some("Python"));
        assert_eq!(named("\"b/caf\\303\\251.go\""), Some("Go"));
        assert_eq!(named("/dev/null"), None);
    }

    /// **`#offset!` moves by columns, a line's end one of them**: forward
    /// and back, over `\n` or `\r\n` alike, a character's bytes all taken
    /// together, rows first, and held at the text's ends.
    #[test]
    fn an_offset_moves_by_columns_a_lines_end_one() {
        let buffer = TextBuffer::from_text("ab\r\ncd\nh\u{e9}!");
        let point = |byte| {
            let (row, column) = buffer.point(byte).unwrap();
            Point { row, column }
        };
        let go = |byte, rows, columns| moved(&buffer, (byte, point(byte)), rows, columns);
        // From `b`: past it to its line's end, then over `\r\n`, whole.
        assert_eq!(go(1, 0, 1), Some(2));
        assert_eq!(go(1, 0, 2), Some(4));
        assert_eq!(go(1, 0, 3), Some(5));
        // Back over `\r\n` from `c`, and over `\n` from `h`.
        assert_eq!(go(4, 0, -1), Some(2));
        assert_eq!(go(7, 0, -1), Some(6));
        // A row down keeps the column; the column goes on from there.
        assert_eq!(go(1, 1, 0), Some(5));
        assert_eq!(go(1, 1, 2), Some(7));
        // `\u{e9}` is two bytes: a column into it is past it.
        assert_eq!(go(8, 0, 1), Some(10));
        // The text's ends hold; past its last row there is nothing.
        assert_eq!(go(10, 0, 5), Some(11));
        assert_eq!(go(1, 0, -5), Some(0));
        assert_eq!(go(1, 3, 0), None);
        // A range whose start passes its end is none.
        let range = |start: usize, end: usize| tree_sitter::Range {
            start_byte: start,
            end_byte: end,
            start_point: point(start),
            end_point: point(end),
        };
        assert_eq!(offset_range(&buffer, &range(0, 2), [0, 1, 0, -1]), None);
        let moved = offset_range(&buffer, &range(0, 2), [0, 1, 0, 1]).unwrap();
        assert_eq!((moved.start_byte, moved.end_byte), (1, 4));
        assert_eq!(moved.end_point, Point { row: 1, column: 0 });
    }

    /// The highlights of `text`, a diff, settled, and its injected
    /// stretches' ranges, sorted.
    fn diff_stretches(text: &str) -> (Vec<HighlightSpan>, Vec<Vec<(usize, usize)>>) {
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named("diff").unwrap().highlighter().unwrap();
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        (spans, kept_stretches(&h))
    }

    /// **A diff with no `diff` line has its hunks coloured too**: `diff
    /// -u`'s lines, which the grammar leaves flat, each hunk in the
    /// language of the file named last before it -- a second hunk as well
    /// as the first, a second file's in its own language -- and one
    /// document for each side of each hunk, no more.
    #[test]
    fn a_flat_diffs_hunks_are_coloured_in_their_files_language() {
        let text = "--- a/x.rs\t2024-01-01 00:00:00\n+++ b/x.rs\t2024-01-02 00:00:00\n@@ -1,2 +1,2 @@\n fn a() {\n-    let old = 1;\n+    let new = 2;\n@@ -10,2 +10,2 @@\n /* c\n+d */ fn e() {}\nIndex: y.py\n===================================================================\n--- y.py\t(revision 1)\n+++ y.py\t(working copy)\n@@ -1 +1 @@\n+def g(): pass\n";
        let (spans, stretches) = diff_stretches(text);
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("let new", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("let old", 0), Some(Highlight::Keyword));
        assert_eq!(at("d */", 0), Some(Highlight::Comment));
        assert_eq!(at("fn e", 0), Some(Highlight::Keyword));
        assert_eq!(at("def g", 0), Some(Highlight::Keyword));
        // x.rs: its two hunks' new sides and their old sides; y.py: its
        // hunk's new side (its old side has no lines).
        assert_eq!(stretches.len(), 5, "{stretches:?}");
    }

    /// **Priority, then depth in injections, then order**: a span of a
    /// higher priority shows over one of a lower wherever both are, however
    /// they nest; of one priority, an injected language's over its host's,
    /// wherever each starts; the order of starts decides only among equals.
    #[test]
    fn spans_rank_by_priority_then_depth_then_order() {
        use Highlight::{Comment, Inserted, Keyword, Punctuation, String as Str};
        let span = |range: Range<usize>, colour, priority, depth| Span {
            range,
            colour: Some(colour),
            priority,
            depth,
        };
        let flat = |spans: Vec<Span>| -> Vec<(Range<usize>, Highlight)> {
            flatten(spans, &(0..20))
                .into_iter()
                .map(|s| (s.range, s.highlight))
                .collect()
        };
        // A marker of priority 95 inside its line of 100: the line's.
        assert_eq!(
            flat(vec![
                span(0..10, Inserted, 100, 0),
                span(0..1, Punctuation, 95, 0)
            ]),
            [(0..10, Inserted)]
        );
        // An outer span of a higher priority hides one inside it.
        assert_eq!(
            flat(vec![span(0..10, Str, 110, 0), span(2..4, Keyword, 100, 0)]),
            [(0..10, Str)]
        );
        // An injected comment begun before its host's line colours the
        // whole line; a token of its own inside the comment goes on top.
        assert_eq!(
            flat(vec![
                span(0..12, Comment, 100, 1),
                span(4..10, Inserted, 100, 0),
                span(6..8, Keyword, 100, 1),
            ]),
            [(0..6, Comment), (6..8, Keyword), (8..12, Comment)]
        );
        // The host shows where the injected language has nothing.
        assert_eq!(
            flat(vec![
                span(4..10, Inserted, 100, 0),
                span(6..8, Keyword, 100, 1)
            ]),
            [(4..6, Inserted), (6..8, Keyword), (8..10, Inserted)]
        );
        // Priority before depth: a host's span of a higher priority over an
        // injected language's.
        assert_eq!(
            flat(vec![span(0..10, Str, 110, 0), span(2..4, Keyword, 100, 1)]),
            [(0..10, Str)]
        );
        // A span of no width shows nowhere.
        assert_eq!(
            flat(vec![span(0..10, Str, 100, 0), span(3..3, Keyword, 100, 0)]),
            [(0..10, Str)]
        );
    }

    /// **An injected stretch's spans are cut to its ranges**: a span over
    /// the text between two of them -- a hole -- colours only its sides;
    /// one wholly in a hole, nothing.
    #[test]
    fn an_injected_stretchs_spans_are_cut_to_its_ranges() {
        let range = |start_byte, end_byte| tree_sitter::Range {
            start_byte,
            end_byte,
            start_point: Point {
                row: 0,
                column: start_byte,
            },
            end_point: Point {
                row: 0,
                column: end_byte,
            },
        };
        let span = |range: Range<usize>| Span {
            range,
            colour: Some(Highlight::Comment),
            priority: DEFAULT_PRIORITY,
            depth: 1,
        };
        let mut out = Vec::new();
        clip(
            vec![span(2..14), span(6..7), span(15..16), span(3..3)],
            &[range(0, 5), range(8, 10), range(12, 20)],
            &mut out,
        );
        let got: Vec<Range<usize>> = out.into_iter().map(|s| s.range).collect();
        assert_eq!(got, [2..5, 8..10, 12..14, 15..16]);
    }

    /// **A pattern's priority outranks the order of patterns and of
    /// nodes**: `(#set! priority 105)` on an earlier pattern wins over a
    /// later one for the same node, and on an outer node over the nodes
    /// inside it; below 100 a node goes under its parent; and a capture's
    /// own priority is read over its pattern's.
    #[test]
    fn a_patterns_priority_outranks_the_order() {
        let text = "fn main() { let x = 1; }\n";
        let colours = |highlights: &str| {
            let buffer = TextBuffer::from_text(text);
            let mut h = with_queries_for("rust", highlights, "");
            h.reset(&buffer);
            let spans = settled(&mut h, &buffer);
            move |needle: &str| colour_at(&spans, text, needle, 0)
        };
        let at = colours("(identifier) @keyword\n(identifier) @function\n");
        assert_eq!(at("main"), Some(Highlight::Function));
        let at = colours("((identifier) @keyword (#set! priority 105))\n(identifier) @function\n");
        assert_eq!(at("main"), Some(Highlight::Keyword));
        let at = colours("((block) @comment (#set! priority 110))\n(identifier) @function\n");
        assert_eq!(at("x ="), Some(Highlight::Comment));
        assert_eq!(at("main"), Some(Highlight::Function));
        let at = colours("(block) @comment\n((identifier) @function (#set! priority 95))\n");
        assert_eq!(at("x ="), Some(Highlight::Comment));
        assert_eq!(at("main"), Some(Highlight::Function));
        let at = colours(
            "((identifier) @keyword (#set! @keyword priority 105) (#set! priority 90))\n(identifier) @function\n",
        );
        assert_eq!(at("main"), Some(Highlight::Keyword));
        // A priority that is not a number is none.
        let at = colours("((identifier) @keyword (#set! priority high))\n(identifier) @function\n");
        assert_eq!(at("main"), Some(Highlight::Function));
    }

    /// **A use coloured as its declaration has the default priority**,
    /// though none of its own captures paints: it shows over the block
    /// round it, as a node inside another does.
    #[test]
    fn a_use_coloured_as_its_declaration_has_the_default_priority() {
        let text = "fn f() { let x = 1; x; }\n";
        let buffer = TextBuffer::from_text(text);
        let mut h = with_queries_for(
            "rust",
            "(block) @comment\n(let_declaration pattern: (identifier) @variable.parameter)\n(identifier) @spell\n",
            "(block) @local.scope\n(let_declaration pattern: (identifier) @local.definition)\n(identifier) @local.reference\n",
        );
        h.reset(&buffer);
        let spans = settled(&mut h, &buffer);
        let at = |needle| colour_at(&spans, text, needle, 0);
        assert_eq!(at("x;"), Some(Highlight::Parameter), "{spans:?}");
        assert_eq!(at("1;"), Some(Highlight::Comment));
    }

    /// **An injected language colours only the text it was given**: a
    /// comment the new file opens before a deleted line and closes after it
    /// does not colour that line, which is the old file's -- plain there,
    /// so in the deletion's colour.
    #[test]
    fn an_injected_language_colours_only_its_own_text() {
        let text = "--- a/x.rs\n+++ b/x.rs\n@@ -1 +1,2 @@\n+/* a\n-y\n+*/\n";
        let spans = highlighted(text, "diff");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("/* a", 0), Some(Highlight::Comment), "{spans:?}");
        assert_eq!(at("*/", 0), Some(Highlight::Comment));
        assert_eq!(at("y\n", 0), Some(Highlight::Deleted));
    }

    /// **A template's `${...}` inside an injected attribute stays its
    /// host's**: HTML's attribute string goes round the hole the
    /// substitution makes in the HTML, not over it.
    #[test]
    fn a_hole_in_an_injected_stretch_keeps_its_hosts_colours() {
        let text = "const t = html`<b class=\"a ${c} b\">x</b>`;\n";
        let spans = highlighted(text, "javascript");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("a ${", 0), Some(Highlight::String), "{spans:?}");
        assert_eq!(at("${", 0), Some(Highlight::Punctuation));
        assert_eq!(at("c}", 0), Some(Highlight::Variable));
        assert_eq!(at(" b\"", 0), Some(Highlight::String));
    }

    /// **A Dockerfile's RUN commands are Bash**, over their line
    /// continuations, and so is a `RUN <<EOF` script; a heredoc fed to a
    /// command, or copied into a file, stays the Dockerfile's text.
    #[test]
    fn a_dockerfiles_shell_is_coloured_as_bash() {
        let text = concat!(
            "FROM debian\n",
            "RUN apt-get update && \\\n",
            "    apt-get install -y curl\n",
            "RUN <<EOF\n",
            "if true; then\n",
            "  echo hi\n",
            "fi\n",
            "EOF\n",
            "RUN cat <<EOF\n",
            "if data\n",
            "EOF\n",
            "COPY <<EOF /etc/x\n",
            "if file\n",
            "EOF\n",
        );
        let spans = highlighted(text, "dockerfile");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("RUN apt", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("apt-get update", 0), Some(Highlight::Function));
        // Over the line continuation: the second command is Bash's too.
        assert_eq!(at("apt-get install", 0), Some(Highlight::Function));
        assert_eq!(at("if true", 0), Some(Highlight::Keyword));
        assert_eq!(at("fi\n", 0), Some(Highlight::Keyword));
        assert_eq!(at("echo hi", 0), Some(Highlight::Function));
        // The delimiter is the Dockerfile's, not a command.
        assert_eq!(at("EOF\nRUN cat", 0), Some(Highlight::Keyword));
        assert_eq!(at("if data", 0), Some(Highlight::String));
        assert_eq!(at("if file", 0), Some(Highlight::String));
    }

    /// A highlighter of `text` in `language`, settled, with the text.
    fn settled_on(text: &str, language: &str) -> (SyntaxHighlighter, TextBuffer) {
        let buffer = TextBuffer::from_text(text);
        let mut h = Language::named(language).unwrap().highlighter().unwrap();
        h.reset(&buffer);
        settled(&mut h, &buffer);
        (h, buffer)
    }

    /// **A bracket is paired as the language reads it**: the partner of one
    /// in code skips over a bracket in a string and one in a comment, which
    /// are none themselves -- from either side of the caret.
    #[test]
    fn a_bracket_is_paired_as_the_language_reads_it() {
        let text = "fn f() { g(\"(\", x) // )\n}\n";
        let (h, buffer) = settled_on(text, "rust");
        let open = text.find("g(").unwrap() + 1;
        let close = text.find("x)").unwrap() + 1;
        assert_eq!(h.brackets(&buffer, open), Brackets::Pair(open, close));
        assert_eq!(h.brackets(&buffer, close), Brackets::Pair(close, open));
        assert_eq!(
            h.brackets(&buffer, close + 1),
            Brackets::Pair(close, open),
            "just after the closer"
        );
        let in_string = text.find("\"(\"").unwrap() + 1;
        assert_eq!(h.brackets(&buffer, in_string), Brackets::Unpaired);
        let in_comment = text.find("// )").unwrap() + 3;
        assert_eq!(h.brackets(&buffer, in_comment), Brackets::Unpaired);
        let body = text.find('{').unwrap();
        let body_end = text.rfind('}').unwrap();
        assert_eq!(h.brackets(&buffer, body), Brackets::Pair(body, body_end));
        // Not by a bracket at all.
        assert_eq!(h.brackets(&buffer, 0), Brackets::Unpaired);
    }

    /// **A language injected pairs its own brackets**: a script's in an
    /// HTML page, whose own tree holds the script as one run of text; a
    /// Markdown code fence's in its language; a template's `${` with its
    /// `}`.
    #[test]
    fn an_injected_language_pairs_its_own_brackets() {
        let text = "<p>(</p><script>f(\"(\", x);</script>\n";
        let (h, buffer) = settled_on(text, "html");
        let open = text.find("f(").unwrap() + 1;
        let close = text.find("x)").unwrap() + 1;
        assert_eq!(h.brackets(&buffer, open), Brackets::Pair(open, close));
        let in_string = text.find("\"(\"").unwrap() + 1;
        assert_eq!(h.brackets(&buffer, in_string), Brackets::Unpaired);
        let text = "Some prose.\n\n```rust\nlet v = vec![1, (2)];\n```\n";
        let (h, buffer) = settled_on(text, "markdown");
        let open = text.find("![").unwrap() + 1;
        let close = text.find("];").unwrap();
        assert_eq!(h.brackets(&buffer, open), Brackets::Pair(open, close));
        let text = "let s = `a${x}b`;\n";
        let (h, buffer) = settled_on(text, "javascript");
        let open = text.find("${").unwrap() + 1;
        let close = text.find("}b").unwrap();
        assert_eq!(h.brackets(&buffer, open), Brackets::Pair(open, close));
        assert_eq!(h.brackets(&buffer, close), Brackets::Pair(close, open));
    }

    /// **A bracket whose partner is not where the tree would have it cannot
    /// be paired by the tree**: broken code leaves it to counting.
    #[test]
    fn a_bracket_with_no_partner_in_the_tree_is_unknown() {
        let (h, buffer) = settled_on("fn f() { g(x; }\n", "rust");
        assert_eq!(h.brackets(&buffer, 10), Brackets::Unknown);
    }

    /// **Behind the text, it cannot say where a bracket's partner is**:
    /// after an edit, until the parse catches up, the view counts brackets
    /// itself.
    #[test]
    fn behind_the_text_a_brackets_partner_is_unknown() {
        let (mut h, mut buffer) = settled_on("f(x)\n", "rust");
        assert_eq!(h.brackets(&buffer, 1), Brackets::Pair(1, 3));
        let _ = buffer.take_changes();
        buffer.insert(0, "g").unwrap();
        let changes = buffer.take_changes();
        h.edited(&buffer, &changes.splices.unwrap());
        assert_eq!(h.brackets(&buffer, 2), Brackets::Unknown);
        while h.work(&buffer, Duration::from_secs(5)) {}
        assert_eq!(h.brackets(&buffer, 2), Brackets::Pair(2, 4));
    }

    /// **A linker script is coloured as its query says**: the commands'
    /// keywords, ld's own words, a label, a constant and a variable -- the
    /// query's `#lua-match?` patterns among them.
    #[test]
    fn a_linker_script_is_coloured_as_its_query_says() {
        let text = "ENTRY(kmain)\nSECTIONS\n{\n    .text : {\n        . = ALIGN(4K);\n        __text_start = .;\n        KEEP(*(.text))\n    }\n}\n";
        let spans = highlighted(text, "linker script");
        let at = |needle, nth| colour_at(&spans, text, needle, nth);
        assert_eq!(at("ENTRY", 0), Some(Highlight::Keyword), "{spans:?}");
        assert_eq!(at("SECTIONS", 0), Some(Highlight::Keyword));
        // An all-capitals name is a constant, the query's last word on it
        // (its `#lua-match?`, read as a `#match?`) -- a call's included.
        assert_eq!(at("ALIGN", 0), Some(Highlight::Constant));
        assert_eq!(at("KEEP", 0), Some(Highlight::Builtin));
        // A name starting with a dot is a label; others are variables.
        assert_eq!(at(".text :", 0), Some(Highlight::Label));
        assert_eq!(at("__text_start", 0), Some(Highlight::Variable));
    }
}
