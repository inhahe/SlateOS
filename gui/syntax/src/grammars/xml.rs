//! XML: tree-sitter-xml 0.7.0 (MIT, ObserverOfTime and Amaan Qureshi), whose
//! package holds two grammars -- this one, and [`super::dtd`] for a document
//! type definition on its own. `grammars/xml/` holds XML's `parser.c`, its
//! highlight query and highlight test, the package's test corpus (the DTD
//! grammar's examples among it), and its `scanner.c` and the
//! `common/scanner.h` both grammars' scanners share, as published; the
//! scanners are ported below and in [`super::dtd`].
//!
//! XML's scanner keeps the elements open where it is -- a stack of names,
//! saved with the parse's state -- and from them says which end tag closes
//! which element. Both scanners take a processing instruction's target and
//! text (`<?target text?>`) and a comment; XML's takes the text between tags
//! and a CDATA section's too.
//!
//! # Where the port differs
//!
//! - **A name is XML's**: a `NameStartChar`, then `NameChar`s, as the XML
//!   specification lists them -- what the C's own comments say it meant to
//!   follow. The C asked the C library's `iswalpha` and `iswalnum`, which on
//!   most systems know only ASCII's letters, so `<données>` was no tag; and
//!   it took a name that starts with a digit, `-` or `.` (`<1a>`). (The
//!   grammar's own names -- an attribute's -- are still ASCII's, and `·`:
//!   its tables say so.)
//! - **A processing instruction's text runs to its first `?>`**, over line
//!   ends and past a `?` that no `>` follows, as XML has it. The C stopped
//!   at a line's end and at the first `?`, and took the text only where
//!   `?>` then ended its line: `<?php echo 1; ?><a/>` and an instruction
//!   over two lines were errors. So one example of the package's corpus --
//!   `<?bar is ?> invalid?>`, which XML reads as an instruction and then
//!   text -- is left out of the corpus test.
//! - **A target is `xml` only when it is**: `xml`, in any case, is kept for
//!   the declaration, and `xml-model` and `xml-stylesheet` are the grammar's
//!   own words where it has a place for them -- whole names, all three. The
//!   C took `xml-modelfoo` for `xml-model`, and a target that began `xm`
//!   took in the character after it (`<?xm?>` had the target `xm?`).
//! - **A CDATA section ends at its first `]]>`**: the C read an empty one,
//!   `<![CDATA[]]>`, on to the next section's `]]>`, taking in everything
//!   between. A section whose text ends in `]` (`<![CDATA[a]]]>`) is still
//!   an error, as it was: the text would have to end before the run's last
//!   two `]`, which a scanner that can look one character ahead cannot know
//!   in time.
//! - **Text between tags ends before a `]]>`**, which XML allows nowhere in
//!   it: the `]]>` is the error, and the text before it is still text. The C
//!   refused the whole text instead, and took a `]]>` at its start as text.
//! - **A saved element keeps its whole name**: the C saved at most 255 bytes
//!   of one, and an element restored under the cut name was closed by no end
//!   tag. An element whose name did not fit in the saved state at all is
//!   closed by whichever end tag comes; the C's was closed by none.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("xml", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/xml/highlights.scm");

/// The external tokens, in the grammar's order. The first three are the
/// DTD grammar's too, in the same places ([`PI_TARGET`], [`PI_CONTENT`],
/// [`COMMENT`]).
#[derive(Clone, Copy)]
enum Token {
    PiTarget,
    PiContent,
    Comment,
    CharData,
    CData,
    XmlModel,
    XmlStylesheet,
    StartTagName,
    EndTagName,
    /// Never produced: an end tag no open element has is no token, as in the
    /// C, which named it but returned none -- the grammar has no place for
    /// one. Here for its place in the list.
    #[expect(dead_code, reason = "kept for the tokens' order, never produced")]
    ErroneousEndName,
    SelfClosingTagDelimiter,
}

impl Token {
    fn symbol(self) -> u16 {
        self as u16
    }
}

/// A processing instruction's target: the first external token of both
/// grammars.
pub(super) const PI_TARGET: u16 = 0;
/// A processing instruction's text: the second of both.
pub(super) const PI_CONTENT: u16 = 1;
/// A comment: the third of both.
pub(super) const COMMENT: u16 = 2;

