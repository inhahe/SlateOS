//! HTML: tree-sitter-html 0.23.2 (MIT, Max Brunsfeld, Amaan Qureshi and the
//! tree-sitter contributors). `grammars/html/` holds its `parser.c`, its
//! queries, its test corpus and highlight tests, and its `scanner.c` and
//! `tag.h` as published; the scanner is ported below. It keeps the elements
//! open where it is -- a stack of tags, saved with the parse's state -- and
//! from them says what a state machine cannot: which end tag closes which
//! element, where an element ends with none (an `<li>` at the next `<li>`, a
//! `<p>` at a `<div>`), a `<script>`'s or `<style>`'s text up to its end tag,
//! and a comment.
//!
//! # Where the port differs
//!
//! - **A tag's name is what HTML's tokenizer takes for one**: an ASCII letter,
//!   then everything up to a blank, `/` or `>`. The C took letters and digits
//!   as `iswalnum` has them in the C locale -- ASCII's -- with `-` and `:`, so
//!   a custom element's name stopped at an `_`, a `.` or a letter past ASCII
//!   (`<my_widget>` was `<my` with an attribute `_widget`), and it upper-cased
//!   each character into a byte, cutting one past ASCII down to its low byte.
//! - **`<blockquote>` and `<figcaption>` are known elements**: the C's table
//!   of names left them out, though its list of what ends a paragraph names
//!   both, so neither ended an open `<p>`.
//! - **A paragraph ends where the HTML standard ends one**: before every
//!   element its list names -- the C's, and `<dialog>`, `<hgroup>`, `<menu>`,
//!   `<search>`, `<table>` and `<ul>`, which the C's lacked. And an
//!   `<option>` ends at the next `<option>`, an `<optgroup>` or an `<hr>`,
//!   and an `<optgroup>` at an `<hr>` too, as the standard has them; the C
//!   never ended an `<option>` without its end tag.
//! - **An end tag closes the elements above its own, not above any of its
//!   kind**: the C, digging down the stack for an element an end tag names,
//!   compared kinds only, so `</x-b>` closed an open `<x-a>` -- every custom
//!   element is one kind. An end tag for no open element closes nothing.
//! - **A script's text ends only at its end tag**: at `</script` (or
//!   `</style`) and then a blank, `/` or `>`, as HTML has it -- the C ended it
//!   at `</scripts` -- and a `<` that breaks off a partial match can begin
//!   the end tag: the C passed it over, and missed the end tag of
//!   `a <</script>`.
//! - **A saved custom element keeps its whole name**: the C saved at most 255
//!   bytes of one, and the element restored under the cut name was closed by
//!   no end tag.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("html", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/html/highlights.scm");

/// The injection query, as published: a `<script>`'s text in JavaScript, a
/// `<style>`'s in CSS.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/html/injections.scm");

/// The external tokens, in the grammar's order.
#[derive(Clone, Copy)]
enum Token {
    StartTagName,
    ScriptStartTagName,
    StyleStartTagName,
    EndTagName,
    ErroneousEndTagName,
    SelfClosingTagDelimiter,
    ImplicitEndTag,
    RawText,
    Comment,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// Declares the elements the scanner knows by name, as [`Kind`] -- the void
/// ones, which have no content and no end tag, first -- and the name of
/// each, which is its variant's.
macro_rules! elements {
    (void: $($void:ident),* $(,)?; other: $($other:ident),* $(,)?) => {
        /// An element the scanner knows by its name.
        #[allow(
            clippy::upper_case_acronyms,
            reason = "each is its element's name as the scanner compares names: upper-cased"
        )]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Kind {
            $($void,)*
            $($other,)*
        }

        impl Kind {
            /// Every one, the void ones first, in the order declared: a
            /// saved state names each by its place here.
            const ALL: &'static [Self] = &[$(Self::$void,)* $(Self::$other,)*];

            /// How many of [`ALL`](Self::ALL) are void.
            const VOID: usize = [$(Self::$void),*].len();

            /// The element named `name`, upper-cased, if the scanner knows
            /// one by it.
            fn named(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($void) => Some(Self::$void),)*
                    $(stringify!($other) => Some(Self::$other),)*
                    _ => None,
                }
            }
        }
    };
}

