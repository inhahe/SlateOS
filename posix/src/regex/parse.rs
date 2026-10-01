//! The pattern, read into a tree: glibc 2.39's grammar for
//! RE_SYNTAX_POSIX_BASIC and RE_SYNTAX_POSIX_EXTENDED, the two syntaxes
//! `regcomp` chooses between (regcomp.c's `peek_token`, `parse_reg_exp`,
//! `parse_branch`, `parse_expression`, `parse_sub_exp`, `parse_dup_op`,
//! `fetch_number` and `parse_bracket_exp`).
//!
//! POSIX leaves undefined most of what a parser has to decide -- what `a**`
//! is, whether `^` in the middle of a BRE is an anchor, what an unmatched
//! `)` means, which error a malformed interval earns -- and there glibc's
//! answer is this one's, found out by the oracle
//! (`posix/tools/oracle/regex_harness.py`, every pair of 75 tokens as an ERE
//! and as a BRE). The one place this reads a pattern otherwise is REG_ICASE:
//! glibc upper-cases the pattern before parsing it, which turns `[Z-a]` into
//! the reversed range `[Z-A]` (refused) and `[a-Z]` into `[A-Z]` (accepted),
//! and loses the case of `\a`; this parses the pattern as written and has
//! each character match itself and its other case, which is what XBD 9.2
//! says REG_ICASE means.
//!
//! glibc's parser recurses once a level of nesting, and so a pattern of a
//! hundred thousand `(` can overflow its stack. This one keeps an explicit
//! stack of the groups open (`Frame`), and makes the tree's nodes children
//! first, so that everything done with the tree afterwards is a loop over
//! its nodes in order.

use super::{
    REG_BADBR, REG_BADPAT, REG_BADRPT, REG_EBRACE, REG_EBRACK, REG_ECOLLATE, REG_ECTYPE,
    REG_EESCAPE, REG_EPAREN, REG_ERANGE, REG_ESIZE, REG_ESPACE, REG_ESUBREG, REG_EXTENDED,
    REG_ICASE, REG_NEWLINE,
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
}

/// The syntax bits of glibc's `reg_syntax_t` that `regcomp`'s two syntaxes,
/// and REG_NEWLINE and REG_ICASE, set differently. The rest are fixed for
/// both: RE_CHAR_CLASSES, RE_INTERVALS, RE_NO_EMPTY_RANGES and
/// RE_DOT_NOT_NULL set; RE_LIMITED_OPS, RE_NO_BK_REFS, RE_NO_GNU_OPS,
/// RE_NEWLINE_ALT, RE_BACKSLASH_ESCAPE_IN_LISTS, RE_INVALID_INTERVAL_ORD and
/// RE_CONTEXT_INDEP_OPS clear.
#[derive(Clone, Copy, Debug)]
pub(super) struct Syntax {
    /// RE_BK_PLUS_QM: `\+` and `\?` are the operators, `+` and `?` literal.
    bk_plus_qm: bool,
    /// RE_CONTEXT_INDEP_ANCHORS: `^` and `$` are anchors wherever they are.
    context_indep_anchors: bool,
    /// RE_CONTEXT_INVALID_OPS: `*` `+` `?` `{` refused with nothing before.
    context_invalid_ops: bool,
    /// RE_CONTEXT_INVALID_DUP: `\{` refused with nothing before it, and a
    /// `*` or `\{` straight after another repetition.
    context_invalid_dup: bool,
    /// RE_NO_BK_BRACES, _PARENS, _VBAR: `{}`, `()` and `|` bare, not after
    /// a backslash.
    no_bk_braces: bool,
    no_bk_parens: bool,
    no_bk_vbar: bool,
    /// RE_UNMATCHED_RIGHT_PAREN_ORD: a `)` with no `(` is an ordinary one.
    unmatched_right_paren_ord: bool,
    /// RE_DOT_NEWLINE: `.` matches a newline.
    dot_newline: bool,
    /// RE_HAT_LISTS_NOT_NEWLINE: `[^...]` does not.
    hat_lists_not_newline: bool,
    /// RE_ICASE.
    icase: bool,
}

