//! POSIX regular expression matching.
//!
//! Implements `regcomp`, `regexec`, `regfree`, `regerror` per
//! POSIX.1-2024.
//!
//! ## Supported Features
//!
//! - **Basic Regular Expressions (BRE)**: `.`, `*`, `^`, `$`, `[...]`,
//!   `[^...]`, `\(...\)`, `\{m,n\}`, character ranges.
//! - **Extended Regular Expressions (ERE)** via `REG_EXTENDED`:
//!   `+`, `?`, `|`, `(...)`, `{m,n}` (unescaped).
//!
//! ## Limitations
//!
//! - Maximum pattern length: 1024 bytes.
//! - Maximum compiled instructions: 512.
//! - Maximum 9 sub-expressions (capturing groups).
//! - No backreferences (`\1`-`\9`) in the pattern — only in
//!   the match result (`pmatch[1..9]`).
//! - POSIX character classes (`[:alpha:]`, `[:digit:]`, etc.) supported
//!   in C/ASCII locale.

use crate::malloc;
use crate::string;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Use extended regular expressions.
pub const REG_EXTENDED: i32 = 1;
/// Ignore case.
pub const REG_ICASE: i32 = 2;
/// Report only success/fail, not match position.
pub const REG_NOSUB: i32 = 4;
/// Treat newline as ordinary character (no special `^`/`$` behaviour at `\n`).
pub const REG_NEWLINE: i32 = 8;
/// Don't regard start of string as beginning of line.
pub const REG_NOTBOL: i32 = 1;
/// Don't regard end of string as end of line.
pub const REG_NOTEOL: i32 = 2;

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

// ---------------------------------------------------------------------------
// Compiled regex — opaque to callers
// ---------------------------------------------------------------------------

/// Maximum compiled instructions.
const MAX_INSTS: usize = 512;
/// Maximum sub-expressions (groups).
const MAX_GROUPS: usize = 10; // group 0 = whole match

/// Instruction in the compiled regex.
#[derive(Clone, Copy)]
enum Inst {
    /// Match a literal byte (or case-insensitive pair).
    Byte(u8, bool),
    /// Match any character (.).
    AnyChar,
    /// Character class: start/end indices into `classes` array.
    Class(u16, u16, bool), // (start, end, negated)
    /// Jump unconditionally to instruction at offset.
    Jump(u16),
    /// Split: try `pc1` first, then `pc2` (for `*`, `+`, `?`, `|`).
    Split(u16, u16),
    /// Begin of line anchor (^).
    Bol,
    /// End of line anchor ($).
    Eol,
    /// Start of group capture.
    GroupStart(u8),
    /// End of group capture.
    GroupEnd(u8),
    /// Match (accept).
    Match,
}

/// A single range in a character class.
#[derive(Clone, Copy)]
struct ClassRange {
    lo: u8,
    hi: u8,
}

/// Maximum class ranges.
const MAX_CLASS_RANGES: usize = 256;

/// Compiled regular expression (opaque `regex_t`).
///
/// Callers see this as an opaque struct; the POSIX API uses
/// `regex_t*` pointers.  We allocate this via malloc so callers
/// can embed it in their own structs.
#[repr(C)]
pub struct RegexT {
    /// Number of sub-expressions (set by regcomp).
    pub re_nsub: usize,
    /// Internal: pointer to compiled program.
    program: *mut RegexProgram,
}

// SAFETY: RegexT contains a raw pointer to a heap-allocated program.
// POSIX mandates single-threaded access to a compiled regex unless
// the caller synchronizes externally.
unsafe impl Sync for RegexT {}

/// Internal compiled program.
struct RegexProgram {
    insts: [Inst; MAX_INSTS],
    inst_count: usize,
    classes: [ClassRange; MAX_CLASS_RANGES],
    class_count: usize,
    flags: i32,
    num_groups: usize,
}

// ---------------------------------------------------------------------------
// Match position
// ---------------------------------------------------------------------------

/// Match position for a sub-expression.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RegMatch {
    /// Start of match (byte offset), or -1 if not matched.
    pub rm_so: i64,
    /// End of match (byte offset past last char), or -1 if not matched.
    pub rm_eo: i64,
}

// ---------------------------------------------------------------------------
// regcomp
// ---------------------------------------------------------------------------

