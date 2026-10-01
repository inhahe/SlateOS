//! Regular expressions: POSIX's `regcomp`, `regexec`, `regfree` and
//! `regerror` (XSH; XBD chapter 9), and glibc's GNU interface beside them --
//! `re_compile_pattern`, `re_search` and the rest, BSD's `re_comp` and
//! `re_exec` -- read as glibc 2.39 reads patterns, and matched as the
//! standard says, which glibc, in its choice of submatches, does not always
//! do.
//!
//! ## What it reads
//!
//! Basic (BRE) and extended (REG_EXTENDED, ERE) expressions, with everything
//! glibc's `regcomp` accepts in them:
//!
//! - ordinary characters, `.`, bracket expressions (`[...]` and `[^...]`,
//!   ranges, `[:class:]`, `[=c=]`, `[.c.]` -- the C locale's, all single
//!   bytes), the anchors `^` and `$`;
//! - `*`, intervals (`\{m,n\}`, `{m,n}`, glibc's `{,n}` too, up to
//!   RE_DUP_MAX, 32767), groups (`\(...\)`, `(...)`), back-references `\1`
//!   to `\9` (in an ERE as well, as glibc has them);
//! - in an ERE `+`, `?` and `|`; in a BRE, glibc's `\+`, `\?` and `\|`;
//! - GNU's `\w` `\W` `\s` `\S` `\b` `\B` `\<` `\>` `` \` `` `\'`.
//!
//! What POSIX leaves undefined -- `a**`, a `^` mid-BRE, an unmatched `)`,
//! which error a malformed interval gets -- is glibc's answer, from its own
//! grammar (`regex/parse.rs`). REG_ICASE, REG_NEWLINE and REG_NOSUB at
//! `regcomp`; REG_NOTBOL, REG_NOTEOL and glibc's REG_STARTEND at `regexec`.
//! Bytes, not characters: the C locale, which is the one this library
//! reports (known-issues.md -> open-questions D-Q7).
//!
//! `re_compile_pattern` reads a pattern in any syntax glibc's
//! `reg_syntax_t` can describe -- the twenty-six `RE_*` bits, which the
//! programs' syntaxes (`RE_SYNTAX_GREP`, `_EGREP`, `_AWK`, `_GNU_AWK`,
//! `_EMACS` and the rest) combine -- through the program's translate table
//! if it set one; `re_search`, `re_search_2`, `re_match` and `re_match_2`
//! search forwards or backwards over a range of starting places, report
//! into a `struct re_registers` they allocate, grow or leave as they find
//! it, and keep the pattern buffer's fastmap of the bytes a match can begin
//! with (`regex/fastmap.rs`). `regex_t` is glibc's `struct
//! re_pattern_buffer`, field for field, as programs written for the GNU
//! interface fill and read it -- with POSIX's `regoff_t`, as wide as
//! `ssize_t`, where glibc's is an `int` (design-decisions §1161).
//!
//! ## Which match
//!
//! The leftmost, and of those the longest (XBD 9.1). Then the submatches:
//! "each subpattern, from left to right, shall match the longest possible
//! string", "a null string ... longer than no match at all", which Okui and
//! Suzuki (CIAA 2010) make exact -- of every parse of that match, the one
//! greatest when parse trees are compared position by position in
//! pre-order, each by the length it matched:
//!
//! - of a concatenation's elements, the first as long as it can be, then the
//!   second, and so on; of an alternation's branches that match the same
//!   text, the first;
//! - a repetition's iterations likewise, in order; one past the minimum
//!   count must be non-empty, and a repetition that matched nothing reports
//!   one empty iteration if its body can match nothing -- POSIX's own
//!   example: `\(a*\)*` against "bc" reports the null string for `\1`.
//!
//! And they are reported as regexec's page says: a group its last match, -1
//! for one that took no part, and one inside another only within what that
//! one reports. `re_search` reports by the same rule.
//!
//! **glibc does otherwise**, taking the first path its automaton finds to
//! the longest match: the first alternative and the greedier loop wherever
//! the length allows, and a stale value for a group inside an earlier
//! iteration. `(a|ab)(c|bcd)` against "abcd" gives `\1` = "a" there and
//! "ab" here; `((a)|b)*` against "ab" gives `\2` = "a" there and no match
//! here. It also answers wrongly outright, contradicting its own answers
//! elsewhere: `$` and `^` take a newline for a line's end between two parts
//! of a pattern without REG_NEWLINE (`$.` matches "\n"); `\B` inside or
//! after a repetition holds where it does not (`(\Ba){0,2}` matches "a",
//! which `\Ba` does not); a `^` inside a repeated group can make a pattern
//! match nothing at all (`(^[a-c]{0,2}){0,2}.{2,}|[^a]{0,1}` against
//! "aaa", though its second branch matches the empty string anywhere);
//! REG_ICASE loses the case of `\a` and of range ends (`[Z-a]` is refused,
//! `[a-Z]` accepted), and a translate table the translation of `\a`;
//! back-references miss longer matches and report half-set pairs; and five
//! patterns, `(){32767}` among them, crash it. design-decisions.md §1160
//! records the choice; `posix/src/regex_deviations.txt` lists every case of
//! the oracle (`posix/tools/oracle/regex_harness.py`, `regex_oracle.txt`:
//! some 544,000 answers -- every pair of 54 pieces against every string of
//! `a` and `b` to length 4, every pair of 75 tokens as a BRE and an ERE,
//! 1,200 patterns drawn at random against strings of `a`, `b` and `c`,
//! glibc's own tests, and the flags' edges) where the answer here is the
//! standard's, as `posix/tools/oracle/regex_model.py` computes it, and not
//! glibc's: 16,444 of them. The GNU interface has an oracle of its own,
//! `regex_gnu_harness.py`, `regex_gnu_oracle.txt`, and its deviations are
//! listed by the same rule.
//!
//! ## How
//!
//! `regcomp` parses the pattern into a tree (`regex/parse.rs`) and compiles
//! it into two Thompson automata, one reading forwards and one backwards
//! (`regex/prog.rs`). `regexec` runs the forward one over the string for the
//! leftmost-longest match -- one pass, in time the program's size for each
//! byte -- and, when submatches are asked for, `regex/dissect.rs` takes the
//! match apart by the rule above, each boundary found with one forward and
//! one backward run over the part of the string it divides. A pattern with a
//! back-reference, which no automaton can match, goes to
//! `regex/backref.rs`, which weighs every parse -- polynomially, keeping the
//! best parse for each place a node can end and each set of spans the
//! referenced groups can have.
//!
//! Nothing recurses once a level of the pattern's nesting, and nothing has a
//! fixed size: the program, every table and every group are allocated.
//! What glibc does not bound, this bounds only where it must -- a program
//! past two million instructions (REG_ESPACE from `regcomp`), and the tables
//! of one back-referencing match (REG_NOMATCH, as glibc answers a `regexec`
//! that runs out of memory).
//!
//! Until 2026-09-30 this had no intervals and no back-references, both of
//! which POSIX requires, and refused patterns past 1024 bytes, programs past
//! 512 instructions and more than nine groups (known-issues.md ->
//! D-POSIX-GLIBC-2026-SECURITY-FIXES-AUDITED); and until later the same day
//! its `regex_t` was musl's, and the GNU interface was missing.

mod backref;
mod dissect;
mod fastmap;
mod parse;
mod prog;

use core::sync::atomic::{AtomicU32, Ordering};

use crate::list::{List, NoMem};
use crate::malloc;
use crate::string;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Use extended regular expressions.
pub const REG_EXTENDED: i32 = 1;
/// Ignore case.
pub const REG_ICASE: i32 = 2;
/// Change `^`/`$`/`.` behaviour around `\n`.
pub const REG_NEWLINE: i32 = 4;
/// Report only success/fail, not match position.
pub const REG_NOSUB: i32 = 8;
/// Don't regard start of string as beginning of line.
pub const REG_NOTBOL: i32 = 1;
/// Don't regard end of string as end of line.
pub const REG_NOTEOL: i32 = 2;
/// glibc's: `pmatch[0]` bounds the string -- `rm_so` where the search
/// begins, `rm_eo` where the string ends, NULs inside it ordinary bytes.
pub const REG_STARTEND: i32 = 4;

/// Not implemented (musl's and glibc's; nothing here returns it).
pub const REG_ENOSYS: i32 = -1;
/// Success (glibc's name for it).
pub const REG_NOERROR: i32 = 0;
/// No match.
pub const REG_NOMATCH: i32 = 1;
/// Invalid regular expression.
pub const REG_BADPAT: i32 = 2;
/// Invalid collating element.
pub const REG_ECOLLATE: i32 = 3;
/// Invalid character class.
pub const REG_ECTYPE: i32 = 4;
/// Trailing backslash.
pub const REG_EESCAPE: i32 = 5;
/// Invalid back reference.
pub const REG_ESUBREG: i32 = 6;
/// Unmatched `[`.
pub const REG_EBRACK: i32 = 7;
/// Unmatched `\(` or `(`.
pub const REG_EPAREN: i32 = 8;
/// Unmatched `\{` or `{`.
pub const REG_EBRACE: i32 = 9;
/// Invalid `\{...\}` contents.
pub const REG_BADBR: i32 = 10;
/// Invalid range expression.
pub const REG_ERANGE: i32 = 11;
/// Out of memory.
pub const REG_ESPACE: i32 = 12;
/// A repetition operator with nothing before it.
pub const REG_BADRPT: i32 = 13;
/// glibc's: premature end of the expression (never returned by `regcomp`).
pub const REG_EEND: i32 = 14;
/// glibc's: an interval's count past RE_DUP_MAX.
pub const REG_ESIZE: i32 = 15;
/// glibc's: an unmatched `)` (`regcomp` reports it as REG_EPAREN,
/// `re_compile_pattern` as itself).
pub const REG_ERPAREN: i32 = 16;

/// The largest count an interval may give, glibc's RE_DUP_MAX: what
/// `sysconf(_SC_RE_DUP_MAX)` reports.
pub(crate) const RE_DUP_MAX: u32 = parse::RE_DUP_MAX;

// The syntax bits of glibc's `reg_syntax_t`, each the one before shifted
// left: `<regex.h>` says what each means.

/// `\` quotes the next byte inside a bracket expression.
pub const RE_BACKSLASH_ESCAPE_IN_LISTS: u64 = 1;
/// `\+` and `\?` are the operators, `+` and `?` literals.
pub const RE_BK_PLUS_QM: u64 = RE_BACKSLASH_ESCAPE_IN_LISTS << 1;
/// `[:alpha:]` and the other classes are recognised.
pub const RE_CHAR_CLASSES: u64 = RE_BK_PLUS_QM << 1;
/// `^` and `$` are anchors wherever they are.
pub const RE_CONTEXT_INDEP_ANCHORS: u64 = RE_CHAR_CLASSES << 1;
/// A repetition with nothing before it is passed over.
pub const RE_CONTEXT_INDEP_OPS: u64 = RE_CONTEXT_INDEP_ANCHORS << 1;
/// ... or is an error.
pub const RE_CONTEXT_INVALID_OPS: u64 = RE_CONTEXT_INDEP_OPS << 1;
/// `.` matches a newline.
pub const RE_DOT_NEWLINE: u64 = RE_CONTEXT_INVALID_OPS << 1;
/// `.` does not match a NUL.
pub const RE_DOT_NOT_NULL: u64 = RE_DOT_NEWLINE << 1;
/// `[^...]` does not match a newline.
pub const RE_HAT_LISTS_NOT_NEWLINE: u64 = RE_DOT_NOT_NULL << 1;
/// Intervals are recognised.
pub const RE_INTERVALS: u64 = RE_HAT_LISTS_NOT_NEWLINE << 1;
/// `+`, `?` and `|` are not operators.
pub const RE_LIMITED_OPS: u64 = RE_INTERVALS << 1;
/// A newline separates alternatives.
pub const RE_NEWLINE_ALT: u64 = RE_LIMITED_OPS << 1;
/// `{}` rather than `\{\}` make an interval.
pub const RE_NO_BK_BRACES: u64 = RE_NEWLINE_ALT << 1;
/// `()` rather than `\(\)` make a group.
pub const RE_NO_BK_PARENS: u64 = RE_NO_BK_BRACES << 1;
/// `\1` ... `\9` are literal digits.
pub const RE_NO_BK_REFS: u64 = RE_NO_BK_PARENS << 1;
/// `|` rather than `\|` separates alternatives.
pub const RE_NO_BK_VBAR: u64 = RE_NO_BK_REFS << 1;
/// A reversed range is an error, not an empty one.
pub const RE_NO_EMPTY_RANGES: u64 = RE_NO_BK_VBAR << 1;
/// A `)` with no `(` is an ordinary one.
pub const RE_UNMATCHED_RIGHT_PAREN_ORD: u64 = RE_NO_EMPTY_RANGES << 1;
/// Accepted and ignored, as glibc ignores it.
pub const RE_NO_POSIX_BACKTRACKING: u64 = RE_UNMATCHED_RIGHT_PAREN_ORD << 1;
/// GNU's `\w` and the rest are ordinary characters.
pub const RE_NO_GNU_OPS: u64 = RE_NO_POSIX_BACKTRACKING << 1;
/// glibc's debugging switch: no effect.
pub const RE_DEBUG: u64 = RE_NO_GNU_OPS << 1;
/// A malformed interval is literal text.
pub const RE_INVALID_INTERVAL_ORD: u64 = RE_DEBUG << 1;
/// Case is ignored.
pub const RE_ICASE: u64 = RE_INVALID_INTERVAL_ORD << 1;
/// `^` is an anchor after `\(` and `\|`.
pub const RE_CARET_ANCHORS_HERE: u64 = RE_ICASE << 1;
/// An interval is an error where a BRE's would be.
pub const RE_CONTEXT_INVALID_DUP: u64 = RE_CARET_ANCHORS_HERE << 1;
/// `re_search` and `re_match` report no subexpressions.
pub const RE_NO_SUB: u64 = RE_CONTEXT_INVALID_DUP << 1;

/// GNU Emacs's syntax: every bit clear.
pub const RE_SYNTAX_EMACS: u64 = 0;
/// awk's.
pub const RE_SYNTAX_AWK: u64 = RE_BACKSLASH_ESCAPE_IN_LISTS
    | RE_DOT_NOT_NULL
    | RE_NO_BK_PARENS
    | RE_NO_BK_REFS
    | RE_NO_BK_VBAR
    | RE_NO_EMPTY_RANGES
    | RE_DOT_NEWLINE
    | RE_CONTEXT_INDEP_ANCHORS
    | RE_CHAR_CLASSES
    | RE_UNMATCHED_RIGHT_PAREN_ORD
    | RE_NO_GNU_OPS;
/// GNU awk's.
pub const RE_SYNTAX_GNU_AWK: u64 =
    (RE_SYNTAX_POSIX_EXTENDED | RE_BACKSLASH_ESCAPE_IN_LISTS | RE_INVALID_INTERVAL_ORD)
        & !(RE_DOT_NOT_NULL | RE_CONTEXT_INDEP_OPS | RE_CONTEXT_INVALID_OPS);