elements! {
    void: AREA, BASE, BASEFONT, BGSOUND, BR, COL, COMMAND, EMBED, FRAME, HR,
        IMAGE, IMG, INPUT, ISINDEX, KEYGEN, LINK, MENUITEM, META, NEXTID,
        PARAM, SOURCE, TRACK, WBR;
    other: A, ABBR, ADDRESS, ARTICLE, ASIDE, AUDIO, B, BDI, BDO, BLOCKQUOTE,
        BODY, BUTTON, CANVAS, CAPTION, CITE, CODE, COLGROUP, DATA, DATALIST,
        DD, DEL, DETAILS, DFN, DIALOG, DIV, DL, DT, EM, FIELDSET, FIGCAPTION,
        FIGURE, FOOTER, FORM, H1, H2, H3, H4, H5, H6, HEAD, HEADER, HGROUP,
        HTML, I, IFRAME, INS, KBD, LABEL, LEGEND, LI, MAIN, MAP, MARK, MATH,
        MENU, METER, NAV, NOSCRIPT, OBJECT, OL, OPTGROUP, OPTION, OUTPUT, P,
        PICTURE, PRE, PROGRESS, Q, RB, RP, RT, RTC, RUBY, S, SAMP, SCRIPT,
        SEARCH, SECTION, SELECT, SLOT, SMALL, SPAN, STRONG, STYLE, SUB,
        SUMMARY, SUP, SVG, TABLE, TBODY, TD, TEMPLATE, TEXTAREA, TFOOT, TH,
        THEAD, TIME, TITLE, TR, U, UL, VAR, VIDEO,
}

impl Kind {
    fn is_void(self) -> bool {
        (self as usize) < Self::VOID
    }
}

/// What ends an open paragraph: the HTML standard's list of the elements
/// before which a `<p>`'s end tag may be left out.
const ENDS_PARAGRAPH: &[Kind] = &[
    Kind::ADDRESS,
    Kind::ARTICLE,
    Kind::ASIDE,
    Kind::BLOCKQUOTE,
    Kind::DETAILS,
    Kind::DIALOG,
    Kind::DIV,
    Kind::DL,
    Kind::FIELDSET,
    Kind::FIGCAPTION,
    Kind::FIGURE,
    Kind::FOOTER,
    Kind::FORM,
    Kind::H1,
    Kind::H2,
    Kind::H3,
    Kind::H4,
    Kind::H5,
    Kind::H6,
    Kind::HEADER,
    Kind::HGROUP,
    Kind::HR,
    Kind::MAIN,
    Kind::MENU,
    Kind::NAV,
    Kind::OL,
    Kind::P,
    Kind::PRE,
    Kind::SEARCH,
    Kind::SECTION,
    Kind::TABLE,
    Kind::UL,
];

/// An open element, as the scanner keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tag {
    /// One it knows by name.
    Known(Kind),
    /// One it does not: by its name, upper-cased as ASCII.
    Custom(String),
    /// One whose tag did not fit when the state was saved: it keeps its
    /// place, and no end tag closes it.
    Lost,
}

impl Tag {
    /// The element a tag's name, upper-cased, opens or closes.
    fn for_name(name: String) -> Self {
        Kind::named(&name).map_or(Self::Custom(name), Self::Known)
    }

    fn is(&self, kind: Kind) -> bool {
        *self == Self::Known(kind)
    }

    fn is_void(&self) -> bool {
        matches!(self, Self::Known(kind) if kind.is_void())
    }

    /// Whether an element opening inside this one leaves it open: not if
    /// this one's end tag may be left out before it.
    fn can_contain(&self, child: &Self) -> bool {
        let Self::Known(parent) = self else {
            return true;
        };
        let child = match child {
            Self::Known(kind) => Some(*kind),
            Self::Custom(_) | Self::Lost => None,
        };
        let none_of = |kinds: &[Kind]| child.is_none_or(|c| !kinds.contains(&c));
        match parent {
            Kind::LI => none_of(&[Kind::LI]),
            Kind::DT | Kind::DD => none_of(&[Kind::DT, Kind::DD]),
            Kind::P => none_of(ENDS_PARAGRAPH),
            Kind::COLGROUP => child == Some(Kind::COL),
            Kind::RB | Kind::RT | Kind::RP => none_of(&[Kind::RB, Kind::RT, Kind::RP]),
            Kind::OPTGROUP => none_of(&[Kind::OPTGROUP, Kind::HR]),
            Kind::OPTION => none_of(&[Kind::OPTION, Kind::OPTGROUP, Kind::HR]),
            Kind::TR => none_of(&[Kind::TR]),
            Kind::TD | Kind::TH => none_of(&[Kind::TD, Kind::TH, Kind::TR]),
            _ => true,
        }
    }
}