impl Syntax {
    /// `regcomp`'s: RE_SYNTAX_POSIX_EXTENDED or _BASIC, then REG_NEWLINE's
    /// and REG_ICASE's changes.
    pub(super) fn posix(cflags: i32) -> Self {
        let ere = cflags & REG_EXTENDED != 0;
        let newline = cflags & REG_NEWLINE != 0;
        Self {
            bk_plus_qm: !ere,
            context_indep_anchors: ere,
            context_invalid_ops: ere,
            context_invalid_dup: !ere,
            no_bk_braces: ere,
            no_bk_parens: ere,
            no_bk_vbar: ere,
            unmatched_right_paren_ord: ere,
            dot_newline: !newline,
            hat_lists_not_newline: newline,
            icase: cflags & REG_ICASE != 0,
        }
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

/// `pattern` read as `syn` says, or the error `regcomp` gives it.
pub(super) fn parse(pattern: &[u8], syn: Syntax) -> Result<Tree, Code> {
    let mut ps = Parser {
        p: pattern,
        syn,
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

    fn peek(&self, i: usize, caret_here: bool) -> Tok {
        let s = self.syn;
        let Some(&c) = self.p.get(i) else {
            return Tok {
                kind: Kind::End,
                c: 0,
                len: 0,
            };
        };
        if c == b'\\' {
            let Some(&c2) = self.p.get(i.wrapping_add(1)) else {
                return Tok {
                    kind: Kind::BackSlash,
                    c,
                    len: 1,
                };
            };
            let kind = match c2 {
                b'|' if !s.no_bk_vbar => Kind::Alt,
                b'1'..=b'9' => Kind::BackRef(c2.wrapping_sub(b'0')),
                b'<' => Kind::Anchor(Assert::WordStart),
                b'>' => Kind::Anchor(Assert::WordEnd),
                b'b' => Kind::Anchor(Assert::WordBoundary),
                b'B' => Kind::Anchor(Assert::NotWordBoundary),
                b'w' => Kind::Word,
                b'W' => Kind::NotWord,
                b's' => Kind::Space,
                b'S' => Kind::NotSpace,
                b'`' => Kind::Anchor(Assert::BufStart),
                b'\'' => Kind::Anchor(Assert::BufEnd),
                b'(' if !s.no_bk_parens => Kind::Open,
                b')' if !s.no_bk_parens => Kind::Close,
                b'+' if s.bk_plus_qm => Kind::Plus,
                b'?' if s.bk_plus_qm => Kind::Qmark,
                b'{' if !s.no_bk_braces => Kind::OpenDup,
                b'}' if !s.no_bk_braces => Kind::CloseDup,
                _ => Kind::Char,
            };
            return Tok {
                kind,
                c: c2,
                len: 2,
            };
        }
        let kind = match c {
            b'|' if s.no_bk_vbar => Kind::Alt,
            b'*' => Kind::Star,
            b'+' if !s.bk_plus_qm => Kind::Plus,
            b'?' if !s.bk_plus_qm => Kind::Qmark,
            b'{' if s.no_bk_braces => Kind::OpenDup,
            b'}' if s.no_bk_braces => Kind::CloseDup,
            b'(' if s.no_bk_parens => Kind::Open,
            b')' if s.no_bk_parens => Kind::Close,
            b'[' => Kind::Bracket,
            b'.' => Kind::Period,
            // A BRE's `^` is an anchor first in the pattern, or first after
            // `\(` or `\|` (glibc's RE_CARET_ANCHORS_HERE); a literal
            // elsewhere.
            b'^' if s.context_indep_anchors || caret_here || i == 0 => Kind::Anchor(Assert::Bol),
            // A BRE's `$` is an anchor last in the pattern, or before `\)`
            // or `\|`.
            b'$' if s.context_indep_anchors
                || i.wrapping_add(1) == self.p.len()
                || self.alt_or_close_at(i.wrapping_add(1)) =>
            {
                Kind::Anchor(Assert::Eol)
            }
            _ => Kind::Char,
        };
        Tok { kind, c, len: 1 }
    }

    /// Whether the token at `i` is an alternation or a closing parenthesis --
    /// all a `$` needs to know of what follows it. (glibc asks `peek_token`
    /// itself, which asks again for every `$` of a run of them.)
    fn alt_or_close_at(&self, i: usize) -> bool {
        let s = self.syn;
        match self.p.get(i) {
            Some(b'\\') => match self.p.get(i.wrapping_add(1)) {
                Some(b'|') => !s.no_bk_vbar,
                Some(b')') => !s.no_bk_parens,
                _ => false,
            },
            Some(b'|') => s.no_bk_vbar,
            Some(b')') => s.no_bk_parens,
            _ => false,
        }
    }

    fn fetch(&mut self, caret_here: bool) {
        self.tok = self.peek(self.pos, caret_here);
        self.pos = self.pos.wrapping_add(usize::from(self.tok.len));
    }

    fn peek_bracket(&self, i: usize) -> BTok {
        let Some(&c) = self.p.get(i) else {
            return BTok {
                kind: BKind::End,
                c: 0,
                len: 0,
            };
        };
        if c == b'[' {
            let c2 = self.p.get(i.wrapping_add(1)).copied().unwrap_or(0);
            let kind = match c2 {
                b'.' => BKind::OpenColl,
                b'=' => BKind::OpenEquiv,
                b':' => BKind::OpenClass,
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
            if self.syn.icase {
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
            Kind::OpenDup
            | Kind::Star
            | Kind::Plus
            | Kind::Qmark
            | Kind::Close
            | Kind::CloseDup => {
                if t.kind == Kind::OpenDup && s.context_invalid_dup {
                    return Err(Code(REG_BADRPT));
                }
                if matches!(
                    t.kind,
                    Kind::OpenDup | Kind::Star | Kind::Plus | Kind::Qmark
                ) && s.context_invalid_ops
                {
                    return Err(Code(REG_BADRPT));
                }
                // A `)` here is one no group is open for: glibc's
                // REG_ERPAREN, which `regcomp` reports as REG_EPAREN.
                if t.kind == Kind::Close && !s.unmatched_right_paren_ord {
                    return Err(Code(REG_EPAREN));
                }
                self.literal(t.c)?
            }
            Kind::Anchor(a) => {
                // No repetition applies to an anchor: glibc reads `^*` as
                // the anchor and then a `*` with nothing before it.
                let n = self.node(Node::Assert(a))?;
                self.fetch(false);
                return Ok(Expr::Atom(Some(n)));
            }
            Kind::Period => {
                let mut set = ByteSet::EMPTY;
                set.invert();
                set.remove(0);
                if !s.dot_newline {
                    set.remove(b'\n');
                }
                self.set_node(set)?
            }
            Kind::Word | Kind::NotWord | Kind::Space | Kind::NotSpace => {
                let mut set = if matches!(t.kind, Kind::Word | Kind::NotWord) {
                    ByteSet::of(is_word)
                } else {
                    class_set(b"space").unwrap_or(ByteSet::EMPTY)
                };
                if matches!(t.kind, Kind::NotWord | Kind::NotSpace) {
                    set.invert();
                }
                self.set_node(set)?
            }
            Kind::BackSlash => return Err(Code(REG_EESCAPE)),
            Kind::Alt | Kind::End => return Ok(Expr::Atom(None)),
        };
        self.fetch(false);
        Ok(Expr::Atom(self.repetitions(Some(node))?))
    }

    /// Every repetition operator after an expression, applied in turn.
    fn repetitions(&mut self, mut node: Option<u32>) -> R<Option<u32>> {
        while matches!(
            self.tok.kind,
            Kind::Star | Kind::Plus | Kind::Qmark | Kind::OpenDup
        ) {
            node = self.dup_op(node)?;
            if self.syn.context_invalid_dup && matches!(self.tok.kind, Kind::Star | Kind::OpenDup) {
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
                return Err(Code(if self.tok.kind == Kind::End {
                    REG_EBRACE
                } else {
                    REG_BADBR
                }));
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
                add_range(&mut set, start, end)?;
            } else {
                add_element(&mut set, start)?;
            }
            if t.kind == BKind::End {
                return Err(Code(REG_EBRACK));
            }
            if t.kind == BKind::Close {
                break;
            }
        }
        self.pos = self.pos.wrapping_add(usize::from(t.len));
        if self.syn.icase {
            set = set.case_closed();
        }
        if non_match {
            set.invert();
            if self.syn.hat_lists_not_newline {
                set.remove(b'\n');
            }
        }
        self.set_node(set)
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

    /// The name inside `[: :]`, `[= =]` or `[. .]`, up to its closing pair.
    fn bracket_symbol(&mut self, t: BTok) -> R<Elem> {
        let delim = t.c;
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
            let ch = self.p.get(self.pos).copied().unwrap_or(0);
            self.pos = self.pos.wrapping_add(1);
            if self.pos >= self.p.len() {
                return Err(Code(REG_EBRACK));
            }
            if ch == delim && self.p.get(self.pos) == Some(&b']') {
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

fn add_element(set: &mut ByteSet, e: Elem) -> R<()> {
    match e {
        Elem::Char(c) => set.insert(c),
        // The C locale's collating elements and equivalence classes are
        // single bytes, each its own class.
        Elem::Coll(n) | Elem::Equiv(n) => match n.get() {
            [c] => set.insert(*c),
            _ => return Err(Code(REG_ECOLLATE)),
        },
        Elem::Class(n) => match class_set(n.get()) {
            Some(s) => set.union(&s),
            None => return Err(Code(REG_ECTYPE)),
        },
    }
    Ok(())
}

fn add_range(set: &mut ByteSet, a: Elem, b: Elem) -> R<()> {
    // A class or an equivalence class at either end: REG_ERANGE before
    // anything else is looked at, as glibc's build_range_exp has it.
    if matches!(a, Elem::Equiv(_) | Elem::Class(_)) || matches!(b, Elem::Equiv(_) | Elem::Class(_))
    {
        return Err(Code(REG_ERANGE));
    }
    let lo = collation(a)?;
    let hi = collation(b)?;
    if lo > hi {
        return Err(Code(REG_ERANGE));
    }
    set.insert_range(lo, hi);
    Ok(())
}