/// POSIX awk's.
pub const RE_SYNTAX_POSIX_AWK: u64 = RE_SYNTAX_POSIX_EXTENDED
    | RE_BACKSLASH_ESCAPE_IN_LISTS
    | RE_INTERVALS
    | RE_NO_GNU_OPS
    | RE_INVALID_INTERVAL_ORD;
/// grep's.
pub const RE_SYNTAX_GREP: u64 =
    (RE_SYNTAX_POSIX_BASIC | RE_NEWLINE_ALT) & !(RE_CONTEXT_INVALID_DUP | RE_DOT_NOT_NULL);
/// egrep's.
pub const RE_SYNTAX_EGREP: u64 =
    (RE_SYNTAX_POSIX_EXTENDED | RE_INVALID_INTERVAL_ORD | RE_NEWLINE_ALT)
        & !(RE_CONTEXT_INVALID_OPS | RE_DOT_NOT_NULL);
/// POSIX egrep's, which is egrep's.
pub const RE_SYNTAX_POSIX_EGREP: u64 = RE_SYNTAX_EGREP;
/// ed's: POSIX's BREs.
pub const RE_SYNTAX_ED: u64 = RE_SYNTAX_POSIX_BASIC;
/// sed's: POSIX's BREs.
pub const RE_SYNTAX_SED: u64 = RE_SYNTAX_POSIX_BASIC;
/// What POSIX's basic and extended syntaxes share.
pub const _RE_SYNTAX_POSIX_COMMON: u64 =
    RE_CHAR_CLASSES | RE_DOT_NEWLINE | RE_DOT_NOT_NULL | RE_INTERVALS | RE_NO_EMPTY_RANGES;
/// POSIX's BREs: `regcomp`'s syntax without REG_EXTENDED.
pub const RE_SYNTAX_POSIX_BASIC: u64 =
    _RE_SYNTAX_POSIX_COMMON | RE_BK_PLUS_QM | RE_CONTEXT_INVALID_DUP;
/// BREs without `\+`, `\?` and `\|`.
pub const RE_SYNTAX_POSIX_MINIMAL_BASIC: u64 = _RE_SYNTAX_POSIX_COMMON | RE_LIMITED_OPS;
/// POSIX's EREs: `regcomp`'s syntax with REG_EXTENDED.
pub const RE_SYNTAX_POSIX_EXTENDED: u64 = _RE_SYNTAX_POSIX_COMMON
    | RE_CONTEXT_INDEP_ANCHORS
    | RE_CONTEXT_INDEP_OPS
    | RE_NO_BK_BRACES
    | RE_NO_BK_PARENS
    | RE_NO_BK_VBAR
    | RE_CONTEXT_INVALID_OPS
    | RE_UNMATCHED_RIGHT_PAREN_ORD;
/// EREs without back-references, and with a repetition with nothing before
/// it an error.
pub const RE_SYNTAX_POSIX_MINIMAL_EXTENDED: u64 = _RE_SYNTAX_POSIX_COMMON
    | RE_CONTEXT_INDEP_ANCHORS
    | RE_CONTEXT_INVALID_OPS
    | RE_NO_BK_BRACES
    | RE_NO_BK_PARENS
    | RE_NO_BK_REFS
    | RE_NO_BK_VBAR
    | RE_UNMATCHED_RIGHT_PAREN_ORD;

/// How many registers `re_search` allocates the first time, at least.
pub const RE_NREGS: usize = 30;
/// `regs_allocated`: `re_search` allocates a `struct re_registers`'s arrays.
pub const REGS_UNALLOCATED: u32 = 0;
/// ... grows them as it needs to.
pub const REGS_REALLOCATE: u32 = 1;
/// ... uses them as they are.
pub const REGS_FIXED: u32 = 2;

// ---------------------------------------------------------------------------
// regex_t, struct re_registers and regmatch_t
// ---------------------------------------------------------------------------

/// `regex_t`: glibc's `struct re_pattern_buffer`, 64 bytes, every field in
/// glibc's place, since a program written for the GNU interface sets and
/// reads them -- `buffer`, `allocated`, `fastmap` and `translate` before
/// `re_compile_pattern`, `re_nsub` and the `not_bol`, `not_eol` and
/// `newline_anchor` bits after it.
#[repr(C)]
pub struct RegexT {
    /// The compiled pattern: a program, at the start of a block of
    /// `allocated` bytes from `malloc` -- or NULL.
    pub buffer: *mut core::ffi::c_void,
    /// The size of that block, and how much of it the program uses.
    pub allocated: usize,
    pub used: usize,
    /// The syntax the pattern was read in.
    pub syntax: u64,
    /// 256 bytes, each 1 if a match can begin with that byte -- or NULL.
    pub fastmap: *mut u8,
    /// 256 bytes, each byte's stand-in in pattern and string -- or NULL.
    pub translate: *mut u8,
    /// The number of subexpressions.
    pub re_nsub: usize,
    /// glibc's seven bit-fields, from the low bit: `can_be_null`,
    /// `regs_allocated` (two bits), `fastmap_accurate`, `no_sub`,
    /// `not_bol`, `not_eol`, `newline_anchor` -- one `unsigned int` in C,
    /// atomic here, since a search changes the first three while another
    /// thread's `regexec` may be reading the rest.
    pub bits: AtomicU32,
    _tail: u32,
}

const CAN_BE_NULL: u32 = 1;
const REGS_ALLOCATED_SHIFT: u32 = 1;
const REGS_ALLOCATED_MASK: u32 = 3 << REGS_ALLOCATED_SHIFT;
const FASTMAP_ACCURATE: u32 = 1 << 3;
const NO_SUB: u32 = 1 << 4;
const NOT_BOL: u32 = 1 << 5;
const NOT_EOL: u32 = 1 << 6;
const NEWLINE_ANCHOR: u32 = 1 << 7;

impl RegexT {
    /// An empty pattern buffer, as a program zeroes one before
    /// `re_compile_pattern`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: core::ptr::null_mut(),
            allocated: 0,
            used: 0,
            syntax: 0,
            fastmap: core::ptr::null_mut(),
            translate: core::ptr::null_mut(),
            re_nsub: 0,
            bits: AtomicU32::new(0),
            _tail: 0,
        }
    }

    fn bits(&self) -> u32 {
        self.bits.load(Ordering::Relaxed)
    }

    fn set(&self, bit: u32, on: bool) {
        if on {
            self.bits.fetch_or(bit, Ordering::Relaxed);
        } else {
            self.bits.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    fn regs_allocated(&self) -> u32 {
        (self.bits() & REGS_ALLOCATED_MASK) >> REGS_ALLOCATED_SHIFT
    }

    fn set_regs_allocated(&self, v: u32) {
        let field = (v << REGS_ALLOCATED_SHIFT) & REGS_ALLOCATED_MASK;
        let mut cur = self.bits();
        while let Err(now) = self.bits.compare_exchange_weak(
            cur,
            (cur & !REGS_ALLOCATED_MASK) | field,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            cur = now;
        }
    }

    /// The program `buffer` holds, if it holds one.
    fn program(&self) -> Option<&prog::Program> {
        let p = self.buffer.cast::<prog::Program>();
        // SAFETY: a non-NULL `buffer` holds the program the compile put
        // there, alive until `regfree`, and no match changes it -- the
        // pattern buffer's contract with its program.
        (!p.is_null()).then(|| unsafe { &*p })
    }
}

impl Default for RegexT {
    fn default() -> Self {
        Self::new()
    }
}

/// glibc's `regex_t` is 64 bytes, its bit-fields one `unsigned int` at 56.
const _: () = {
    assert!(size_of::<RegexT>() == 64, "glibc's regex_t is 64 bytes");
    assert!(core::mem::offset_of!(RegexT, re_nsub) == 48);
    assert!(core::mem::offset_of!(RegexT, bits) == 56);
    assert!(align_of::<RegexT>() <= 8);
};

// SAFETY: RegexT holds a pointer to a heap-allocated program that no
// `regexec` changes: every one keeps its working state in memory of its
// own, so any number of threads may match with one `regex_t` at once, as
// POSIX requires of `regexec`.
unsafe impl Sync for RegexT {}

/// Match position for a sub-expression.
///
/// # `regoff_t` is `long` here, and the comment above this used to say `int`
///
/// It said "Layout matches glibc/musl `regmatch_t` (`regoff_t` = `int`)",
/// which is true of glibc and false of musl -- and musl is the C library every
/// port in this tree links against. Measured:
///
/// | | glibc | musl |
/// |---|---|---|
/// | `regoff_t` | 4 | **8** |
/// | `regmatch_t` | 8 | **16** |
///
/// So a C program declaring `regmatch_t m[10]` gave `regexec` a 160-byte array
/// and got 80 bytes written into the front of it, with every element after the
/// first landing at the wrong offset and every position truncated to 32 bits.
/// Found by `scripts/check-libc-abi.py` on the day it was written; see
/// `design-decisions.md` §1011 and §1010 for the family. Since 2026-09-30
/// `<regex.h>` is posix's own, glibc's in its `_REGEX_LARGE_OFFSETS` form,
/// and keeps `regoff_t` the width POSIX requires (§1161).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegMatch {
    /// Start of match (byte offset), or -1 if not matched.
    pub rm_so: isize,
    /// End of match (byte offset past last char), or -1 if not matched.
    pub rm_eo: isize,
}

/// glibc's `struct re_registers`: where `re_search` and `re_match` report
/// the subexpressions -- `num_regs` of each, `start[i]` and `end[i]` for
/// subexpression `i`, -1 for one that took no part.
#[repr(C)]
#[derive(Debug)]
pub struct ReRegisters {
    pub num_regs: usize,
    pub start: *mut isize,
    pub end: *mut isize,
}

/// The syntax `re_compile_pattern` and `re_comp` read a pattern in: glibc's
/// `re_syntax_options`, a C variable a program may assign to.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut re_syntax_options: u64 = 0;

fn syntax_options() -> u64 {
    // SAFETY: a plain read of a C variable; a program that assigns it from
    // one thread while compiling in another races in C as in glibc.
    unsafe { (&raw const re_syntax_options).read() }
}

// ---------------------------------------------------------------------------
// Compiling
// ---------------------------------------------------------------------------

/// glibc's `re_compile_internal`: `pattern` compiled in `syntax`, through
/// `preg`'s translate table, into `preg`'s buffer -- the one it has if it is
/// big enough (a program may give `re_compile_pattern` a block of its own),
/// else that one grown, which for NULL is a new one. 0, or the error. After
/// an error, as after glibc's, the block is freed and `buffer` is NULL.
///
/// # Safety
///
/// `preg` is a pattern buffer whose `buffer` is NULL or `malloc`ed and
/// `allocated` bytes long, and whose `translate` is NULL or 256 bytes.
unsafe fn compile_into(preg: &mut RegexT, pattern: &[u8], syntax: u64) -> i32 {
    let size = size_of::<prog::Program>();
    preg.set(FASTMAP_ACCURATE, false);
    preg.syntax = syntax;
    preg.set(NOT_BOL, false);
    preg.set(NOT_EOL, false);
    preg.used = 0;
    preg.re_nsub = 0;
    preg.set(CAN_BE_NULL, false);
    preg.set_regs_allocated(REGS_UNALLOCATED);
    if preg.allocated < size || preg.buffer.is_null() {
        // SAFETY: NULL or the block `malloc` gave, per the contract.
        let grown = unsafe { malloc::realloc(preg.buffer.cast(), size) };
        if grown.is_null() {
            return REG_ESPACE;
        }
        preg.buffer = grown.cast();
        preg.allocated = size;
    }
    preg.used = size;
    // SAFETY: NULL or 256 bytes, per the contract.
    let trans = unsafe { preg.translate.cast::<[u8; 256]>().as_ref() };
    let syn = parse::Syntax(syntax);
    let built = parse::parse(pattern, syn, trans)
        .map_err(|c| c.0)
        .and_then(|tree| prog::compile(tree, syn.icase()).map_err(|_| REG_ESPACE));
    match built {
        Ok(program) => {
            preg.re_nsub = program.tree.nsub as usize;
            // SAFETY: `buffer` is a block of at least the program's size
            // from `malloc`, aligned for any object. Whatever it held is
            // written over, not dropped: a program compiling into a buffer
            // it never freed loses that pattern, as glibc's does.
            unsafe { preg.buffer.cast::<prog::Program>().write(program) };
            REG_NOERROR
        }
        Err(code) => {
            // SAFETY: the block, from `malloc`; nothing is left in it.
            unsafe { malloc::free(preg.buffer.cast()) };
            preg.buffer = core::ptr::null_mut();
            preg.allocated = 0;
            code
        }
    }
}

/// Compile a regular expression.
///
/// Returns 0 on success, or the error code: REG_BADPAT and the rest as
/// glibc gives them, REG_ESPACE for a program past two million
/// instructions or a heap with no room for it. As glibc's does, it
/// allocates a fastmap and fills it in, sets `newline_anchor` for
/// REG_NEWLINE and `no_sub` for REG_NOSUB, and leaves no translate table.
///
/// # Safety
///
/// `preg` points to a writable `regex_t`; `pattern` is a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn regcomp(preg: *mut RegexT, pattern: *const u8, cflags: i32) -> i32 {
    if preg.is_null() || pattern.is_null() {
        return REG_BADPAT;
    }
    // SAFETY: the caller's writable `regex_t`.
    let r = unsafe { &mut *preg };
    // As glibc does, it holds no program until one is built, so that a
    // `regfree` after a failed `regcomp` frees nothing.
    r.buffer = core::ptr::null_mut();
    r.allocated = 0;
    r.used = 0;
    r.re_nsub = 0;
    r.fastmap = malloc::malloc(256);
    if r.fastmap.is_null() {
        return REG_ESPACE;
    }
    r.set(NEWLINE_ANCHOR, cflags & REG_NEWLINE != 0);
    r.set(NO_SUB, cflags & REG_NOSUB != 0);
    r.translate = core::ptr::null_mut();
    // SAFETY: `pattern` is a C string, per the contract.
    let pat = unsafe { core::slice::from_raw_parts(pattern, string::strlen(pattern)) };
    // SAFETY: an empty buffer, and no translate table.
    let mut code = unsafe { compile_into(r, pat, parse::Syntax::posix(cflags).0) };
    // POSIX has no REG_ERPAREN: an unmatched `)` is REG_EPAREN, as an
    // unmatched `(` is.
    if code == REG_ERPAREN {
        code = REG_EPAREN;
    }
    if code == REG_NOERROR {
        // Made now, since `regexec` cannot change the pattern buffer. One
        // the heap had no room for is left not made (`fastmap_accurate`
        // clear), which nothing here needs: glibc's cannot fail at all.
        let _ = fill_fastmap(r);
    } else {
        // SAFETY: the fastmap this allocated.
        unsafe { malloc::free(r.fastmap) };
        r.fastmap = core::ptr::null_mut();
    }
    code
}