/// What a saved state writes for a custom element; a known one is its place
/// in [`Kind::ALL`].
const SAVED_CUSTOM: u8 = 0xFE;

/// What a saved state writes for a [`Tag::Lost`].
const SAVED_LOST: u8 = 0xFF;

/// The scanner: the elements open where it is, innermost last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    tags: Vec<Tag>,
}

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// A blank, as the grammar has them between tokens: ASCII's six -- space,
/// tab, line feed, vertical tab, form feed, carriage return.
fn is_blank(c: i32) -> bool {
    c == 0x20 || (0x09..=0x0d).contains(&c)
}

/// `c` as a character, if it is one.
fn char_of(c: i32) -> Option<char> {
    u32::try_from(c).ok().and_then(char::from_u32)
}

/// A tag's name, upper-cased as ASCII -- empty if there is none: an ASCII
/// letter, then everything up to a blank, `/`, `>` or the end.
fn scan_tag_name(lexer: &mut Lexer<'_>) -> String {
    let mut name = String::new();
    if !char_of(lexer.lookahead()).is_some_and(|c| c.is_ascii_alphabetic()) {
        return name;
    }
    while let Some(c) = char_of(lexer.lookahead())
        .filter(|&c| c != '\0' && c != '/' && c != '>' && !is_blank(c as i32))
    {
        name.push(c.to_ascii_uppercase());
        lexer.advance();
    }
    name
}

/// A comment, its `<!` taken: `--`, then everything up to and including the
/// first `>` after two dashes.
fn scan_comment(lexer: &mut Lexer<'_>) -> bool {
    for _ in 0..2 {
        if !is(lexer.lookahead(), '-') {
            return false;
        }
        lexer.advance();
    }
    let mut dashes: u32 = 0;
    while lexer.lookahead() != 0 {
        let c = lexer.lookahead();
        if is(c, '-') {
            dashes = dashes.saturating_add(1);
        } else if is(c, '>') && dashes >= 2 {
            lexer.set_result(Token::Comment.symbol());
            lexer.advance();
            lexer.mark_end();
            return true;
        } else {
            dashes = 0;
        }
        lexer.advance();
    }
    false
}

impl Scanner {
    /// A `<script>`'s or `<style>`'s text: everything up to its end tag --
    /// `</script` or `</style`, in any case, and then a blank, `/` or `>` --
    /// or to the end of the text.
    fn scan_raw_text(&self, lexer: &mut Lexer<'_>) -> bool {
        let Some(top) = self.tags.last() else {
            return false;
        };
        lexer.mark_end();
        let end: &[u8] = if top.is(Kind::SCRIPT) {
            b"</SCRIPT"
        } else {
            b"</STYLE"
        };
        let mut matched = 0;
        while lexer.lookahead() != 0 {
            let c = char_of(lexer.lookahead()).map(|c| c.to_ascii_uppercase());
            let expected = end.get(matched).map(|&b| char::from(b));
            if c.is_some() && c == expected {
                matched = matched.saturating_add(1);
                lexer.advance();
                if matched == end.len() {
                    let next = lexer.lookahead();
                    if next == 0 || is_blank(next) || is(next, '/') || is(next, '>') {
                        break;
                    }
                    // `</scripts`: text, and what follows may begin the end
                    // tag.
                    matched = 0;
                    lexer.mark_end();
                }
            } else if is(lexer.lookahead(), '<') {
                // The text ends before this `<`, unless what follows it
                // says otherwise.
                lexer.mark_end();
                matched = 1;
                lexer.advance();
            } else {
                matched = 0;
                lexer.advance();
                lexer.mark_end();
            }
        }
        lexer.set_result(Token::RawText.symbol());
        true
    }