/// Whether `c` is the character `ch`.
pub(super) fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

/// Whether `c` may start a name: XML's `NameStartChar`.
pub(super) fn is_name_start(c: i32) -> bool {
    u32::try_from(c).is_ok_and(|c| {
        matches!(
            c,
            0x3A | 0x41..=0x5A
                | 0x5F
                | 0x61..=0x7A
                | 0xC0..=0xD6
                | 0xD8..=0xF6
                | 0xF8..=0x2FF
                | 0x370..=0x37D
                | 0x37F..=0x1FFF
                | 0x200C..=0x200D
                | 0x2070..=0x218F
                | 0x2C00..=0x2FEF
                | 0x3001..=0xD7FF
                | 0xF900..=0xFDCF
                | 0xFDF0..=0xFFFD
                | 0x1_0000..=0xE_FFFF
        )
    })
}

/// Whether `c` may go on a name: XML's `NameChar`.
pub(super) fn is_name_char(c: i32) -> bool {
    is_name_start(c)
        || u32::try_from(c).is_ok_and(|c| {
            matches!(
                c,
                0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040
            )
        })
}

/// A name, as it is -- empty where the next character cannot start one.
fn scan_name(lexer: &mut Lexer<'_>) -> String {
    let mut name = String::new();
    if !is_name_start(lexer.lookahead()) {
        return name;
    }
    while is_name_char(lexer.lookahead()) {
        if let Some(c) = u32::try_from(lexer.lookahead())
            .ok()
            .and_then(char::from_u32)
        {
            name.push(c);
        }
        lexer.advance();
    }
    name
}

/// A processing instruction's target, its `<?` taken: a name -- but not
/// `xml`, in any case, which only the declaration may use (the grammar
/// reads it), nor a name `keyword` says the grammar has a word of its own
/// for here (`xml-model`, `xml-stylesheet`).
pub(super) fn scan_pi_target(lexer: &mut Lexer<'_>, keyword: impl Fn(&str) -> bool) -> bool {
    let name = scan_name(lexer);
    if name.is_empty() || name.eq_ignore_ascii_case("xml") || keyword(&name) {
        return false;
    }
    lexer.mark_end();
    lexer.set_result(PI_TARGET);
    true
}

/// A processing instruction's text: everything up to its first `?>` --
/// over line ends, and past a `?` no `>` follows -- none if the text ends
/// first. It may be empty (`<?target ?>`).
pub(super) fn scan_pi_content(lexer: &mut Lexer<'_>) -> bool {
    while !lexer.eof() {
        if is(lexer.lookahead(), '?') {
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '>') {
                lexer.set_result(PI_CONTENT);
                return true;
            }
        } else {
            lexer.advance();
        }
    }
    false
}

/// A comment, its `<!` taken: `--`, then everything up to the first `--`,
/// which must be followed by `>` -- XML allows no `--` inside a comment --
/// none if the text ends first.
pub(super) fn scan_comment(lexer: &mut Lexer<'_>) -> bool {
    for _ in 0..2 {
        if lexer.eof() || !is(lexer.lookahead(), '-') {
            return false;
        }
        lexer.advance();
    }
    while !lexer.eof() {
        if is(lexer.lookahead(), '-') {
            lexer.advance();
            if is(lexer.lookahead(), '-') {
                lexer.advance();
                break;
            }
        } else {
            lexer.advance();
        }
    }
    if is(lexer.lookahead(), '>') {
        lexer.advance();
        lexer.mark_end();
        lexer.set_result(COMMENT);
        return true;
    }
    false
}

/// How a scan for a run of text went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// Taken, its token's end marked.
    Taken,
    /// Nothing here, and nothing read: the scan may go on to other tokens.
    Nothing,
    /// Refused after reading on: no other token starts where it did.
    Refused,
}

/// A run of `]` at the lexer, the end marked before it: whether it is the
/// start of a `]]>` -- two or more, then `>` -- rather than text.
fn ends_at_run(lexer: &mut Lexer<'_>) -> bool {
    lexer.mark_end();
    let mut run: u32 = 0;
    while is(lexer.lookahead(), ']') {
        lexer.advance();
        run = run.saturating_add(1);
    }
    run >= 2 && is(lexer.lookahead(), '>')
}

