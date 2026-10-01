//! The pattern, read into a tree: glibc 2.39's grammar, for every syntax a
//! `reg_syntax_t` can describe -- the two `regcomp` chooses between,
//! RE_SYNTAX_POSIX_BASIC and RE_SYNTAX_POSIX_EXTENDED, and whatever bits a
//! program gives `re_compile_pattern` (regcomp.c's `peek_token`,
//! `parse_reg_exp`, `parse_branch`, `parse_expression`, `parse_sub_exp`,
//! `parse_dup_op`, `fetch_number` and `parse_bracket_exp`).
//!
//! POSIX leaves undefined most of what a parser has to decide -- what `a**`
//! is, whether `^` in the middle of a BRE is an anchor, what an unmatched
//! `)` means, which error a malformed interval earns -- and the GNU syntaxes
//! are glibc's own; there glibc's answer is this one's, found out by the
//! oracles (`posix/tools/oracle/regex_harness.py`, every pair of 75 tokens as
//! an ERE and as a BRE; `regex_gnu_harness.py`, every pair under each syntax
//! bit and the programs' syntaxes). The one place this reads a pattern
//! otherwise is the case of a letter: glibc upper-cases the pattern under
//! RE_ICASE before parsing it, which turns `[Z-a]` into the reversed range
//! `[Z-A]` (refused) and `[a-Z]` into `[A-Z]` (accepted), and leaves `\a`
//! as written, so that it matches no `a` at all; this parses the pattern as
//! written and has each letter match itself and its other case, which is
//! what XBD 9.2 says REG_ICASE means. A translate table (`translate`, for
//! `re_compile_pattern`) is applied as glibc applies it -- to every byte the
//! grammar reads, before it decides what the byte is -- but for the same
//! `\a`: glibc leaves an escaped letter untranslated, so that it can never
//! match a translated subject, where this translates it, as glibc's header
//! says the table is applied "to a pattern when it is compiled".
//!
//! glibc's parser recurses once a level of nesting, and so a pattern of a
//! hundred thousand `(` can overflow its stack. This one keeps an explicit
//! stack of the groups open (`Frame`), and makes the tree's nodes children
//! first, so that everything done with the tree afterwards is a loop over
//! its nodes in order.

use super::{
    RE_BACKSLASH_ESCAPE_IN_LISTS, RE_BK_PLUS_QM, RE_CARET_ANCHORS_HERE, RE_CHAR_CLASSES,
    RE_CONTEXT_INDEP_ANCHORS, RE_CONTEXT_INDEP_OPS, RE_CONTEXT_INVALID_DUP, RE_CONTEXT_INVALID_OPS,
    RE_DOT_NEWLINE, RE_DOT_NOT_NULL, RE_HAT_LISTS_NOT_NEWLINE, RE_ICASE, RE_INTERVALS,
    RE_INVALID_INTERVAL_ORD, RE_LIMITED_OPS, RE_NEWLINE_ALT, RE_NO_BK_BRACES, RE_NO_BK_PARENS,
    RE_NO_BK_REFS, RE_NO_BK_VBAR, RE_NO_EMPTY_RANGES, RE_NO_GNU_OPS, RE_SYNTAX_POSIX_BASIC,
    RE_SYNTAX_POSIX_EXTENDED, RE_UNMATCHED_RIGHT_PAREN_ORD, REG_BADBR, REG_BADPAT, REG_BADRPT,
    REG_EBRACE, REG_EBRACK, REG_ECOLLATE, REG_ECTYPE, REG_EESCAPE, REG_EPAREN, REG_ERANGE,
    REG_ERPAREN, REG_ESIZE, REG_ESPACE, REG_ESUBREG, REG_EXTENDED, REG_ICASE, REG_NEWLINE,
};
use crate::list::{List, NoMem};

/// No limit on a repetition's count; also "no node" where one is optional.
pub(super) const NONE: u32 = u32::MAX;

/// The largest count an interval may give: glibc's RE_DUP_MAX, 2^15 - 1.
pub(super) const RE_DUP_MAX: u32 = 0x7FFF;

/// A `regcomp` error code, the answer of everything here that can fail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Code(pub(super) i32);

impl From<NoMem> for Code {
    fn from(_: NoMem) -> Self {
        Self(REG_ESPACE)
    }
}

type R<T> = Result<T, Code>;

/// A zero-width test of where in the string a match stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Assert {
    /// `^`: the start of the string (unless REG_NOTBOL), or after a newline
    /// under REG_NEWLINE.
    Bol,
    /// `$`: the end (unless REG_NOTEOL), or before a newline under
    /// REG_NEWLINE.
    Eol,
    /// `` \` ``: the start of the string, whatever REG_NOTBOL says.
    BufStart,
    /// `\'`: the end of the string, whatever REG_NOTEOL says.
    BufEnd,
    /// `\<`: a word character follows and none precedes.
    WordStart,
    /// `\>`: one precedes and none follows.
    WordEnd,
    /// `\b`: either.
    WordBoundary,
    /// `\B`: neither -- a word character on both sides, or on neither.
    NotWordBoundary,
}