    /// An end tag left out: the one of a void element, before anything; of
    /// an element a coming end tag closes one further down than; of one the
    /// coming element may not be inside; or at the end of the text, of
    /// `<html>`, `<head>` or `<body>`. The `<` is taken.
    fn scan_implicit_end_tag(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let closing = is(lexer.lookahead(), '/');
        if closing {
            lexer.advance();
        } else if self.tags.last().is_some_and(Tag::is_void) {
            self.tags.pop();
            lexer.set_result(Token::ImplicitEndTag.symbol());
            return true;
        }
        let name = scan_tag_name(lexer);
        if name.is_empty() && !lexer.eof() {
            return false;
        }
        let next = Tag::for_name(name);
        let end = if closing {
            // Its own element on top closes by this tag; one further down
            // is closed after every one above it.
            self.tags.last() != Some(&next) && self.tags.contains(&next)
        } else {
            self.tags.last().is_some_and(|parent| {
                !parent.can_contain(&next)
                    || ((parent.is(Kind::HTML) || parent.is(Kind::HEAD) || parent.is(Kind::BODY))
                        && lexer.eof())
            })
        };
        if end {
            self.tags.pop();
            lexer.set_result(Token::ImplicitEndTag.symbol());
        }
        end
    }

    /// A start tag's name, opening its element.
    fn scan_start_tag_name(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let name = scan_tag_name(lexer);
        if name.is_empty() {
            return false;
        }
        let tag = Tag::for_name(name);
        let token = if tag.is(Kind::SCRIPT) {
            Token::ScriptStartTagName
        } else if tag.is(Kind::STYLE) {
            Token::StyleStartTagName
        } else {
            Token::StartTagName
        };
        self.tags.push(tag);
        lexer.set_result(token.symbol());
        true
    }

    /// An end tag's name: closing the element on top, if it is that one's --
    /// otherwise an end tag in error.
    fn scan_end_tag_name(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let name = scan_tag_name(lexer);
        if name.is_empty() {
            return false;
        }
        let tag = Tag::for_name(name);
        if self.tags.last() == Some(&tag) {
            self.tags.pop();
            lexer.set_result(Token::EndTagName.symbol());
        } else {
            lexer.set_result(Token::ErroneousEndTagName.symbol());
        }
        true
    }

    /// `/>`, closing the element its tag opened.
    fn scan_self_closing_tag_delimiter(&mut self, lexer: &mut Lexer<'_>) -> bool {
        lexer.advance();
        if !is(lexer.lookahead(), '>') {
            return false;
        }
        lexer.advance();
        if self.tags.pop().is_some() {
            lexer.set_result(Token::SelfClosingTagDelimiter.symbol());
        }
        true
    }