/// `preg`'s fastmap filled in from its program, `can_be_null` with it: 0, or
/// -2 when the heap had no room to work it out. A buffer with no program or
/// no fastmap is left alone. The caller has the buffer to itself, or holds
/// its program's lock.
fn fill_fastmap(preg: &RegexT) -> i32 {
    let map = preg.fastmap;
    if map.is_null() {
        return 0;
    }
    let Some(p) = preg.program() else {
        return 0;
    };
    let Ok((set, can_be_null)) = fastmap::fastmap(&p.tree) else {
        return -2;
    };
    for b in 0..=255u8 {
        // SAFETY: a fastmap is 256 bytes, the pattern buffer's contract.
        unsafe { map.add(usize::from(b)).write(u8::from(set.contains(b))) };
    }
    preg.set(CAN_BE_NULL, can_be_null);
    preg.set(FASTMAP_ACCURATE, true);
    0
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// A program's lock, held: glibc's `dfa->lock`, which its `re_search` and
/// `re_compile_fastmap` hold throughout, since each may fill in the pattern
/// buffer's fastmap and change its `regs_allocated` -- so two threads may
/// search with one pattern buffer, and take turns. `regexec` does not take
/// it: it changes nothing, and reads the bits atomically.
struct Held<'a>(&'a core::sync::atomic::AtomicI32);

impl<'a> Held<'a> {
    fn take(p: &'a prog::Program) -> Self {
        crate::lowlevellock::lll_lock(&p.lock);
        Self(&p.lock)
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        crate::lowlevellock::lll_unlock(self.0);
    }
}

/// The match of program `p` in `sub` beginning at `from` or after it but not
/// after `last`, groups `0..=nsub`, or `None`; `want` of them asked for.
fn execute(
    p: &prog::Program,
    sub: &prog::Subject<'_>,
    from: usize,
    last: usize,
    want: usize,
    pm: &mut List<(isize, isize)>,
) -> Result<bool, NoMem> {
    let nsub = p.tree.nsub as usize;
    pm.clear();
    pm.resize(nsub.wrapping_add(1), (-1, -1))?;
    let mut vm = prog::Vm::new(p.fwd.len().max(p.rev.len()))?;
    if p.backrefs {
        // The forward program, reading each back-reference as any string,
        // finds the leftmost place a match could begin; the exact matcher
        // looks there, and past it only where the program finds the next.
        let mut m = backref::Matcher::new(p, sub)?;
        let mut pos = from;
        while let Some((s, _)) = vm.search(p, sub, pos, last, false)? {
            if m.at(s, pm)? {
                return Ok(true);
            }
            if s >= sub.end() || s >= last {
                break;
            }
            pos = s.wrapping_add(1);
        }
        return Ok(false);
    }
    let Some((s, e)) = vm.search(p, sub, from, last, want == 0)? else {
        return Ok(false);
    };
    if let Some(slot) = pm.get_mut(0) {
        *slot = (s.cast_signed(), e.cast_signed());
    }
    if want > 1 && nsub > 0 {
        dissect::dissect(p, &mut vm, sub, s, e, pm)?;
    }
    Ok(true)
}

/// glibc's `re_search_internal`: a match of `preg` in `string`, `length`
/// bytes, beginning at `start` or between it and `last_start` -- the first
/// found scanning forwards, or, when `last_start` is the smaller, backwards
/// -- and ending by `stop`; `want` of its groups in `pm`. The string is read
/// through `preg`'s translate table; `^` and `$` see a newline as a line's
/// edge under its `newline_anchor`.
///
/// No match begins or ends past `stop`. glibc's search goes on past it, and
/// finds an empty match there -- `$` at the end of the second string, with
/// `stop` at 2 -- though its header says the search stops at `stop`.
///
/// # Safety
///
/// `string` holds `length` bytes; `preg`'s `translate` is NULL or 256 bytes.
#[allow(clippy::too_many_arguments)]
unsafe fn search(
    preg: &RegexT,
    string: *const u8,
    length: usize,
    start: usize,
    last_start: usize,
    stop: usize,
    want: usize,
    eflags: i32,
    pm: &mut List<(isize, isize)>,
) -> Result<bool, NoMem> {
    let Some(p) = preg.program() else {
        return Ok(false);
    };
    // SAFETY: `length` bytes, per the contract.
    let raw = unsafe { core::slice::from_raw_parts(string, length) };
    let mut translated: List<u8> = List::new();
    // SAFETY: NULL or 256 bytes, per the contract.
    let s: &[u8] = if let Some(table) = unsafe { preg.translate.cast::<[u8; 256]>().as_ref() } {
        translated.reserve(length)?;
        for &c in raw {
            translated.push(table.get(usize::from(c)).copied().unwrap_or(c))?;
        }
        translated.as_slice()
    } else {
        raw
    };
    let stop = stop.min(length);
    let sub = prog::Subject {
        s: s.get(..stop).unwrap_or(s),
        after: s.get(stop).copied(),
        notbol: eflags & REG_NOTBOL != 0,
        noteol: eflags & REG_NOTEOL != 0,
        newline: preg.bits() & NEWLINE_ANCHOR != 0,
    };
    if start <= last_start {
        if start > stop {
            return Ok(false);
        }
        return execute(p, &sub, start, last_start.min(stop), want, pm);
    }
    let mut pos = start.min(stop);
    loop {
        if pos < last_start {
            return Ok(false);
        }
        if execute(p, &sub, pos, pos, want, pm)? {
            return Ok(true);
        }
        if pos == 0 {
            return Ok(false);
        }
        pos = pos.wrapping_sub(1);
    }
}

/// Execute a compiled regular expression against a string.
///
/// Returns 0 if the string matches, `REG_NOMATCH` otherwise -- also, as
/// glibc answers them, when the heap runs out; REG_BADPAT for `eflags` it
/// does not know. On a match, the first `nmatch` entries of `pmatch` get
/// the match and its groups, those past `re_nsub` -1; on none, `pmatch` is
/// left alone. A `regex_t` compiled with REG_NOSUB (its `no_sub` set)
/// writes nothing there.
///
/// # Safety
///
/// `preg` is a `regex_t` `regcomp` compiled; `string` is a C string, or with
/// REG_STARTEND valid up to `pmatch[0].rm_eo`; `pmatch` has `nmatch`
/// writable entries, and at least one for REG_STARTEND.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn regexec(
    preg: *const RegexT,
    string_arg: *const u8,
    nmatch: usize,
    pmatch: *mut RegMatch,
    eflags: i32,
) -> i32 {
    if eflags & !(REG_NOTBOL | REG_NOTEOL | REG_STARTEND) != 0 {
        return REG_BADPAT;
    }
    if preg.is_null() || string_arg.is_null() {
        return REG_NOMATCH;
    }
    // SAFETY: `preg` is a `regex_t`, per the contract.
    let r = unsafe { &*preg };
    let (from, end) = if eflags & REG_STARTEND != 0 {
        if pmatch.is_null() {
            return REG_NOMATCH;
        }
        // SAFETY: REG_STARTEND's `pmatch[0]` is the caller's to give.
        let m0 = unsafe { *pmatch };
        // A range that is not one -- negative, or ending before it begins --
        // is no string to search (glibc's search reads outside it).
        let (Ok(so), Ok(eo)) = (usize::try_from(m0.rm_so), usize::try_from(m0.rm_eo)) else {
            return REG_NOMATCH;
        };
        if so > eo {
            return REG_NOMATCH;
        }
        (so, eo)
    } else {
        // SAFETY: a C string, per the contract.
        (0, unsafe { string::strlen(string_arg) })
    };
    let want = if r.bits() & NO_SUB != 0 || pmatch.is_null() {
        0
    } else {
        nmatch
    };
    let mut pm = List::new();
    // SAFETY: `end` bytes at `string_arg`, per the contract; the pattern
    // buffer's translate table is NULL or 256 bytes.
    match unsafe { search(r, string_arg, end, from, end, end, want, eflags, &mut pm) } {
        Ok(true) => {}
        Ok(false) | Err(_) => return REG_NOMATCH,
    }
    for k in 0..want {
        let (so, eo) = pm.get(k).copied().unwrap_or((-1, -1));
        // SAFETY: `pmatch` has `nmatch` entries, per the contract, and `k`
        // is below it.
        unsafe {
            pmatch.add(k).write(RegMatch {
                rm_so: so,
                rm_eo: eo,
            });
        }
    }
    0
}

// ---------------------------------------------------------------------------
// regfree
// ---------------------------------------------------------------------------

/// Free a compiled regular expression, as glibc's does: the program and its
/// block, the fastmap and the translate table, each pointer cleared, so a
/// second `regfree` frees nothing. `re_nsub` is left as it was.
///
/// # Safety
///
/// `preg` is NULL or a `regex_t` that `regcomp` or `re_compile_pattern` has
/// seen; its translate table, if any, is `malloc`ed, as glibc's header asks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn regfree(preg: *mut RegexT) {
    if preg.is_null() {
        return;
    }
    // SAFETY: `preg` is a `regex_t`, per the contract.
    let r = unsafe { &mut *preg };
    let program = r.buffer.cast::<prog::Program>();
    if !program.is_null() {
        // SAFETY: the program the compile moved into this block: dropped
        // once (its tables returned to the heap), then the block freed.
        unsafe {
            program.drop_in_place();
            malloc::free(program.cast::<u8>());
        }
    }
    r.buffer = core::ptr::null_mut();
    r.allocated = 0;
    // SAFETY: NULL or `malloc`ed, per the contract (and as `regcomp` and
    // `re_comp` leave them).
    unsafe {
        malloc::free(r.fastmap);
        malloc::free(r.translate);
    }
    r.fastmap = core::ptr::null_mut();
    r.translate = core::ptr::null_mut();
}

// ---------------------------------------------------------------------------
// regerror
// ---------------------------------------------------------------------------

/// glibc's message for each error code, 0 to REG_ERPAREN.
const MESSAGES: [&[u8]; 17] = [
    b"Success\0",
    b"No match\0",
    b"Invalid regular expression\0",
    b"Invalid collation character\0",
    b"Invalid character class name\0",
    b"Trailing backslash\0",
    b"Invalid back reference\0",
    b"Unmatched [, [^, [:, [., or [=\0",
    b"Unmatched ( or \\(\0",
    b"Unmatched \\{\0",
    b"Invalid content of \\{\\}\0",
    b"Invalid range end\0",
    b"Memory exhausted\0",
    b"Invalid preceding regular expression\0",
    b"Premature end of regular expression\0",
    b"Regular expression too big\0",
    b"Unmatched ) or \\)\0",
];

/// Error `code`'s message, as `re_compile_pattern` and `re_comp` return it:
/// a static C string.
fn message(code: i32) -> *const u8 {
    usize::try_from(code)
        .ok()
        .and_then(|i| MESSAGES.get(i))
        .or_else(|| MESSAGES.get(2))
        .map_or(core::ptr::null(), |m| m.as_ptr())
}

/// Describe an error code, as glibc does.
///
/// Returns the size of the whole message, its NUL counted; writes as much
/// of it as fits `errbuf_size`, cut short with a NUL. A code no function
/// here returns is a bug in the caller, and as glibc does, `abort`s.
///
/// # Safety
///
/// `errbuf` has `errbuf_size` writable bytes (or is NULL).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn regerror(
    errcode: i32,
    _preg: *const RegexT,
    errbuf: *mut u8,
    errbuf_size: usize,
) -> usize {
    let Some(msg) = usize::try_from(errcode).ok().and_then(|i| MESSAGES.get(i)) else {
        // "Only error codes returned by the rest of the code should be passed
        // to this routine. If we are given anything else ... then the program
        // has a bug. Dump core so we can fix it." -- glibc's regerror.
        crate::unistd::abort();
    };
    let size = msg.len();
    if !errbuf.is_null() && errbuf_size != 0 {
        let copy = if size > errbuf_size {
            errbuf_size.wrapping_sub(1)
        } else {
            size
        };
        // SAFETY: `errbuf` has `errbuf_size` bytes, per the contract, and at
        // most that many are written: `copy` bytes of the message, then --
        // when it was cut short -- a NUL at `copy`, still inside.
        unsafe {
            core::ptr::copy_nonoverlapping(msg.as_ptr(), errbuf, copy);
            if size > errbuf_size {
                errbuf.add(copy).write(0);
            }
        }
    }
    size
}

// ---------------------------------------------------------------------------
// The GNU interface
// ---------------------------------------------------------------------------

/// `re_syntax_options` set to `syntax`; the old value returned.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn re_set_syntax(syntax: u64) -> u64 {
    let old = syntax_options();
    // SAFETY: a plain write of a C variable, as `syntax_options` reads it.
    unsafe { (&raw mut re_syntax_options).write(syntax) };
    old
}

/// `length` bytes of `pattern` compiled in `re_syntax_options`'s syntax
/// into `buffer` -- whose `buffer` and `allocated` may give a block to
/// compile into, whose `translate` table, if set, the pattern is read
/// through, and whose `fastmap` is filled in at the first search. NULL, or
/// what `regerror` would say of the error (REG_ERPAREN's own message for an
/// unmatched `)`). `newline_anchor` is set, and `no_sub` as RE_NO_SUB says,
/// as glibc's does.
///
/// # Safety
///
/// `pattern` holds `length` bytes; `buffer` is a pattern buffer as its doc
/// says, its `buffer` NULL or `malloc`ed and `allocated` bytes long, its
/// `translate` NULL or 256 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_compile_pattern(
    pattern: *const u8,
    length: usize,
    buffer: *mut RegexT,
) -> *const u8 {
    if buffer.is_null() || (pattern.is_null() && length != 0) {
        return message(REG_BADPAT);
    }
    // SAFETY: the caller's pattern buffer.
    let b = unsafe { &mut *buffer };
    let syntax = syntax_options();
    b.set(NO_SUB, syntax & RE_NO_SUB != 0);
    b.set(NEWLINE_ANCHOR, true);
    let pat = if length == 0 {
        &[][..]
    } else {
        // SAFETY: `length` bytes, per the contract.
        unsafe { core::slice::from_raw_parts(pattern, length) }
    };
    // SAFETY: per the contract.
    match unsafe { compile_into(b, pat, syntax) } {
        REG_NOERROR => core::ptr::null(),
        code => message(code),
    }
}

/// `buffer`'s fastmap filled in from its pattern: 0, or -2 when the heap
/// had no room to work it out.
///
/// # Safety
///
/// `buffer` is a compiled pattern buffer, its `fastmap` NULL or 256 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_compile_fastmap(buffer: *mut RegexT) -> i32 {
    if buffer.is_null() {
        return 0;
    }
    // SAFETY: the caller's pattern buffer, shared: what this changes in it
    // is atomic or under the program's lock.
    let b = unsafe { &*buffer };
    let Some(p) = b.program() else {
        return 0;
    };
    let _held = Held::take(p);
    fill_fastmap(b)
}

