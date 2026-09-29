//! Syntax highlighting, as the code editor sees it: what kinds of thing a
//! highlighter tells apart ([`Highlight`]), what it answers for a stretch of
//! text ([`HighlightSpan`]), and the hook a [`CodeView`](crate::codeview::CodeView)
//! drives one through ([`Highlighter`]).
//!
//! # Why the toolkit has the hook and not the highlighter
//!
//! A highlighter worth having parses: tree-sitter's grammars and runtime,
//! which are large, and which most programs that use the toolkit never need.
//! So the toolkit says what a highlighter must do and draws what it answers,
//! and the tree-sitter highlighter is its own crate (`gui/syntax`) that only
//! the programs editing code link. A program with a highlighter of its own --
//! a log viewer colouring levels, say -- implements the same trait.
//!
//! # The kinds
//!
//! One list for every language, so a theme colours keywords once rather than
//! once per language, and named after tree-sitter's capture names, which are
//! the nearest thing to a common vocabulary highlighters have:
//! `@function.method` in a grammar's query is a [`Highlight::Function`]
//! ([`Highlight::for_capture`]). The colours are the palette's
//! ([`Palette::syntax_ink`](crate::palette::Palette::syntax_ink)), which a
//! theme's `syntax` section can set.

use core::fmt;
use core::ops::Range;
use core::time::Duration;

use crate::textbuffer::{Splice, TextBuffer};

/// What a stretch of code is, for colouring it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Highlight {
    /// `fn`, `if`, `return`, `import`.
    Keyword,
    /// A string or character literal.
    String,
    /// An escape in a string (`\n`), a regular expression.
    Escape,
    /// A comment, documentation comments included.
    Comment,
    /// A number.
    Number,
    /// A named constant, `true`, `None`.
    Constant,
    /// A type's name.
    Type,
    /// A function's or method's name, where it is defined or called.
    Function,
    /// A macro: `println!`, `#define`d names.
    Macro,
    /// Something the language itself provides: `self`, `print`.
    Builtin,
    /// A constructor: an enum variant, a struct built by name.
    Constructor,
    /// A variable. Drawn in the text's own ink unless a theme says otherwise.
    Variable,
    /// A function's parameter.
    Parameter,
    /// A field, a property, a key in a map.
    Property,
    /// A module or namespace.
    Module,
    /// A label: a loop label, a lifetime.
    Label,
    /// An operator.
    Operator,
    /// Brackets, commas, semicolons.
    Punctuation,
    /// An attribute or annotation: `#[derive]`, `@decorator`.
    Attribute,
    /// A markup tag's name: `<div>`.
    Tag,
    /// A heading in a markup language.
    Heading,
    /// A link or URL in a markup language.
    Link,
    /// Text a change adds: a diff's `+` line.
    Inserted,
    /// Text a change takes away: a diff's `-` line.
    Deleted,
    /// Text a change alters: a context diff's `!` line.
    Changed,
}

impl Highlight {
    /// How many kinds there are.
    pub const COUNT: usize = 25;