/// A node of the tree. Children always have smaller ids than their parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Node {
    /// Matches the empty string: an empty alternative, `()`'s inside.
    Empty,
    /// One byte of `Tree::sets[.0]`: a literal, `.`, a bracket expression,
    /// `\w` and the rest.
    Set(u32),
    Assert(Assert),
    /// A parenthesised subexpression, `index` from 1 in the order of the
    /// `(`s, around `body` -- `NONE` for `()`.
    Group {
        index: u32,
        body: u32,
    },
    /// `Tree::kids[first..first + len]`, one after another.
    Cat {
        first: u32,
        len: u32,
    },
    /// `Tree::kids[first..first + len]`, the first preferred.
    Alt {
        first: u32,
        len: u32,
    },
    /// `body`, `min` to `max` times (`NONE`: no limit).
    Rep {
        body: u32,
        min: u32,
        max: u32,
    },
    /// `\n`: what group `n` last matched.
    BackRef(u32),
}

/// A set of bytes, one bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct ByteSet(pub(super) [u64; 4]);

impl ByteSet {
    pub(super) const EMPTY: Self = Self([0; 4]);

    pub(super) fn contains(&self, b: u8) -> bool {
        self.0
            .get(usize::from(b >> 6))
            .is_some_and(|w| w & (1u64 << (b & 63)) != 0)
    }

    pub(super) fn insert(&mut self, b: u8) {
        if let Some(w) = self.0.get_mut(usize::from(b >> 6)) {
            *w |= 1u64 << (b & 63);
        }
    }

    fn remove(&mut self, b: u8) {
        if let Some(w) = self.0.get_mut(usize::from(b >> 6)) {
            *w &= !(1u64 << (b & 63));
        }
    }

    fn insert_range(&mut self, lo: u8, hi: u8) {
        for b in lo..=hi {
            self.insert(b);
        }
    }

    pub(super) fn union(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a |= *b;
        }
    }

    fn invert(&mut self) {
        for w in &mut self.0 {
            *w = !*w;
        }
    }

    /// Every letter in the set with its other case too: REG_ICASE's reading
    /// of a matching list (and of a literal, a set of one).
    fn case_closed(mut self) -> Self {
        for b in b'A'..=b'Z' {
            let lower = b.wrapping_add(32);
            if self.contains(b) || self.contains(lower) {
                self.insert(b);
                self.insert(lower);
            }
        }
        self
    }

    fn of(pred: impl Fn(u8) -> bool) -> Self {
        let mut s = Self::EMPTY;
        for b in 0..=255u8 {
            if pred(b) {
                s.insert(b);
            }
        }
        s
    }
}

/// A character class of the C locale -- glibc's ctype there, which gives the
/// bytes past 127 no class at all.
pub(super) fn class_set(name: &[u8]) -> Option<ByteSet> {
    let set = match name {
        b"alpha" => ByteSet::of(|b| b.is_ascii_alphabetic()),
        b"upper" => ByteSet::of(|b| b.is_ascii_uppercase()),
        b"lower" => ByteSet::of(|b| b.is_ascii_lowercase()),
        b"digit" => ByteSet::of(|b| b.is_ascii_digit()),
        b"xdigit" => ByteSet::of(|b| b.is_ascii_hexdigit()),
        b"space" => ByteSet::of(|b| b == b' ' || (0x09..=0x0D).contains(&b)),
        b"print" => ByteSet::of(|b| (0x20..=0x7E).contains(&b)),
        b"punct" => ByteSet::of(|b| b.is_ascii_punctuation()),
        b"graph" => ByteSet::of(|b| b.is_ascii_graphic()),
        b"cntrl" => ByteSet::of(|b| b < 0x20 || b == 0x7F),
        b"blank" => ByteSet::of(|b| b == b' ' || b == b'\t'),
        b"alnum" => ByteSet::of(|b| b.is_ascii_alphanumeric()),
        _ => return None,
    };
    Some(set)
}

/// The bytes of a word: `\w`, and what `\b` `\<` `\>` look for.
pub(super) fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The parsed pattern.
pub(super) struct Tree {
    pub(super) nodes: List<Node>,
    /// The children of every `Cat` and `Alt`, each node's contiguous.
    pub(super) kids: List<u32>,
    pub(super) sets: List<ByteSet>,
    pub(super) root: u32,
    /// The number of groups: `re_nsub`.
    pub(super) nsub: u32,
    /// The set every `.` matches, `NONE` for a pattern with none: glibc's
    /// fastmap takes a `.` that can begin a match to mean any byte can.
    pub(super) period: u32,
    /// For a bracket expression under RE_ICASE, its set as glibc's fastmap
    /// sees it, by set id, ascending (see `fastmap.rs`).
    pub(super) views: List<(u32, ByteSet)>,
}

/// A pattern's syntax: glibc's `reg_syntax_t`, the `RE_*` bits of
/// `<regex.h>`, which the grammar below asks as glibc's does.
#[derive(Clone, Copy, Debug)]
pub(super) struct Syntax(pub(super) u64);

impl Syntax {
    /// `regcomp`'s: RE_SYNTAX_POSIX_EXTENDED or _BASIC, then REG_ICASE's and
    /// REG_NEWLINE's changes, as glibc's `regcomp` makes them.
    pub(super) fn posix(cflags: i32) -> Self {
        let mut s = if cflags & REG_EXTENDED != 0 {
            RE_SYNTAX_POSIX_EXTENDED
        } else {
            RE_SYNTAX_POSIX_BASIC
        };
        if cflags & REG_ICASE != 0 {
            s |= RE_ICASE;
        }
        if cflags & REG_NEWLINE != 0 {
            s &= !RE_DOT_NEWLINE;
            s |= RE_HAT_LISTS_NOT_NEWLINE;
        }
        Self(s)
    }

    fn has(self, bit: u64) -> bool {
        self.0 & bit != 0
    }