/// Text between tags: everything up to a `<`, an `&` or the end, and before
/// a `]]>`, which XML allows nowhere in it -- the text before one is text,
/// and the `]]>` is left for the parser to find no place for.
fn scan_char_data(lexer: &mut Lexer<'_>) -> Text {
    let mut taken = false;
    loop {
        let c = lexer.lookahead();
        if lexer.eof() || is(c, '<') || is(c, '&') {
            break;
        }
        if is(c, ']') {
            if ends_at_run(lexer) {
                if taken {
                    lexer.set_result(Token::CharData.symbol());
                    return Text::Taken;
                }
                return Text::Refused;
            }
        } else {
            lexer.advance();
        }
        taken = true;
    }
    if taken {
        lexer.mark_end();
        lexer.set_result(Token::CharData.symbol());
        return Text::Taken;
    }
    Text::Nothing
}

/// A CDATA section's text, its `<![CDATA[` taken: everything up to its
/// first `]]>` -- nothing if that comes at once (the section is empty: the
/// grammar reads the `]]>`), and nothing if the text ends first.
fn scan_cdata(lexer: &mut Lexer<'_>) -> Text {
    let mut taken = false;
    while !lexer.eof() {
        if is(lexer.lookahead(), ']') {
            if ends_at_run(lexer) {
                if taken {
                    lexer.set_result(Token::CData.symbol());
                    return Text::Taken;
                }
                return Text::Refused;
            }
        } else {
            lexer.advance();
        }
        taken = true;
    }
    if taken { Text::Refused } else { Text::Nothing }
}

/// How a saved state marks an element whose name did not fit: in the place
/// of its name's length.
const SAVED_LOST: u16 = u16::MAX;

/// An element open where the scanner is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Open {
    /// By its name.
    Named(String),
    /// One whose name did not fit in the state last saved: closed by
    /// whichever end tag comes (see the module docs).
    Lost,
}

/// The scanner's state: the elements open where it is, outermost first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanner {
    tags: Vec<Open>,
}

impl Scanner {
    /// A start tag's name, its `<` taken: the element is open from here.
    fn scan_start_tag_name(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let name = scan_name(lexer);
        if name.is_empty() {
            return false;
        }
        lexer.set_result(Token::StartTagName.symbol());
        self.tags.push(Open::Named(name));
        true
    }

    /// An end tag's name, its `</` taken -- only the innermost open
    /// element's (or any, for one whose name was lost), which it closes. A
    /// name no open element has is no token: the grammar has no place for
    /// one, and the parse finds the error.
    fn scan_end_tag_name(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let name = scan_name(lexer);
        if name.is_empty() {
            return false;
        }
        match self.tags.last() {
            Some(Open::Named(open)) if *open == name => {}
            Some(Open::Lost) => {}
            _ => return false,
        }
        self.tags.pop();
        lexer.set_result(Token::EndTagName.symbol());
        true
    }

    /// A start tag's closing `/>`: the element it opened is closed with it.
    fn scan_self_closing_tag_delimiter(&mut self, lexer: &mut Lexer<'_>) -> bool {
        lexer.advance();
        if lexer.eof() || !is(lexer.lookahead(), '>') {
            return false;
        }
        lexer.advance();
        self.tags.pop();
        lexer.set_result(Token::SelfClosingTagDelimiter.symbol());
        true
    }