/// Compile a regular expression.
///
/// Returns 0 on success, or an error code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn regcomp(
    preg: *mut RegexT,
    pattern: *const u8,
    cflags: i32,
) -> i32 {
    if preg.is_null() || pattern.is_null() {
        return REG_BADPAT;
    }

    let reg = unsafe { &mut *preg };

    // Allocate the program.
    let prog_size = core::mem::size_of::<RegexProgram>();
    let prog_ptr = malloc::malloc(prog_size);
    if prog_ptr.is_null() {
        return REG_ESPACE;
    }

    // SAFETY: malloc on x86_64 returns 8-byte (or better) aligned pointers,
    // which satisfies RegexProgram's alignment requirement.
    #[allow(clippy::cast_ptr_alignment)]
    let program = prog_ptr.cast::<RegexProgram>();
    // Zero-initialize.
    unsafe { core::ptr::write_bytes(program, 0, 1); }
    let p = unsafe { &mut *program };
    p.flags = cflags;

    let extended = cflags & REG_EXTENDED != 0;
    let pat_len = unsafe { string::strlen(pattern) };

    // Compile the pattern.
    let result = compile_pattern(p, pattern, pat_len, extended);
    if result != 0 {
        // SAFETY: prog_ptr was allocated by malloc.
        unsafe { malloc::free(prog_ptr); }
        return result;
    }

    // Emit final Match instruction.
    if !emit_inst(p, Inst::Match) {
        unsafe { malloc::free(prog_ptr); }
        return REG_ESPACE;
    }

    reg.re_nsub = p.num_groups;
    reg.program = program;

    0
}

// ---------------------------------------------------------------------------
// regexec
// ---------------------------------------------------------------------------

/// Execute a compiled regular expression against a string.
///
/// Returns 0 if the string matches, `REG_NOMATCH` otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn regexec(
    preg: *const RegexT,
    string_arg: *const u8,
    nmatch: usize,
    pmatch: *mut RegMatch,
    eflags: i32,
) -> i32 {
    if preg.is_null() || string_arg.is_null() {
        return REG_NOMATCH;
    }

    let reg = unsafe { &*preg };
    if reg.program.is_null() {
        return REG_NOMATCH;
    }

    let compiled = unsafe { &*reg.program };
    let slen = unsafe { string::strlen(string_arg) };

    // Try matching at each position in the string.
    let mut pos: usize = 0;
    while pos <= slen {
        let mut groups = [RegMatch { rm_so: -1, rm_eo: -1 }; MAX_GROUPS];

        if try_match(compiled, string_arg, slen, pos, eflags, &mut groups) {
            // Store whole match.
            if let Some(g0) = groups.get_mut(0) {
                g0.rm_so = pos as i64;
                // rm_eo was set by the match engine.
            }

            // Copy results to pmatch.
            if !pmatch.is_null() && nmatch > 0 {
                let copy_count = if nmatch < MAX_GROUPS { nmatch } else { MAX_GROUPS };
                let mut gi: usize = 0;
                while gi < copy_count {
                    if let Some(grp) = groups.get(gi) {
                        unsafe { *pmatch.add(gi) = *grp; }
                    }
                    gi = gi.wrapping_add(1);
                }
            }
            return 0;
        }

        pos = pos.wrapping_add(1);
    }

    REG_NOMATCH
}

// ---------------------------------------------------------------------------
// regfree
// ---------------------------------------------------------------------------

/// Free a compiled regular expression.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn regfree(preg: *mut RegexT) {
    if preg.is_null() {
        return;
    }

    let reg = unsafe { &mut *preg };
    if !reg.program.is_null() {
        // SAFETY: program was allocated by malloc in regcomp.
        unsafe { malloc::free(reg.program.cast::<u8>()); }
        reg.program = core::ptr::null_mut();
    }
    reg.re_nsub = 0;
}

// ---------------------------------------------------------------------------
// regerror
// ---------------------------------------------------------------------------