    pub(super) fn icase(self) -> bool {
        self.has(RE_ICASE)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Char,
    Alt,
    Star,
    Plus,
    Qmark,
    OpenDup,
    CloseDup,
    Open,
    Close,
    Bracket,
    Period,
    Anchor(Assert),
    BackRef(u8),
    Word,
    NotWord,
    Space,
    NotSpace,
    /// A backslash that ends the pattern.
    BackSlash,
    End,
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    kind: Kind,
    /// The byte it stands for as a literal: the escaped one of `\c`.
    c: u8,
    len: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BKind {
    Char,
    OpenColl,
    OpenEquiv,
    OpenClass,
    Range,
    Close,
    NonMatch,
    End,
}

#[derive(Clone, Copy, Debug)]
struct BTok {
    kind: BKind,
    c: u8,
    len: u8,
}

/// A name inside `[: :]`, `[= =]` or `[. .]`: glibc's buffer holds 31 bytes.
#[derive(Clone, Copy)]
struct Name {
    bytes: [u8; 32],
    len: usize,
}

impl Name {
    fn get(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}

#[derive(Clone, Copy)]
enum Elem {
    Char(u8),
    Coll(Name),
    Equiv(Name),
    Class(Name),
}

/// A group being read -- or, at the bottom, the whole pattern.
#[derive(Clone, Copy)]
struct Frame {
    /// Its number; 0 for the whole pattern.
    group: u32,
    /// The groups closed when it opened, as `Parser::completed` holds them.
    initial: u32,
    /// Those closed in its alternatives before the one being read: glibc
    /// checks a back-reference against the groups closed before it in its
    /// own alternative, so each alternative starts again from `initial`.
    acc: u32,
    /// Where its alternatives so far begin on `Parser::alts`.
    alts_start: usize,
    /// Where the elements of the alternative being read begin on
    /// `Parser::items`.
    items_start: usize,
}

struct Parser<'a> {
    p: &'a [u8],
    syn: Syntax,
    /// The translate table, if any: each byte's stand-in.
    trans: Option<&'a [u8; 256]>,
    pos: usize,
    tok: Tok,
    nsub: u32,
    /// Bit k set once group k + 1 (k < 9) has been closed: which of `\1` to
    /// `\9` may be written yet.
    completed: u32,
    t: Tree,
    items: List<u32>,
    alts: List<u32>,
    frames: List<Frame>,
    /// Each byte's literal set, made once.
    literal: [u32; 256],
}

/// `pattern` read as `syn` says, through `trans` if there is one -- or the
/// error glibc's parser gives it (REG_ERPAREN for an unmatched `)`, which
/// `regcomp` reports as REG_EPAREN and `re_compile_pattern` does not).
pub(super) fn parse(pattern: &[u8], syn: Syntax, trans: Option<&[u8; 256]>) -> Result<Tree, Code> {
    let mut ps = Parser {
        p: pattern,
        syn,
        trans,
        pos: 0,
        tok: Tok {
            kind: Kind::End,
            c: 0,
            len: 0,
        },
        nsub: 0,
        completed: 0,
        t: Tree {
            nodes: List::new(),
            kids: List::new(),
            sets: List::new(),
            root: NONE,
            nsub: 0,
            period: NONE,
            views: List::new(),
        },
        items: List::new(),
        alts: List::new(),
        frames: List::new(),
        literal: [NONE; 256],
    };
    ps.run()?;
    ps.t.nsub = ps.nsub;
    Ok(ps.t)
}

/// What one expression turned out to be.
enum Expr {
    /// A node, its repetitions applied -- or none, for `a{0}`.
    Atom(Option<u32>),
    /// A `(`: a group to read.
    Open,
}

impl Parser<'_> {
    // -- tokens --------------------------------------------------------------

    /// `c` through the translate table.
    fn tr(&self, c: u8) -> u8 {
        self.trans
            .and_then(|t| t.get(usize::from(c)).copied())
            .unwrap_or(c)
    }

    /// The pattern's byte at `i` as the grammar reads it: translated, as
    /// glibc's `re_string` holds the pattern.
    fn at(&self, i: usize) -> Option<u8> {
        self.p.get(i).map(|&c| self.tr(c))
    }

    fn peek(&self, i: usize, caret_here: bool) -> Tok {
        let s = self.syn;
        let Some(c) = self.at(i) else {
            return Tok {
                kind: Kind::End,
                c: 0,
                len: 0,
            };
        };
        if c == b'\\' {
            // What an escaped byte is, is decided by the byte as written
            // (glibc's `re_string_peek_byte_case`), so that a table cannot
            // turn `\W` into `\w`...
            let Some(&raw) = self.p.get(i.wrapping_add(1)) else {
                return Tok {
                    kind: Kind::BackSlash,
                    c,
                    len: 1,
                };
            };
            let gnu = !s.has(RE_NO_GNU_OPS);
            let ops = !s.has(RE_LIMITED_OPS);
            let intervals = s.has(RE_INTERVALS) && !s.has(RE_NO_BK_BRACES);
            let kind = match raw {
                b'|' if ops && !s.has(RE_NO_BK_VBAR) => Kind::Alt,
                b'1'..=b'9' if !s.has(RE_NO_BK_REFS) => Kind::BackRef(raw.wrapping_sub(b'0')),
                b'<' if gnu => Kind::Anchor(Assert::WordStart),
                b'>' if gnu => Kind::Anchor(Assert::WordEnd),
                b'b' if gnu => Kind::Anchor(Assert::WordBoundary),
                b'B' if gnu => Kind::Anchor(Assert::NotWordBoundary),
                b'w' if gnu => Kind::Word,
                b'W' if gnu => Kind::NotWord,
                b's' if gnu => Kind::Space,
                b'S' if gnu => Kind::NotSpace,
                b'`' if gnu => Kind::Anchor(Assert::BufStart),
                b'\'' if gnu => Kind::Anchor(Assert::BufEnd),
                b'(' if !s.has(RE_NO_BK_PARENS) => Kind::Open,
                b')' if !s.has(RE_NO_BK_PARENS) => Kind::Close,
                b'+' if ops && s.has(RE_BK_PLUS_QM) => Kind::Plus,
                b'?' if ops && s.has(RE_BK_PLUS_QM) => Kind::Qmark,
                b'{' if intervals => Kind::OpenDup,
                b'}' if intervals => Kind::CloseDup,
                _ => Kind::Char,
            };
            // ... and as a literal it stands for its translation, as every
            // other byte of the pattern does (glibc's for itself: the
            // module's doc says why not here).
            return Tok {
                kind,
                c: self.tr(raw),
                len: 2,
            };
        }
        let ops = !s.has(RE_LIMITED_OPS);
        let intervals = s.has(RE_INTERVALS) && s.has(RE_NO_BK_BRACES);
        let kind = match c {
            b'\n' if s.has(RE_NEWLINE_ALT) => Kind::Alt,
            b'|' if ops && s.has(RE_NO_BK_VBAR) => Kind::Alt,
            b'*' => Kind::Star,
            b'+' if ops && !s.has(RE_BK_PLUS_QM) => Kind::Plus,
            b'?' if ops && !s.has(RE_BK_PLUS_QM) => Kind::Qmark,
            b'{' if intervals => Kind::OpenDup,
            b'}' if intervals => Kind::CloseDup,
            b'(' if s.has(RE_NO_BK_PARENS) => Kind::Open,
            b')' if s.has(RE_NO_BK_PARENS) => Kind::Close,
            b'[' => Kind::Bracket,
            b'.' => Kind::Period,
            // `^` is an anchor where the syntax makes every one one; first
            // in the pattern, or first after `(` or `|` (glibc's
            // RE_CARET_ANCHORS_HERE, which its parser sets there and a
            // program may set everywhere); and after a newline that
            // separates alternatives. A literal elsewhere.
            b'^' if s.has(RE_CONTEXT_INDEP_ANCHORS | RE_CARET_ANCHORS_HERE)
                || caret_here
                || i == 0
                || (s.has(RE_NEWLINE_ALT) && self.at(i.wrapping_sub(1)) == Some(b'\n')) =>
            {
                Kind::Anchor(Assert::Bol)
            }
            // `$` is one where every one is; last in the pattern; or before
            // a `)` or anything that separates alternatives.
            b'$' if s.has(RE_CONTEXT_INDEP_ANCHORS)
                || i.wrapping_add(1) == self.p.len()
                || self.alt_or_close_at(i.wrapping_add(1)) =>
            {
                Kind::Anchor(Assert::Eol)
            }
            _ => Kind::Char,
        };
        Tok { kind, c, len: 1 }
    }

    /// Whether the token at `i` separates alternatives or closes a group --
    /// all a `$` needs to know of what follows it. (glibc asks `peek_token`
    /// itself, which asks again for every `$` of a run of them.)
    fn alt_or_close_at(&self, i: usize) -> bool {
        let s = self.syn;
        let ops = !s.has(RE_LIMITED_OPS);
        match self.at(i) {
            Some(b'\\') => match self.p.get(i.wrapping_add(1)) {
                Some(b'|') => ops && !s.has(RE_NO_BK_VBAR),
                Some(b')') => !s.has(RE_NO_BK_PARENS),
                _ => false,
            },
            Some(b'\n') => s.has(RE_NEWLINE_ALT),
            Some(b'|') => ops && s.has(RE_NO_BK_VBAR),
            Some(b')') => s.has(RE_NO_BK_PARENS),
            _ => false,
        }
    }

    fn fetch(&mut self, caret_here: bool) {
        self.tok = self.peek(self.pos, caret_here);
        self.pos = self.pos.wrapping_add(usize::from(self.tok.len));
    }

    fn peek_bracket(&self, i: usize) -> BTok {
        let Some(c) = self.at(i) else {
            return BTok {
                kind: BKind::End,
                c: 0,
                len: 0,
            };
        };
        if c == b'\\' && self.syn.has(RE_BACKSLASH_ESCAPE_IN_LISTS) {
            // `\` quotes the byte after it, which is then an ordinary one.
            if let Some(c2) = self.at(i.wrapping_add(1)) {
                return BTok {
                    kind: BKind::Char,
                    c: c2,
                    len: 2,
                };
            }
        }
        if c == b'[' {
            let c2 = self.at(i.wrapping_add(1)).unwrap_or(0);
            let kind = match c2 {
                b'.' => BKind::OpenColl,
                b'=' => BKind::OpenEquiv,
                b':' if self.syn.has(RE_CHAR_CLASSES) => BKind::OpenClass,
                _ => {
                    return BTok {
                        kind: BKind::Char,
                        c,
                        len: 1,
                    };
                }
            };
            return BTok {
                kind,
                c: c2,
                len: 2,
            };
        }
        let kind = match c {
            b'-' => BKind::Range,
            b']' => BKind::Close,
            b'^' => BKind::NonMatch,
            _ => BKind::Char,
        };
        BTok { kind, c, len: 1 }
    }

    // -- nodes ---------------------------------------------------------------

    fn node(&mut self, n: Node) -> R<u32> {
        let id = u32::try_from(self.t.nodes.len()).map_err(|_| Code(REG_ESPACE))?;
        if id == NONE {
            return Err(Code(REG_ESPACE));
        }
        self.t.nodes.push(n)?;
        Ok(id)
    }

    fn set_node(&mut self, s: ByteSet) -> R<u32> {
        let id = u32::try_from(self.t.sets.len()).map_err(|_| Code(REG_ESPACE))?;
        self.t.sets.push(s)?;
        self.node(Node::Set(id))
    }

    /// The byte `c` itself -- and its other case, under REG_ICASE.
    fn literal(&mut self, c: u8) -> R<u32> {
        let cached = self.literal.get(usize::from(c)).copied().unwrap_or(NONE);
        let set = if cached == NONE {
            let mut s = ByteSet::EMPTY;
            s.insert(c);
            if self.syn.icase() {
                s = s.case_closed();
            }
            let id = u32::try_from(self.t.sets.len()).map_err(|_| Code(REG_ESPACE))?;
            self.t.sets.push(s)?;
            if let Some(slot) = self.literal.get_mut(usize::from(c)) {
                *slot = id;
            }
            id
        } else {
            cached
        };
        self.node(Node::Set(set))
    }

    // -- the grammar ---------------------------------------------------------

    fn run(&mut self) -> R<()> {
        self.frames.push(Frame {
            group: 0,
            initial: 0,
            acc: 0,
            alts_start: 0,
            items_start: 0,
        })?;
        self.fetch(true);
        loop {
            let nested = self.frames.len() > 1;
            match self.tok.kind {
                Kind::Alt => {
                    self.end_branch()?;
                    if let Some(f) = self.frames.last_mut() {
                        f.acc |= self.completed;
                        self.completed = f.initial;
                    }
                    self.fetch(true);
                }
                Kind::End => {
                    if nested {
                        return Err(Code(REG_EPAREN));
                    }
                    let body = self.end_frame()?;
                    self.t.root = if body == NONE {
                        self.node(Node::Empty)?
                    } else {
                        body
                    };
                    return Ok(());
                }
                Kind::Close if nested => {
                    let body = self.end_frame()?;
                    let Some(f) = self.frames.pop() else {
                        return Err(Code(REG_BADPAT));
                    };
                    if f.group <= 9 {
                        self.completed |= 1u32.wrapping_shl(f.group.wrapping_sub(1));
                    }
                    let g = self.node(Node::Group {
                        index: f.group,
                        body,
                    })?;
                    self.fetch(false);
                    if let Some(n) = self.repetitions(Some(g))? {
                        self.items.push(n)?;
                    }
                }
                _ => match self.expression()? {
                    Expr::Atom(Some(n)) => self.items.push(n)?,
                    Expr::Atom(None) => {}
                    Expr::Open => {
                        self.nsub = self.nsub.checked_add(1).ok_or(Code(REG_ESPACE))?;
                        self.frames.push(Frame {
                            group: self.nsub,
                            initial: self.completed,
                            acc: 0,
                            alts_start: self.alts.len(),
                            items_start: self.items.len(),
                        })?;
                        self.fetch(true);
                    }
                },
            }
        }
    }

    /// The alternative being read, finished: its elements made one node (or
    /// `NONE`, for an empty one) on `alts`.
    fn end_branch(&mut self) -> R<()> {
        let start = self.frames.last().map_or(0, |f| f.items_start);
        let n = self.items.len().saturating_sub(start);
        let node = match n {
            0 => NONE,
            1 => self.items.get(start).copied().unwrap_or(NONE),
            _ => {
                let first = u32::try_from(self.t.kids.len()).map_err(|_| Code(REG_ESPACE))?;
                let len = u32::try_from(n).map_err(|_| Code(REG_ESPACE))?;
                self.t
                    .kids
                    .extend_from_slice(self.items.get(start..).unwrap_or(&[]))?;
                self.node(Node::Cat { first, len })?
            }
        };
        self.items.truncate(start);
        self.alts.push(node)?;
        Ok(())
    }

    /// The frame on top, finished: its alternatives made one node, `NONE`
    /// for a single empty one.
    fn end_frame(&mut self) -> R<u32> {
        self.end_branch()?;
        let Some(&f) = self.frames.last() else {
            return Err(Code(REG_BADPAT));
        };
        let start = f.alts_start;
        let n = self.alts.len().saturating_sub(start);
        let body = if n == 1 {
            self.alts.get(start).copied().unwrap_or(NONE)
        } else {
            for k in start..self.alts.len() {
                if self.alts.get(k) == Some(&NONE) {
                    let e = self.node(Node::Empty)?;
                    if let Some(slot) = self.alts.get_mut(k) {
                        *slot = e;
                    }
                }
            }
            let first = u32::try_from(self.t.kids.len()).map_err(|_| Code(REG_ESPACE))?;
            let len = u32::try_from(n).map_err(|_| Code(REG_ESPACE))?;
            self.t
                .kids
                .extend_from_slice(self.alts.get(start..).unwrap_or(&[]))?;
            self.node(Node::Alt { first, len })?
        };
        self.alts.truncate(start);
        self.completed |= f.acc;
        Ok(body)
    }

    /// One of glibc's `parse_expression`s, for anything but `|`, the end, or
    /// a `)` closing a group.
    fn expression(&mut self) -> R<Expr> {
        loop {
            let t = self.tok;
            let s = self.syn;
            let node = match t.kind {
                Kind::Char => self.literal(t.c)?,
                Kind::Open => return Ok(Expr::Open),
                Kind::Bracket => self.bracket()?,
                Kind::BackRef(n) => {
                    if self.completed & 1u32.wrapping_shl(u32::from(n).wrapping_sub(1)) == 0 {
                        return Err(Code(REG_ESUBREG));
                    }
                    self.node(Node::BackRef(u32::from(n)))?
                }
                Kind::OpenDup | Kind::Star | Kind::Plus | Kind::Qmark => {
                    // A repetition with nothing before it is an error, is
                    // passed over -- the next expression read in its place
                    // -- or is a literal, as the syntax says, asked in
                    // glibc's order.
                    if t.kind == Kind::OpenDup && s.has(RE_CONTEXT_INVALID_DUP) {
                        return Err(Code(REG_BADRPT));
                    }
                    if s.has(RE_CONTEXT_INVALID_OPS) {
                        return Err(Code(REG_BADRPT));
                    }
                    if s.has(RE_CONTEXT_INDEP_OPS) {
                        self.fetch(false);
                        continue;
                    }
                    self.literal(t.c)?
                }
                Kind::Close => {
                    // A `)` here is one no group is open for.
                    if !s.has(RE_UNMATCHED_RIGHT_PAREN_ORD) {
                        return Err(Code(REG_ERPAREN));
                    }
                    self.literal(t.c)?
                }
                Kind::CloseDup => self.literal(t.c)?,
                Kind::Anchor(a) => {
                    // No repetition applies to an anchor: glibc reads `^*`
                    // as the anchor and then a `*` with nothing before it.
                    let n = self.node(Node::Assert(a))?;
                    self.fetch(false);
                    return Ok(Expr::Atom(Some(n)));
                }
                Kind::Period => self.period()?,
                Kind::Word | Kind::NotWord | Kind::Space | Kind::NotSpace => {
                    // glibc's `build_charclass_op`: the class's bytes
                    // translated, `\w`'s `_` not, and no REG_ICASE.
                    let word = matches!(t.kind, Kind::Word | Kind::NotWord);
                    let class = if word {
                        ByteSet::of(|b| b.is_ascii_alphanumeric())
                    } else {
                        class_set(b"space").unwrap_or(ByteSet::EMPTY)
                    };
                    let mut set = self.translated(class);
                    if word {
                        set.insert(b'_');
                    }
                    if matches!(t.kind, Kind::NotWord | Kind::NotSpace) {
                        set.invert();
                    }
                    self.set_node(set)?
                }
                Kind::BackSlash => return Err(Code(REG_EESCAPE)),
                Kind::Alt | Kind::End => return Ok(Expr::Atom(None)),
            };
            self.fetch(false);
            return Ok(Expr::Atom(self.repetitions(Some(node))?));
        }
    }

    /// The set a `.` matches -- every byte but a newline (unless
    /// RE_DOT_NEWLINE) and a NUL (if RE_DOT_NOT_NULL) -- made once.
    fn period(&mut self) -> R<u32> {
        if self.t.period == NONE {
            let mut set = ByteSet::EMPTY;
            set.invert();
            if self.syn.has(RE_DOT_NOT_NULL) {
                set.remove(0);
            }
            if !self.syn.has(RE_DOT_NEWLINE) {
                set.remove(b'\n');
            }
            let id = u32::try_from(self.t.sets.len()).map_err(|_| Code(REG_ESPACE))?;
            self.t.sets.push(set)?;
            self.t.period = id;
        }
        self.node(Node::Set(self.t.period))
    }

    /// `set`'s bytes through the translate table.
    fn translated(&self, set: ByteSet) -> ByteSet {
        if self.trans.is_none() {
            return set;
        }
        let mut out = ByteSet::EMPTY;
        for b in 0..=255u8 {
            if set.contains(b) {
                out.insert(self.tr(b));
            }
        }
        out
    }

    /// Every repetition operator after an expression, applied in turn.
    fn repetitions(&mut self, mut node: Option<u32>) -> R<Option<u32>> {
        while matches!(
            self.tok.kind,
            Kind::Star | Kind::Plus | Kind::Qmark | Kind::OpenDup
        ) {
            node = self.dup_op(node)?;
            // In a BRE a `*` or an interval straight after another is
            // refused.
            if self.syn.has(RE_CONTEXT_INVALID_DUP)
                && matches!(self.tok.kind, Kind::Star | Kind::OpenDup)
            {
                return Err(Code(REG_BADRPT));
            }
        }
        Ok(node)
    }

    /// The count of an interval, as glibc's `fetch_number` reads it: -1 for
    /// none, -2 for something not a number, at most RE_DUP_MAX + 1.
    fn fetch_number(&mut self) -> i64 {
        let mut num: i64 = -1;
        loop {
            self.fetch(false);
            let t = self.tok;
            if t.kind == Kind::End {
                return -2;
            }
            if t.kind == Kind::CloseDup || t.c == b',' {
                return num;
            }
            num = if t.kind != Kind::Char || !t.c.is_ascii_digit() || num == -2 {
                -2
            } else {
                let d = i64::from(t.c.wrapping_sub(b'0'));
                if num == -1 {
                    d
                } else {
                    num.saturating_mul(10)
                        .saturating_add(d)
                        .min(i64::from(RE_DUP_MAX).saturating_add(1))
                }
            };
        }
    }

    /// One repetition operator applied to `elem`.
    fn dup_op(&mut self, elem: Option<u32>) -> R<Option<u32>> {
        let t = self.tok;
        // Just after the `{`: where a malformed interval is read again from,
        // as text, under RE_INVALID_INTERVAL_ORD.
        let after_open = self.pos;
        let (start, end) = if t.kind == Kind::OpenDup {
            let mut start = self.fetch_number();
            if start == -1 {
                if self.tok.kind == Kind::Char && self.tok.c == b',' {
                    // `{,n}` is `{0,n}`.
                    start = 0;
                } else {
                    return Err(Code(REG_BADBR));
                }
            }
            let mut end = 0;
            if start != -2 {
                end = if self.tok.kind == Kind::CloseDup {
                    start
                } else if self.tok.kind == Kind::Char && self.tok.c == b',' {
                    self.fetch_number()
                } else {
                    -2
                };
            }
            if start == -2 || end == -2 {
                if !self.syn.has(RE_INVALID_INTERVAL_ORD) {
                    return Err(Code(if self.tok.kind == Kind::End {
                        REG_EBRACE
                    } else {
                        REG_BADBR
                    }));
                }
                // Not an interval: its `{` is an ordinary character, read
                // next, and the text after it as it would have been.
                self.pos = after_open;
                self.tok = Tok {
                    kind: Kind::Char,
                    ..t
                };
                return Ok(elem);
            }
            if (end != -1 && start > end) || self.tok.kind != Kind::CloseDup {
                return Err(Code(REG_BADBR));
            }
            if i64::from(RE_DUP_MAX) < (if end == -1 { start } else { end }) {
                return Err(Code(REG_ESIZE));
            }
            (start, end)
        } else {
            (
                i64::from(t.kind == Kind::Plus),
                if t.kind == Kind::Qmark { 1 } else { -1 },
            )
        };
        self.fetch(false);
        let Some(elem) = elem else {
            return Ok(None);
        };
        if start == 0 && end == 0 {
            return Ok(None);
        }
        if start == 1 && end == 1 {
            return Ok(Some(elem));
        }
        let min = u32::try_from(start).map_err(|_| Code(REG_BADBR))?;
        let max = if end == -1 {
            NONE
        } else {
            u32::try_from(end).map_err(|_| Code(REG_BADBR))?
        };
        Ok(Some(self.node(Node::Rep {
            body: elem,
            min,
            max,
        })?))
    }

    // -- bracket expressions -------------------------------------------------

    fn bracket(&mut self) -> R<u32> {
        let mut t = self.peek_bracket(self.pos);
        if t.kind == BKind::End {
            return Err(Code(REG_BADPAT));
        }
        let mut non_match = false;
        if t.kind == BKind::NonMatch {
            non_match = true;
            self.pos = self.pos.wrapping_add(usize::from(t.len));
            t = self.peek_bracket(self.pos);
            if t.kind == BKind::End {
                return Err(Code(REG_BADPAT));
            }
        }
        // A `]` first is an ordinary one.
        if t.kind == BKind::Close {
            t.kind = BKind::Char;
        }
        let mut set = ByteSet::EMPTY;
        // Under RE_ICASE, the set as glibc builds it from its upper-cased
        // pattern, for the fastmap (`Tree::views`).
        let icase = self.syn.icase();
        let mut view = ByteSet::EMPTY;
        let mut first_round = true;
        loop {
            let start = self.bracket_element(t, first_round)?;
            first_round = false;
            t = self.peek_bracket(self.pos);
            let mut range_end = None;
            if !matches!(start, Elem::Class(_) | Elem::Equiv(_)) {
                if t.kind == BKind::End {
                    return Err(Code(REG_EBRACK));
                }
                if t.kind == BKind::Range {
                    self.pos = self.pos.wrapping_add(usize::from(t.len));
                    let t2 = self.peek_bracket(self.pos);
                    if t2.kind == BKind::End {
                        return Err(Code(REG_EBRACK));
                    }
                    if t2.kind == BKind::Close {
                        // A `-` last is an ordinary one.
                        self.pos = self.pos.wrapping_sub(usize::from(t.len));
                        t.kind = BKind::Char;
                    } else {
                        range_end = Some(t2);
                    }
                }
            }
            if let Some(t2) = range_end {
                let end = self.bracket_element(t2, true)?;
                t = self.peek_bracket(self.pos);
                add_range(&mut set, start, end, self.syn.has(RE_NO_EMPTY_RANGES))?;
                if icase && let (Ok(lo), Ok(hi)) = (collation(start), collation(end)) {
                    let (lo, hi) = (lo.to_ascii_uppercase(), hi.to_ascii_uppercase());
                    if lo <= hi {
                        view.insert_range(lo, hi);
                    }
                }
            } else {
                self.add_element(&mut set, start)?;
                if icase {
                    self.add_view(&mut view, start);
                }
            }
            if t.kind == BKind::End {
                return Err(Code(REG_EBRACK));
            }
            if t.kind == BKind::Close {
                break;
            }
        }
        self.pos = self.pos.wrapping_add(usize::from(t.len));
        let hat = self.syn.has(RE_HAT_LISTS_NOT_NEWLINE);
        let mut fastmap_view = None;
        if icase {
            set = set.case_closed();
            // glibc's set, and the lower case of each of its letters: what
            // its fastmap takes this one to let a match begin with.
            let mut g = view;
            if non_match {
                g.invert();
                if hat {
                    g.remove(b'\n');
                }
            }
            let mut fm = g;
            for b in b'A'..=b'Z' {
                if g.contains(b) {
                    fm.insert(b.to_ascii_lowercase());
                }
            }
            fastmap_view = Some(fm);
        }
        if non_match {
            set.invert();
            if hat {
                set.remove(b'\n');
            }
        }
        let id = u32::try_from(self.t.sets.len()).map_err(|_| Code(REG_ESPACE))?;
        let node = self.set_node(set)?;
        if let Some(fm) = fastmap_view {
            self.t.views.push((id, fm))?;
        }
        Ok(node)
    }

    /// One element of a bracket expression as glibc reads it under
    /// RE_ICASE, upper-cased, into `view`: a class of letters' case is all
    /// of them (glibc's `[:upper:]` and `[:lower:]` are `[:alpha:]` there).
    fn add_view(&self, view: &mut ByteSet, e: Elem) {
        match e {
            Elem::Char(c) => view.insert(c.to_ascii_uppercase()),
            Elem::Coll(n) | Elem::Equiv(n) => {
                if let [c] = n.get() {
                    view.insert(c.to_ascii_uppercase());
                }
            }
            Elem::Class(n) => {
                let name = match n.get() {
                    b"upper" | b"lower" => &b"alpha"[..],
                    other => other,
                };
                if let Some(s) = class_set(name) {
                    view.union(&self.translated(s));
                }
            }
        }
    }

    /// One element of a bracket expression, added to `set`. A class's
    /// bytes are translated, as glibc's `build_charclass` translates them.
    fn add_element(&self, set: &mut ByteSet, e: Elem) -> R<()> {
        match e {
            Elem::Char(c) => set.insert(c),
            // The C locale's collating elements and equivalence classes are
            // single bytes, each its own class.
            Elem::Coll(n) | Elem::Equiv(n) => match n.get() {
                [c] => set.insert(*c),
                _ => return Err(Code(REG_ECOLLATE)),
            },
            Elem::Class(n) => match class_set(n.get()) {
                Some(s) => set.union(&self.translated(s)),
                None => return Err(Code(REG_ECTYPE)),
            },
        }
        Ok(())
    }

    fn bracket_element(&mut self, t: BTok, accept_hyphen: bool) -> R<Elem> {
        self.pos = self.pos.wrapping_add(usize::from(t.len));
        if matches!(
            t.kind,
            BKind::OpenColl | BKind::OpenEquiv | BKind::OpenClass
        ) {
            return self.bracket_symbol(t);
        }
        // A `-` that neither ends a range nor stands last: glibc's
        // REG_ERANGE for what POSIX leaves undefined.
        if t.kind == BKind::Range
            && !accept_hyphen
            && self.peek_bracket(self.pos).kind != BKind::Close
        {
            return Err(Code(REG_ERANGE));
        }
        Ok(Elem::Char(t.c))
    }

    /// The name inside `[: :]`, `[= =]` or `[. .]`, up to its closing pair:
    /// a class's name as written, the others translated (glibc's
    /// `re_string_fetch_byte_case` and `_fetch_byte`).
    fn bracket_symbol(&mut self, t: BTok) -> R<Elem> {
        let delim = t.c;
        let as_written = t.kind == BKind::OpenClass;
        if self.pos >= self.p.len() {
            return Err(Code(REG_EBRACK));
        }
        let mut name = Name {
            bytes: [0; 32],
            len: 0,
        };
        loop {
            if name.len >= 32 {
                return Err(Code(REG_EBRACK));
            }
            let ch = if as_written {
                self.p.get(self.pos).copied()
            } else {
                self.at(self.pos)
            }
            .unwrap_or(0);
            self.pos = self.pos.wrapping_add(1);
            if self.pos >= self.p.len() {
                return Err(Code(REG_EBRACK));
            }
            if ch == delim && self.at(self.pos) == Some(b']') {
                break;
            }
            if let Some(slot) = name.bytes.get_mut(name.len) {
                *slot = ch;
            }
            name.len = name.len.wrapping_add(1);
        }
        self.pos = self.pos.wrapping_add(1);
        Ok(match t.kind {
            BKind::OpenColl => Elem::Coll(name),
            BKind::OpenEquiv => Elem::Equiv(name),
            _ => Elem::Class(name),
        })
    }
}

/// A collating element's place in the C locale's order, which is the byte's
/// own value: glibc's `collseqmb`, the identity there. A name longer than
/// one byte names none (REG_ECOLLATE).
fn collation(e: Elem) -> R<u8> {
    match e {
        Elem::Char(c) => Ok(c),
        Elem::Coll(n) => match n.get() {
            [c] => Ok(*c),
            _ => Err(Code(REG_ECOLLATE)),
        },
        Elem::Equiv(_) | Elem::Class(_) => Err(Code(REG_ERANGE)),
    }
}

/// The range `a`-`b` added to `set`. One whose end comes before its start is
/// refused under RE_NO_EMPTY_RANGES, and matches nothing otherwise.
fn add_range(set: &mut ByteSet, a: Elem, b: Elem, no_empty_ranges: bool) -> R<()> {
    // A class or an equivalence class at either end: REG_ERANGE before
    // anything else is looked at, as glibc's build_range_exp has it.
    if matches!(a, Elem::Equiv(_) | Elem::Class(_)) || matches!(b, Elem::Equiv(_) | Elem::Class(_))
    {
        return Err(Code(REG_ERANGE));
    }
    let lo = collation(a)?;
    let hi = collation(b)?;
    if lo > hi {
        if no_empty_ranges {
            return Err(Code(REG_ERANGE));
        }
        return Ok(());
    }
    set.insert_range(lo, hi);
    Ok(())
}
