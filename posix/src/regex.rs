//! POSIX regular expressions: `regcomp`, `regexec`, `regfree` and
//! `regerror` (XSH; XBD chapter 9), read as glibc 2.39 reads patterns and
//! matched as the standard says -- which glibc, in its choice of submatches,
//! does not always do.
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
//! one reports.
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
//! `[a-Z]` accepted); back-references miss longer matches and report
//! half-set pairs; and five patterns, `(){32767}` among them, crash it.
//! design-decisions.md §1160 records the choice; `posix/src/regex_deviations.txt`
//! lists every case of the oracle (`posix/tools/oracle/regex_harness.py`,
//! `regex_oracle.txt`: some 544,000 answers -- every pair of 54 pieces
//! against every string of `a` and `b` to length 4, every pair of 75 tokens
//! as a BRE and an ERE, 1,200 patterns drawn at random against strings of
//! `a`, `b` and `c`, glibc's own tests, and the flags' edges) where the
//! answer here is the standard's, as `posix/tools/oracle/regex_model.py`
//! computes it, and not glibc's: 16,444 of them.
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
//! D-POSIX-GLIBC-2026-SECURITY-FIXES-AUDITED).

mod backref;
mod dissect;
mod parse;
mod prog;

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
/// glibc's: an unmatched `)` (`regcomp` reports it as REG_EPAREN).
pub const REG_ERPAREN: i32 = 16;

/// The largest count an interval may give, glibc's RE_DUP_MAX: what
/// `sysconf(_SC_RE_DUP_MAX)` reports.
pub(crate) const RE_DUP_MAX: u32 = parse::RE_DUP_MAX;

// ---------------------------------------------------------------------------
// regex_t and regmatch_t
// ---------------------------------------------------------------------------

/// Compiled regular expression (opaque `regex_t`).
///
/// Callers see this as an opaque struct; the POSIX API uses `regex_t*`
/// pointers, and a caller may embed one in its own structs, so the program
/// `regcomp` builds is allocated and only a pointer to it kept here.
#[repr(C)]
pub struct RegexT {
    /// Number of sub-expressions (set by regcomp).
    pub re_nsub: usize,
    /// Internal: the compiled program, or NULL.
    program: *mut prog::Program,
    /// The 48 bytes of musl's `regex_t` that our two fields do not use.
    ///
    /// Never read or written.  Present so that `size_of::<RegexT>()` equals
    /// what `<regex.h>` declares — see the assertion below.
    _reserved: [u8; 48],
}

impl RegexT {
    /// An uncompiled `regex_t`: no sub-expressions, no program, zeroed tail.
    ///
    /// Constructed through this rather than with a struct literal so the
    /// `_reserved` tail can track its header without touching a call site.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            re_nsub: 0,
            program: core::ptr::null_mut(),
            _reserved: [0; 48],
        }
    }
}

impl Default for RegexT {
    fn default() -> Self {
        Self::new()
    }
}

/// Our `regex_t` is exactly the one the caller's `<regex.h>` reserved.
///
/// See `pthread.rs`'s module note for why this is a `const` rather than a
/// `#[test]`.  `re_nsub` is public and sits at offset 0 in musl and here
/// alike -- it is the one field POSIX lets a caller read -- and the
/// `_reserved` tail brings the whole object to musl's 64 bytes, so `==`
/// holds and a by-value copy of a `regex_t` moves our bytes and only ours.
const _: () = {
    assert!(size_of::<RegexT>() == 64, "musl/glibc regex_t is 64 bytes");
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
/// `design-decisions.md` §1011 and §1010 for the family.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegMatch {
    /// Start of match (byte offset), or -1 if not matched.
    pub rm_so: isize,
    /// End of match (byte offset past last char), or -1 if not matched.
    pub rm_eo: isize,
}

// ---------------------------------------------------------------------------
// regcomp
// ---------------------------------------------------------------------------

/// The program for `pattern`, or `regcomp`'s error for it.
fn compile(pattern: &[u8], cflags: i32) -> Result<prog::Program, i32> {
    let tree = parse::parse(pattern, parse::Syntax::posix(cflags)).map_err(|c| c.0)?;
    let mut p = prog::compile(tree, cflags & REG_ICASE != 0, cflags & REG_NEWLINE != 0)
        .map_err(|_| REG_ESPACE)?;
    p.nosub = cflags & REG_NOSUB != 0;
    Ok(p)
}