    /// Every kind, in declaration order -- which is [`index`](Self::index)'s.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Keyword,
        Self::String,
        Self::Escape,
        Self::Comment,
        Self::Number,
        Self::Constant,
        Self::Type,
        Self::Function,
        Self::Macro,
        Self::Builtin,
        Self::Constructor,
        Self::Variable,
        Self::Parameter,
        Self::Property,
        Self::Module,
        Self::Label,
        Self::Operator,
        Self::Punctuation,
        Self::Attribute,
        Self::Tag,
        Self::Heading,
        Self::Link,
        Self::Inserted,
        Self::Deleted,
        Self::Changed,
    ];

    /// Where this kind is in [`ALL`](Self::ALL).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The kind's name in a theme's `syntax` section.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::String => "string",
            Self::Escape => "escape",
            Self::Comment => "comment",
            Self::Number => "number",
            Self::Constant => "constant",
            Self::Type => "type",
            Self::Function => "function",
            Self::Macro => "macro",
            Self::Builtin => "builtin",
            Self::Constructor => "constructor",
            Self::Variable => "variable",
            Self::Parameter => "parameter",
            Self::Property => "property",
            Self::Module => "module",
            Self::Label => "label",
            Self::Operator => "operator",
            Self::Punctuation => "punctuation",
            Self::Attribute => "attribute",
            Self::Tag => "tag",
            Self::Heading => "heading",
            Self::Link => "link",
            Self::Inserted => "inserted",
            Self::Deleted => "deleted",
            Self::Changed => "changed",
        }
    }

    /// The kind named `name` in a theme's `syntax` section.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.name() == name)
    }

    /// The kind a tree-sitter capture name falls in, or `None` for one that
    /// is not coloured (`@embedded`, `@none`, a query's private `@_name`).
    ///
    /// The most specific rule that matches wins, so `function.builtin` is a
    /// [`Builtin`](Self::Builtin) where `function.method` is a
    /// [`Function`](Self::Function): a capture's dotted name is read as a
    /// path, and the longest known prefix of it decides. The names are the
    /// ones tree-sitter's own grammars use, with the older Neovim ones
    /// (`text.title`, `conditional`) where a grammar still ships them.
    #[must_use]
    pub fn for_capture(name: &str) -> Option<Self> {
        /// Most specific first within a family; each entry matches the name
        /// itself or the name followed by a dot.
        const RULES: &[(&str, Option<Highlight>)] = &[
            ("none", None),
            ("embedded", None),
            ("spell", None),
            ("nospell", None),
            ("conceal", None),
            ("attribute", Some(Highlight::Attribute)),
            ("comment", Some(Highlight::Comment)),
            ("constant.builtin", Some(Highlight::Constant)),
            ("constant.macro", Some(Highlight::Macro)),
            ("constant.numeric", Some(Highlight::Number)),
            ("constant.character.escape", Some(Highlight::Escape)),
            ("constant", Some(Highlight::Constant)),
            ("boolean", Some(Highlight::Constant)),
            ("constructor", Some(Highlight::Constructor)),
            ("escape", Some(Highlight::Escape)),
            ("function.builtin", Some(Highlight::Builtin)),
            ("function.macro", Some(Highlight::Macro)),
            ("function", Some(Highlight::Function)),
            ("method", Some(Highlight::Function)),
            ("macro", Some(Highlight::Macro)),
            ("keyword", Some(Highlight::Keyword)),
            ("conditional", Some(Highlight::Keyword)),
            ("repeat", Some(Highlight::Keyword)),
            ("include", Some(Highlight::Keyword)),
            ("exception", Some(Highlight::Keyword)),
            ("storageclass", Some(Highlight::Keyword)),
            ("label", Some(Highlight::Label)),
            ("module", Some(Highlight::Module)),
            ("namespace", Some(Highlight::Module)),
            ("number", Some(Highlight::Number)),
            ("float", Some(Highlight::Number)),
            ("operator", Some(Highlight::Operator)),
            ("property", Some(Highlight::Property)),
            ("field", Some(Highlight::Property)),
            ("punctuation", Some(Highlight::Punctuation)),
            ("string.escape", Some(Highlight::Escape)),
            ("string.regexp", Some(Highlight::Escape)),
            ("string.regex", Some(Highlight::Escape)),
            ("string.special.regex", Some(Highlight::Escape)),
            ("string.special.key", Some(Highlight::Property)),
            ("string.special.symbol", Some(Highlight::Constant)),
            ("string.special.url", Some(Highlight::Link)),
            ("string.special.path", Some(Highlight::String)),
            ("string", Some(Highlight::String)),
            ("character.special", Some(Highlight::Escape)),
            ("character", Some(Highlight::String)),
            ("tag.attribute", Some(Highlight::Attribute)),
            ("tag.delimiter", Some(Highlight::Punctuation)),
            ("tag", Some(Highlight::Tag)),
            ("type", Some(Highlight::Type)),
            ("variable.builtin", Some(Highlight::Builtin)),
            ("variable.parameter", Some(Highlight::Parameter)),
            ("parameter", Some(Highlight::Parameter)),
            ("variable.member", Some(Highlight::Property)),
            ("variable", Some(Highlight::Variable)),
            ("markup.heading", Some(Highlight::Heading)),
            ("markup.link.url", Some(Highlight::Link)),
            ("markup.link", Some(Highlight::Link)),
            ("markup.raw", Some(Highlight::String)),
            ("markup.quote", Some(Highlight::Comment)),
            ("markup.list", Some(Highlight::Punctuation)),
            ("text.title", Some(Highlight::Heading)),
            ("text.uri", Some(Highlight::Link)),
            ("text.reference", Some(Highlight::Link)),
            ("text.literal", Some(Highlight::String)),
            ("text.quote", Some(Highlight::Comment)),
            // A change's lines, as diff grammars and editors name them.
            ("diff.plus", Some(Highlight::Inserted)),
            ("diff.add", Some(Highlight::Inserted)),
            ("markup.inserted", Some(Highlight::Inserted)),
            ("text.diff.add", Some(Highlight::Inserted)),
            ("diff.minus", Some(Highlight::Deleted)),
            ("diff.delete", Some(Highlight::Deleted)),
            ("markup.deleted", Some(Highlight::Deleted)),
            ("text.diff.delete", Some(Highlight::Deleted)),
            ("diff.delta", Some(Highlight::Changed)),
            ("diff.change", Some(Highlight::Changed)),
            ("markup.changed", Some(Highlight::Changed)),
        ];
        let name = name.strip_prefix('@').unwrap_or(name);
        if name.starts_with('_') {
            return None;
        }
        let mut best: Option<(usize, Option<Self>)> = None;
        for &(rule, kind) in RULES {
            let matches = name == rule
                || name
                    .strip_prefix(rule)
                    .is_some_and(|rest| rest.starts_with('.'));
            if matches && best.is_none_or(|(len, _)| rule.len() > len) {
                best = Some((rule.len(), kind));
            }
        }
        best.and_then(|(_, kind)| kind)
    }
}