    /// The saved element at `at` in `bytes`, moving `at` past it.
    fn saved_tag(bytes: &[u8], at: &mut usize) -> Option<Open> {
        let length = bytes.get(*at..at.checked_add(2)?)?;
        let length = u16::from_le_bytes([*length.first()?, *length.get(1)?]);
        *at = at.checked_add(2)?;
        if length == SAVED_LOST {
            return Some(Open::Lost);
        }
        let end = at.checked_add(usize::from(length))?;
        let name = core::str::from_utf8(bytes.get(*at..end)?).ok()?.to_owned();
        *at = end;
        Some(Open::Named(name))
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "PITarget",
        "_pi_content",
        "Comment",
        "CharData",
        "CData",
        "xml_DASHmodel",
        "xml_DASHstylesheet",
        "_start_tag_name",
        "_end_tag_name",
        "_erroneous_end_name",
        "SLASH_GT",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: Token| valid_symbols.get(t as usize).copied().unwrap_or(false);
        // Error recovery asks for every token at once: nothing is sure
        // enough to say then.
        if [
            Token::PiTarget,
            Token::PiContent,
            Token::Comment,
            Token::CharData,
            Token::CData,
        ]
        .into_iter()
        .all(valid)
        {
            return false;
        }
        if valid(Token::PiTarget) {
            return scan_pi_target(lexer, |name| {
                (name == "xml-model" && valid(Token::XmlModel))
                    || (name == "xml-stylesheet" && valid(Token::XmlStylesheet))
            });
        }
        if valid(Token::PiContent) {
            return scan_pi_content(lexer);
        }
        if valid(Token::CharData) {
            match scan_char_data(lexer) {
                Text::Taken => return true,
                Text::Refused => return false,
                Text::Nothing => {}
            }
        }
        if valid(Token::CData) {
            match scan_cdata(lexer) {
                Text::Taken => return true,
                Text::Refused => return false,
                Text::Nothing => {}
            }
        }
        let c = lexer.lookahead();
        if is(c, '<') {
            lexer.mark_end();
            lexer.advance();
            if is(lexer.lookahead(), '!') {
                lexer.advance();
                return scan_comment(lexer);
            }
            return false;
        }
        if is(c, '/') {
            return valid(Token::SelfClosingTagDelimiter)
                && self.scan_self_closing_tag_delimiter(lexer);
        }
        if lexer.eof() {
            return false;
        }
        if valid(Token::StartTagName) {
            return self.scan_start_tag_name(lexer);
        }
        if valid(Token::EndTagName) {
            return self.scan_end_tag_name(lexer);
        }
        false
    }