/// glibc's `re_copy_regs`: the `nregs` groups of `pm` into `regs`, which
/// `how` says to allocate, to grow, or to use as they are -- one element
/// more than the groups, -1 past them. What `regs_allocated` becomes:
/// REGS_UNALLOCATED when the heap had no room.
///
/// # Safety
///
/// `regs`'s arrays are as `how` says: unallocated, or `malloc`ed and
/// `num_regs` long (REGS_REALLOCATE), or `num_regs` long (REGS_FIXED, which
/// is given no more groups than that).
unsafe fn copy_regs(
    regs: &mut ReRegisters,
    pm: &List<(isize, isize)>,
    nregs: usize,
    how: u32,
) -> u32 {
    let need = nregs.saturating_add(1);
    let bytes = need.saturating_mul(size_of::<isize>());
    let mut rval = REGS_REALLOCATE;
    if how == REGS_UNALLOCATED {
        let start = malloc::malloc(bytes).cast::<isize>();
        if start.is_null() {
            return REGS_UNALLOCATED;
        }
        let end = malloc::malloc(bytes).cast::<isize>();
        if end.is_null() {
            // SAFETY: the block just allocated.
            unsafe { malloc::free(start.cast()) };
            return REGS_UNALLOCATED;
        }
        regs.start = start;
        regs.end = end;
        regs.num_regs = need;
    } else if how == REGS_REALLOCATE {
        if need > regs.num_regs {
            // SAFETY: `malloc`ed arrays, per the contract.
            let start = unsafe { malloc::realloc(regs.start.cast(), bytes) }.cast::<isize>();
            if start.is_null() {
                return REGS_UNALLOCATED;
            }
            // SAFETY: as above.
            let end = unsafe { malloc::realloc(regs.end.cast(), bytes) }.cast::<isize>();
            if end.is_null() {
                // SAFETY: the grown block, which `regs` no longer points to
                // (glibc's loses the original `start` the same way).
                unsafe { malloc::free(start.cast()) };
                return REGS_UNALLOCATED;
            }
            regs.start = start;
            regs.end = end;
            regs.num_regs = need;
        }
    } else {
        rval = REGS_FIXED;
    }
    for i in 0..regs.num_regs {
        let (so, eo) = if i < nregs {
            pm.get(i).copied().unwrap_or((-1, -1))
        } else {
            (-1, -1)
        };
        // SAFETY: both arrays hold `num_regs` elements: allocated or grown
        // to it above, or the caller's, per the contract.
        unsafe {
            regs.start.add(i).write(so);
            regs.end.add(i).write(eo);
        }
    }
    rval
}

/// glibc's `re_search_stub`: `re_search` and `re_match` -- `range` bytes of
/// starting places from `start`, ending by `stop`; the match's start, or
/// with `ret_len` its length -- -1 for no match, -2 for an internal error.
///
/// # Safety
///
/// `string` holds `length` bytes; `bufp` is a compiled pattern buffer;
/// `regs` is NULL or as `copy_regs` asks.
#[allow(clippy::too_many_arguments)]
unsafe fn search_stub(
    bufp: *mut RegexT,
    string: *const u8,
    length: isize,
    start: isize,
    range: isize,
    stop: isize,
    regs: *mut ReRegisters,
    ret_len: bool,
) -> isize {
    if bufp.is_null() || (string.is_null() && length > 0) {
        return -2;
    }
    // SAFETY: the caller's pattern buffer, shared: another thread's
    // `regexec` may be reading it, and what a search changes in it is atomic
    // or under the program's lock.
    let b = unsafe { &*bufp };
    let mut last_start = start.wrapping_add(range);
    if start < 0 || start > length {
        return -1;
    }
    if length < last_start || (range >= 0 && last_start < start) {
        last_start = length;
    } else if last_start < 0 || (range < 0 && start <= last_start) {
        last_start = 0;
    }
    let Some(program) = b.program() else {
        return -1;
    };
    let _held = Held::take(program);
    let mut eflags = 0;
    if b.bits() & NOT_BOL != 0 {
        eflags |= REG_NOTBOL;
    }
    if b.bits() & NOT_EOL != 0 {
        eflags |= REG_NOTEOL;
    }
    if start < last_start && !b.fastmap.is_null() && b.bits() & FASTMAP_ACCURATE == 0 {
        // A failure is only a fastmap not made: the search does not need it.
        let _ = fill_fastmap(b);
    }
    let mut regs = if b.bits() & NO_SUB != 0 {
        core::ptr::null_mut()
    } else {
        regs
    };
    let nregs = if regs.is_null() {
        1
    } else {
        // SAFETY: the caller's registers.
        let n = unsafe { (*regs).num_regs };
        if b.regs_allocated() == REGS_FIXED && n <= b.re_nsub {
            if n < 1 {
                regs = core::ptr::null_mut();
                1
            } else {
                n
            }
        } else {
            b.re_nsub.saturating_add(1)
        }
    };
    let mut pm = List::new();
    // All four are in [0, length] now, and so are no longer negative.
    let (length, start, last_start, stop) = (
        length.cast_unsigned(),
        start.cast_unsigned(),
        last_start.cast_unsigned(),
        stop.max(0).cast_unsigned(),
    );
    // SAFETY: `length` bytes at `string`, per the contract.
    let found = unsafe {
        search(
            b, string, length, start, last_start, stop, nregs, eflags, &mut pm,
        )
    };
    match found {
        Ok(true) => {}
        Ok(false) => return -1,
        Err(NoMem) => return -2,
    }
    if !regs.is_null() {
        let how = b.regs_allocated();
        // SAFETY: the caller's registers, as `how` says, per the contract.
        let now = unsafe { copy_regs(&mut *regs, &pm, nregs, how) };
        b.set_regs_allocated(now);
        if now == REGS_UNALLOCATED {
            return -2;
        }
    }
    let (so, eo) = pm.get(0).copied().unwrap_or((-1, -1));
    if ret_len {
        eo.wrapping_sub(start.cast_signed())
    } else {
        so
    }
}

/// glibc's `re_search_2_stub`: the two strings taken as one -- copied
/// together when both have bytes -- and searched as `search_stub` does.
///
/// # Safety
///
/// Each string holds its length's bytes; the rest as `search_stub` asks.
#[allow(clippy::too_many_arguments)]
unsafe fn search_2_stub(
    bufp: *mut RegexT,
    string1: *const u8,
    length1: isize,
    string2: *const u8,
    length2: isize,
    start: isize,
    range: isize,
    regs: *mut ReRegisters,
    stop: isize,
    ret_len: bool,
) -> isize {
    if length1 < 0 || length2 < 0 || stop < 0 {
        return -2;
    }
    let Some(len) = length1.checked_add(length2) else {
        return -2;
    };
    if length2 > 0 && length1 > 0 {
        let joined = malloc::malloc(len.cast_unsigned());
        if joined.is_null() {
            return -2;
        }
        // SAFETY: `joined` has `len` bytes; each string its length's.
        unsafe {
            core::ptr::copy_nonoverlapping(string1, joined, length1.cast_unsigned());
            core::ptr::copy_nonoverlapping(
                string2,
                joined.add(length1.cast_unsigned()),
                length2.cast_unsigned(),
            );
        }
        // SAFETY: `len` bytes at `joined`; the rest per the contract.
        let r = unsafe { search_stub(bufp, joined, len, start, range, stop, regs, ret_len) };
        // SAFETY: the block allocated above.
        unsafe { malloc::free(joined) };
        r
    } else if length2 > 0 {
        // SAFETY: per the contract.
        unsafe { search_stub(bufp, string2, len, start, range, stop, regs, ret_len) }
    } else {
        // SAFETY: per the contract.
        unsafe { search_stub(bufp, string1, len, start, range, stop, regs, ret_len) }
    }
}

/// The first match of `buffer` in `string` (`length` bytes) beginning at
/// `start` or `range` bytes on at most -- before it, scanning backwards, if
/// `range` is negative: where it begins, -1 for none, -2 for an internal
/// error; and its subexpressions in `regs`, if neither it nor the pattern's
/// `no_sub` says not to.
///
/// # Safety
///
/// `string` holds `length` bytes; `buffer` is a compiled pattern buffer;
/// `regs` is NULL, or its arrays are as `buffer.regs_allocated` says --
/// unallocated, `malloc`ed and `num_regs` long, or fixed at `num_regs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_search(
    buffer: *mut RegexT,
    string: *const u8,
    length: isize,
    start: isize,
    range: isize,
    regs: *mut ReRegisters,
) -> isize {
    // SAFETY: per the contract.
    unsafe { search_stub(buffer, string, length, start, range, length, regs, false) }
}

/// `re_search` in `string1` and `string2` taken as one, a match ending by
/// `stop`.
///
/// # Safety
///
/// Each string holds its length's bytes; the rest as `re_search` asks.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_search_2(
    buffer: *mut RegexT,
    string1: *const u8,
    length1: isize,
    string2: *const u8,
    length2: isize,
    start: isize,
    range: isize,
    regs: *mut ReRegisters,
    stop: isize,
) -> isize {
    // SAFETY: per the contract.
    unsafe {
        search_2_stub(
            buffer, string1, length1, string2, length2, start, range, regs, stop, false,
        )
    }
}

/// A match beginning at `start` itself: how many bytes it matched, -1 for
/// none, -2 for an internal error; and `regs` as `re_search` fills them.
///
/// # Safety
///
/// As `re_search`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_match(
    buffer: *mut RegexT,
    string: *const u8,
    length: isize,
    start: isize,
    regs: *mut ReRegisters,
) -> isize {
    // SAFETY: per the contract.
    unsafe { search_stub(buffer, string, length, start, 0, length, regs, true) }
}

/// `re_match` in `string1` and `string2` taken as one, a match ending by
/// `stop`.
///
/// # Safety
///
/// As `re_search_2`.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_match_2(
    buffer: *mut RegexT,
    string1: *const u8,
    length1: isize,
    string2: *const u8,
    length2: isize,
    start: isize,
    regs: *mut ReRegisters,
    stop: isize,
) -> isize {
    // SAFETY: per the contract.
    unsafe {
        search_2_stub(
            buffer, string1, length1, string2, length2, start, 0, regs, stop, true,
        )
    }
}

/// `regs` given the program's own arrays, `num_regs` long, which later
/// searches with `buffer` fill and may grow (`malloc`ed, then) -- or, for
/// 0, none, so that the next search allocates its own.
///
/// # Safety
///
/// `buffer` and `regs` are the caller's, writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_set_registers(
    buffer: *mut RegexT,
    regs: *mut ReRegisters,
    num_regs: usize,
    starts: *mut isize,
    ends: *mut isize,
) {
    if buffer.is_null() || regs.is_null() {
        return;
    }
    // SAFETY: the caller's, per the contract.
    let (b, r) = unsafe { (&*buffer, &mut *regs) };
    if num_regs != 0 {
        b.set_regs_allocated(REGS_REALLOCATE);
        r.num_regs = num_regs;
        r.start = starts;
        r.end = ends;
    } else {
        b.set_regs_allocated(REGS_UNALLOCATED);
        r.num_regs = 0;
        r.start = core::ptr::null_mut();
        r.end = core::ptr::null_mut();
    }
}

// ---------------------------------------------------------------------------
// 4.2BSD's re_comp and re_exec
// ---------------------------------------------------------------------------

/// `re_comp`'s one pattern, which `re_exec` matches.
static mut RE_COMP_BUF: RegexT = RegexT::new();
/// Held while either uses it: glibc's has no lock, and two threads using
/// these at once race there; here they take turns.
static RE_COMP_LOCK: crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();

/// glibc's message for `re_comp(NULL)` before any pattern.
const NO_PREVIOUS: &[u8] = b"No previous regular expression\0";

/// 4.2BSD's: `s` compiled in `re_syntax_options`'s syntax, in place of the
/// pattern before it -- NULL, or the error's message. `re_comp(NULL)` keeps
/// the pattern there is, and says so if there is none.
///
/// # Safety
///
/// `s` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_comp(s: *const u8) -> *mut u8 {
    // SAFETY: a static lock, alive for the process.
    let _g = unsafe { crate::perprocess::lock_pool((&raw const RE_COMP_LOCK).cast_mut()) };
    let buf = &raw mut RE_COMP_BUF;
    // SAFETY: the lock is held, so this thread alone uses the buffer.
    let b = unsafe { &mut *buf };
    if s.is_null() {
        return if b.buffer.is_null() {
            NO_PREVIOUS.as_ptr().cast_mut()
        } else {
            core::ptr::null_mut()
        };
    }
    if !b.buffer.is_null() {
        // The pattern before freed, its fastmap kept.
        let fastmap = b.fastmap;
        b.fastmap = core::ptr::null_mut();
        // SAFETY: a buffer `re_comp` compiled.
        unsafe { regfree(b) };
        *b = RegexT::new();
        b.fastmap = fastmap;
    }
    if b.fastmap.is_null() {
        b.fastmap = malloc::malloc(256);
        if b.fastmap.is_null() {
            return message(REG_ESPACE).cast_mut();
        }
    }
    b.set(NEWLINE_ANCHOR, true);
    // SAFETY: a C string, per the contract.
    let pat = unsafe { core::slice::from_raw_parts(s, string::strlen(s)) };
    // SAFETY: an empty buffer (or one `re_comp` emptied), no translate table.
    match unsafe { compile_into(b, pat, syntax_options()) } {
        REG_NOERROR => core::ptr::null_mut(),
        code => message(code).cast_mut(),
    }
}