impl fmt::Display for Highlight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A stretch of text and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
    /// The bytes it covers.
    pub range: Range<usize>,
    /// What they are.
    pub highlight: Highlight,
}

/// What a [`CodeView`](crate::codeview::CodeView) asks of a highlighter.
///
/// The view tells it about every change to the text as the text's own journal
/// recorded it ([`TextBuffer::take_changes`]), gives it time to work in small
/// slices -- right after an edit, and whenever the host calls
/// [`CodeView::work`](crate::codeview::CodeView::work) -- and asks for the
/// highlights of what is on screen each time it draws. A highlighter that
/// parses does its parsing in [`work`](Self::work), so a file that takes a
/// second to parse costs a second of frames, not a second of a frozen window.
pub trait Highlighter: fmt::Debug {
    /// Start over on `text`: the first thing the view calls, and what it
    /// calls whenever it cannot say what changed since the last call.
    fn reset(&mut self, text: &TextBuffer);

    /// `text` is the text after `splices`, which are every change since the
    /// last call to this or [`reset`](Self::reset), in the order they were
    /// made.
    fn edited(&mut self, text: &TextBuffer, splices: &[Splice]);

    /// Work toward highlighting `text` as it is now for at most about
    /// `budget`, and answer whether work remains.
    fn work(&mut self, text: &TextBuffer, budget: Duration) -> bool;

    /// Whether [`work`](Self::work) has something to do that it has not
    /// said -- work that drawing found. A highlighter whose
    /// [`highlights`](Self::highlights) starts parses it cannot finish at
    /// once (a language inside another, a code fence in its language) says
    /// so here, and the view keeps calling [`work`](Self::work), and drawing
    /// again, until it says no. The default: drawing leaves nothing for
    /// later.
    fn has_work(&self) -> bool {
        false
    }

    /// The highlights over `range` of `text`: sorted, none overlapping
    /// another, each within `range`. As the last finished work left them --
    /// text changed since may be uncoloured or coloured as it was before, but
    /// never coloured at the wrong offsets.
    fn highlights(&self, text: &TextBuffer, range: Range<usize>) -> Vec<HighlightSpan>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// **Every kind is where its index says, under its own name.**
    #[test]
    fn the_kinds_index_and_name_themselves() {
        for (i, h) in Highlight::ALL.into_iter().enumerate() {
            assert_eq!(h.index(), i, "{h}");
            assert_eq!(Highlight::from_name(h.name()), Some(h));
        }
        let mut names: Vec<&str> = Highlight::ALL.iter().map(|h| h.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Highlight::COUNT);
        assert_eq!(Highlight::from_name("keywords"), None);
    }

    /// **A capture falls in the most specific kind that names it**, a dotted
    /// name read as a path: `function.method.call` is a function,
    /// `function.builtin` a builtin, and `functions` nothing.
    #[test]
    fn a_capture_falls_in_its_most_specific_kind() {
        let cases = [
            ("keyword", Some(Highlight::Keyword)),
            ("@keyword.control.return", Some(Highlight::Keyword)),
            ("function", Some(Highlight::Function)),
            ("function.method", Some(Highlight::Function)),
            ("function.method.call", Some(Highlight::Function)),
            ("function.builtin", Some(Highlight::Builtin)),
            ("function.macro", Some(Highlight::Macro)),
            ("functions", None),
            ("type.builtin", Some(Highlight::Type)),
            ("variable", Some(Highlight::Variable)),
            ("variable.parameter", Some(Highlight::Parameter)),
            ("variable.builtin", Some(Highlight::Builtin)),
            ("string", Some(Highlight::String)),
            ("string.special.key", Some(Highlight::Property)),
            ("string.special", Some(Highlight::String)),
            ("string.escape", Some(Highlight::Escape)),
            ("constant.builtin", Some(Highlight::Constant)),
            ("constant.numeric.float", Some(Highlight::Number)),
            ("punctuation.bracket", Some(Highlight::Punctuation)),
            ("comment.documentation", Some(Highlight::Comment)),
            ("text.title", Some(Highlight::Heading)),
            ("diff.plus", Some(Highlight::Inserted)),
            ("markup.inserted", Some(Highlight::Inserted)),
            ("diff.minus", Some(Highlight::Deleted)),
            ("diff.delete", Some(Highlight::Deleted)),
            ("diff.delta", Some(Highlight::Changed)),
            ("diffs", None),
            ("embedded", None),
            ("_name", None),
            ("", None),
            ("local.definition", None),
        ];
        for (name, want) in cases {
            assert_eq!(Highlight::for_capture(name), want, "{name}");
        }
    }
}