    /// One saved tag, from `bytes` at `*at`, moving `*at` past it.
    fn saved_tag(bytes: &[u8], at: &mut usize) -> Option<Tag> {
        let byte = *bytes.get(*at)?;
        *at = at.checked_add(1)?;
        match byte {
            SAVED_LOST => Some(Tag::Lost),
            SAVED_CUSTOM => {
                let length = bytes.get(*at..at.checked_add(2)?)?;
                let length = usize::from(u16::from_le_bytes([*length.first()?, *length.get(1)?]));
                *at = at.checked_add(2)?;
                let name = bytes.get(*at..at.checked_add(length)?)?;
                *at = at.checked_add(length)?;
                Some(Tag::Custom(String::from_utf8(name.to_vec()).ok()?))
            }
            index => Kind::ALL.get(usize::from(index)).copied().map(Tag::Known),
        }
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_start_tag_name",
        "_script_start_tag_name",
        "_style_start_tag_name",
        "_end_tag_name",
        "erroneous_end_tag_name",
        "SLASH_GT",
        "_implicit_end_tag",
        "raw_text",
        "comment",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        if valid(Token::RawText) && !valid(Token::StartTagName) && !valid(Token::EndTagName) {
            return self.scan_raw_text(lexer);
        }
        while is_blank(lexer.lookahead()) {
            lexer.skip();
        }
        let c = lexer.lookahead();
        if is(c, '<') {
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '!') {
                lexer.advance();
                return scan_comment(lexer);
            }
            if valid(Token::ImplicitEndTag) {
                return self.scan_implicit_end_tag(lexer);
            }
        } else if c == 0 {
            if valid(Token::ImplicitEndTag) {
                return self.scan_implicit_end_tag(lexer);
            }
        } else if is(c, '/') {
            if valid(Token::SelfClosingTagDelimiter) {
                return self.scan_self_closing_tag_delimiter(lexer);
            }
        } else if (valid(Token::StartTagName) || valid(Token::EndTagName)) && !valid(Token::RawText)
        {
            return if valid(Token::StartTagName) {
                self.scan_start_tag_name(lexer)
            } else {
                self.scan_end_tag_name(lexer)
            };
        }
        false
    }

    /// Saved as the C saves it, in outline: how many tags were saved and
    /// how many are open (two little-endian `u16`s), then each saved tag --
    /// its place in [`Kind::ALL`], or [`SAVED_CUSTOM`] and its name's length
    /// (a `u16`) and bytes, or [`SAVED_LOST`] -- for as many as fit, from
    /// the outermost in.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let open = u16::try_from(self.tags.len()).unwrap_or(u16::MAX);
        let mut size: usize = 4;
        let mut saved: u16 = 0;
        for tag in self.tags.iter().take(usize::from(open)) {
            let start = size;
            let need = match tag {
                Tag::Custom(name) => name.len().saturating_add(3),
                Tag::Known(_) | Tag::Lost => 1,
            };
            // As the C: stop before a tag that would reach the buffer's end.
            if size.saturating_add(need) >= buffer.len() {
                break;
            }
            let written = match tag {
                Tag::Known(kind) => u8::try_from(*kind as usize)
                    .ok()
                    .and_then(|index| buffer.get_mut(start).map(|b| *b = index)),
                Tag::Lost => buffer.get_mut(start).map(|b| *b = SAVED_LOST),
                Tag::Custom(name) => u16::try_from(name.len()).ok().and_then(|length| {
                    let [low, high] = length.to_le_bytes();
                    let end = start.checked_add(need)?;
                    let out = buffer.get_mut(start..end)?;
                    let (head, rest) = out.split_at_mut(3);
                    head.copy_from_slice(&[SAVED_CUSTOM, low, high]);
                    rest.copy_from_slice(name.as_bytes());
                    Some(())
                }),
            };
            if written.is_none() {
                break;
            }
            size = start.saturating_add(need);
            saved = saved.saturating_add(1);
        }
        if let Some(head) = buffer.get_mut(..4) {
            let [s0, s1] = saved.to_le_bytes();
            let [o0, o1] = open.to_le_bytes();
            head.copy_from_slice(&[s0, s1, o0, o1]);
        }
        size.min(buffer.len())
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.tags.clear();
        let (Some(&[s0, s1]), Some(&[o0, o1])) = (
            bytes.get(0..2).and_then(|b| <&[u8; 2]>::try_from(b).ok()),
            bytes.get(2..4).and_then(|b| <&[u8; 2]>::try_from(b).ok()),
        ) else {
            return;
        };
        let (saved, open) = (u16::from_le_bytes([s0, s1]), u16::from_le_bytes([o0, o1]));
        let mut at = 4;
        for _ in 0..saved {
            let Some(tag) = Self::saved_tag(bytes, &mut at) else {
                break;
            };
            self.tags.push(tag);
        }
        // The tags that did not fit, in their places.
        while self.tags.len() < usize::from(open) {
            self.tags.push(Tag::Lost);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a parse that fails, or a slice out of range, is the failure"
)]
mod tests {
    use super::*;
    use crate::ffi::SERIALIZATION_BUFFER_SIZE;