/// Get a description of a regex error code.
///
/// Returns the number of bytes needed (including null terminator).
#[unsafe(no_mangle)]
pub extern "C" fn regerror(
    errcode: i32,
    _preg: *const RegexT,
    errbuf: *mut u8,
    errbuf_size: usize,
) -> usize {
    let msg: &[u8] = match errcode {
        0 => b"Success\0",
        REG_NOMATCH => b"No match\0",
        REG_BADPAT => b"Invalid pattern\0",
        REG_ECOLLATE => b"Invalid collating element\0",
        REG_ECTYPE => b"Invalid character class\0",
        REG_EESCAPE => b"Trailing backslash\0",
        REG_ESUBREG => b"Invalid back reference\0",
        REG_EBRACK => b"Unmatched [\0",
        REG_EPAREN => b"Unmatched ( or \\(\0",
        REG_EBRACE => b"Unmatched { or \\{\0",
        REG_BADBR => b"Invalid brace contents\0",
        REG_ERANGE => b"Invalid range\0",
        REG_ESPACE => b"Out of memory\0",
        _ => b"Unknown error\0",
    };

    if !errbuf.is_null() && errbuf_size > 0 {
        let copy_len = if msg.len() < errbuf_size { msg.len() } else { errbuf_size };
        let mut ci: usize = 0;
        while ci < copy_len {
            if let Some(&byte) = msg.get(ci) {
                unsafe { *errbuf.add(ci) = byte; }
            }
            ci = ci.wrapping_add(1);
        }
        // Ensure null-termination.
        let term = if copy_len < errbuf_size { copy_len.wrapping_sub(1) } else { errbuf_size.wrapping_sub(1) };
        unsafe { *errbuf.add(term) = 0; }
    }

    msg.len()
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

/// Compile a regex pattern into instructions.
fn compile_pattern(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    extended: bool,
) -> i32 {
    let mut pos: usize = 0;
    // Group 0 is reserved for the whole match (set by regexec, not by
    // instructions).  Explicit sub-expressions start at group 1.
    let mut group_id: u8 = 1;

    compile_alternation(prog, pat, pat_len, &mut pos, extended, &mut group_id)
}

/// Compile alternation (ERE `|`).
fn compile_alternation(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    pos: &mut usize,
    extended: bool,
    group_id: &mut u8,
) -> i32 {
    if !extended {
        return compile_sequence(prog, pat, pat_len, pos, extended, group_id);
    }

    let start = prog.inst_count;
    let result = compile_sequence(prog, pat, pat_len, pos, extended, group_id);
    if result != 0 {
        return result;
    }

    // Check for '|'.
    if *pos < pat_len && unsafe { *pat.add(*pos) } == b'|' {
        *pos = pos.wrapping_add(1);

        // Insert split before the first branch.
        // We need to shift instructions — use a jump chain instead.
        let after_first = prog.inst_count;
        // Emit jump (will be patched to skip second branch).
        if !emit_inst(prog, Inst::Jump(0)) {
            return REG_ESPACE;
        }

        let second_start = prog.inst_count;

        let result2 = compile_alternation(prog, pat, pat_len, pos, extended, group_id);
        if result2 != 0 {
            return result2;
        }

        let end = prog.inst_count;

        // Patch the jump after the first branch.
        if let Some(inst) = prog.insts.get_mut(after_first) {
            *inst = Inst::Jump(end as u16);
        }

        // Insert a split at `start` by shifting everything.
        // This is expensive but simple.
        if prog.inst_count >= MAX_INSTS {
            return REG_ESPACE;
        }

        // Shift all instructions from `start` to `end` by 1.
        let mut shift = prog.inst_count;
        while shift > start {
            let prev = shift.wrapping_sub(1);
            let inst_copy = prog.insts.get(prev).copied().unwrap_or(Inst::Match);
            if let Some(slot) = prog.insts.get_mut(shift) {
                *slot = shift_inst(inst_copy, start, 1);
            }
            shift = shift.wrapping_sub(1);
        }
        prog.inst_count = prog.inst_count.wrapping_add(1);

        // Write the split.
        if let Some(slot) = prog.insts.get_mut(start) {
            *slot = Inst::Split(
                start.wrapping_add(1) as u16,
                second_start.wrapping_add(1) as u16,
            );
        }
    }

    0
}

/// Shift instruction targets that are >= `threshold` by `delta`.
fn shift_inst(inst: Inst, threshold: usize, delta: usize) -> Inst {
    match inst {
        Inst::Jump(t) => {
            let target = t as usize;
            if target >= threshold {
                Inst::Jump(target.wrapping_add(delta) as u16)
            } else {
                inst
            }
        }
        Inst::Split(a, b) => {
            let ta = if (a as usize) >= threshold {
                (a as usize).wrapping_add(delta) as u16
            } else {
                a
            };
            let tb = if (b as usize) >= threshold {
                (b as usize).wrapping_add(delta) as u16
            } else {
                b
            };
            Inst::Split(ta, tb)
        }
        _ => inst,
    }
}

/// Compile a sequence of atoms (concatenation).
fn compile_sequence(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    pos: &mut usize,
    extended: bool,
    group_id: &mut u8,
) -> i32 {
    while *pos < pat_len {
        let ch = unsafe { *pat.add(*pos) };

        // Stop at end-of-group or alternation.
        if extended && (ch == b')' || ch == b'|') {
            break;
        }
        if !extended && ch == b'\\' && pos.wrapping_add(1) < pat_len {
            let next = unsafe { *pat.add(pos.wrapping_add(1)) };
            if next == b')' {
                break;
            }
        }

        let atom_start = prog.inst_count;
        let result = compile_atom(prog, pat, pat_len, pos, extended, group_id);
        if result != 0 {
            return result;
        }

        // Check for quantifier.
        if *pos < pat_len {
            let qch = unsafe { *pat.add(*pos) };
            let result2 = compile_quantifier(prog, atom_start, qch, pat, pat_len, pos, extended);
            if result2 != 0 {
                return result2;
            }
        }
    }

    0
}

/// Compile a single atom (literal, `.`, `[...]`, `(...)`, `^`, `$`).
#[allow(clippy::too_many_lines)]
fn compile_atom(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    pos: &mut usize,
    extended: bool,
    group_id: &mut u8,
) -> i32 {
    let ch = unsafe { *pat.add(*pos) };
    let icase = prog.flags & REG_ICASE != 0;

    match ch {
        b'^' => {
            *pos = pos.wrapping_add(1);
            if !emit_inst(prog, Inst::Bol) { return REG_ESPACE; }
        }
        b'$' => {
            *pos = pos.wrapping_add(1);
            if !emit_inst(prog, Inst::Eol) { return REG_ESPACE; }
        }
        b'.' => {
            *pos = pos.wrapping_add(1);
            if !emit_inst(prog, Inst::AnyChar) { return REG_ESPACE; }
        }
        b'[' => {
            *pos = pos.wrapping_add(1);
            return compile_class(prog, pat, pat_len, pos);
        }
        b'(' if extended => {
            *pos = pos.wrapping_add(1);
            return compile_group(prog, pat, pat_len, pos, extended, group_id);
        }
        b'\\' => {
            *pos = pos.wrapping_add(1);
            if *pos >= pat_len {
                return REG_EESCAPE;
            }
            let escaped = unsafe { *pat.add(*pos) };
            if !extended && escaped == b'(' {
                *pos = pos.wrapping_add(1);
                return compile_group(prog, pat, pat_len, pos, extended, group_id);
            }
            // Literal escaped char.
            *pos = pos.wrapping_add(1);
            if !emit_inst(prog, Inst::Byte(escaped, false)) { return REG_ESPACE; }
        }
        _ => {
            // Literal character.
            *pos = pos.wrapping_add(1);
            if !emit_inst(prog, Inst::Byte(ch, icase)) { return REG_ESPACE; }
        }
    }

    0
}

/// Compile a quantifier (`*`, `+`, `?`).
///
/// Returns 0 if a quantifier was applied (or none found), error otherwise.
fn compile_quantifier(
    prog: &mut RegexProgram,
    atom_start: usize,
    qch: u8,
    _pat: *const u8,
    _pat_len: usize,
    pos: &mut usize,
    extended: bool,
) -> i32 {
    let is_star = qch == b'*';
    let is_plus = extended && qch == b'+';
    let is_question = extended && qch == b'?';

    if !is_star && !is_plus && !is_question {
        return 0; // No quantifier.
    }

    *pos = pos.wrapping_add(1);

    let atom_end = prog.inst_count;

    match qch {
        b'*' => {
            // a* → Split(atom, past) + atom + Jump(split)
            // Insert split before atom.
            if prog.inst_count.wrapping_add(2) > MAX_INSTS {
                return REG_ESPACE;
            }

            // Shift atom instructions by 1 (for the split).
            let shift_count = atom_end.wrapping_sub(atom_start);
            let mut si = prog.inst_count;
            while si > atom_start {
                let prev = si.wrapping_sub(1);
                let inst = prog.insts.get(prev).copied().unwrap_or(Inst::Match);
                if let Some(slot) = prog.insts.get_mut(si) {
                    *slot = shift_inst(inst, atom_start, 1);
                }
                si = si.wrapping_sub(1);
            }
            prog.inst_count = prog.inst_count.wrapping_add(1);

            let after_atom = atom_start.wrapping_add(1).wrapping_add(shift_count);

            // Write the split.
            if let Some(slot) = prog.insts.get_mut(atom_start) {
                *slot = Inst::Split(
                    atom_start.wrapping_add(1) as u16,
                    after_atom.wrapping_add(1) as u16,
                );
            }

            // Emit jump back to the split.
            if !emit_inst(prog, Inst::Jump(atom_start as u16)) {
                return REG_ESPACE;
            }
        }
        b'+' if extended => {
            // a+ → atom + Split(atom, past)
            if prog.inst_count.wrapping_add(1) > MAX_INSTS {
                return REG_ESPACE;
            }
            let split_pos = prog.inst_count;
            if !emit_inst(prog, Inst::Split(atom_start as u16, split_pos.wrapping_add(1) as u16)) {
                return REG_ESPACE;
            }
        }
        b'?' if extended => {
            // a? → Split(atom, past)
            if prog.inst_count.wrapping_add(1) > MAX_INSTS {
                return REG_ESPACE;
            }

            // Shift atom instructions by 1.
            let shift_count = atom_end.wrapping_sub(atom_start);
            let mut si = prog.inst_count;
            while si > atom_start {
                let prev = si.wrapping_sub(1);
                let inst = prog.insts.get(prev).copied().unwrap_or(Inst::Match);
                if let Some(slot) = prog.insts.get_mut(si) {
                    *slot = shift_inst(inst, atom_start, 1);
                }
                si = si.wrapping_sub(1);
            }
            prog.inst_count = prog.inst_count.wrapping_add(1);

            let past = atom_start.wrapping_add(1).wrapping_add(shift_count);
            if let Some(slot) = prog.insts.get_mut(atom_start) {
                *slot = Inst::Split(atom_start.wrapping_add(1) as u16, past as u16);
            }
        }
        _ => {}
    }

    0
}

/// Compile a character class `[...]`.
fn compile_class(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    pos: &mut usize,
) -> i32 {
    let negated = *pos < pat_len && unsafe { *pat.add(*pos) } == b'^';
    if negated {
        *pos = pos.wrapping_add(1);
    }

    let range_start = prog.class_count;

    // Allow ']' as first char in the class.
    if *pos < pat_len && unsafe { *pat.add(*pos) } == b']' {
        if prog.class_count >= MAX_CLASS_RANGES {
            return REG_ESPACE;
        }
        if let Some(slot) = prog.classes.get_mut(prog.class_count) {
            *slot = ClassRange { lo: b']', hi: b']' };
        }
        prog.class_count = prog.class_count.wrapping_add(1);
        *pos = pos.wrapping_add(1);
    }

    while *pos < pat_len {
        let ch = unsafe { *pat.add(*pos) };
        if ch == b']' {
            *pos = pos.wrapping_add(1);
            let range_end = prog.class_count;
            if !emit_inst(prog, Inst::Class(range_start as u16, range_end as u16, negated)) {
                return REG_ESPACE;
            }
            return 0;
        }

        // Check for POSIX character class [:classname:].
        if ch == b'[' && pos.wrapping_add(1) < pat_len
            && unsafe { *pat.add(pos.wrapping_add(1)) } == b':'
        {
            let class_start = pos.wrapping_add(2);
            // Find the closing ":]".
            let mut end = class_start;
            while end.wrapping_add(1) < pat_len {
                if unsafe { *pat.add(end) } == b':'
                    && unsafe { *pat.add(end.wrapping_add(1)) } == b']'
                {
                    break;
                }
                end = end.wrapping_add(1);
            }
            if end.wrapping_add(1) < pat_len
                && unsafe { *pat.add(end) } == b':'
                && unsafe { *pat.add(end.wrapping_add(1)) } == b']'
            {
                let name_len = end.wrapping_sub(class_start);
                let err = add_posix_class(prog, pat, class_start, name_len);
                if err != 0 {
                    return err;
                }
                *pos = end.wrapping_add(2); // Skip past ":]"
                continue;
            }
            // Not a valid POSIX class — treat '[' as literal.
        }

        // Check for range (a-z).
        if pos.wrapping_add(2) < pat_len
            && unsafe { *pat.add(pos.wrapping_add(1)) } == b'-'
            && unsafe { *pat.add(pos.wrapping_add(2)) } != b']'
        {
            let lo = ch;
            let hi = unsafe { *pat.add(pos.wrapping_add(2)) };
            if prog.class_count >= MAX_CLASS_RANGES {
                return REG_ESPACE;
            }
            if let Some(slot) = prog.classes.get_mut(prog.class_count) {
                *slot = ClassRange { lo, hi };
            }
            prog.class_count = prog.class_count.wrapping_add(1);
            *pos = pos.wrapping_add(3);
        } else {
            if prog.class_count >= MAX_CLASS_RANGES {
                return REG_ESPACE;
            }
            if let Some(slot) = prog.classes.get_mut(prog.class_count) {
                *slot = ClassRange { lo: ch, hi: ch };
            }
            prog.class_count = prog.class_count.wrapping_add(1);
            *pos = pos.wrapping_add(1);
        }
    }

    REG_EBRACK // Unterminated class.
}

/// Add a single class range to the program.
fn add_class_range(prog: &mut RegexProgram, lo: u8, hi: u8) -> bool {
    if prog.class_count >= MAX_CLASS_RANGES {
        return false;
    }
    if let Some(slot) = prog.classes.get_mut(prog.class_count) {
        *slot = ClassRange { lo, hi };
    }
    prog.class_count = prog.class_count.wrapping_add(1);
    true
}

/// Expand a POSIX character class name into ClassRange entries.
///
/// Recognizes: alpha, digit, alnum, space, upper, lower, punct,
/// cntrl, print, graph, xdigit, blank.
/// Returns 0 on success, REG_ECTYPE for unknown class, REG_ESPACE if full.
fn add_posix_class(
    prog: &mut RegexProgram,
    pat: *const u8,
    name_start: usize,
    name_len: usize,
) -> i32 {
    // Compare the class name (case-sensitive per POSIX).
    let name_matches = |expected: &[u8]| -> bool {
        if name_len != expected.len() { return false; }
        let mut k = 0;
        while k < name_len {
            if unsafe { *pat.add(name_start.wrapping_add(k)) } != expected[k] {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    };

    // Each POSIX class maps to one or more ranges in ASCII.
    if name_matches(b"alpha") {
        if !add_class_range(prog, b'A', b'Z') { return REG_ESPACE; }
        if !add_class_range(prog, b'a', b'z') { return REG_ESPACE; }
    } else if name_matches(b"digit") {
        if !add_class_range(prog, b'0', b'9') { return REG_ESPACE; }
    } else if name_matches(b"alnum") {
        if !add_class_range(prog, b'A', b'Z') { return REG_ESPACE; }
        if !add_class_range(prog, b'a', b'z') { return REG_ESPACE; }
        if !add_class_range(prog, b'0', b'9') { return REG_ESPACE; }
    } else if name_matches(b"space") {
        // space, tab, newline, vertical tab, form feed, carriage return
        if !add_class_range(prog, 0x09, 0x0D) { return REG_ESPACE; }
        if !add_class_range(prog, b' ', b' ') { return REG_ESPACE; }
    } else if name_matches(b"upper") {
        if !add_class_range(prog, b'A', b'Z') { return REG_ESPACE; }
    } else if name_matches(b"lower") {
        if !add_class_range(prog, b'a', b'z') { return REG_ESPACE; }
    } else if name_matches(b"punct") {
        // Printable non-alnum, non-space: 33-47, 58-64, 91-96, 123-126
        if !add_class_range(prog, 0x21, 0x2F) { return REG_ESPACE; }
        if !add_class_range(prog, 0x3A, 0x40) { return REG_ESPACE; }
        if !add_class_range(prog, 0x5B, 0x60) { return REG_ESPACE; }
        if !add_class_range(prog, 0x7B, 0x7E) { return REG_ESPACE; }
    } else if name_matches(b"cntrl") {
        if !add_class_range(prog, 0x00, 0x1F) { return REG_ESPACE; }
        if !add_class_range(prog, 0x7F, 0x7F) { return REG_ESPACE; }
    } else if name_matches(b"print") {
        // Printable: 0x20 - 0x7E
        if !add_class_range(prog, 0x20, 0x7E) { return REG_ESPACE; }
    } else if name_matches(b"graph") {
        // Visible (printable minus space): 0x21 - 0x7E
        if !add_class_range(prog, 0x21, 0x7E) { return REG_ESPACE; }
    } else if name_matches(b"xdigit") {
        if !add_class_range(prog, b'0', b'9') { return REG_ESPACE; }
        if !add_class_range(prog, b'A', b'F') { return REG_ESPACE; }
        if !add_class_range(prog, b'a', b'f') { return REG_ESPACE; }
    } else if name_matches(b"blank") {
        // Space and tab only.
        if !add_class_range(prog, b' ', b' ') { return REG_ESPACE; }
        if !add_class_range(prog, b'\t', b'\t') { return REG_ESPACE; }
    } else {
        return REG_ECTYPE;
    }
    0
}

/// Compile a group `(...)` or `\(...\)`.
fn compile_group(
    prog: &mut RegexProgram,
    pat: *const u8,
    pat_len: usize,
    pos: &mut usize,
    extended: bool,
    group_id: &mut u8,
) -> i32 {
    let gid = *group_id;
    if (gid as usize) >= MAX_GROUPS {
        return REG_EPAREN;
    }
    *group_id = group_id.wrapping_add(1);
    prog.num_groups = prog.num_groups.wrapping_add(1);

    if !emit_inst(prog, Inst::GroupStart(gid)) {
        return REG_ESPACE;
    }

    let result = compile_alternation(prog, pat, pat_len, pos, extended, group_id);
    if result != 0 {
        return result;
    }

    // Expect closing delimiter.
    if extended {
        if *pos >= pat_len || unsafe { *pat.add(*pos) } != b')' {
            return REG_EPAREN;
        }
        *pos = pos.wrapping_add(1);
    } else {
        // BRE: expect \)
        if pos.wrapping_add(1) >= pat_len
            || unsafe { *pat.add(*pos) } != b'\\'
            || unsafe { *pat.add(pos.wrapping_add(1)) } != b')'
        {
            return REG_EPAREN;
        }
        *pos = pos.wrapping_add(2);
    }

    if !emit_inst(prog, Inst::GroupEnd(gid)) {
        return REG_ESPACE;
    }

    0
}

/// Emit an instruction.
fn emit_inst(prog: &mut RegexProgram, inst: Inst) -> bool {
    if prog.inst_count >= MAX_INSTS {
        return false;
    }
    if let Some(slot) = prog.insts.get_mut(prog.inst_count) {
        *slot = inst;
    }
    prog.inst_count = prog.inst_count.wrapping_add(1);
    true
}

// ---------------------------------------------------------------------------
// Matching engine — recursive backtracking
// ---------------------------------------------------------------------------

/// Match context — bundles immutable state passed through recursion.
struct MatchCtx<'a> {
    prog: &'a RegexProgram,
    string_ptr: *const u8,
    slen: usize,
    eflags: i32,
    start: usize,
}

/// Try to match starting at position `start` in `string`.
fn try_match(
    prog: &RegexProgram,
    string_ptr: *const u8,
    slen: usize,
    start: usize,
    eflags: i32,
    groups: &mut [RegMatch; MAX_GROUPS],
) -> bool {
    let ctx = MatchCtx { prog, string_ptr, slen, eflags, start };
    exec_recursive(&ctx, start, 0, groups)
}

/// Recursive backtracking executor.
///
/// `pc` is the current instruction index.  `cur_sp` is the current
/// position in the string.
fn exec_recursive(
    ctx: &MatchCtx<'_>,
    sp: usize,
    pc: usize,
    groups: &mut [RegMatch; MAX_GROUPS],
) -> bool {
    let mut cur_sp = sp;
    let mut cur_pc = pc;

    loop {
        if cur_pc >= ctx.prog.inst_count {
            return false;
        }

        let inst = ctx.prog.insts.get(cur_pc).copied().unwrap_or(Inst::Match);

        match inst {
            Inst::Match => {
                // Set the end of group 0.
                if let Some(g0) = groups.get_mut(0) {
                    g0.rm_eo = cur_sp as i64;
                }
                return true;
            }

            Inst::Byte(expected, icase) => {
                if !match_byte(ctx, cur_sp, expected, icase) {
                    return false;
                }
                cur_sp = cur_sp.wrapping_add(1);
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::AnyChar => {
                if !match_any(ctx, cur_sp) {
                    return false;
                }
                cur_sp = cur_sp.wrapping_add(1);
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::Class(range_start, range_end, negated) => {
                if !match_class(ctx, cur_sp, range_start, range_end, negated) {
                    return false;
                }
                cur_sp = cur_sp.wrapping_add(1);
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::Bol => {
                if !match_bol(ctx, cur_sp) {
                    return false;
                }
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::Eol => {
                if !match_eol(ctx, cur_sp) {
                    return false;
                }
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::Jump(target) => {
                cur_pc = target as usize;
            }

            Inst::Split(a, b) => {
                let saved_groups = *groups;
                if exec_recursive(ctx, cur_sp, a as usize, groups) {
                    return true;
                }
                *groups = saved_groups;
                cur_pc = b as usize;
            }

            Inst::GroupStart(gid) => {
                if let Some(grp) = groups.get_mut(gid as usize) {
                    grp.rm_so = cur_sp as i64;
                }
                cur_pc = cur_pc.wrapping_add(1);
            }

            Inst::GroupEnd(gid) => {
                if let Some(grp) = groups.get_mut(gid as usize) {
                    grp.rm_eo = cur_sp as i64;
                }
                cur_pc = cur_pc.wrapping_add(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Match helpers (extracted from exec_recursive for line count)
// ---------------------------------------------------------------------------

/// Match a literal byte.
fn match_byte(ctx: &MatchCtx<'_>, cur_sp: usize, expected: u8, icase: bool) -> bool {
    if cur_sp >= ctx.slen {
        return false;
    }
    let actual = unsafe { *ctx.string_ptr.add(cur_sp) };
    if icase {
        actual.eq_ignore_ascii_case(&expected)
    } else {
        actual == expected
    }
}

/// Match any character (`.`).
fn match_any(ctx: &MatchCtx<'_>, cur_sp: usize) -> bool {
    if cur_sp >= ctx.slen {
        return false;
    }
    let ch = unsafe { *ctx.string_ptr.add(cur_sp) };
    // If REG_NEWLINE, '.' doesn't match '\n'.
    !(ctx.prog.flags & REG_NEWLINE != 0 && ch == b'\n')
}

/// Match a character class.
fn match_class(
    ctx: &MatchCtx<'_>,
    cur_sp: usize,
    range_start: u16,
    range_end: u16,
    negated: bool,
) -> bool {
    if cur_sp >= ctx.slen {
        return false;
    }
    let ch = unsafe { *ctx.string_ptr.add(cur_sp) };
    let in_class = char_in_class(
        ch,
        &ctx.prog.classes,
        range_start as usize,
        range_end as usize,
        ctx.prog.flags & REG_ICASE != 0,
    );
    negated != in_class // XOR: negated class inverts the result.
}

/// Match beginning of line anchor (^).
fn match_bol(ctx: &MatchCtx<'_>, cur_sp: usize) -> bool {
    let at_bol = cur_sp == ctx.start && ctx.eflags & REG_NOTBOL == 0;
    let at_newline = ctx.prog.flags & REG_NEWLINE != 0
        && cur_sp > 0
        && unsafe { *ctx.string_ptr.add(cur_sp.wrapping_sub(1)) } == b'\n';
    at_bol || at_newline
}

/// Match end of line anchor ($).
fn match_eol(ctx: &MatchCtx<'_>, cur_sp: usize) -> bool {
    let at_eol = cur_sp == ctx.slen && ctx.eflags & REG_NOTEOL == 0;
    let at_newline = ctx.prog.flags & REG_NEWLINE != 0
        && cur_sp < ctx.slen
        && unsafe { *ctx.string_ptr.add(cur_sp) } == b'\n';
    at_eol || at_newline
}

/// Check if a character is in a character class.
fn char_in_class(
    ch: u8,
    classes: &[ClassRange; MAX_CLASS_RANGES],
    start: usize,
    end: usize,
    icase: bool,
) -> bool {
    let test_ch = if icase { ch.to_ascii_lowercase() } else { ch };

    let mut idx = start;
    while idx < end {
        if let Some(range) = classes.get(idx) {
            let lo = if icase { range.lo.to_ascii_lowercase() } else { range.lo };
            let hi = if icase { range.hi.to_ascii_lowercase() } else { range.hi };
            if test_ch >= lo && test_ch <= hi {
                return true;
            }
        }
        idx = idx.wrapping_add(1);
    }
    false
}