/// Compile a regular expression.
///
/// Returns 0 on success, or the error code: REG_BADPAT and the rest as
/// glibc gives them, REG_ESPACE for a program past two million
/// instructions or a heap with no room for it.
///
/// # Safety
///
/// `preg` points to a writable `regex_t`; `pattern` is a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn regcomp(preg: *mut RegexT, pattern: *const u8, cflags: i32) -> i32 {
    if preg.is_null() || pattern.is_null() {
        return REG_BADPAT;
    }
    // SAFETY: `preg` is the caller's writable `regex_t`. As glibc does, it
    // holds no program until one is built, so that a `regfree` after a
    // failed `regcomp` frees nothing.
    unsafe {
        (*preg).program = core::ptr::null_mut();
        (*preg).re_nsub = 0;
    }
    // SAFETY: `pattern` is a C string, per the contract.
    let pat = unsafe { core::slice::from_raw_parts(pattern, string::strlen(pattern)) };
    let program = match compile(pat, cflags) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let nsub = program.tree.nsub as usize;
    let mem = malloc::malloc(size_of::<prog::Program>()).cast::<prog::Program>();
    if mem.is_null() {
        return REG_ESPACE;
    }
    // SAFETY: `mem` is a fresh block of the program's size from `malloc`,
    // aligned for any object; the program moves into it and is owned by the
    // `regex_t` from here, until `regfree` drops it.
    unsafe {
        mem.write(program);
        (*preg).program = mem;
        (*preg).re_nsub = nsub;
    }
    0
}

// ---------------------------------------------------------------------------
// regexec
// ---------------------------------------------------------------------------

/// The match of program `p` in `sub` at or after `from`, groups
/// `0..=nsub`, or `None`; `want` of them asked for.
fn execute(
    p: &prog::Program,
    sub: &prog::Subject<'_>,
    from: usize,
    want: usize,
    pm: &mut crate::list::List<(isize, isize)>,
) -> Result<bool, crate::list::NoMem> {
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
        while let Some((s, _)) = vm.search(p, sub, pos, false)? {
            if m.at(s, pm)? {
                return Ok(true);
            }
            if s >= sub.end() {
                break;
            }
            pos = s.wrapping_add(1);
        }
        return Ok(false);
    }
    let Some((s, e)) = vm.search(p, sub, from, want == 0)? else {
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

/// Execute a compiled regular expression against a string.
///
/// Returns 0 if the string matches, `REG_NOMATCH` otherwise -- also, as
/// glibc answers them, when the heap runs out; REG_BADPAT for `eflags` it
/// does not know. On a match, the first `nmatch` entries of `pmatch` get
/// the match and its groups, those past `re_nsub` -1; on none, `pmatch` is
/// left alone. A `regex_t` compiled with REG_NOSUB writes nothing there.
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
    let program = unsafe { (*preg).program };
    if program.is_null() {
        return REG_NOMATCH;
    }
    // SAFETY: a program `regcomp` installed, alive until `regfree`, and not
    // changed by any `regexec`.
    let p = unsafe { &*program };
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
    // SAFETY: `end` bytes at `string_arg`, per the contract.
    let s = unsafe { core::slice::from_raw_parts(string_arg, end) };
    let sub = prog::Subject {
        s,
        notbol: eflags & REG_NOTBOL != 0,
        noteol: eflags & REG_NOTEOL != 0,
        newline: p.newline,
    };
    let want = if p.nosub || pmatch.is_null() {
        0
    } else {
        nmatch
    };
    let mut pm = crate::list::List::new();
    match execute(p, &sub, from, want, &mut pm) {
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
            })
        };
    }
    0
}

// ---------------------------------------------------------------------------
// regfree
// ---------------------------------------------------------------------------

/// Free a compiled regular expression.
///
/// The program is dropped and the pointer cleared, so a second `regfree`
/// frees nothing; `re_nsub` is left as it was, as glibc leaves it.
///
/// # Safety
///
/// `preg` is NULL or a `regex_t` that `regcomp` has seen.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn regfree(preg: *mut RegexT) {
    if preg.is_null() {
        return;
    }
    // SAFETY: `preg` is a `regex_t`, per the contract.
    let program = unsafe { (*preg).program };
    if !program.is_null() {
        // SAFETY: the program `regcomp` moved into this block: dropped once
        // (its tables returned to the heap), then the block freed, and the
        // pointer cleared so that nothing frees it again.
        unsafe {
            program.drop_in_place();
            malloc::free(program.cast::<u8>());
            (*preg).program = core::ptr::null_mut();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::collections::HashMap;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    // -- ABI layout --

    /// `regex_t` is 64 bytes with `re_nsub` first, as musl declares it.
    /// `re_nsub` is the only field POSIX lets a caller read.
    ///
    /// The size is also a `const` assertion above; this test adds the offsets.
    #[test]
    fn test_regex_t_matches_musl_layout() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<RegexT>(), 64, "musl/glibc regex_t is 64 bytes");
        assert_eq!(align_of::<RegexT>(), 8);
        let r = RegexT::new();
        let base = (&raw const r).cast::<u8>() as usize;
        assert_eq!((&raw const r.re_nsub).cast::<u8>() as usize - base, 0);
        assert_eq!((&raw const r.program).cast::<u8>() as usize - base, 8);
        assert_eq!((&raw const r._reserved).cast::<u8>() as usize - base, 16);
        assert_eq!(r.re_nsub, 0);
        assert!(r.program.is_null());
        assert_eq!(r._reserved, [0u8; 48]);
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
        assert!(!re.program.is_null(), "regcomp installed a program");

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
        assert!(re.program.is_null(), "regfree clears the program pointer");
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
        assert!(re.program.is_null());
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
                re.program.is_null(),
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
        assert!(re.program.is_null(), "nothing installed on rejection");
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
}