    /// How many elements were saved and how many are open (two
    /// little-endian `u16`s), then each saved element from the outermost
    /// in -- its name's length (a `u16`) and bytes, or [`SAVED_LOST`] -- for
    /// as many as fit; the rest are restored lost, in their places.
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let open = u16::try_from(self.tags.len()).unwrap_or(u16::MAX);
        let mut size: usize = 4;
        let mut saved: u16 = 0;
        for tag in self.tags.iter().take(usize::from(open)) {
            let name = match tag {
                Open::Named(name) if u16::try_from(name.len()).is_ok_and(|n| n < SAVED_LOST) => {
                    Some(name.as_str())
                }
                Open::Named(_) | Open::Lost => None,
            };
            let need = name.map_or(2, |name| name.len().saturating_add(2));
            let Some(end) = size.checked_add(need).filter(|&end| end <= buffer.len()) else {
                break;
            };
            let Some(out) = buffer.get_mut(size..end) else {
                break;
            };
            let length = name.map_or(SAVED_LOST, |name| {
                u16::try_from(name.len()).unwrap_or(SAVED_LOST)
            });
            let (head, rest) = out.split_at_mut(2);
            head.copy_from_slice(&length.to_le_bytes());
            if let Some(name) = name {
                rest.copy_from_slice(name.as_bytes());
            }
            size = end;
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
        // The elements that did not fit, in their places.
        while self.tags.len() < usize::from(open) {
            self.tags.push(Open::Lost);
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

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// **A saved state comes back whole**: names past 255 bytes, which the
    /// C cut, names past ASCII, and elements lost before.
    #[test]
    fn a_saved_state_comes_back_whole() {
        let scanner = Scanner {
            tags: vec![
                Open::Named("root".into()),
                Open::Named("données".into()),
                Open::Lost,
                Open::Named("x".repeat(300)),
                Open::Named(String::new()),
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

    /// **Elements past what the buffer holds are lost, in their places**:
    /// the stack keeps its depth.
    #[test]
    fn elements_past_the_buffer_are_lost_in_their_places() {
        let scanner = Scanner {
            tags: (0..40).map(|i| Open::Named(format!("{i:0>40}"))).collect(),
        };
        let mut buffer = [0u8; SERIALIZATION_BUFFER_SIZE];
        let size = scanner.serialize(&mut buffer);
        assert!(size <= SERIALIZATION_BUFFER_SIZE);
        let mut back = Scanner::default();
        back.deserialize(&buffer[..size]);
        assert_eq!(back.tags.len(), 40);
        let kept = back.tags.iter().take_while(|t| **t != Open::Lost).count();
        assert!((20..40).contains(&kept), "{kept}");
        assert_eq!(back.tags[..kept], scanner.tags[..kept]);
        assert!(back.tags[kept..].iter().all(|t| *t == Open::Lost));
    }

    /// **A name is XML's**: letters past ASCII start one and go on it; a
    /// digit, `-`, `.` or `·` go on one but start none.
    #[test]
    fn a_name_is_xmls() {
        for c in ['a', 'Z', '_', ':', 'é', 'ж', '中', '\u{10000}'] {
            assert!(is_name_start(c as i32), "{c:?}");
        }
        for c in [
            '1', '-', '.', '\u{b7}', '\u{300}', ' ', '<', '>', '/', '=', '\u{d7}',
        ] {
            assert!(!is_name_start(c as i32), "{c:?}");
        }
        for c in ['1', '-', '.', '\u{b7}', '\u{300}', '\u{203f}', 'é'] {
            assert!(is_name_char(c as i32), "{c:?}");
        }
        for c in [' ', '<', '>', '/', '=', '\u{d7}', '?'] {
            assert!(!is_name_char(c as i32), "{c:?}");
        }
        assert!(!is_name_start(-1) && !is_name_char(-1));
        let tree = sexp("<données><élément/></données>");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(sexp("<1a></1a>").contains("ERROR"));
    }

    /// **Every range of XML's name characters, at both ends**: the first
    /// and last of each start a name (or go on one), and the characters
    /// either side of each do not.
    #[test]
    fn every_range_of_name_characters_holds_at_both_ends() {
        let starts: [(i32, i32); 16] = [
            (0x3A, 0x3A),
            (0x41, 0x5A),
            (0x5F, 0x5F),
            (0x61, 0x7A),
            (0xC0, 0xD6),
            (0xD8, 0xF6),
            (0xF8, 0x2FF),
            (0x370, 0x37D),
            (0x37F, 0x1FFF),
            (0x200C, 0x200D),
            (0x2070, 0x218F),
            (0x2C00, 0x2FEF),
            (0x3001, 0xD7FF),
            (0xF900, 0xFDCF),
            (0xFDF0, 0xFFFD),
            (0x1_0000, 0xE_FFFF),
        ];
        for (low, high) in starts {
            assert!(
                is_name_start(low) && is_name_start(high),
                "{low:#x}..={high:#x}"
            );
            assert!(
                is_name_char(low) && is_name_char(high),
                "{low:#x}..={high:#x}"
            );
        }
        for outside in [
            0x39, 0x3B, 0x40, 0x5B, 0x5E, 0x60, 0x7B, 0xBF, 0xD7, 0xF7, 0x37E, 0x2000, 0x200B,
            0x200E, 0x206F, 0x2190, 0x2BFF, 0x2FF0, 0x3000, 0xD800, 0xF8FF, 0xFDD0, 0xFDEF, 0xFFFE,
            0xF_0000,
        ] {
            assert!(!is_name_start(outside), "{outside:#x}");
        }
        let more: [(i32, i32); 6] = [
            (0x2D, 0x2D),
            (0x2E, 0x2E),
            (0x30, 0x39),
            (0xB7, 0xB7),
            (0x300, 0x36F),
            (0x203F, 0x2040),
        ];
        for (low, high) in more {
            for c in [low, high] {
                assert!(is_name_char(c) && !is_name_start(c), "{c:#x}");
            }
        }
        for outside in [0x2C, 0x2F, 0xB6, 0xB8, 0x203E, 0x2041, 0xD7, 0x20] {
            assert!(!is_name_char(outside), "{outside:#x}");
        }
    }

    /// **A processing instruction's text runs to its first `?>`**: over
    /// line ends, past a `?`, and with more on its line after it.
    #[test]
    fn an_instructions_text_runs_to_its_first_end() {
        for text in [
            "<?php echo 1; ?><a/>",
            "<a><?php\necho 1;\n?></a>",
            "<a><?q is this one? yes?></a>",
            "<a><?empty ?></a>",
            "<a><?bar is ?> text?></a>",
        ] {
            let tree = sexp(text);
            assert!(
                !tree.contains("ERROR") && !tree.contains("MISSING"),
                "{text}: {tree}"
            );
            assert!(tree.contains("(PI (PITarget)"), "{text}: {tree}");
        }
        assert!(sexp("<a><?open to the end</a>").contains("ERROR"));
    }

    /// **A target is `xml` only when it is**: `xml` in any case is the
    /// declaration's; a name that only begins so is a target; `xml-model`
    /// and `xml-stylesheet` are the grammar's words -- and only whole.
    #[test]
    fn a_target_is_xml_only_when_it_is() {
        assert!(sexp("<?XmL version=\"1.0\"?><a/>").contains("ERROR"));
        for text in ["<?xmlfoo x?><a/>", "<?xm?><a/>", "<?xml-modelx a?><a/>"] {
            let tree = sexp(text);
            assert!(!tree.contains("ERROR"), "{text}: {tree}");
            assert!(tree.contains("(PI (PITarget)"), "{text}: {tree}");
        }
        let tree = sexp("<?xml-model href=\"a.rng\"?><a/>");
        assert!(tree.contains("XmlModelPI"), "{tree}");
    }

    /// **A CDATA section ends at its first `]]>`**: an empty one reads no
    /// further, and one after it is its own.
    #[test]
    fn a_cdata_section_ends_at_its_first_end() {
        let tree = sexp("<a><![CDATA[]]><b/><![CDATA[x]]></a>");
        assert!(!tree.contains("ERROR"), "{tree}");
        assert!(tree.contains("(element (EmptyElemTag (Name)))"), "{tree}");
        assert_eq!(tree.matches("(CDSect").count(), 2, "{tree}");
        assert_eq!(tree.matches("(CData)").count(), 1, "{tree}");
        let tree = sexp("<a><![CDATA[ <not> & a tag ]]></a>");
        assert!(
            !tree.contains("ERROR") && tree.contains("(CData)"),
            "{tree}"
        );
    }

    /// **Text ends before a `]]>`**: the text before it is still text, and
    /// the `]]>` is the error -- at the text's start as well.
    #[test]
    fn text_ends_before_a_cdata_end() {
        let tree = sexp("<a>fine ]] and ] too, ]> and ]]x></a>");
        assert!(!tree.contains("ERROR"), "{tree}");
        for text in [
            "<a>before ]]> after</a>",
            "<a>]]> after</a>",
            "<a>x]]]>y</a>",
        ] {
            let tree = sexp(text);
            assert!(tree.contains("ERROR"), "{text}: {tree}");
        }
        assert!(sexp("<a>before ]]> after</a>").contains("(CharData)"));
    }

    /// **An end tag closes the innermost element, and only by its name**;
    /// an element whose name was lost is closed by whichever comes.
    #[test]
    fn an_end_tag_closes_the_innermost_element_by_its_name() {
        assert!(!sexp("<a><b></b></a>").contains("ERROR"));
        assert!(sexp("<a><b></a></b>").contains("ERROR"));
        assert!(sexp("<From>Jani</from>").contains("ERROR"));
        // Forty elements whose names do not all fit in a saved state:
        // the innermost are lost, and closed by their end tags all the same.
        let names: Vec<String> = (0..40).map(|i| format!("e{i:0>39}")).collect();
        let mut text = String::new();
        for name in &names {
            text.push_str(&format!("<{name}>"));
        }
        for name in names.iter().rev() {
            text.push_str(&format!("</{name}>"));
        }
        let tree = sexp(&text);
        assert!(!tree.contains("ERROR"), "{tree}");
    }

    /// **A comment is `<!--` to the first `--`, and that must end it.**
    #[test]
    fn a_comment_ends_at_its_first_double_dash() {
        assert!(!sexp("<a><!-- fine - here --></a>").contains("ERROR"));
        assert!(!sexp("<a><!----></a>").contains("ERROR"));
        assert!(sexp("<a><!-- not -- fine --></a>").contains("ERROR"));
        assert!(sexp("<a><!-- open</a>").contains("ERROR"));
    }
}