/// 4.2BSD's: 1 if `s` matches `re_comp`'s pattern, else 0 (and 0 with none).
///
/// # Safety
///
/// `s` is a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn re_exec(s: *const u8) -> i32 {
    // SAFETY: a static lock, alive for the process.
    let _g = unsafe { crate::perprocess::lock_pool((&raw const RE_COMP_LOCK).cast_mut()) };
    // SAFETY: the lock is held; a C string, per the contract.
    let r = unsafe { regexec(&raw const RE_COMP_BUF, s, 0, core::ptr::null_mut(), 0) };
    i32::from(r == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::collections::HashMap;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    // -- ABI layout --

    /// `regex_t` is glibc's `struct re_pattern_buffer`, field for field,
    /// its seven bit-fields the low bits of the `unsigned int` at 56 in
    /// glibc's order -- measured from glibc's own header, a bit set at a
    /// time.
    ///
    /// The size and two offsets are also `const` assertions above; this test
    /// adds the rest.
    #[test]
    fn test_regex_t_is_glibcs_pattern_buffer() {
        use core::mem::{align_of, offset_of, size_of};
        assert_eq!(size_of::<RegexT>(), 64);
        assert_eq!(align_of::<RegexT>(), 8);
        let offsets = [
            offset_of!(RegexT, buffer),
            offset_of!(RegexT, allocated),
            offset_of!(RegexT, used),
            offset_of!(RegexT, syntax),
            offset_of!(RegexT, fastmap),
            offset_of!(RegexT, translate),
            offset_of!(RegexT, re_nsub),
            offset_of!(RegexT, bits),
        ];
        assert_eq!(offsets, [0, 8, 16, 24, 32, 40, 48, 56]);
        assert_eq!(
            [
                CAN_BE_NULL,
                REGS_ALLOCATED_MASK,
                FASTMAP_ACCURATE,
                NO_SUB,
                NOT_BOL,
                NOT_EOL,
                NEWLINE_ANCHOR
            ],
            [0x01, 0x06, 0x08, 0x10, 0x20, 0x40, 0x80]
        );
        let r = RegexT::new();
        assert!(r.buffer.is_null() && r.fastmap.is_null() && r.translate.is_null());
        assert_eq!(
            (r.allocated, r.used, r.syntax, r.re_nsub, r.bits()),
            (0, 0, 0, 0, 0)
        );
    }

    /// A null-terminated pattern's pointer, as `regcomp` expects.
    fn cstr(s: &[u8]) -> *const u8 {
        assert_eq!(s.last(), Some(&0), "test patterns must be NUL-terminated");
        s.as_ptr()
    }

    fn nul(s: &[u8]) -> Vec<u8> {
        let mut v = s.to_vec();
        v.push(0);
        v
    }

    // -- glibc's answers, and the standard's where glibc's are not --

    const ORACLE: &str = include_str!("regex_oracle.txt");
    const DEVIATIONS: &str = include_str!("regex_deviations.txt");

    /// An oracle token: `\xNN` escapes, `\-` the empty string.
    fn unescape(tok: &str) -> Vec<u8> {
        if tok == "\\-" {
            return Vec::new();
        }
        let b = tok.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&tok[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// `regcomp`'s answer as the oracle writes it, and the compiled object.
    fn compiled(pat: &[u8], cflags: i32) -> (String, RegexT) {
        let mut re = RegexT::new();
        let p = nul(pat);
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        let rc = unsafe { regcomp(&raw mut re, p.as_ptr(), cflags) };
        let answer = if rc == 0 {
            format!("n{}", re.re_nsub)
        } else {
            format!("e{rc}")
        };
        (answer, re)
    }

    /// An S line's answer for one string: `!`, or pmatch[0..=re_nsub] as
    /// two digits a pair.
    fn s_answer(re: &RegexT, s: &[u8]) -> String {
        let n = re.re_nsub + 1;
        let mut m = std::vec![
            RegMatch {
                rm_so: -7,
                rm_eo: -7
            };
            n
        ];
        let subject = nul(s);
        // SAFETY: a compiled RegexT, a C string, `n` slots.
        let r = unsafe { regexec(re, subject.as_ptr(), n, m.as_mut_ptr(), 0) };
        if r == REG_NOMATCH {
            return String::from("!");
        }
        if r != 0 {
            return format!("x{r}");
        }
        let mut out = String::new();
        for x in &m {
            if x.rm_so < 0 && x.rm_eo < 0 {
                out.push_str("__");
            } else {
                out.push_str(&format!("{}{}", x.rm_so, x.rm_eo));
            }
        }
        out
    }

    /// An I line's answer: regexec's return, then every one of `nmatch`
    /// entries, -7 where it wrote nothing.
    fn i_answer(
        re: &RegexT,
        s: &[u8],
        eflags: i32,
        startend: Option<(isize, isize)>,
        nmatch: usize,
    ) -> String {
        let mut m = std::vec![
            RegMatch {
                rm_so: -7,
                rm_eo: -7
            };
            nmatch.max(1)
        ];
        if let Some((so, eo)) = startend {
            m[0] = RegMatch {
                rm_so: so,
                rm_eo: eo,
            };
        }
        let subject = nul(s);
        // SAFETY: a compiled RegexT; the subject NUL-terminated and, for
        // REG_STARTEND, valid to `eo`; `m` has at least `nmatch` slots.
        let r = unsafe { regexec(re, subject.as_ptr(), nmatch, m.as_mut_ptr(), eflags) };
        let mut out = format!("{r}");
        for (k, x) in m.iter().take(nmatch).enumerate() {
            out.push(if k == 0 { '=' } else { ',' });
            out.push_str(&format!("{}:{}", x.rm_so, x.rm_eo));
        }
        out
    }

    /// Every line of the oracle, answered: glibc's answer, or where it is not
    /// the standard's the model's (regex_deviations.txt).
    #[test]
    fn every_case_is_glibcs_or_the_standards() {
        let mut dev: HashMap<(usize, String), String> = HashMap::new();
        for line in DEVIATIONS.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(' ').collect();
            dev.insert(
                (f[0].parse().unwrap(), String::from(f[1])),
                String::from(f[3]),
            );
        }
        let mut sets: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
        let mut wrong: Vec<String> = Vec::new();
        let mut cases = 0usize;
        let mut deviating = 0usize;
        for (idx, line) in ORACLE.lines().enumerate() {
            let line_no = idx + 1;
            if let Some(rest) = line.strip_prefix("# strings ") {
                let (name, list) = rest.split_once(": ").unwrap();
                sets.insert(String::from(name), list.split(' ').map(unescape).collect());
                continue;
            }
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(' ').collect();
            let want = |subject: &str, glibc: &str| -> String {
                dev.get(&(line_no, String::from(subject)))
                    .cloned()
                    .unwrap_or_else(|| String::from(glibc))
            };
            match f[0] {
                "S" => {
                    let cflags: i32 = f[2].parse().unwrap();
                    let pat = unescape(f[3]);
                    let want_comp = want("c", f[4]);
                    let (got_comp, mut re) = compiled(&pat, cflags);
                    cases += 1;
                    if got_comp != want_comp {
                        wrong.push(format!(
                            "{line_no}: {cflags} {:?}: regcomp {got_comp}, want {want_comp}",
                            String::from_utf8_lossy(&pat)
                        ));
                    } else if want_comp.starts_with('n') {
                        for (j, s) in sets[f[1]].iter().enumerate() {
                            let glibc = f.get(5 + j).copied().unwrap_or("-");
                            let expect = want(&format!("{j}"), glibc);
                            if dev.contains_key(&(line_no, format!("{j}"))) {
                                deviating += 1;
                            }
                            if expect == "-" {
                                continue;
                            }
                            cases += 1;
                            let got = s_answer(&re, s);
                            if got != expect {
                                wrong.push(format!(
                                    "{line_no}: {cflags} {:?} {:?}: {got}, want {expect} (glibc {glibc})",
                                    String::from_utf8_lossy(&pat),
                                    String::from_utf8_lossy(s)
                                ));
                            }
                        }
                    }
                    // SAFETY: a RegexT regcomp has seen, freed once.
                    unsafe { regfree(&raw mut re) };
                }
                "I" => {
                    let cflags: i32 = f[1].parse().unwrap();
                    let pat = unescape(f[2]);
                    let s = unescape(f[4]);
                    let eflags: i32 = f[5].parse().unwrap();
                    let startend = (f[6] != "-").then(|| {
                        let (a, b) = f[6].split_once(',').unwrap();
                        (a.parse().unwrap(), b.parse().unwrap())
                    });
                    let want_comp = want("c", f[3]);
                    let (got_comp, mut re) = compiled(&pat, cflags);
                    cases += 1;
                    if got_comp != want_comp {
                        wrong.push(format!(
                            "{line_no}: {cflags} {:?}: regcomp {got_comp}, want {want_comp}",
                            String::from_utf8_lossy(&pat[..pat.len().min(60)])
                        ));
                    } else if want_comp.starts_with('n') {
                        let nmatch = if f[7] == "-" {
                            re.re_nsub + 1
                        } else {
                            f[7].parse::<usize>().unwrap().min(256)
                        };
                        let glibc = f.get(8).copied().unwrap_or("-");
                        let expect = want("0", glibc);
                        if dev.contains_key(&(line_no, String::from("0"))) {
                            deviating += 1;
                        }
                        if expect != "-" {
                            cases += 1;
                            let got = i_answer(&re, &s, eflags, startend, nmatch);
                            if got != expect {
                                wrong.push(format!(
                                    "{line_no}: {cflags} {:?} {:?} {eflags} {startend:?}: {got}, want {expect} (glibc {glibc})",
                                    String::from_utf8_lossy(&pat[..pat.len().min(60)]),
                                    String::from_utf8_lossy(&s[..s.len().min(40)])
                                ));
                            }
                        }
                    }
                    // SAFETY: a RegexT regcomp has seen, freed once.
                    unsafe { regfree(&raw mut re) };
                }
                "E" => {
                    let code: i32 = f[1].parse().unwrap();
                    let size: usize = f[2].parse().unwrap();
                    let returned: usize = f[3].parse().unwrap();
                    let mut buf = [b'#'; 256];
                    buf[255] = 0;
                    let got = regerror(code, core::ptr::null(), buf.as_mut_ptr(), size);
                    let mut hex = String::new();
                    for b in &buf[..size + 2] {
                        hex.push_str(&format!("{b:02x}"));
                    }
                    cases += 1;
                    if got != returned || hex != f[4] {
                        wrong.push(format!(
                            "{line_no}: regerror({code}, {size}) = {got} {hex}, want {returned} {}",
                            f[4]
                        ));
                    }
                }
                other => panic!("{line_no}: unknown line kind {other}"),
            }
        }
        for w in wrong.iter().take(60) {
            std::eprintln!("{w}");
        }
        assert!(
            wrong.is_empty(),
            "{} of {cases} answers are neither glibc's nor the standard's",
            wrong.len()
        );
        assert!(cases > 400_000, "the oracle read as {cases} answers");
        assert!(deviating > 10_000, "{deviating} deviations read");
    }

    // -- the public API, as C calls it --

    /// Compile, match, free — the whole life of a `regex_t` as C uses it.
    #[test]
    fn test_regcomp_regexec_regfree_round_trip() {
        let mut re = RegexT::new();
        // SAFETY: `re` is a live, writable RegexT; the pattern is NUL-terminated.
        let rc = unsafe { regcomp(&raw mut re, cstr(b"^a(b+)c$\0"), REG_EXTENDED) };
        assert_eq!(rc, 0, "pattern is valid");
        assert_eq!(re.re_nsub, 1, "one parenthesised sub-expression");
        assert!(!re.buffer.is_null(), "regcomp installed a program");

        let mut m = [RegMatch {
            rm_so: -1,
            rm_eo: -1,
        }; 2];
        // SAFETY: `re` is compiled; the subject is NUL-terminated; `m` has 2 slots.
        let rc = unsafe { regexec(&raw const re, cstr(b"abbbc\0"), 2, m.as_mut_ptr(), 0) };
        assert_eq!(rc, 0, "abbbc matches ^a(b+)c$");
        assert_eq!((m[0].rm_so, m[0].rm_eo), (0, 5), "whole match");
        assert_eq!((m[1].rm_so, m[1].rm_eo), (1, 4), "the b+ group");

        // SAFETY: `re` is compiled and not yet freed.
        let rc = unsafe { regexec(&raw const re, cstr(b"ac\0"), 2, m.as_mut_ptr(), 0) };
        assert_eq!(rc, REG_NOMATCH, "b+ needs at least one b");

        // SAFETY: `re` was compiled by `regcomp` and is freed exactly once here.
        unsafe { regfree(&raw mut re) };
        assert!(re.buffer.is_null(), "regfree clears the program pointer");
    }

    /// `regfree` leaves the object safe to free again, and frees once.
    #[test]
    fn test_regfree_is_idempotent() {
        let before = crate::malloc::live_allocations::count();
        let mut re = RegexT::new();
        // SAFETY: `re` is a live, writable RegexT; the pattern is NUL-terminated.
        assert_eq!(unsafe { regcomp(&raw mut re, cstr(b"x\0"), 0) }, 0);
        // SAFETY: compiled above; the second call sees a null program and returns.
        unsafe { regfree(&raw mut re) };
        // SAFETY: `re` is a valid RegexT whose program is already null.
        unsafe { regfree(&raw mut re) };
        assert!(re.buffer.is_null());
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "every block regcomp took, returned once"
        );
    }

    /// The live-region counter the leak tests rely on actually moves.
    #[test]
    fn test_live_region_counter_is_wired_up() {
        let before = crate::malloc::live_allocations::count();
        let mut re = RegexT::new();
        // SAFETY: `re` is a live, writable RegexT; the pattern is NUL-terminated.
        assert_eq!(unsafe { regcomp(&raw mut re, cstr(b"a\0"), 0) }, 0);
        assert!(
            crate::malloc::live_allocations::count() > before,
            "regcomp holds a program, so the count must have risen"
        );
        // SAFETY: compiled above, freed exactly once.
        unsafe { regfree(&raw mut re) };
        assert_eq!(crate::malloc::live_allocations::count(), before);
    }

    /// A rejected pattern leaves nothing allocated and no program installed,
    /// whichever stage refused it.
    #[test]
    fn test_regcomp_error_paths_leak_nothing() {
        let cases: &[(&[u8], i32, i32)] = &[
            (b"a\\\0", 0, REG_EESCAPE),
            (b"(a\0", REG_EXTENDED, REG_EPAREN),
            (b"\\(a\0", 0, REG_EPAREN),
            (b"[z-a]\0", 0, REG_ERANGE),
            (b"[[:nosuch:]]\0", 0, REG_ECTYPE),
            (b"a{2,1}\0", REG_EXTENDED, REG_BADBR),
            (b"a{32768}\0", REG_EXTENDED, REG_ESIZE),
            (b"(a)\\2\0", REG_EXTENDED, REG_ESUBREG),
            (b"*a\0", REG_EXTENDED, REG_BADRPT),
            (b"((((a){1000}){1000}){1000})\0", REG_EXTENDED, REG_ESPACE),
        ];
        for &(pat, cflags, want) in cases {
            let before = crate::malloc::live_allocations::count();
            let mut re = RegexT::new();
            // SAFETY: `re` is a live, writable RegexT; `pat` is NUL-terminated.
            let rc = unsafe { regcomp(&raw mut re, cstr(pat), cflags) };
            assert_eq!(rc, want, "pattern {pat:?} under cflags {cflags}");
            assert!(
                re.buffer.is_null(),
                "a failed regcomp installs no program ({pat:?})"
            );
            assert_eq!(
                crate::malloc::live_allocations::count(),
                before,
                "failed regcomp leaked ({pat:?})"
            );
        }
    }

    /// Every allocation `regcomp` and `regexec` make, failed in turn -- glibc's
    /// test of CVE-2025-8058, which found its `regcomp` freeing twice after
    /// one: each call answers REG_ESPACE (`regexec`, as glibc's does,
    /// REG_NOMATCH) or succeeds, and leaves nothing allocated behind it.
    #[test]
    fn every_allocation_failing_in_turn_leaks_and_breaks_nothing() {
        let cases: &[(&[u8], i32, &[u8])] = &[
            (b"[[:alpha:][:digit:]]x[^a-c]\0", REG_EXTENDED, b"ax1\0"),
            (b"(a|ab)(c|bcd)(d*)\0", REG_EXTENDED, b"abcd\0"),
            (b"\\(a*\\)*\\1b\0", 0, b"aab\0"),
            (b"(x{2,3}|y){1,4}z\0", REG_EXTENDED, b"xxyxxxz\0"),
        ];
        for &(pat, cflags, subject) in cases {
            let mut completed = false;
            for k in 1..10_000u64 {
                let before = crate::malloc::live_allocations::count();
                let mut re = RegexT::new();
                crate::malloc::live_allocations::fail_after(k);
                // SAFETY: a writable RegexT and a NUL-terminated pattern.
                let rc = unsafe { regcomp(&raw mut re, cstr(pat), cflags) };
                let mut matched = None;
                if rc == 0 {
                    let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 4];
                    // SAFETY: compiled above; `m` has 4 slots.
                    matched = Some(unsafe {
                        regexec(&raw const re, cstr(subject), 4, m.as_mut_ptr(), 0)
                    });
                }
                crate::malloc::live_allocations::fail_after(0);
                assert!(rc == 0 || rc == REG_ESPACE, "{pat:?}, allocation {k}: {rc}");
                if let Some(r) = matched {
                    assert!(r == 0 || r == REG_NOMATCH, "{pat:?}, allocation {k}: {r}");
                }
                // SAFETY: a RegexT regcomp has seen, freed once.
                unsafe { regfree(&raw mut re) };
                assert_eq!(
                    crate::malloc::live_allocations::count(),
                    before,
                    "{pat:?}: leaked with allocation {k} failed"
                );
                if matched == Some(0) {
                    // Every allocation the two made has had its turn.
                    completed = true;
                    break;
                }
            }
            assert!(completed, "{pat:?} never matched");
        }
    }

    /// `regcomp` rejects null pointers rather than dereferencing them.
    #[test]
    fn test_regcomp_rejects_null_arguments() {
        let mut re = RegexT::new();
        // SAFETY: a null pattern is the case under test; it is checked first.
        assert_eq!(
            unsafe { regcomp(&raw mut re, core::ptr::null(), 0) },
            REG_BADPAT
        );
        assert!(re.buffer.is_null(), "nothing installed on rejection");
        // SAFETY: a null `preg` is likewise the case under test.
        assert_eq!(
            unsafe { regcomp(core::ptr::null_mut(), cstr(b"a\0"), 0) },
            REG_BADPAT
        );
    }

    /// `regexec` on a `regex_t` that was never compiled reports no match
    /// instead of following a null program pointer.
    #[test]
    fn test_regexec_on_uncompiled_regex_is_nomatch() {
        let re = RegexT::new();
        let mut m = [RegMatch {
            rm_so: -1,
            rm_eo: -1,
        }; 1];
        // SAFETY: a valid RegexT with a null program, the case under test.
        let rc = unsafe { regexec(&raw const re, cstr(b"anything\0"), 1, m.as_mut_ptr(), 0) };
        assert_eq!(rc, REG_NOMATCH);
        let mut re = re;
        // SAFETY: freeing a never-compiled RegexT is a no-op.
        unsafe { regfree(&raw mut re) };
    }

    /// Every one of `nmatch` slots past the groups is written -1, as POSIX
    /// has it -- here for many more slots than groups.
    #[test]
    fn test_regexec_clears_surplus_pmatch_slots() {
        let mut re = RegexT::new();
        // SAFETY: `re` is a live, writable RegexT; the pattern is NUL-terminated.
        assert_eq!(
            unsafe { regcomp(&raw mut re, cstr(b"(a)\0"), REG_EXTENDED) },
            0
        );
        let mut m = [RegMatch {
            rm_so: 77,
            rm_eo: 77,
        }; 40];
        // SAFETY: `re` is compiled; `m` has 40 slots, the `nmatch` passed.
        let rc = unsafe { regexec(&raw const re, cstr(b"a\0"), 40, m.as_mut_ptr(), 0) };
        assert_eq!(rc, 0);
        assert_eq!((m[0].rm_so, m[0].rm_eo), (0, 1), "whole match");
        assert_eq!((m[1].rm_so, m[1].rm_eo), (0, 1), "group 1");
        for (i, slot) in m.iter().enumerate().skip(2) {
            assert_eq!((slot.rm_so, slot.rm_eo), (-1, -1), "slot {i}");
        }
        // SAFETY: compiled above, freed exactly once.
        unsafe { regfree(&raw mut re) };
    }

    /// Each `regcomp` owns its own program, and the count returns to where it
    /// started only if every one is released.
    #[test]
    fn test_many_regcomps_all_free() {
        let before = crate::malloc::live_allocations::count();
        for i in 0..8 {
            let mut re = RegexT::new();
            // SAFETY: `re` is a live, writable RegexT; the pattern is NUL-terminated.
            assert_eq!(
                unsafe { regcomp(&raw mut re, cstr(b"([0-9]+)\\1\0"), REG_EXTENDED) },
                0
            );
            let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 2];
            // SAFETY: compiled immediately above.
            let rc = unsafe { regexec(&raw const re, cstr(b"n4242\0"), 2, m.as_mut_ptr(), 0) };
            assert_eq!(rc, 0, "iteration {i}");
            assert_eq!((m[1].rm_so, m[1].rm_eo), (1, 3));
            // SAFETY: compiled above, freed exactly once per iteration.
            unsafe { regfree(&raw mut re) };
        }
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "eight compiles, eight frees"
        );
    }

    /// `regerror` NUL-terminates within the buffer it was given, reports the
    /// size it wanted, and writes nothing to a buffer of no size or none.
    #[test]
    fn test_regerror_truncates_and_reports_full_length() {
        let mut buf = [0xAAu8; 32];
        let want = regerror(REG_NOMATCH, core::ptr::null(), buf.as_mut_ptr(), buf.len());
        assert_eq!(want, b"No match\0".len(), "length includes the NUL");
        assert_eq!(&buf[..want], b"No match\0");
        let mut small = [0xAAu8; 4];
        let want = regerror(
            REG_NOMATCH,
            core::ptr::null(),
            small.as_mut_ptr(),
            small.len(),
        );
        assert_eq!(want, b"No match\0".len(), "unchanged by truncation");
        assert_eq!(&small, b"No \0");
        let mut untouched = [0xAAu8; 4];
        let want = regerror(REG_NOMATCH, core::ptr::null(), untouched.as_mut_ptr(), 0);
        assert_eq!(want, b"No match\0".len());
        assert_eq!(untouched, [0xAAu8; 4], "size 0 must write nothing");
        assert_eq!(
            regerror(REG_NOMATCH, core::ptr::null(), core::ptr::null_mut(), 32),
            b"No match\0".len(),
        );
    }

    // -- what the oracle cannot show --

    /// A pattern nested far deeper than any stack could recurse compiles,
    /// matches and frees: nothing here recurses once a level.
    #[test]
    fn nesting_a_hundred_thousand_deep_costs_heap_not_stack() {
        let depth = 100_000;
        let mut pat = Vec::new();
        pat.extend(core::iter::repeat_n(b'(', depth));
        pat.push(b'a');
        pat.extend(core::iter::repeat_n(b')', depth));
        pat.push(0);
        let mut re = RegexT::new();
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        assert_eq!(
            unsafe { regcomp(&raw mut re, pat.as_ptr(), REG_EXTENDED) },
            0
        );
        assert_eq!(re.re_nsub, depth);
        let mut m = std::vec![RegMatch { rm_so: 0, rm_eo: 0 }; depth + 1];
        // SAFETY: compiled above; `m` has `depth + 1` slots.
        let rc = unsafe { regexec(&raw const re, cstr(b"xa\0"), depth + 1, m.as_mut_ptr(), 0) };
        assert_eq!(rc, 0);
        assert!(m.iter().all(|x| (x.rm_so, x.rm_eo) == (1, 2)));
        // SAFETY: compiled above, freed once.
        unsafe { regfree(&raw mut re) };
    }

    /// Long subjects: the search is one pass, and the submatches' runs are
    /// over what their nodes span, so a megabyte costs a megabyte's time.
    #[test]
    fn a_megabyte_subject_is_matched_in_one_pass() {
        let mut s = std::vec![b'a'; 1 << 20];
        s.extend_from_slice(b"bc\0");
        let mut re = RegexT::new();
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        assert_eq!(
            unsafe { regcomp(&raw mut re, cstr(b"(a|b)*(b)(c)\0"), REG_EXTENDED) },
            0
        );
        let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 4];
        // SAFETY: compiled above; `m` has 4 slots.
        let rc = unsafe { regexec(&raw const re, s.as_ptr(), 4, m.as_mut_ptr(), 0) };
        assert_eq!(rc, 0);
        let n = (1isize << 20) as isize;
        assert_eq!((m[0].rm_so, m[0].rm_eo), (0, n + 2));
        assert_eq!((m[1].rm_so, m[1].rm_eo), (n - 1, n));
        assert_eq!((m[2].rm_so, m[2].rm_eo), (n, n + 1));
        assert_eq!((m[3].rm_so, m[3].rm_eo), (n + 1, n + 2));
        // SAFETY: compiled above, freed once.
        unsafe { regfree(&raw mut re) };
    }

    /// A back-reference is matched exactly only where the automaton, reading
    /// it as any string, finds a match could begin: `(a*)\1b` over twenty
    /// thousand `a`s is one pass, where glibc takes cubic time (a fifth of a
    /// second over 400 of them, measured). And where one can, the longest:
    /// a string of period 1352 is two periods, `\1` the first.
    #[test]
    fn back_references_look_only_where_a_match_could_begin() {
        let mut s = std::vec![b'a'; 20_000];
        s.push(0);
        let mut re = RegexT::new();
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        assert_eq!(
            unsafe { regcomp(&raw mut re, cstr(b"(a*)\\1b\0"), REG_EXTENDED) },
            0
        );
        // SAFETY: compiled above; no slots asked for.
        let rc = unsafe { regexec(&raw const re, s.as_ptr(), 0, core::ptr::null_mut(), 0) };
        assert_eq!(rc, REG_NOMATCH);
        // SAFETY: compiled above, freed once.
        unsafe { regfree(&raw mut re) };

        let mut t: Vec<u8> = (0..4000u32)
            .map(|i| b'a' + ((i * 7 + i / 26) % 26) as u8)
            .collect();
        t.push(0);
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        assert_eq!(
            unsafe { regcomp(&raw mut re, cstr(b"(.*)\\1\0"), REG_EXTENDED) },
            0
        );
        let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 2];
        // SAFETY: compiled above; `m` has 2 slots.
        let rc = unsafe { regexec(&raw const re, t.as_ptr(), 2, m.as_mut_ptr(), 0) };
        assert_eq!(rc, 0);
        assert_eq!((m[0].rm_so, m[0].rm_eo), (0, 2704));
        assert_eq!((m[1].rm_so, m[1].rm_eo), (0, 1352));
        // SAFETY: compiled above, freed once.
        unsafe { regfree(&raw mut re) };
    }

    /// A repeated group before a back-reference, over thousands of bytes:
    /// each state of the repetition keeps one way there, not one to every
    /// end, so this is near-linear, where glibc takes 5.8 s over 8000 bytes
    /// (measured). The subjects are the probe's own -- one generator drawn
    /// on through strings of 250, 500, ... 8000 bytes -- and the answers
    /// for the last two glibc's.
    #[test]
    fn a_repeated_group_before_a_back_reference_scales() {
        let mut x: u32 = 12345;
        let mut subjects = Vec::new();
        for n in [250, 500, 1000, 2000, 4000, 8000] {
            let s: Vec<u8> = (0..n)
                .map(|_| {
                    x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    if (x >> 16) & 1 == 1 { b'a' } else { b'b' }
                })
                .collect();
            subjects.push(s);
        }
        let cases = [
            (&subjects[4], (0, 4000), (3998, 3999)),
            (&subjects[5], (0, 7999), (7997, 7998)),
        ];
        for (s, whole, group) in cases {
            let n = s.len();
            let mut s = s.clone();
            s.push(0);
            let mut re = RegexT::new();
            // SAFETY: a writable RegexT and a NUL-terminated pattern.
            assert_eq!(
                unsafe { regcomp(&raw mut re, cstr(b"(a|b)*\\1\0"), REG_EXTENDED) },
                0
            );
            let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 2];
            // SAFETY: compiled above; `m` has 2 slots.
            let rc = unsafe { regexec(&raw const re, s.as_ptr(), 2, m.as_mut_ptr(), 0) };
            assert_eq!(rc, 0, "{n} bytes");
            assert_eq!((m[0].rm_so, m[0].rm_eo), whole, "{n} bytes");
            assert_eq!((m[1].rm_so, m[1].rm_eo), group, "{n} bytes");
            // SAFETY: compiled above, freed once.
            unsafe { regfree(&raw mut re) };
        }
    }

    /// Concurrent `regexec`s of one `regex_t` agree: none of them writes to
    /// the program.
    #[test]
    fn one_regex_t_matched_by_many_threads_at_once() {
        let mut re = RegexT::new();
        // SAFETY: a writable RegexT and a NUL-terminated pattern.
        assert_eq!(
            unsafe { regcomp(&raw mut re, cstr(b"(a|ab)(c|bcd)(d*)\0"), REG_EXTENDED) },
            0
        );
        let shared = &re;
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(move || {
                    for _ in 0..200 {
                        let mut m = [RegMatch { rm_so: 0, rm_eo: 0 }; 4];
                        // SAFETY: compiled above and not freed until the scope ends.
                        let rc = unsafe { regexec(shared, cstr(b"abcd\0"), 4, m.as_mut_ptr(), 0) };
                        assert_eq!(rc, 0);
                        let got: Vec<(isize, isize)> =
                            m.iter().map(|x| (x.rm_so, x.rm_eo)).collect();
                        assert_eq!(got, std::vec![(0, 4), (0, 2), (2, 3), (3, 4)]);
                    }
                });
            }
        });
        // SAFETY: compiled above; every thread has finished with it.
        unsafe { regfree(&raw mut re) };
    }

    // -- the GNU interface ---------------------------------------------------

    use std::boxed::Box;
    use std::string::ToString;

    const GNU_ORACLE: &str = include_str!("regex_gnu_oracle.txt");
    const GNU_DEVIATIONS: &str = include_str!("regex_gnu_deviations.txt");

    /// Serialises the tests that set `re_syntax_options` or use `re_comp`'s
    /// pattern: each is the process's one.
    static GNU_GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn gnu_lock() -> std::sync::MutexGuard<'static, ()> {
        GNU_GLOBALS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The harness's translate tables, `malloc`ed, as `regfree` frees one.
    fn table(name: &str) -> *mut u8 {
        let map = |c: u8| -> u8 {
            match name {
                "fold" => c.to_ascii_lowercase(),
                "upper" => c.to_ascii_uppercase(),
                "swap" => match c {
                    b'a' => b'b',
                    b'b' => b'a',
                    _ => c,
                },
                _ => c,
            }
        };
        if name == "-" {
            return core::ptr::null_mut();
        }
        let t = crate::malloc::malloc(256);
        assert!(!t.is_null());
        for c in 0..=255u8 {
            // SAFETY: 256 bytes.
            unsafe { t.add(usize::from(c)).write(map(c)) };
        }
        t
    }

    /// The code whose message `msg` is; -1 for none (NULL).
    fn code_of(msg: *const u8) -> i32 {
        if msg.is_null() {
            return -1;
        }
        // SAFETY: one of MESSAGES, a C string.
        let text = unsafe { core::ffi::CStr::from_ptr(msg.cast()) }.to_bytes_with_nul();
        MESSAGES
            .iter()
            .position(|m| *m == text)
            .map_or(-99, |i| i32::try_from(i).unwrap())
    }

    /// `p` compiled by `re_compile_pattern` in `syntax` through table
    /// `trans` into a zeroed pattern buffer, as the harness's C side does:
    /// the buffer, and the code of the message it returned. The caller holds
    /// `gnu_lock`.
    fn gnu_compile(syntax: u64, p: &[u8], trans: &str) -> (Box<RegexT>, Option<i32>) {
        let mut re = Box::new(RegexT::new());
        re.translate = table(trans);
        re_set_syntax(syntax);
        // SAFETY: `p.len()` bytes; a zeroed buffer with a malloc'ed table.
        let msg = unsafe { re_compile_pattern(p.as_ptr(), p.len(), &raw mut *re) };
        let err = (!msg.is_null()).then(|| code_of(msg));
        (re, err)
    }

    fn compiled_text(re: &RegexT, err: Option<i32>) -> String {
        err.map_or_else(|| format!("n{}", re.re_nsub), |c| format!("e{c}"))
    }

    fn two_digits(so: isize, eo: isize) -> String {
        if so < 0 && eo < 0 {
            String::from("__")
        } else if (0..=9).contains(&so) && (0..=9).contains(&eo) {
            format!("{so}{eo}")
        } else {
            String::from("?")
        }
    }

    /// The deviations file, by oracle line: (subject, the model's answer).
    fn gnu_deviations() -> HashMap<usize, Vec<(String, String)>> {
        let mut out: HashMap<usize, Vec<(String, String)>> = HashMap::new();
        for line in GNU_DEVIATIONS.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.splitn(4, ' ');
            let (Some(n), Some(subj), Some(rest)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            // An M line's answers have spaces in them, so glibc's and the
            // model's are kept together here, and `want` takes glibc's --
            // which the oracle line has -- off the front.
            let model = f.next().map_or_else(String::new, String::from);
            out.entry(n.parse().unwrap())
                .or_default()
                .push((String::from(subj), format!("{rest} {model}")));
        }
        out
    }

    /// A G or M answer of ours, against glibc's -- or the model's, where the
    /// deviations file has one -- for oracle line `n`, subject `subj`.
    fn want<'a>(
        dev: &'a HashMap<usize, Vec<(String, String)>>,
        n: usize,
        subj: &str,
        glibc: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        let Some(list) = dev.get(&n) else {
            return std::borrow::Cow::Borrowed(glibc);
        };
        for (s, both) in list {
            if s == subj {
                // `both` is "<glibc's> <the model's>": the model's is what
                // remains once glibc's, as this line has it, is taken off.
                let g = glibc.trim();
                if let Some(m) = both.strip_prefix(g) {
                    return std::borrow::Cow::Owned(String::from(m.trim()));
                }
            }
        }
        std::borrow::Cow::Borrowed(glibc)
    }

    /// Every case of the GNU interface's oracle answered as glibc does, or
    /// as the standard does where the deviations file says glibc does not.
    #[test]
    fn every_gnu_case_is_glibcs_or_the_standards() {
        let _g = gnu_lock();
        let dev = gnu_deviations();
        let mut wrong = Vec::new();
        let mut counts = [0usize; 4];
        for (i, line) in GNU_ORACLE.lines().enumerate() {
            let n = i + 1;
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split(' ').collect();
            match f[0] {
                "G" => {
                    counts[0] += 1;
                    gnu_g(&f, n, &dev, &mut wrong);
                }
                "M" => {
                    counts[1] += 1;
                    gnu_m(&f, n, &dev, &mut wrong);
                }
                "F" => {
                    counts[2] += 1;
                    gnu_f(&f, n, &mut wrong);
                }
                "C" => {
                    counts[3] += 1;
                    gnu_c(&f, n, &mut wrong);
                }
                other => panic!("line {n}: {other}"),
            }
        }
        for w in wrong.iter().take(25) {
            std::eprintln!("{w}");
        }
        assert!(
            wrong.is_empty(),
            "{} answers are not glibc's or the standard's",
            wrong.len()
        );
        assert!(
            counts[0] > 40_000 && counts[1] > 2_500 && counts[2] > 400 && counts[3] > 20,
            "{counts:?}"
        );
    }

    const GSTRINGS: [&[u8]; 20] = [
        b"", b"a", b"b", b"A", b"ab", b"ba", b"aa", b"\n", b"a\nb", b"\0", b"(", b")", b"|", b"+",
        b"?", b"*", b"{1}", b"1", b"\\", b"]a",
    ];

    fn gnu_g(
        f: &[&str],
        n: usize,
        dev: &HashMap<usize, Vec<(String, String)>>,
        wrong: &mut Vec<String>,
    ) {
        let syn = u64::from_str_radix(f[1], 16).unwrap();
        let pat = unescape(f[2]);
        let (mut re, err) = gnu_compile(syn, &pat, "-");
        let comp = compiled_text(&re, err);
        let want_comp = want(dev, n, "c", f[3]);
        if comp != want_comp {
            wrong.push(format!(
                "line {n}: G {} {}: compiled {comp}, want {want_comp}",
                f[1], f[2]
            ));
        }
        if err.is_none() {
            for (j, s) in GSTRINGS.iter().enumerate() {
                let mut regs = ReRegisters {
                    num_regs: 0,
                    start: core::ptr::null_mut(),
                    end: core::ptr::null_mut(),
                };
                re.set_regs_allocated(REGS_UNALLOCATED);
                let len = isize::try_from(s.len()).unwrap();
                // SAFETY: `len` bytes; a compiled buffer; unallocated registers.
                let r = unsafe { re_search(&raw mut *re, s.as_ptr(), len, 0, len, &raw mut regs) };
                let got = if r == -1 {
                    String::from("!")
                } else if r < 0 {
                    format!("x{r}")
                } else if regs.num_regs == 0 {
                    format!("@{r}")
                } else {
                    (0..=re.re_nsub)
                        .map(|k| {
                            // SAFETY: re_search allocated num_regs > re_nsub.
                            unsafe { two_digits(*regs.start.add(k), *regs.end.add(k)) }
                        })
                        .collect()
                };
                // SAFETY: NULL or what re_search allocated.
                unsafe {
                    crate::malloc::free(regs.start.cast());
                    crate::malloc::free(regs.end.cast());
                }
                let glibc = f.get(4 + j).copied().unwrap_or("-");
                let w = want(dev, n, &format!("{j}"), glibc);
                if got != w {
                    wrong.push(format!(
                        "line {n}: G {} {} {:?}: {got}, want {w}",
                        f[1], f[2], s
                    ));
                }
            }
        }
        // SAFETY: compiled (or failed) above; its table malloc'ed.
        unsafe { regfree(&raw mut *re) };
    }

    /// glibc's `regoff_t` is an int: the harness's starts and ranges reached
    /// glibc cut to 32 bits.
    fn as_c_int(s: &str) -> isize {
        let v: i128 = s.parse().unwrap();
        #[allow(clippy::cast_possible_truncation)]
        let c = v as i32;
        c as isize
    }

    fn gnu_m(
        f: &[&str],
        n: usize,
        dev: &HashMap<usize, Vec<(String, String)>>,
        wrong: &mut Vec<String>,
    ) {
        let syn = u64::from_str_radix(f[1], 16).unwrap();
        let pat = unescape(f[2]);
        let trans = f[3];
        let bits: u32 = f[4].parse().unwrap();
        let call = f[5];
        let s1 = unescape(f[6]);
        let s2 = unescape(f[7]);
        let (start, range) = (as_c_int(f[8]), as_c_int(f[9]));
        let stop: isize = f[10].parse().unwrap();
        let mode = &f[11][..1];
        let k: usize = f[11][1..].parse().unwrap();
        let glibc = f[13..].join(" ");
        let (mut re, err) = gnu_compile(syn, &pat, trans);
        let comp = compiled_text(&re, err);
        if comp != f[12] {
            wrong.push(format!("line {n}: M compiled {comp}, want {}", f[12]));
        }
        if err.is_some() {
            // SAFETY: as compiled.
            unsafe { regfree(&raw mut *re) };
            return;
        }
        if bits & 1 != 0 {
            re.set(NOT_BOL, true);
        }
        if bits & 2 != 0 {
            re.set(NOT_EOL, true);
        }
        if bits & 4 != 0 {
            re.set(NEWLINE_ANCHOR, false);
        }
        if bits & 8 != 0 {
            re.set(NO_SUB, true);
        }
        let mut regs = ReRegisters {
            num_regs: 0,
            start: core::ptr::null_mut(),
            end: core::ptr::null_mut(),
        };
        let mut fixed_s = [-9isize; 64];
        let mut fixed_e = [-9isize; 64];
        let rp: *mut ReRegisters = match mode {
            "u" | "U" => &raw mut regs,
            "r" => {
                let bytes = k * size_of::<isize>();
                let s = crate::malloc::malloc(bytes).cast::<isize>();
                let e = crate::malloc::malloc(bytes).cast::<isize>();
                for i in 0..k {
                    // SAFETY: k elements each.
                    unsafe {
                        s.add(i).write(-9);
                        e.add(i).write(-9);
                    }
                }
                // SAFETY: the buffer and registers above.
                unsafe { re_set_registers(&raw mut *re, &raw mut regs, k, s, e) };
                &raw mut regs
            }
            "f" => {
                regs.num_regs = k;
                regs.start = fixed_s.as_mut_ptr();
                regs.end = fixed_e.as_mut_ptr();
                re.set_regs_allocated(REGS_FIXED);
                &raw mut regs
            }
            _ => core::ptr::null_mut(),
        };
        let mut got = String::new();
        let times = if mode == "U" { 2 } else { 1 };
        for _ in 0..times {
            let (l1, l2) = (
                isize::try_from(s1.len()).unwrap(),
                isize::try_from(s2.len()).unwrap(),
            );
            // SAFETY: each string its length's bytes; the buffer compiled;
            // the registers as their mode says.
            let r = unsafe {
                match call {
                    "s" => re_search(&raw mut *re, s1.as_ptr(), l1, start, range, rp),
                    "m" => re_match(&raw mut *re, s1.as_ptr(), l1, start, rp),
                    "S" => re_search_2(
                        &raw mut *re,
                        s1.as_ptr(),
                        l1,
                        s2.as_ptr(),
                        l2,
                        start,
                        range,
                        rp,
                        stop,
                    ),
                    "M" => re_match_2(
                        &raw mut *re,
                        s1.as_ptr(),
                        l1,
                        s2.as_ptr(),
                        l2,
                        start,
                        rp,
                        stop,
                    ),
                    _ => {
                        let z = s1.iter().position(|&c| c == 0).unwrap_or(s1.len());
                        let s = nul(&s1[..z]);
                        let nm = (re.re_nsub + 1).min(16);
                        let mut pm = [RegMatch {
                            rm_so: -9,
                            rm_eo: -9,
                        }; 16];
                        let eflags = i32::try_from(start).unwrap();
                        let rc = regexec(&raw const *re, s.as_ptr(), nm, pm.as_mut_ptr(), eflags);
                        got.push_str(&format!(" {rc}"));
                        for (q, m) in pm.iter().take(nm).enumerate() {
                            got.push_str(&format!(
                                "{}{}:{}",
                                if q == 0 { '=' } else { ',' },
                                m.rm_so,
                                m.rm_eo
                            ));
                        }
                        continue;
                    }
                }
            };
            got.push_str(&format!(" {r}"));
            if !rp.is_null() {
                let shown = regs.num_regs.min(64);
                // SAFETY: num_regs elements each, as re_search left them.
                let (s, e): (Vec<String>, Vec<String>) = (0..shown)
                    .map(|q| unsafe {
                        (
                            (*regs.start.add(q)).to_string(),
                            (*regs.end.add(q)).to_string(),
                        )
                    })
                    .unzip();
                got.push_str(&format!(
                    " {}:{}/{} {}",
                    regs.num_regs,
                    s.join(","),
                    e.join(","),
                    re.regs_allocated()
                ));
            }
        }
        if mode != "f" {
            // SAFETY: NULL or malloc'ed, by re_search or above.
            unsafe {
                crate::malloc::free(regs.start.cast());
                crate::malloc::free(regs.end.cast());
            }
        }
        // SAFETY: compiled above.
        unsafe { regfree(&raw mut *re) };
        let w = want(dev, n, "0", &glibc);
        if got.trim() != w.trim() {
            wrong.push(format!(
                "line {n}: {} -> {got}, want {w}",
                &f[..13].join(" ")
            ));
        }
    }

    fn gnu_f(f: &[&str], n: usize, wrong: &mut Vec<String>) {
        let syn = u64::from_str_radix(f[1], 16).unwrap();
        let cf: i32 = f[2].parse().unwrap();
        let pat = unescape(f[3]);
        let want_rest = f[5..].join(" ");
        let (mut re, comp) = if cf >= 0 {
            let mut re = Box::new(RegexT::new());
            let p = nul(&pat);
            // SAFETY: a C string; a writable buffer.
            let rc = unsafe { regcomp(&raw mut *re, p.as_ptr(), cf) };
            let comp = if rc == 0 {
                format!("n{}", re.re_nsub)
            } else {
                format!("e{rc}")
            };
            (re, comp)
        } else {
            let (mut re, err) = gnu_compile(syn, &pat, f[4]);
            let comp = compiled_text(&re, err);
            if err.is_none() {
                re.fastmap = crate::malloc::malloc(256);
                // SAFETY: compiled; a 256-byte fastmap.
                assert_eq!(unsafe { re_compile_fastmap(&raw mut *re) }, 0);
            }
            (re, comp)
        };
        if comp != f[5] {
            wrong.push(format!(
                "line {n}: F {} {} {}: {comp}, want {}",
                f[1], f[2], f[3], f[5]
            ));
        } else if comp.starts_with('n') {
            let mut hex = String::new();
            for b in (0..256).step_by(4) {
                let mut d = 0;
                for q in 0..4 {
                    // SAFETY: the fastmap, 256 bytes.
                    if unsafe { *re.fastmap.add(b + q) } != 0 {
                        d |= 1 << q;
                    }
                }
                hex.push(char::from_digit(d, 16).unwrap());
            }
            let fields = [
                re.bits() & CAN_BE_NULL != 0,
                false,
                re.bits() & FASTMAP_ACCURATE != 0,
                re.bits() & NO_SUB != 0,
                re.bits() & NOT_BOL != 0,
                re.bits() & NOT_EOL != 0,
                re.bits() & NEWLINE_ANCHOR != 0,
            ];
            let mut ftext: String = fields.iter().map(|&b| if b { '1' } else { '0' }).collect();
            ftext.replace_range(1..2, &format!("{}", re.regs_allocated()));
            let got = format!("{comp} {hex} {ftext} {:x}", re.syntax);
            if got != want_rest {
                wrong.push(format!(
                    "line {n}: F {} {} {} {}:\n   {got}\n   want {want_rest}",
                    f[1], f[2], f[3], f[4]
                ));
            }
        }
        // SAFETY: compiled (or failed) above.
        unsafe { regfree(&raw mut *re) };
    }

    fn gnu_c(f: &[&str], n: usize, wrong: &mut Vec<String>) {
        let got = match f[1] {
            "s" => {
                re_set_syntax(u64::from_str_radix(f[2], 16).unwrap());
                String::from("ok")
            }
            "c" => {
                let arg = (f[2] != "-").then(|| nul(&unescape(f[2])));
                // SAFETY: NULL or a C string.
                let r = unsafe { re_comp(arg.as_ref().map_or(core::ptr::null(), |a| a.as_ptr())) };
                if r.is_null() {
                    String::from("-")
                } else {
                    // SAFETY: a static message.
                    let text =
                        unsafe { core::ffi::CStr::from_ptr(r.cast_const().cast()) }.to_bytes();
                    text.iter()
                        .map(|&c| {
                            if c > 0x20 && c < 0x7f && c != b'\\' {
                                String::from(char::from(c))
                            } else {
                                format!("\\x{c:02x}")
                            }
                        })
                        .collect()
                }
            }
            _ => {
                let arg = nul(&unescape(f[2]));
                // SAFETY: a C string.
                format!("{}", unsafe { re_exec(arg.as_ptr()) })
            }
        };
        if got != f[4] {
            wrong.push(format!("line {n}: {}: {got}", f.join(" ")));
        }
    }

    /// `re_compile_pattern` reads `re_syntax_options`, sets `newline_anchor`
    /// and, from RE_NO_SUB, `no_sub`; returns glibc's messages -- an
    /// unmatched `)` its own, which `regcomp` reports as REG_EPAREN; and
    /// `re_set_syntax` answers the old syntax.
    #[test]
    fn re_compile_pattern_takes_the_global_syntax_and_says_why() {
        let _g = gnu_lock();
        let old = re_set_syntax(RE_SYNTAX_POSIX_BASIC);
        let mut re = RegexT::new();
        // SAFETY: three bytes; a zeroed buffer.
        let m = unsafe { re_compile_pattern(b"a\\)".as_ptr(), 3, &raw mut re) };
        assert_eq!(code_of(m), REG_ERPAREN);
        assert!(re.buffer.is_null(), "a failed compile leaves no block");
        assert_eq!(
            re_set_syntax(RE_SYNTAX_POSIX_EXTENDED | RE_NO_SUB),
            RE_SYNTAX_POSIX_BASIC
        );
        let mut re = RegexT::new();
        // SAFETY: as above.
        assert!(unsafe { re_compile_pattern(b"(a)".as_ptr(), 3, &raw mut re) }.is_null());
        assert_eq!(re.re_nsub, 1);
        assert_ne!(re.bits() & NEWLINE_ANCHOR, 0);
        assert_ne!(re.bits() & NO_SUB, 0);
        assert_eq!(re.syntax, RE_SYNTAX_POSIX_EXTENDED | RE_NO_SUB);
        // SAFETY: compiled above.
        unsafe { regfree(&raw mut re) };
        re_set_syntax(old);
        // An ERE's `)` with no `(` is an ordinary one; a BRE's `\)` is an
        // error, which regcomp names as POSIX has it.
        let mut posix = RegexT::new();
        // SAFETY: a C string.
        assert_eq!(
            unsafe { regcomp(&raw mut posix, c"a)".as_ptr().cast(), REG_EXTENDED) },
            0
        );
        // SAFETY: as compiled.
        unsafe { regfree(&raw mut posix) };
        let mut posix = RegexT::new();
        // SAFETY: a C string.
        assert_eq!(
            unsafe { regcomp(&raw mut posix, c"a\\)".as_ptr().cast(), 0) },
            REG_EPAREN
        );
    }

    /// A program's own block, given in `buffer` and `allocated`, is compiled
    /// into when big enough and grown when not; on an error it is freed and
    /// `buffer` left NULL, as glibc's is.
    #[test]
    fn re_compile_pattern_uses_and_grows_the_programs_block() {
        let _g = gnu_lock();
        re_set_syntax(RE_SYNTAX_POSIX_BASIC);
        let before = crate::malloc::live_allocations::count();
        let size = size_of::<prog::Program>();
        let mut re = RegexT::new();
        let block = crate::malloc::malloc(size * 2);
        re.buffer = block.cast();
        re.allocated = size * 2;
        // SAFETY: a malloc'ed block of `allocated` bytes.
        assert!(unsafe { re_compile_pattern(b"ab".as_ptr(), 2, &raw mut re) }.is_null());
        assert_eq!(
            re.buffer,
            block.cast(),
            "a block big enough is used as it is"
        );
        assert_eq!((re.allocated, re.used), (size * 2, size));
        // SAFETY: compiled above.
        unsafe { regfree(&raw mut re) };
        let mut re = RegexT::new();
        re.buffer = crate::malloc::malloc(8).cast();
        re.allocated = 8;
        // SAFETY: as above.
        assert!(unsafe { re_compile_pattern(b"ab".as_ptr(), 2, &raw mut re) }.is_null());
        assert_eq!((re.allocated, re.used), (size, size));
        // SAFETY: compiled above.
        unsafe { regfree(&raw mut re) };
        let mut re = RegexT::new();
        re.buffer = crate::malloc::malloc(size).cast();
        re.allocated = size;
        // SAFETY: as above.
        assert_eq!(
            code_of(unsafe { re_compile_pattern(b"\\(".as_ptr(), 2, &raw mut re) }),
            REG_EPAREN
        );
        assert!(re.buffer.is_null());
        assert_eq!(re.allocated, 0);
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "nothing left allocated"
        );
    }

    /// Starts, ranges and stops a C caller can give that glibc's int
    /// arithmetic cannot hold, here held: the full range of a `long`, and
    /// the refusals.
    #[test]
    fn re_search_holds_every_long_start_and_range() {
        let _g = gnu_lock();
        let (mut re, err) = gnu_compile(RE_SYNTAX_POSIX_BASIC, b"b", "-");
        assert_eq!(err, None);
        let s = b"aab";
        let p = &raw mut *re;
        let none = core::ptr::null_mut();
        // SAFETY: three bytes; a compiled buffer; no registers.
        unsafe {
            assert_eq!(re_search(p, s.as_ptr(), 3, 0, isize::MAX, none), 2);
            assert_eq!(re_search(p, s.as_ptr(), 3, 3, isize::MIN, none), 2);
            assert_eq!(re_search(p, s.as_ptr(), 3, 0, isize::MIN, none), -1);
            assert_eq!(re_search(p, s.as_ptr(), 3, 4, 0, none), -1);
            assert_eq!(re_search(p, s.as_ptr(), 3, -1, 5, none), -1);
            assert_eq!(
                re_search(p, s.as_ptr(), 3, isize::MAX, isize::MAX, none),
                -1
            );
            assert_eq!(re_match(p, s.as_ptr(), 3, 2, none), 1);
            assert_eq!(
                re_search_2(p, s.as_ptr(), 1, s.as_ptr().add(1), 2, 0, 3, none, -1),
                -2
            );
            assert_eq!(
                re_search_2(p, s.as_ptr(), -1, s.as_ptr(), 2, 0, 3, none, 3),
                -2
            );
            assert_eq!(
                re_search_2(p, s.as_ptr(), isize::MAX, s.as_ptr(), 2, 0, 3, none, 3),
                -2
            );
            assert_eq!(
                re_search_2(p, s.as_ptr(), 2, s.as_ptr().add(2), 1, 0, 3, none, 99),
                2
            );
            regfree(p);
        }
    }

    /// The fastmap holds every byte a match can begin with: each byte that
    /// begins a match of a pattern somewhere in a string of the alphabet is
    /// in its fastmap, whatever the syntax.
    #[test]
    fn a_fastmap_holds_every_byte_a_match_begins_with() {
        let _g = gnu_lock();
        let alphabet = b"ab\n_A ";
        let patterns: [&[u8]; 14] = [
            b"a",
            b"\\<a",
            b"\\ba",
            b"^a",
            b"a*b",
            b"\\(a\\)\\1",
            b"\\(\\)\\1b",
            b"[^a]",
            b"\\W",
            b"b\\|^",
            b"\\`a",
            b"\\>b",
            b"a\\B",
            b".",
        ];
        for syn in [
            RE_SYNTAX_POSIX_BASIC,
            RE_SYNTAX_POSIX_BASIC | RE_ICASE,
            RE_SYNTAX_GREP,
        ] {
            for &p in &patterns {
                let (mut re, err) = gnu_compile(syn, p, "-");
                assert_eq!(err, None, "{p:?}");
                re.fastmap = crate::malloc::malloc(256);
                // SAFETY: compiled; a 256-byte fastmap.
                assert_eq!(unsafe { re_compile_fastmap(&raw mut *re) }, 0);
                for a in alphabet {
                    for b in alphabet {
                        let s = [*a, *b];
                        for at in 0..2 {
                            // SAFETY: two bytes; a compiled buffer.
                            let len = unsafe {
                                re_match(&raw mut *re, s.as_ptr(), 2, at, core::ptr::null_mut())
                            };
                            if len > 0 {
                                let first = s[at as usize];
                                // SAFETY: the fastmap.
                                let in_map = unsafe { *re.fastmap.add(usize::from(first)) } != 0;
                                assert!(
                                    in_map,
                                    "{p:?} in {syn:x}: {s:?} at {at}, {first} not in the fastmap"
                                );
                            }
                        }
                    }
                }
                // SAFETY: compiled above.
                unsafe { regfree(&raw mut *re) };
            }
        }
    }

    /// `regfree` frees what the pattern buffer holds -- the program, the
    /// fastmap and the translate table -- and clears each.
    #[test]
    fn regfree_frees_the_fastmap_and_the_table() {
        let _g = gnu_lock();
        let before = crate::malloc::live_allocations::count();
        let (mut re, err) = gnu_compile(RE_SYNTAX_POSIX_BASIC, b"a*", "fold");
        assert_eq!(err, None);
        re.fastmap = crate::malloc::malloc(256);
        // SAFETY: compiled; a fastmap.
        assert_eq!(unsafe { re_compile_fastmap(&raw mut *re) }, 0);
        // SAFETY: as compiled.
        unsafe { regfree(&raw mut *re) };
        assert!(re.buffer.is_null() && re.fastmap.is_null() && re.translate.is_null());
        assert_eq!(crate::malloc::live_allocations::count(), before);
    }

    /// Every allocation of a GNU search failing in turn -- the translated
    /// string, the joined one, the registers, the matcher's tables -- leaks
    /// nothing, and is answered -2.
    #[test]
    fn every_allocation_of_a_search_failing_in_turn_leaks_nothing() {
        let _g = gnu_lock();
        let (mut re, err) = gnu_compile(RE_SYNTAX_POSIX_BASIC, b"\\(a*\\)\\(b\\)", "fold");
        assert_eq!(err, None);
        let mut completed = None;
        for k in 1..10_000u64 {
            let before = crate::malloc::live_allocations::count();
            let mut regs = ReRegisters {
                num_regs: 0,
                start: core::ptr::null_mut(),
                end: core::ptr::null_mut(),
            };
            re.set_regs_allocated(REGS_UNALLOCATED);
            crate::malloc::live_allocations::fail_after(k);
            // SAFETY: two strings; a compiled buffer; unallocated registers.
            let r = unsafe {
                re_search_2(
                    &raw mut *re,
                    b"xA".as_ptr(),
                    2,
                    b"Ab".as_ptr(),
                    2,
                    0,
                    4,
                    &raw mut regs,
                    4,
                )
            };
            crate::malloc::live_allocations::fail_after(0);
            if r >= 0 {
                assert_eq!(r, 1);
                // SAFETY: re_search's arrays.
                unsafe {
                    crate::malloc::free(regs.start.cast());
                    crate::malloc::free(regs.end.cast());
                }
            } else {
                assert_eq!(r, -2, "allocation {k} failed");
                assert!(regs.start.is_null() && regs.end.is_null());
            }
            assert_eq!(
                crate::malloc::live_allocations::count(),
                before,
                "allocation {k} leaked"
            );
            if r >= 0 {
                // Every allocation the search made has had its turn.
                completed = Some(k);
                break;
            }
        }
        assert!(completed.is_some_and(|k| k > 3), "{completed:?}");
        // SAFETY: compiled above.
        unsafe { regfree(&raw mut *re) };
    }

    /// `re_comp` and `re_exec` from many threads at once take turns at the
    /// one pattern instead of racing on it.
    #[test]
    fn re_comp_and_re_exec_from_many_threads_take_turns() {
        let _g = gnu_lock();
        re_set_syntax(RE_SYNTAX_POSIX_BASIC);
        let threads: Vec<_> = (0..4)
            .map(|t| {
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        let p = if t % 2 == 0 { c"a\\(b\\)*" } else { c"x" };
                        // SAFETY: C strings.
                        unsafe {
                            assert!(re_comp(p.as_ptr().cast()).is_null());
                            // Whichever pattern is there when it runs: what
                            // is under test is that the two never race.
                            let r = re_exec(c"zab".as_ptr().cast());
                            assert!(r == 0 || r == 1);
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
    }

    /// One pattern buffer searched by many threads at once -- its fastmap not
    /// yet made, each with registers of its own -- while others match it
    /// with `regexec`: every answer the one a thread alone gets, the searches
    /// taking turns at what they change in the buffer.
    #[test]
    fn one_pattern_buffer_searched_by_many_threads_at_once() {
        let _g = gnu_lock();
        let (mut re, err) = gnu_compile(RE_SYNTAX_POSIX_BASIC, b"\\(a*\\)b", "-");
        assert_eq!(err, None);
        re.fastmap = crate::malloc::malloc(256);
        let shared = (&raw mut *re) as usize;
        let threads: Vec<_> = (0..6)
            .map(|t| {
                std::thread::spawn(move || {
                    let p = shared as *mut RegexT;
                    for _ in 0..300 {
                        if t % 2 == 0 {
                            let mut regs = ReRegisters {
                                num_regs: 0,
                                start: core::ptr::null_mut(),
                                end: core::ptr::null_mut(),
                            };
                            // SAFETY: four bytes; the buffer outlives the
                            // threads; registers of this thread's own.
                            let r =
                                unsafe { re_search(p, b"xaab".as_ptr(), 4, 0, 4, &raw mut regs) };
                            assert_eq!(r, 1);
                            // SAFETY: what re_search allocated, then freed.
                            unsafe {
                                assert!(regs.num_regs >= 2);
                                assert_eq!((*regs.start.add(1), *regs.end.add(1)), (1, 3));
                                crate::malloc::free(regs.start.cast());
                                crate::malloc::free(regs.end.cast());
                            }
                        } else {
                            let mut m = [RegMatch {
                                rm_so: -9,
                                rm_eo: -9,
                            }; 2];
                            // SAFETY: a C string; two slots.
                            let rc = unsafe {
                                regexec(p, c"xaab".as_ptr().cast(), 2, m.as_mut_ptr(), 0)
                            };
                            assert_eq!(rc, 0);
                            assert_eq!(m[1], RegMatch { rm_so: 1, rm_eo: 3 });
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_ne!(re.bits() & FASTMAP_ACCURATE, 0, "a search made the fastmap");
        // SAFETY: compiled above; no thread uses it now.
        unsafe { regfree(&raw mut *re) };
    }
}