    /// `text`'s tree.
    fn tree(text: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap()
    }

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        tree(text).root_node().to_sexp()
    }

    /// Every node of `kind` in `tree`, as the text it covers.
    fn texts<'t>(tree: &tree_sitter::Tree, text: &'t str, kind: &str) -> Vec<&'t str> {
        let mut out = Vec::new();
        let mut cursor = tree.walk();
        let mut ascending = false;
        loop {
            if !ascending && cursor.node().kind() == kind {
                out.push(&text[cursor.node().byte_range()]);
            }
            if !ascending && cursor.goto_first_child() {
                continue;
            }
            if cursor.goto_next_sibling() {
                ascending = false;
            } else if cursor.goto_parent() {
                ascending = true;
            } else {
                return out;
            }
        }
    }

    /// **A saved state comes back whole**: known elements, custom ones by
    /// their whole names -- past 255 bytes, which the C cut -- and ones lost
    /// before.
    #[test]
    fn a_saved_state_comes_back_whole() {
        let long = "X-".repeat(200);
        let scanner = Scanner {
            tags: vec![
                Tag::Known(Kind::HTML),
                Tag::Custom("MY-ÉLÉMENT".into()),
                Tag::Lost,
                Tag::Custom(long),
                Tag::Known(Kind::WBR),
                Tag::Known(Kind::VIDEO),
            ],
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        let mut back = Scanner::default();
        back.deserialize(&buffer[..size]);
        assert_eq!(back, scanner);
        back.deserialize(&[]);
        assert_eq!(back, Scanner::default());
    }

    /// **Tags past what the buffer holds are lost, in their places**: the
    /// stack keeps its depth, as the C keeps it.
    #[test]
    fn tags_past_the_buffer_are_lost_in_their_places() {
        let scanner = Scanner {
            tags: (0..3000).map(|_| Tag::Known(Kind::DIV)).collect(),
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        assert!(size < SERIALIZATION_BUFFER_SIZE);
        let mut back = Scanner::default();
        back.deserialize(&buffer[..size]);
        assert_eq!(back.tags.len(), 3000);
        assert_eq!(back.tags[0], Tag::Known(Kind::DIV));
        assert_eq!(back.tags[2999], Tag::Lost);
        let kept = back
            .tags
            .iter()
            .filter(|t| **t == Tag::Known(Kind::DIV))
            .count();
        assert_eq!(kept, SERIALIZATION_BUFFER_SIZE - 5);
    }

    /// **A tag's name is what HTML's tokenizer takes for one**: an `_`, a `.`
    /// or a letter past ASCII goes on with it, and its end tag closes it in
    /// any case of its ASCII letters -- HTML's names are case-blind in ASCII
    /// only, so `</MY-ÉLÉMENT>` is no end tag of `<my-élément>`.
    #[test]
    fn a_tag_name_is_what_html_takes_for_one() {
        for name in ["my_widget", "x.y", "my-élément", "svg:rect", "a-ñandú"] {
            let end = name.to_ascii_uppercase();
            let text = format!("<{name} id=a>x</{end}>\n");
            let tree = tree(&text);
            assert_eq!(
                texts(&tree, &text, "tag_name"),
                [name, &end[..]],
                "{}",
                tree.root_node().to_sexp()
            );
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
        }
        let text = "<my-élément>x</MY-ÉLÉMENT></my-élément>\n";
        let tree = tree(text);
        assert_eq!(texts(&tree, text, "erroneous_end_tag_name"), ["MY-ÉLÉMENT"]);
        assert_eq!(texts(&tree, text, "tag_name"), ["my-élément", "my-élément"]);
    }

    /// **A paragraph ends where the HTML standard ends one** -- before a
    /// `<blockquote>` or a `<figcaption>`, which the C did not know by name,
    /// and before a `<ul>`, a `<table>` and the rest of the standard's list,
    /// which the C's lacked -- and not before an element the list does not
    /// name.
    #[test]
    fn a_paragraph_ends_where_the_standard_ends_one() {
        for closer in [
            "blockquote",
            "figcaption",
            "ul",
            "table",
            "menu",
            "dialog",
            "hgroup",
            "search",
            "div",
        ] {
            let text = format!("<p>One<{closer}>Two</{closer}>\n");
            assert_eq!(
                sexp(&text),
                "(document (element (start_tag (tag_name)) (text)) (element (start_tag (tag_name)) (text) (end_tag (tag_name))))",
                "{closer}"
            );
        }
        assert_eq!(
            sexp("<p>One<span>Two</span></p>\n"),
            "(document (element (start_tag (tag_name)) (text) (element (start_tag (tag_name)) (text) (end_tag (tag_name))) (end_tag (tag_name))))"
        );
    }

    /// **An option ends at the next**, or at an `<optgroup>`, as the
    /// standard has it: a `<select>` of options with no end tags is a list,
    /// not a staircase.
    #[test]
    fn an_option_ends_at_the_next() {
        assert_eq!(
            sexp("<select><option>A<option>B<optgroup><option>C</select>\n"),
            "(document (element (start_tag (tag_name)) (element (start_tag (tag_name)) (text)) (element (start_tag (tag_name)) (text)) (element (start_tag (tag_name)) (element (start_tag (tag_name)) (text))) (end_tag (tag_name))))"
        );
    }

    /// **An end tag closes the elements above its own, and nothing when its
    /// own is not open**: `</x-b>`, with only an `<x-a>` open, is an end tag
    /// in error -- where the C closed the `<x-a>`, every custom element being
    /// one kind to it -- and the `</x-a>` after it closes the `<x-a>`.
    #[test]
    fn an_end_tag_closes_only_its_own_element() {
        let text = "<x-a><b>1</x-b>2</b></x-a>\n";
        let tree = tree(text);
        let root = tree.root_node();
        assert_eq!(texts(&tree, text, "erroneous_end_tag_name"), ["x-b"]);
        let outer = root.named_child(0).unwrap();
        assert_eq!(outer.kind(), "element");
        assert_eq!(
            &text[outer.byte_range()],
            "<x-a><b>1</x-b>2</b></x-a>",
            "{}",
            root.to_sexp()
        );
        // An end tag for an element further down closes what is above it.
        assert_eq!(
            sexp("<div><span>1</div>\n"),
            "(document (element (start_tag (tag_name)) (element (start_tag (tag_name)) (text)) (end_tag (tag_name))))"
        );
    }

    /// **A script's text ends only at its end tag**: not at `</scripts`,
    /// and at `</script>` even when a `<` comes just before it.
    #[test]
    fn a_scripts_text_ends_only_at_its_end_tag() {
        for (text, raw) in [
            ("<script>a <</script>\n", "a <"),
            ("<script>s = '</scripts>';</script>\n", "s = '</scripts>';"),
            ("<script>x << 1</SCRIPT >\n", "x << 1"),
            ("<style>a {}</style>\n", "a {}"),
        ] {
            let tree = tree(text);
            assert_eq!(
                texts(&tree, text, "raw_text"),
                [raw],
                "{}",
                tree.root_node().to_sexp()
            );
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
        }
    }

    /// **A tag's name begins with a letter**, as HTML's tokenizer has it:
    /// `<?xml ...?>` opens no element -- which, never closed, would take in
    /// the rest of the document -- and a name ends at the `/` of `/>`.
    #[test]
    fn a_tag_name_begins_with_a_letter_and_ends_at_a_slash() {
        let text = "<?xml version=\"1.0\"?>\n<p>x</p>\n";
        let declared = tree(text);
        assert_eq!(
            texts(&declared, text, "tag_name"),
            ["p", "p"],
            "{}",
            declared.root_node().to_sexp()
        );
        let text = "<x-y/><br/>\n";
        let closed = tree(text);
        assert_eq!(texts(&closed, text, "tag_name"), ["x-y", "br"]);
        assert!(
            !closed.root_node().has_error(),
            "{}",
            closed.root_node().to_sexp()
        );
    }

    /// **An `<hr>` ends an open `<option>` and its `<optgroup>`**, as the
    /// standard has it.
    #[test]
    fn an_hr_ends_an_option_and_its_group() {
        assert_eq!(
            sexp("<select><optgroup><option>A<hr><option>B</select>\n"),
            "(document (element (start_tag (tag_name)) (element (start_tag (tag_name)) (element (start_tag (tag_name)) (text))) (element (start_tag (tag_name))) (element (start_tag (tag_name)) (text)) (end_tag (tag_name))))"
        );
    }

    /// **A comment ends at `-->`, not at a `->` inside it.**
    #[test]
    fn a_comment_ends_at_two_dashes_and_a_bracket() {
        let text = "<!-- a->b --><p>x</p>\n";
        let tree = tree(text);
        assert_eq!(texts(&tree, text, "comment"), ["<!-- a->b -->"]);
    }
}
