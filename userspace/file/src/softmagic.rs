//! libmagic's `softmagic.c`: running the rules.
//!
//! Each top-level rule is tried in strength order; when one matches, its
//! continuations are walked level by level, each one read (`mget`), converted
//! (`mconvert`), compared (`magiccheck`) and described (`mprint`), and the
//! offset after it noted (`moffset`) for the relative ones under it. That is
//! upstream's control flow statement for statement -- including where it
//! surprises: a `!` rule that cannot read its value matches, an `indirect`
//! count is never given back, a rule that reads past the end of the buffer
//! drops out silently -- because those are what make `file` say what it says.

use std::cell::OnceCell;
use std::io::Write;
use std::rc::Rc;

use crate::apprentice::{
    file_magicfind, file_pstring_get_length, file_pstring_length_size, file_signextend,
};
use crate::buffer::Buffer;
use crate::cstd::{cstr, cstrlen, isspace, strtoull};
use crate::fmtcheck::fmtcheck;
use crate::funcs::{
    Ms, RegFlags, file_print_guid, file_printable, file_regcomp, file_regexec, file_strtrim,
};
use crate::magic::*;
use crate::print::{FILE_T_LOCAL, FILE_T_WINDOWS, file_fmtdate, file_fmtdatetime, file_fmtnum, file_fmttime};
use crate::printf::Arg;

/// A regex compiled from a rule on first use (`magic_rxcomp`).
pub type Rx = OnceCell<Rc<ere::Regex>>;

/// A run of rules and their regex slots: a whole set, or one `name` entry.
#[derive(Clone, Copy)]
pub struct Rules<'r> {
    pub magic: &'r [Magic],
    pub rx: &'r [Rx],
}

/// What `match` shares through its recursion -- `indirect` and `use` call it
/// again, and these are C's pointers into the outermost call.
pub struct Walk {
    pub indir_count: u16,
    pub name_count: u16,
    pub printed_something: bool,
    pub need_separator: bool,
    pub firstline: bool,
    /// `BINTEST` or `TEXTTEST`: which top-level rules apply.
    pub mode: u8,
    /// Whether the buffer looks like text.
    pub text: bool,
}

/// What one `match` call reports back: `returnval` and `found_match`, which a
/// `use` shares with its caller only in part -- the caller's `returnval`, but
/// a `found_match` of its own.
pub struct Outcome<'o> {
    pub returnval: &'o mut i32,
    pub found_match: &'o mut bool,
}

/// Where a rule is read: the offset its run was asked to start at (`o`), its
/// continuation level, and whether `use ^name` flipped its byte order.
#[derive(Clone, Copy)]
struct Pos {
    o: usize,
    cont_level: usize,
    flip: bool,
}

/// `file_softmagic`: try every loaded database's rules, in load order.
pub fn file_softmagic(
    ms: &mut Ms,
    b: &Buffer<'_>,
    counts: Option<(&mut u16, &mut u16)>,
    mode: u8,
    text: bool,
) -> i32 {
    let (ic, nc) = match &counts {
        Some((i, n)) => (**i, **n),
        None => (0, 0),
    };
    let mut walk = Walk {
        indir_count: ic,
        name_count: nc,
        printed_something: false,
        need_separator: false,
        firstline: true,
        mode,
        text,
    };
    let mut rv = 0;
    let lists: Vec<Rc<crate::funcs::MList>> = ms.mlist[0].clone().unwrap_or_default();
    for ml in &lists {
        let mut returnval = 0;
        let mut found = false;
        let ret = match_rules(
            ms,
            Rules {
                magic: &ml.magic,
                rx: &ml.rx,
            },
            b,
            0,
            false,
            &mut walk,
            Outcome {
                returnval: &mut returnval,
                found_match: &mut found,
            },
        );
        match ret {
            -1 => {
                rv = -1;
                break;
            }
            0 => {}
            _ => {
                if ms.flags & MAGIC_CONTINUE == 0 {
                    rv = ret;
                    break;
                }
                rv = ret;
            }
        }
    }
    if let Some((i, n)) = counts {
        *i = walk.indir_count;
        *n = walk.name_count;
    }
    rv
}

/// The line of `softmagic.c` each `F()` call sits on, which upstream's
/// format-mismatch error quotes.
mod line {
    pub const BYTE: usize = 630;
    pub const SHORT: usize = 635;
    pub const LONG: usize = 641;
    pub const QUAD: usize = 647;
    pub const STRING_EQ: usize = 654;
    pub const STRING: usize = 670;
    pub const DATE: usize = 687;
    pub const LDATE: usize = 696;
    pub const QDATE: usize = 705;
    pub const QLDATE: usize = 713;
    pub const QWDATE: usize = 721;
    pub const FLOAT_S: usize = 736;
    pub const FLOAT: usize = 740;
    pub const DOUBLE_S: usize = 755;
    pub const DOUBLE: usize = 759;
    pub const SEARCH: usize = 778;
    pub const DER: usize = 798;
    pub const GUID: usize = 805;
    pub const MSDOSDATE: usize = 811;
    pub const MSDOSTIME: usize = 818;
    pub const OCTAL: usize = 824;
    pub const INDIRECT: usize = 1914;
}

/// `file_fmtcheck` (the `F()` macro): the description, if its format takes
/// what `def` takes; else `def`, and an error naming the mismatch.
fn fmt_or<'a>(ms: &mut Ms, desc: &'a [u8], def: &'a [u8], line: usize) -> &'a [u8] {
    if !cstr(desc).contains(&b'%') {
        return desc;
    }
    let ptr = fmtcheck(desc, def);
    if std::ptr::eq(ptr, def) {
        let mut msg = format!("softmagic.c, {line}: format `").into_bytes();
        msg.extend_from_slice(cstr(desc));
        msg.extend_from_slice(b"' does not match with `");
        msg.extend_from_slice(def);
        msg.push(b'\'');
        ms.magerror(&msg);
    }
    ptr
}

/// `IS_STRING(t)` rules that apply only to text, or only to binary data.
const FLT: u32 = STRING_BINTEST | STRING_TEXTTEST;

/// `match`: run `rules` against `b`, starting at `offset`. 1 when something
/// matched and was reported, 0 when nothing did, -1 on an error.
#[allow(clippy::too_many_lines)]
fn match_rules(
    ms: &mut Ms,
    rules: Rules<'_>,
    b: &Buffer<'_>,
    offset: usize,
    flip: bool,
    walk: &mut Walk,
    out: Outcome<'_>,
) -> i32 {
    let magic = rules.magic;
    let nmagic = magic.len();
    let mut cont_level: usize = 0;
    let print = ms.flags & MAGIC_NODESC == 0;
    let mode = walk.mode;
    let text = walk.text;
    // `bb`: the buffer the current rule reads -- the start of the file, or
    // its end for a negative offset. It persists from rule to rule, as C's
    // does: a continuation with no offset of its own keeps its parent's.
    let mut bb: &[u8] = b.fbuf;

    ms.check_mem(cont_level);

    let mut magindex = 0usize;
    'top: while magindex < nmagic {
        // `goto flush`: skip this entry's continuations, go to the next one.
        macro_rules! flush {
            () => {{
                while magindex + 1 < nmagic && magic[magindex + 1].cont_level != 0 {
                    magindex += 1;
                }
                cont_level = 0;
                magindex += 1;
                continue 'top;
            }};
        }
        let m = &magic[magindex];
        let flush;
        if m.typ != FILE_NAME
            && ((is_string(m.typ)
                && ((text && m.str_flags() & FLT == STRING_BINTEST)
                    || (!text && m.str_flags() & FLT == STRING_TEXTTEST)))
                || m.flag & mode != mode)
        {
            flush!();
        }

        if msetoffset(ms, m, &mut bb, b, offset, cont_level) == -1 {
            flush!();
        }
        ms.line = m.lineno as usize;

        let pos = Pos {
            o: offset,
            cont_level,
            flip,
        };
        match mget(ms, m, b, bb, pos, walk, &mut *out.returnval, &mut *out.found_match) {
            -1 => return -1,
            0 => flush = m.reln != b'!',
            _ => {
                if m.typ == FILE_INDIRECT {
                    *out.found_match = true;
                    *out.returnval = 1;
                }
                match magiccheck(ms, m, rx_slot(rules, magindex), bb) {
                    -1 => return -1,
                    0 => flush = true,
                    _ => flush = false,
                }
            }
        }
        if flush {
            flush!();
        }

        let e = handle_annotation(ms, m, walk.firstline);
        if e != 0 {
            *out.found_match = true;
            walk.need_separator = true;
            walk.printed_something = true;
            *out.returnval = 1;
            walk.firstline = false;
            return e;
        }

        if m.desc[0] != 0 {
            *out.found_match = true;
            if print {
                *out.returnval = 1;
                walk.need_separator = true;
                walk.printed_something = true;
                if print_sep(ms, walk.firstline) == -1 {
                    return -1;
                }
                if mprint(ms, m, bb) == -1 {
                    return -1;
                }
            }
        }

        match moffset(ms, m, bb) {
            None => flush!(),
            Some(o) => {
                if let Some(li) = ms.c.get_mut(cont_level) {
                    li.off = o;
                }
            }
        }

        // And any continuations that match.
        cont_level += 1;
        ms.check_mem(cont_level);

        while magindex + 1 < nmagic && magic[magindex + 1].cont_level != 0 {
            magindex += 1;
            let m = &magic[magindex];
            ms.line = m.lineno as usize;

            let mcl = usize::from(m.cont_level);
            if cont_level < mcl {
                continue;
            }
            if cont_level > mcl {
                // The end of the level `cont_level` continuations.
                cont_level = mcl;
            }
            if msetoffset(ms, m, &mut bb, b, offset, cont_level) == -1 {
                flush!();
            }
            if m.flag & OFFADD != 0 {
                if cont_level == 0 {
                    if ms.flags & MAGIC_DEBUG != 0 {
                        debug(b"direct *zero* cont_level\n");
                    }
                    return 0;
                }
                let prev = ms.c.get(cont_level - 1).map_or(0, |li| li.off);
                #[allow(clippy::cast_sign_loss)]
                {
                    ms.offset = ms.offset.wrapping_add(prev as u32);
                }
            }

            let pos = Pos {
                o: offset,
                cont_level,
                flip,
            };
            let mut skip_check = false;
            match mget(ms, m, b, bb, pos, walk, &mut *out.returnval, &mut *out.found_match) {
                -1 => return -1,
                0 => {
                    if m.reln != b'!' {
                        continue;
                    }
                    // Treated as a match, with no comparison at all.
                    skip_check = true;
                }
                _ => {
                    if m.typ == FILE_INDIRECT {
                        *out.found_match = true;
                        *out.returnval = 1;
                    }
                }
            }

            let r = if skip_check {
                1
            } else {
                magiccheck(ms, m, rx_slot(rules, magindex), bb)
            };
            match r {
                -1 => return -1,
                0 => {}
                _ => {
                    let got = ms.c.get(cont_level).is_some_and(|li| li.got_match);
                    if m.typ == FILE_CLEAR {
                        if let Some(li) = ms.c.get_mut(cont_level) {
                            li.got_match = false;
                        }
                    } else if got {
                        if m.typ == FILE_DEFAULT {
                            continue;
                        }
                    } else if let Some(li) = ms.c.get_mut(cont_level) {
                        li.got_match = true;
                    }

                    let e = handle_annotation(ms, m, walk.firstline);
                    if e != 0 {
                        *out.found_match = true;
                        walk.need_separator = true;
                        walk.printed_something = true;
                        *out.returnval = 1;
                        return e;
                    }
                    if m.desc[0] != 0 {
                        *out.found_match = true;
                    }
                    if print && m.desc[0] != 0 {
                        *out.returnval = 1;
                        // If we are going to print something, make sure that
                        // we have a separator first.
                        if !walk.printed_something {
                            walk.printed_something = true;
                            if print_sep(ms, walk.firstline) == -1 {
                                return -1;
                            }
                        }
                        // A space if the previous item printed.
                        if walk.need_separator && m.flag & NOSPACE == 0 && ms.printf(b" ", &[]) == -1 {
                            return -1;
                        }
                        if mprint(ms, m, bb) == -1 {
                            return -1;
                        }
                        walk.need_separator = true;
                    }

                    match moffset(ms, m, bb) {
                        None => cont_level = cont_level.wrapping_sub(1),
                        Some(o) => {
                            if let Some(li) = ms.c.get_mut(cont_level) {
                                li.off = o;
                            }
                        }
                    }
                    // If we see any continuations at a higher level, process
                    // them.
                    cont_level = cont_level.wrapping_add(1);
                    ms.check_mem(cont_level);
                }
            }
        }
        if walk.printed_something {
            walk.firstline = false;
        }
        if *out.found_match {
            if ms.flags & MAGIC_CONTINUE == 0 {
                return *out.returnval;
            }
            // So that we print a separator.
            walk.printed_something = false;
            walk.firstline = false;
        }
        cont_level = 0;
        magindex += 1;
    }
    *out.returnval
}

/// The regex slot of rule `i`.
fn rx_slot(rules: Rules<'_>, i: usize) -> Option<&Rx> {
    rules.rx.get(i)
}

/// Raw bytes to stderr, for `-d`.
fn debug(msg: &[u8]) {
    let _written = std::io::stderr().write_all(msg);
}

/// `print_sep`: `\n- ` before every answer but the first line's.
fn print_sep(ms: &mut Ms, firstline: bool) -> i32 {
    if firstline {
        return 0;
    }
    ms.separator()
}

/// `handle_annotation`: with `--apple`, `--extension` or `--mime-type`, the
/// rule's annotation is the answer.
fn handle_annotation(ms: &mut Ms, m: &Magic, firstline: bool) -> i32 {
    if ms.flags & MAGIC_APPLE != 0 && m.apple[0] != 0 {
        if print_sep(ms, firstline) == -1 {
            return -1;
        }
        if ms.printf(b"%.8s", &[Arg::Str(&m.apple)]) == -1 {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_EXTENSION != 0 && m.ext[0] != 0 {
        if print_sep(ms, firstline) == -1 {
            return -1;
        }
        if ms.print_str(&m.ext) == -1 {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_MIME_TYPE != 0 && m.mimetype[0] != 0 {
        if print_sep(ms, firstline) == -1 {
            return -1;
        }
        let p = varexpand(ms, 1024, m.mimetype()).unwrap_or_else(|| m.mimetype().to_vec());
        if ms.print_str(&p) == -1 {
            return -1;
        }
        return 1;
    }
    0
}

/// `varexpand`: `${x?yes:no}` -- `yes` if the file is executable, else `no`.
/// `None` when the string does not fit `len` bytes or a `${` is malformed,
/// and the caller uses it unexpanded.
fn varexpand(ms: &Ms, len: usize, s: &[u8]) -> Option<Vec<u8>> {
    let s = cstr(s);
    let mut out = Vec::new();
    let mut room = len;
    let mut sp = 0usize;
    loop {
        let rest = s.get(sp..).unwrap_or_default();
        let Some(k) = rest.windows(2).position(|w| w == b"${") else {
            break;
        };
        if k >= room {
            return None;
        }
        out.extend_from_slice(rest.get(..k).unwrap_or_default());
        room -= k;
        let p = sp + k + 2;
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        if at(p) == 0 || at(p + 1) != b'?' {
            return None;
        }
        let t = p + 2;
        let mut et = t;
        while at(et) != 0 && at(et) != b':' {
            et += 1;
        }
        if at(et) != b':' {
            return None;
        }
        let e = et + 1;
        let mut ee = e;
        while at(ee) != 0 && at(ee) != b'}' {
            ee += 1;
        }
        if at(ee) != b'}' {
            return None;
        }
        let (from, to) = match at(p) {
            b'x' => {
                if ms.mode & 0o111 != 0 {
                    (t, et)
                } else {
                    (e, ee)
                }
            }
            _ => return None,
        };
        let l = to - from;
        if l >= room {
            return None;
        }
        out.extend_from_slice(s.get(from..to).unwrap_or_default());
        room -= l;
        sp = ee + 1;
    }
    let tail = s.get(sp..).unwrap_or_default();
    if tail.len() >= room {
        return None;
    }
    out.extend_from_slice(tail);
    Some(out)
}

/// `check_fmt`: whether the description prints its value with a `%s` --
/// upstream's `%[-0-9\.]*s`, searched for -- so a number has to be written
/// out first.
fn check_fmt(desc: &[u8]) -> bool {
    let d = cstr(desc);
    if !d.contains(&b'%') {
        return false;
    }
    d.iter().enumerate().any(|(i, &c)| {
        c == b'%'
            && d.get(i + 1..)
                .unwrap_or_default()
                .iter()
                .find(|&&x| !(x == b'-' || x == b'.' || x == b'\\' || x.is_ascii_digit()))
                == Some(&b's')
    })
}

/// `mprint`'s `PRINTER`: an integer through the description, signed or not
/// as the rule says, at `bits` wide.
fn print_int(ms: &mut Ms, m: &Magic, desc: &[u8], value: u64, bits: u32, line: usize) -> i32 {
    let v = file_signextend(ms, m, value);
    let unsigned = m.flag & UNSIGNED != 0;
    let quad = bits == 64;
    // The argument as the C call passes it.
    #[allow(clippy::cast_possible_truncation)]
    let arg = if quad {
        Arg::I64(v)
    } else {
        let narrowed: u32 = match (bits, unsigned) {
            (8, true) => u32::from(v as u8),
            (8, false) => i32::from(v as u8 as i8) as u32,
            (16, true) => u32::from(v as u16),
            (16, false) => i32::from(v as u16 as i16) as u32,
            (_, _) => v as u32,
        };
        Arg::I32(narrowed)
    };
    let def: &[u8] = match (quad, unsigned) {
        (true, true) => b"%llu",
        (true, false) => b"%lld",
        (false, true) => b"%u",
        (false, false) => b"%d",
    };
    if check_fmt(desc) {
        let buf = crate::printf::format(def, &[arg]).unwrap_or_default();
        let f = fmt_or(ms, desc, b"%s", line).to_vec();
        if ms.printf(&f, &[Arg::Str(&buf)]) == -1 {
            return -1;
        }
    } else {
        let f = fmt_or(ms, desc, def, line).to_vec();
        if ms.printf(&f, &[arg]) == -1 {
            return -1;
        }
    }
    0
}

/// `mprint`: the rule's description, with what it read.
#[allow(clippy::too_many_lines)]
fn mprint(ms: &mut Ms, m: &Magic, s: &[u8]) -> i32 {
    let expanded = varexpand(ms, 512, m.desc());
    let desc: Vec<u8> = expanded.unwrap_or_else(|| m.desc().to_vec());
    let p = ms.ms_value;
    let print_s = |ms: &mut Ms, line: usize, arg: &[u8]| -> i32 {
        let f = fmt_or(ms, &desc, b"%s", line).to_vec();
        ms.printf(&f, &[Arg::Str(arg)])
    };
    match m.typ {
        FILE_BYTE => return print_int(ms, m, &desc, u64::from(p.b()), 8, line::BYTE),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => {
            return print_int(ms, m, &desc, u64::from(p.h()), 16, line::SHORT);
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => {
            return print_int(ms, m, &desc, u64::from(p.l()), 32, line::LONG);
        }
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_OFFSET => {
            return print_int(ms, m, &desc, p.q(), 64, line::QUAD);
        }
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
            if m.reln == b'=' || m.reln == b'!' {
                let shown = file_printable(ms.flags & MAGIC_RAW != 0, 512, &m.value.0, MAXSTRING);
                if print_s(ms, line::STRING_EQ, &shown) == -1 {
                    return -1;
                }
            } else {
                // What was read, cut at a line end when the rule's value is
                // empty, and trimmed when it asks -- which change the value
                // itself, as they do in C, and `moffset` measures after.
                if m.value.0[0] == 0 {
                    let n = cstrlen(&ms.ms_value.0);
                    if let Some(k) = ms.ms_value.0.get(..n).and_then(|v| v.iter().position(|&c| c == b'\r' || c == b'\n')) {
                        ms.ms_value.0[k] = 0;
                    }
                }
                let mut start = 0usize;
                if m.str_flags() & STRING_TRIM != 0 {
                    let whole = cstr(&ms.ms_value.0).to_vec();
                    let lead = whole.iter().take_while(|&&c| isspace(c)).count();
                    let trimmed = file_strtrim(&whole);
                    if !trimmed.is_empty() {
                        let end = lead + trimmed.len();
                        if end < MAXSTRING {
                            ms.ms_value.0[end] = 0;
                        }
                        start = lead;
                    } else {
                        start = whole.len();
                    }
                }
                let v = ms.ms_value;
                let shown = file_printable(
                    ms.flags & MAGIC_RAW != 0,
                    512,
                    v.0.get(start..).unwrap_or_default(),
                    MAXSTRING - start,
                );
                if print_s(ms, line::STRING, &shown) == -1 {
                    return -1;
                }
                if m.typ == FILE_PSTRING && file_pstring_length_size(ms, m).is_none() {
                    return -1;
                }
            }
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE => {
            let t = file_fmtdatetime(u64::from(p.l()), 0);
            if print_s(ms, line::DATE, &t) == -1 {
                return -1;
            }
        }
        FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            let t = file_fmtdatetime(u64::from(p.l()), FILE_T_LOCAL);
            if print_s(ms, line::LDATE, &t) == -1 {
                return -1;
            }
        }
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE => {
            let t = file_fmtdatetime(p.q(), 0);
            if print_s(ms, line::QDATE, &t) == -1 {
                return -1;
            }
        }
        FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE => {
            let t = file_fmtdatetime(p.q(), FILE_T_LOCAL);
            if print_s(ms, line::QLDATE, &t) == -1 {
                return -1;
            }
        }
        FILE_QWDATE | FILE_BEQWDATE | FILE_LEQWDATE => {
            let t = file_fmtdatetime(p.q(), FILE_T_WINDOWS);
            if print_s(ms, line::QWDATE, &t) == -1 {
                return -1;
            }
        }
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            let vf = f64::from(p.f());
            if check_fmt(&desc) {
                let buf = crate::printf::format(b"%g", &[Arg::F64(vf)]).unwrap_or_default();
                if print_s(ms, line::FLOAT_S, &buf) == -1 {
                    return -1;
                }
            } else {
                let f = fmt_or(ms, &desc, b"%g", line::FLOAT).to_vec();
                if ms.printf(&f, &[Arg::F64(vf)]) == -1 {
                    return -1;
                }
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let vd = p.d();
            if check_fmt(&desc) {
                let buf = crate::printf::format(b"%g", &[Arg::F64(vd)]).unwrap_or_default();
                if print_s(ms, line::DOUBLE_S, &buf) == -1 {
                    return -1;
                }
            } else {
                let f = fmt_or(ms, &desc, b"%g", line::DOUBLE).to_vec();
                if ms.printf(&f, &[Arg::F64(vd)]) == -1 {
                    return -1;
                }
            }
        }
        FILE_SEARCH | FILE_REGEX => {
            // `strndup(search.s, rm_len)`: up to a NUL too.
            let start = ms.search.s.unwrap_or(0);
            let found = s.get(start..).unwrap_or_default();
            let cp: Vec<u8> = found
                .iter()
                .take(ms.search.rm_len)
                .take_while(|&&c| c != 0)
                .copied()
                .collect();
            let scp: &[u8] = if m.str_flags() & STRING_TRIM != 0 {
                file_strtrim(&cp)
            } else {
                &cp
            };
            let shown = file_printable(ms.flags & MAGIC_RAW != 0, 512, scp, ms.search.rm_len);
            if print_s(ms, line::SEARCH, &shown) == -1 {
                return -1;
            }
        }
        FILE_DEFAULT | FILE_CLEAR => {
            if ms.print_str(m.desc()) == -1 {
                return -1;
            }
        }
        FILE_INDIRECT | FILE_USE | FILE_NAME => {}
        FILE_DER => {
            let shown = file_printable(ms.flags & MAGIC_RAW != 0, 512, &p.0, MAXSTRING);
            if print_s(ms, line::DER, &shown) == -1 {
                return -1;
            }
        }
        FILE_GUID => {
            let mut g = [0u8; 16];
            g.copy_from_slice(&p.0[..16]);
            let buf = file_print_guid(&g);
            if print_s(ms, line::GUID, &buf) == -1 {
                return -1;
            }
        }
        FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE => {
            let t = file_fmtdate(p.h());
            if print_s(ms, line::MSDOSDATE, &t) == -1 {
                return -1;
            }
        }
        FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
            let t = file_fmttime(p.h());
            if print_s(ms, line::MSDOSTIME, &t) == -1 {
                return -1;
            }
        }
        FILE_OCTAL => {
            // Upstream formats the rule's value here, not what was read.
            let buf = file_fmtnum(m.value.s(), 8);
            if print_s(ms, line::OCTAL, &buf) == -1 {
                return -1;
            }
        }
        _ => {
            ms.magerror(format!("invalid m->type ({}) in mprint()", m.typ).as_bytes());
            return -1;
        }
    }
    0
}

/// `moffset`: where the rule's match ended, for the relative offsets under
/// it. `None` when that is past the end of the buffer (upstream's -1 and its
/// DER 0 alike end the entry).
fn moffset(ms: &mut Ms, m: &Magic, s: &[u8]) -> Option<i32> {
    let nbytes = s.len();
    let off = ms.offset;
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let o: i32 = match m.typ {
        FILE_BYTE => off.wrapping_add(1) as i32,
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            off.wrapping_add(2) as i32
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => off.wrapping_add(4) as i32,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD => off.wrapping_add(8) as i32,
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
            if m.reln == b'=' || m.reln == b'!' {
                off.wrapping_add(u32::from(m.vallen)) as i32
            } else {
                if m.value.0[0] == 0 {
                    let n = cstrlen(&ms.ms_value.0);
                    if let Some(k) = ms.ms_value.0.get(..n).and_then(|v| v.iter().position(|&c| c == b'\r' || c == b'\n')) {
                        ms.ms_value.0[k] = 0;
                    }
                }
                let mut o = off.wrapping_add(cstrlen(&ms.ms_value.0) as u32);
                if m.typ == FILE_PSTRING {
                    let l = file_pstring_length_size(ms, m)?;
                    o = o.wrapping_add(l as u32);
                }
                o as i32
            }
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE
        | FILE_LELDATE | FILE_MELDATE => off.wrapping_add(4) as i32,
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE | FILE_QLDATE | FILE_BEQLDATE
        | FILE_LEQLDATE => off.wrapping_add(8) as i32,
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => off.wrapping_add(4) as i32,
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => off.wrapping_add(8) as i32,
        FILE_REGEX => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                ms.search.offset as i32
            } else {
                ms.search.offset.wrapping_add(ms.search.rm_len) as i32
            }
        }
        FILE_SEARCH => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                ms.search.offset as i32
            } else {
                ms.search.offset.wrapping_add(usize::from(m.vallen)) as i32
            }
        }
        FILE_CLEAR | FILE_DEFAULT | FILE_INDIRECT | FILE_OFFSET | FILE_USE => off as i32,
        FILE_DER => {
            let o = crate::der::der_offs(ms, m, s, nbytes);
            #[allow(clippy::cast_sign_loss)]
            if o == -1 || o as usize > nbytes {
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(format!("Bad DER offset {o} nbytes={nbytes}").as_bytes());
                }
                // Not a failure: the offset becomes 0.
                return Some(0);
            }
            o
        }
        FILE_GUID => off.wrapping_add(16) as i32,
        _ => 0,
    };
    #[allow(clippy::cast_sign_loss)]
    if o as isize as usize > nbytes {
        return None;
    }
    Some(o)
}

/// `cvt_id3`: an ID3 "syncsafe" integer -- seven bits a byte.
fn cvt_id3(ms: &Ms, v: u32) -> u32 {
    let v = (v & 0x7f) | (((v >> 8) & 0x7f) << 7) | (((v >> 16) & 0x7f) << 14) | (((v >> 24) & 0x7f) << 21);
    if ms.flags & MAGIC_DEBUG != 0 {
        debug(format!("id3 offs={v}\n").as_bytes());
    }
    v
}

/// `cvt_flip`: the type read with the other byte order, under `use ^name`.
fn cvt_flip(typ: u8, flip: bool) -> u8 {
    if !flip {
        return typ;
    }
    match typ {
        FILE_BESHORT => FILE_LESHORT,
        FILE_BELONG => FILE_LELONG,
        FILE_BEDATE => FILE_LEDATE,
        FILE_BELDATE => FILE_LELDATE,
        FILE_BEQUAD => FILE_LEQUAD,
        FILE_BEQDATE => FILE_LEQDATE,
        FILE_BEQLDATE => FILE_LEQLDATE,
        FILE_BEQWDATE => FILE_LEQWDATE,
        FILE_LESHORT => FILE_BESHORT,
        FILE_LELONG => FILE_BELONG,
        FILE_LEDATE => FILE_BEDATE,
        FILE_LELDATE => FILE_BELDATE,
        FILE_LEQUAD => FILE_BEQUAD,
        FILE_LEQDATE => FILE_BEQDATE,
        FILE_LEQLDATE => FILE_BEQLDATE,
        FILE_LEQWDATE => FILE_BEQWDATE,
        FILE_BEFLOAT => FILE_LEFLOAT,
        FILE_LEFLOAT => FILE_BEFLOAT,
        FILE_BEDOUBLE => FILE_LEDOUBLE,
        FILE_LEDOUBLE => FILE_BEDOUBLE,
        _ => typ,
    }
}

/// The integer conversions' `DO_CVT`: the rule's mask operation, then `~`.
/// `Err` is a division or modulo by zero.
macro_rules! do_cvt {
    ($v:expr, $m:expr, $ty:ty) => {{
        let mut v: $ty = $v;
        #[allow(clippy::cast_possible_truncation)]
        let mask = $m.num_mask() as $ty;
        if $m.num_mask() != 0 {
            match $m.mask_op & FILE_OPS_MASK {
                FILE_OPAND => v &= mask,
                FILE_OPOR => v |= mask,
                FILE_OPXOR => v ^= mask,
                FILE_OPADD => v = v.wrapping_add(mask),
                FILE_OPMINUS => v = v.wrapping_sub(mask),
                FILE_OPMULTIPLY => v = v.wrapping_mul(mask),
                FILE_OPDIVIDE => {
                    if mask == 0 {
                        return Err(());
                    }
                    v /= mask;
                }
                FILE_OPMODULO => {
                    if mask == 0 {
                        return Err(());
                    }
                    v %= mask;
                }
                _ => {}
            }
        }
        if $m.mask_op & FILE_OPINVERSE != 0 {
            v = !v;
        }
        v
    }};
}

fn cvt_8(p: &mut Value, m: &Magic) -> Result<(), ()> {
    let v = do_cvt!(p.b(), m, u8);
    p.set_b(v);
    Ok(())
}

fn cvt_16(p: &mut Value, m: &Magic) -> Result<(), ()> {
    let v = do_cvt!(p.h(), m, u16);
    p.set_h(v);
    Ok(())
}

fn cvt_32(p: &mut Value, m: &Magic) -> Result<(), ()> {
    let v = do_cvt!(p.l(), m, u32);
    p.set_l(v);
    Ok(())
}

fn cvt_64(p: &mut Value, m: &Magic) -> Result<(), ()> {
    let v = do_cvt!(p.q(), m, u64);
    p.set_q(v);
    Ok(())
}

/// `cvt_float` (`DO_CVT2`): only `+ - * /`, and no `~`.
#[allow(clippy::cast_precision_loss)]
fn cvt_float(p: &mut Value, m: &Magic) -> Result<(), ()> {
    if m.num_mask() != 0 {
        let mask = m.num_mask() as f32;
        let mut v = p.f();
        match m.mask_op & FILE_OPS_MASK {
            FILE_OPADD => v += mask,
            FILE_OPMINUS => v -= mask,
            FILE_OPMULTIPLY => v *= mask,
            FILE_OPDIVIDE => {
                if mask == 0.0 {
                    return Err(());
                }
                v /= mask;
            }
            _ => {}
        }
        p.set_f(v);
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn cvt_double(p: &mut Value, m: &Magic) -> Result<(), ()> {
    if m.num_mask() != 0 {
        let mask = m.num_mask() as f64;
        let mut v = p.d();
        match m.mask_op & FILE_OPS_MASK {
            FILE_OPADD => v += mask,
            FILE_OPMINUS => v -= mask,
            FILE_OPMULTIPLY => v *= mask,
            FILE_OPDIVIDE => {
                if mask == 0.0 {
                    return Err(());
                }
                v /= mask;
            }
            _ => {}
        }
        p.set_d(v);
    }
    Ok(())
}

fn be16(p: &Value) -> u16 {
    u16::from_be_bytes([p.0[0], p.0[1]])
}
fn le16(p: &Value) -> u16 {
    u16::from_le_bytes([p.0[0], p.0[1]])
}
fn be32(p: &Value) -> u32 {
    u32::from_be_bytes([p.0[0], p.0[1], p.0[2], p.0[3]])
}
fn le32(p: &Value) -> u32 {
    u32::from_le_bytes([p.0[0], p.0[1], p.0[2], p.0[3]])
}
/// The PDP-11's middle-endian long: the two halves little-endian, high half
/// first.
fn me32(p: &Value) -> u32 {
    (u32::from(p.0[1]) << 24) | (u32::from(p.0[0]) << 16) | (u32::from(p.0[3]) << 8) | u32::from(p.0[2])
}
fn be64(p: &Value) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&p.0[..8]);
    u64::from_be_bytes(b)
}
fn le64(p: &Value) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&p.0[..8]);
    u64::from_le_bytes(b)
}

/// `mconvert`: what was read, put in host order and masked. 0 when the
/// value cannot be used.
fn mconvert(ms: &mut Ms, m: &Magic, flip: bool) -> i32 {
    let mut p = ms.ms_value;
    let r = match cvt_flip(m.typ, flip) {
        FILE_BYTE => cvt_8(&mut p, m),
        FILE_SHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE | FILE_BEMSDOSDATE | FILE_MSDOSTIME
        | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => cvt_16(&mut p, m),
        FILE_LONG | FILE_DATE | FILE_LDATE => cvt_32(&mut p, m),
        FILE_QUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE | FILE_OFFSET => cvt_64(&mut p, m),
        FILE_STRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
            // NUL-terminate.
            p.0[MAXSTRING - 1] = 0;
            Ok(())
        }
        FILE_PSTRING => {
            let Some(sz) = file_pstring_length_size(ms, m) else {
                return 0;
            };
            let Some(len) = file_pstring_get_length(ms, m, &p.0) else {
                return 0;
            };
            let room = MAXSTRING - sz;
            let len = len.min(room);
            p.0.copy_within(sz..sz + len, 0);
            p.0[len] = 0;
            Ok(())
        }
        FILE_BESHORT => {
            let v = be16(&p);
            p.set_h(v);
            cvt_16(&mut p, m)
        }
        FILE_BELONG | FILE_BEDATE | FILE_BELDATE => {
            let v = be32(&p);
            p.set_l(v);
            cvt_32(&mut p, m)
        }
        FILE_BEQUAD | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE => {
            let v = be64(&p);
            p.set_q(v);
            cvt_64(&mut p, m)
        }
        FILE_LESHORT => {
            let v = le16(&p);
            p.set_h(v);
            cvt_16(&mut p, m)
        }
        FILE_LELONG | FILE_LEDATE | FILE_LELDATE => {
            let v = le32(&p);
            p.set_l(v);
            cvt_32(&mut p, m)
        }
        FILE_LEQUAD | FILE_LEQDATE | FILE_LEQLDATE | FILE_LEQWDATE => {
            let v = le64(&p);
            p.set_q(v);
            cvt_64(&mut p, m)
        }
        FILE_MELONG | FILE_MEDATE | FILE_MELDATE => {
            let v = me32(&p);
            p.set_l(v);
            cvt_32(&mut p, m)
        }
        FILE_FLOAT => cvt_float(&mut p, m),
        FILE_BEFLOAT => {
            let v = be32(&p);
            p.set_l(v);
            cvt_float(&mut p, m)
        }
        FILE_LEFLOAT => {
            let v = le32(&p);
            p.set_l(v);
            cvt_float(&mut p, m)
        }
        FILE_DOUBLE => cvt_double(&mut p, m),
        FILE_BEDOUBLE => {
            let v = be64(&p);
            p.set_q(v);
            cvt_double(&mut p, m)
        }
        FILE_LEDOUBLE => {
            let v = le64(&p);
            p.set_q(v);
            cvt_double(&mut p, m)
        }
        FILE_REGEX | FILE_SEARCH | FILE_DEFAULT | FILE_CLEAR | FILE_NAME | FILE_USE
        | FILE_DER | FILE_GUID => return 1,
        _ => {
            ms.magerror(format!("invalid type {} in mconvert()", m.typ).as_bytes());
            return 0;
        }
    };
    ms.ms_value = p;
    if r.is_err() {
        ms.magerror(b"zerodivide in mconvert()");
        return 0;
    }
    1
}

/// `mdebug`.
fn mdebug(offset: u32, s: &[u8]) {
    let mut w = format!("mget/{} @{}: ", s.len(), offset as i32).into_bytes();
    crate::print::file_showstr(&mut w, s, Some(s.len()));
    w.extend_from_slice(b"\n\n");
    debug(&w);
}

/// `mcopy`: read what the rule's type needs from `s` at `offset` into
/// `ms_value` -- or, for `search` and `regex`, only note where to look.
fn mcopy(ms: &mut Ms, typ: u8, indir: bool, s: &[u8], offset: u32, nbytes: usize, m: &Magic) -> i32 {
    let mut size = MAXSTRING;
    let offset = offset as usize;
    if !indir {
        match typ {
            FILE_DER | FILE_SEARCH => {
                let offset = offset.min(nbytes);
                ms.search.s = Some(offset);
                ms.search.s_len = nbytes - offset;
                ms.search.offset = offset;
                return 0;
            }
            FILE_REGEX => {
                if nbytes < offset {
                    ms.search.s_len = 0;
                    ms.search.s = None;
                    return 0;
                }
                let (linecnt, mut bytecnt) = if m.str_flags() & REGEX_LINE_COUNT != 0 {
                    let l = m.str_range() as usize;
                    (l, l.saturating_mul(80))
                } else {
                    (0, m.str_range() as usize)
                };
                if bytecnt == 0 || bytecnt > nbytes - offset {
                    bytecnt = nbytes - offset;
                }
                if bytecnt > usize::from(ms.regex_max) {
                    bytecnt = usize::from(ms.regex_max);
                }
                let buf = offset;
                let end = offset + bytecnt;
                let mut last = end;
                let at = |i: usize| s.get(i).copied().unwrap_or(0);
                let mut lines = linecnt;
                let mut bpos = buf;
                while lines > 0 && bpos < end {
                    let c = bpos;
                    let hit = s
                        .get(c..end)
                        .and_then(|w| w.iter().position(|&x| x == b'\n'))
                        .or_else(|| s.get(c..end).and_then(|w| w.iter().position(|&x| x == b'\r')));
                    let Some(k) = hit else {
                        break;
                    };
                    bpos = c + k;
                    if bpos + 1 < end && at(bpos) == b'\r' && at(bpos + 1) == b'\n' {
                        bpos += 1;
                    }
                    if bpos + 1 < end && at(bpos) == b'\n' {
                        bpos += 1;
                    }
                    last = bpos;
                    lines -= 1;
                    bpos += 1;
                }
                if lines > 0 {
                    last = end;
                }
                ms.search.s = Some(buf);
                ms.search.s_len = last - buf;
                ms.search.offset = offset;
                ms.search.rm_len = 0;
                return 0;
            }
            FILE_BESTRING16 | FILE_LESTRING16 => {
                if offset < nbytes {
                    let mut src = offset + usize::from(typ == FILE_BESTRING16);
                    let esrc = nbytes;
                    let mut dst = 0usize;
                    let edst = MAXSTRING - 1;
                    let at = |i: usize| s.get(i).copied().unwrap_or(0);
                    while src < esrc {
                        if dst < edst {
                            ms.ms_value.0[dst] = at(src);
                        } else {
                            break;
                        }
                        if ms.ms_value.0[dst] == 0 {
                            let wide = if typ == FILE_BESTRING16 {
                                at(src - 1) != 0
                            } else {
                                src + 1 < esrc && at(src + 1) != 0
                            };
                            if wide {
                                ms.ms_value.0[dst] = b' ';
                            }
                        }
                        src += 2;
                        dst += 1;
                    }
                    ms.ms_value.0[edst] = 0;
                    return 0;
                }
            }
            FILE_STRING | FILE_PSTRING => {
                let r = m.str_range() as usize;
                if r != 0 && r < MAXSTRING {
                    size = r;
                }
            }
            _ => {}
        }
    }

    if typ == FILE_OFFSET {
        ms.ms_value = Value::default();
        ms.ms_value.set_q(offset as u64);
        return 0;
    }
    if offset >= nbytes {
        ms.ms_value = Value::default();
        return 0;
    }
    let n = (nbytes - offset).min(size);
    let mut v = Value::default();
    if let Some(src) = s.get(offset..offset + n) {
        v.0[..n].copy_from_slice(src);
    }
    ms.ms_value = v;
    0
}

/// `do_ops`: an indirect offset's arithmetic. `Err` when an operand or the
/// result does not fit 32 bits, which ends the rule.
fn do_ops(ms: &Ms, m: &Magic, lhs: i64, off: i64) -> Result<u32, ()> {
    let uint_max = i64::from(u32::MAX);
    let int_min = i64::from(i32::MIN);
    if lhs >= uint_max || lhs <= int_min || off >= uint_max || off <= int_min {
        if ms.flags & MAGIC_DEBUG != 0 {
            debug(format!("lhs/off overflow {lhs} {off}\n").as_bytes());
        }
        return Err(());
    }
    let mut offset = if off != 0 {
        match m.in_op & FILE_OPS_MASK {
            FILE_OPAND => lhs & off,
            FILE_OPOR => lhs | off,
            FILE_OPXOR => lhs ^ off,
            FILE_OPADD => lhs + off,
            FILE_OPMINUS => lhs - off,
            FILE_OPMULTIPLY => lhs * off,
            FILE_OPDIVIDE => lhs / off,
            FILE_OPMODULO => lhs % off,
            _ => lhs,
        }
    } else {
        lhs
    };
    if m.in_op & FILE_OPINVERSE != 0 {
        offset = !offset;
    }
    if offset >= uint_max {
        if ms.flags & MAGIC_DEBUG != 0 {
            debug(format!("offset overflow {offset}\n").as_bytes());
        }
        return Err(());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(offset as u32)
}

/// `msetoffset`: the offset the rule reads at, and from which buffer -- the
/// tail of the file for a negative offset.
fn msetoffset<'a>(ms: &mut Ms, m: &Magic, bb: &mut &'a [u8], b: &'a Buffer<'a>, o: usize, cont_level: usize) -> i32 {
    #[allow(clippy::cast_sign_loss)]
    let normal = |ms: &mut Ms, bb: &mut &'a [u8], offset: i32| {
        *bb = b.fbuf;
        ms.offset = offset as u32;
        ms.eoffset = 0;
    };
    if m.flag & OFFNEGATIVE != 0 {
        let offset = m.offset.wrapping_neg();
        if cont_level > 0 && m.flag & (OFFADD | INDIROFFADD) != 0 {
            normal(ms, bb, offset);
        } else {
            let Some(tail) = b.fill() else {
                return -1;
            };
            if o != 0 {
                ms.magerror(format!("non zero offset {o} at level {cont_level}").as_bytes());
                return -1;
            }
            #[allow(clippy::cast_sign_loss)]
            if m.offset as u32 as usize > tail.len() {
                return -1;
            }
            *bb = tail;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                let e = (tail.len() as u32).wrapping_sub(m.offset as u32);
                ms.offset = e;
                ms.eoffset = e;
            }
        }
    } else if cont_level == 0 {
        normal(ms, bb, m.offset);
    } else {
        #[allow(clippy::cast_sign_loss)]
        {
            ms.offset = ms.eoffset.wrapping_add(m.offset as u32);
        }
    }
    if ms.flags & MAGIC_DEBUG != 0 {
        debug(
            format!(
                "bb=[{:p},{},0], {} [b={:p},{},0], [o={:#x}, c={}]\n",
                bb.as_ptr(),
                bb.len(),
                ms.offset as i32,
                b.fbuf.as_ptr(),
                b.flen(),
                m.offset,
                cont_level
            )
            .as_bytes(),
        );
    }
    0
}

/// `OFFSET_OOB(n, o, i)`: whether `i` bytes at `o` run past `n`.
fn oob(n: usize, o: i64, i: usize) -> bool {
    let n = n as u64;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    {
        n < u64::from(o as u32) || i as u64 > n.wrapping_sub(o as u64)
    }
}

/// `SEXT(sgn, bits, v)`: `v` at `bits` wide, signed or not.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn sext(sgn: bool, bits: u32, v: u64) -> i64 {
    match (bits, sgn) {
        (8, true) => i64::from(v as u8 as i8),
        (8, false) => i64::from(v as u8),
        (16, true) => i64::from(v as u16 as i16),
        (16, false) => i64::from(v as u16),
        (32, true) => i64::from(v as u32 as i32),
        (32, false) => i64::from(v as u32),
        _ => v as i64,
    }
}

/// Bytes of `s` at `at` as a value, zero past its end.
fn value_at(s: &[u8], at: i64) -> Value {
    let mut v = Value::default();
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let at = at as u64 as usize;
    if let Some(src) = s.get(at..) {
        let n = src.len().min(MAXSTRING);
        v.0[..n].copy_from_slice(&src[..n]);
    }
    v
}

/// `mget`: read the rule's value -- following an indirect offset first -- and
/// for `indirect`, `use` and `name` do what they do. 1 read, 0 not there,
/// -1 an error.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn mget(
    ms: &mut Ms,
    m: &Magic,
    b: &Buffer<'_>,
    s: &[u8],
    pos: Pos,
    walk: &mut Walk,
    returnval: &mut i32,
    found_match: &mut bool,
) -> i32 {
    let nbytes = s.len();
    let mut offset = ms.offset;
    let o = pos.o;
    let cont_level = pos.cont_level;
    let mut flip = pos.flip;

    if walk.indir_count >= ms.indir_max {
        ms.error(None, format!("indirect count ({}) exceeded", walk.indir_count).as_bytes());
        return -1;
    }
    if walk.name_count >= ms.name_max {
        ms.error(None, format!("name use count ({}) exceeded", walk.name_count).as_bytes());
        return -1;
    }

    #[allow(clippy::cast_possible_truncation)]
    let at = offset.wrapping_add(o as u32);
    if mcopy(ms, m.typ, m.flag & INDIR != 0, s, at, nbytes, m) == -1 {
        return -1;
    }

    if ms.flags & MAGIC_DEBUG != 0 {
        debug(
            format!(
                "mget(type={}, flag={:#x}, offset={}, o={}, nbytes={}, il={}, nc={})\n",
                m.typ, m.flag, offset, o, nbytes, walk.indir_count, walk.name_count
            )
            .as_bytes(),
        );
        mdebug(offset, &ms.ms_value.0);
        crate::print::file_mdump(m);
    }

    if m.flag & INDIR != 0 {
        let mut off: i64 = i64::from(m.in_offset);
        let sgn = m.in_op & FILE_OPSIGNED != 0;
        let p = ms.ms_value;
        if m.in_op & FILE_OPINDIRECT != 0 {
            let qa = i64::from(offset) + off;
            let q = value_at(s, qa);
            let op = cvt_flip(m.in_type, flip);
            off = match op {
                FILE_BYTE => {
                    if oob(nbytes, qa, 1) {
                        return 0;
                    }
                    sext(sgn, 8, u64::from(q.b()))
                }
                FILE_SHORT => {
                    if oob(nbytes, qa, 2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(q.h()))
                }
                FILE_BESHORT => {
                    if oob(nbytes, qa, 2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(be16(&q)))
                }
                FILE_LESHORT => {
                    if oob(nbytes, qa, 2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(le16(&q)))
                }
                FILE_LONG => {
                    if oob(nbytes, qa, 4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(q.l()))
                }
                FILE_BELONG | FILE_BEID3 => {
                    if oob(nbytes, qa, 4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(be32(&q)))
                }
                FILE_LEID3 | FILE_LELONG => {
                    if oob(nbytes, qa, 4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(le32(&q)))
                }
                FILE_MELONG => {
                    if oob(nbytes, qa, 4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(me32(&q)))
                }
                FILE_BEQUAD => {
                    if oob(nbytes, qa, 8) {
                        return 0;
                    }
                    sext(sgn, 64, be64(&q))
                }
                FILE_LEQUAD => {
                    if oob(nbytes, qa, 8) {
                        return 0;
                    }
                    sext(sgn, 64, le64(&q))
                }
                FILE_OCTAL => {
                    if oob(nbytes, i64::from(offset), usize::from(m.vallen)) {
                        return 0;
                    }
                    sext(sgn, 64, strtoull(p.s(), 8).0)
                }
                _ => {
                    if ms.flags & MAGIC_DEBUG != 0 {
                        debug(format!("bad op={op}\n").as_bytes());
                    }
                    return 0;
                }
            };
            if ms.flags & MAGIC_DEBUG != 0 {
                debug(format!("indirect offs={off}\n").as_bytes());
            }
        }
        let in_type = cvt_flip(m.in_type, flip);
        let lhs: i64 = match in_type {
            FILE_BYTE => {
                if oob(nbytes, i64::from(offset), 1) {
                    return 0;
                }
                sext(sgn, 8, u64::from(p.b()))
            }
            FILE_BESHORT => {
                if oob(nbytes, i64::from(offset), 2) {
                    return 0;
                }
                sext(sgn, 16, u64::from(be16(&p)))
            }
            FILE_LESHORT => {
                if oob(nbytes, i64::from(offset), 2) {
                    return 0;
                }
                sext(sgn, 16, u64::from(le16(&p)))
            }
            FILE_SHORT => {
                if oob(nbytes, i64::from(offset), 2) {
                    return 0;
                }
                sext(sgn, 16, u64::from(p.h()))
            }
            FILE_BELONG | FILE_BEID3 => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                let mut lhs = be32(&p);
                if in_type == FILE_BEID3 {
                    lhs = cvt_id3(ms, lhs);
                }
                sext(sgn, 32, u64::from(lhs))
            }
            FILE_LELONG | FILE_LEID3 => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                let mut lhs = le32(&p);
                if in_type == FILE_LEID3 {
                    lhs = cvt_id3(ms, lhs);
                }
                sext(sgn, 32, u64::from(lhs))
            }
            FILE_MELONG => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                sext(sgn, 32, u64::from(me32(&p)))
            }
            FILE_LONG => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                sext(sgn, 32, u64::from(p.l()))
            }
            FILE_LEQUAD => {
                if oob(nbytes, i64::from(offset), 8) {
                    return 0;
                }
                sext(sgn, 64, le64(&p))
            }
            FILE_BEQUAD => {
                if oob(nbytes, i64::from(offset), 8) {
                    return 0;
                }
                sext(sgn, 64, be64(&p))
            }
            FILE_OCTAL => {
                if oob(nbytes, i64::from(offset), usize::from(m.vallen)) {
                    return 0;
                }
                sext(sgn, 64, strtoull(p.s(), 8).0)
            }
            _ => {
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(format!("bad in_type={in_type}\n").as_bytes());
                }
                return 0;
            }
        };
        match do_ops(ms, m, lhs, off) {
            Ok(v) => offset = v,
            Err(()) => return 0,
        }

        if m.flag & INDIROFFADD != 0 {
            if cont_level == 0 {
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(b"indirect *zero* cont_level\n");
                }
                return 0;
            }
            let prev = ms.c.get(cont_level - 1).map_or(0, |li| li.off);
            #[allow(clippy::cast_sign_loss)]
            {
                offset = offset.wrapping_add(prev as u32);
            }
            if offset == 0 {
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(b"indirect *zero* offset\n");
                }
                return 0;
            }
            if ms.flags & MAGIC_DEBUG != 0 {
                debug(format!("indirect +offs={offset}\n").as_bytes());
            }
        }
        if mcopy(ms, m.typ, false, s, offset, nbytes, m) == -1 {
            return -1;
        }
        ms.offset = offset;
        if ms.flags & MAGIC_DEBUG != 0 {
            mdebug(offset, &ms.ms_value.0);
            crate::print::file_mdump(m);
        }
    }

    // Verify there is enough data to match the type.
    let off64 = i64::from(offset);
    match m.typ {
        FILE_BYTE => {
            if oob(nbytes, off64, 1) {
                return 0;
            }
        }
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => {
            if oob(nbytes, off64, 2) {
                return 0;
            }
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            if oob(nbytes, off64, 4) {
                return 0;
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            if oob(nbytes, off64, 8) {
                return 0;
            }
        }
        FILE_GUID => {
            if oob(nbytes, off64, 16) {
                return 0;
            }
        }
        FILE_STRING | FILE_PSTRING | FILE_SEARCH | FILE_OCTAL => {
            if oob(nbytes, off64, usize::from(m.vallen)) {
                return 0;
            }
        }
        FILE_REGEX => {
            if (nbytes as u64) < u64::from(offset) {
                return 0;
            }
        }
        FILE_INDIRECT => {
            if m.str_flags() & INDIRECT_RELATIVE != 0 {
                #[allow(clippy::cast_possible_truncation)]
                {
                    offset = offset.wrapping_add(o as u32);
                }
            }
            if offset == 0 {
                return 0;
            }
            if (nbytes as u64) < u64::from(offset) {
                return 0;
            }
            let Some(pb) = ms.push_buffer() else {
                return -1;
            };
            walk.indir_count += 1;
            let sub = s.get(offset as usize..).unwrap_or_default();
            let bb = b.with_data(sub);
            let mut rv = -1;
            let lists: Vec<Rc<crate::funcs::MList>> = ms.mlist[0].clone().unwrap_or_default();
            let saved_mode = walk.mode;
            walk.mode = BINTEST;
            for ml in &lists {
                let mut returnval2 = 0;
                let mut found2 = false;
                rv = match_rules(
                    ms,
                    Rules {
                        magic: &ml.magic,
                        rx: &ml.rx,
                    },
                    &bb,
                    0,
                    false,
                    walk,
                    Outcome {
                        returnval: &mut returnval2,
                        found_match: &mut found2,
                    },
                );
                if rv != 0 {
                    break;
                }
            }
            walk.mode = saved_mode;
            if ms.flags & MAGIC_DEBUG != 0 {
                debug(format!("indirect @offs={offset}[{rv}]\n").as_bytes());
            }
            let rbuf = ms.pop_buffer(pb);
            if rbuf.is_none() && ms.had_err() {
                return -1;
            }
            if rv == 1 {
                if ms.flags & MAGIC_NODESC == 0 {
                    let f = fmt_or(ms, m.desc(), b"%u", line::INDIRECT).to_vec();
                    if ms.printf(&f, &[Arg::I32(offset)]) == -1 {
                        return -1;
                    }
                }
                let r = rbuf.unwrap_or_default();
                if ms.print_str(&r) == -1 {
                    return -1;
                }
            }
            return rv;
        }
        FILE_USE => {
            if (nbytes as u64) < u64::from(offset) {
                return 0;
            }
            let mut name = m.value.s();
            if name.first() == Some(&b'^') {
                name = &name[1..];
                flip = !flip;
            }
            let Some((ml, start, len)) = file_magicfind(ms, name) else {
                let mut msg = b"cannot find entry `".to_vec();
                msg.extend_from_slice(name);
                msg.push(b'\'');
                ms.error(None, &msg);
                return -1;
            };
            let saved_c = ms.c.clone();
            let oneed_separator = walk.need_separator;
            if m.flag & NOSPACE != 0 {
                walk.need_separator = false;
            }
            let mut nfound_match = false;
            walk.name_count += 1;
            let eoffset = ms.eoffset;
            let sub = Rules {
                magic: ml.magic.get(start..start + len).unwrap_or_default(),
                rx: ml.rx.get(start..start + len).unwrap_or_default(),
            };
            let rv = match_rules(
                ms,
                sub,
                b,
                (offset as usize).wrapping_add(o),
                flip,
                walk,
                Outcome {
                    returnval: &mut *returnval,
                    found_match: &mut nfound_match,
                },
            );
            ms.ms_value.set_q(u64::from(nfound_match));
            walk.name_count -= 1;
            *found_match |= nfound_match;
            ms.c = saved_c;
            if rv != 1 {
                walk.need_separator = oneed_separator;
            }
            ms.offset = offset;
            ms.eoffset = eoffset;
            return i32::from(rv != 0 || *found_match);
        }
        FILE_NAME => {
            if ms.flags & MAGIC_NODESC != 0 {
                return 1;
            }
            if ms.print_str(m.desc()) == -1 {
                return -1;
            }
            return 1;
        }
        _ => {}
    }
    if mconvert(ms, m, flip) == 0 {
        return 0;
    }
    1
}

/// The bytes after `ms_value` in C's `struct magic_set`, which a string
/// comparison can read one past the value: the six `uint16_t` limits.
fn value_with_tail(ms: &Ms) -> Vec<u8> {
    let mut v = ms.ms_value.0.to_vec();
    for x in [ms.indir_max, ms.name_max, ms.elf_shnum_max, ms.elf_phnum_max, ms.elf_notes_max, ms.regex_max] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

/// `file_strncmp`: `strncmp` of the rule's string and the data -- but
/// ignoring NULs, and under the rule's flags: case, compacted white space,
/// whole words. 0 is equal.
fn file_strncmp(a: &[u8], b: &[u8], len: usize, maxlen: usize, flags: u32) -> u64 {
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let ws = flags & (STRING_COMPACT_WHITESPACE | STRING_COMPACT_OPTIONAL_WHITESPACE);
    let eb = if ws != 0 { maxlen } else { len };
    let mut v: u64 = 0;
    let (mut ai, mut bi) = (0usize, 0usize);
    let mut left = len + 1;
    let diff = |x: u8, y: u8| -> u64 { (i64::from(x) - i64::from(y)) as u64 };
    if flags == 0 {
        loop {
            left -= 1;
            if left == 0 {
                break;
            }
            v = diff(at(b, bi), at(a, ai));
            bi += 1;
            ai += 1;
            if v != 0 {
                break;
            }
        }
    } else {
        loop {
            left -= 1;
            if left == 0 {
                break;
            }
            if bi >= eb {
                v = 1;
                break;
            }
            let ac = at(a, ai);
            if flags & STRING_IGNORE_LOWERCASE != 0 && ac.is_ascii_lowercase() {
                v = diff(at(b, bi).to_ascii_lowercase(), ac);
                bi += 1;
                ai += 1;
                if v != 0 {
                    break;
                }
            } else if flags & STRING_IGNORE_UPPERCASE != 0 && ac.is_ascii_uppercase() {
                v = diff(at(b, bi).to_ascii_uppercase(), ac);
                bi += 1;
                ai += 1;
                if v != 0 {
                    break;
                }
            } else if flags & STRING_COMPACT_WHITESPACE != 0 && isspace(ac) {
                ai += 1;
                if isspace(at(b, bi)) {
                    bi += 1;
                    if !isspace(at(a, ai)) {
                        while bi < eb && isspace(at(b, bi)) {
                            bi += 1;
                        }
                    }
                } else {
                    v = 1;
                    break;
                }
            } else if flags & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 && isspace(ac) {
                ai += 1;
                while bi < eb && isspace(at(b, bi)) {
                    bi += 1;
                }
            } else {
                v = diff(at(b, bi), at(a, ai));
                bi += 1;
                ai += 1;
                if v != 0 {
                    break;
                }
            }
        }
        if left == 0 && v == 0 && flags & STRING_FULL_WORD != 0 {
            let c = at(b, bi);
            if c != 0 && !isspace(c) {
                v = 1;
            }
        }
    }
    v
}

/// `alloc_regex`: the rule's pattern as `regex` rules match it.
fn alloc_regex(ms: &mut Ms, m: &Magic) -> Option<ere::Regex> {
    let pat = m.value.s().to_vec();
    file_regcomp(
        ms,
        &pat,
        RegFlags {
            icase: m.str_flags() & STRING_IGNORE_CASE != 0,
            newline: true,
        },
    )
    .ok()
}

/// `magiccheck`: whether what `mget` read satisfies the rule. 1, 0 or -1.
#[allow(clippy::too_many_lines)]
fn magiccheck(ms: &mut Ms, m: &Magic, rx: Option<&Rx>, s: &[u8]) -> i32 {
    let mut l: u64 = m.value.q();
    let v: u64;
    let p = ms.ms_value;
    match m.typ {
        FILE_BYTE => v = u64::from(p.b()),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            v = u64::from(p.h());
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            v = u64::from(p.l());
        }
        FILE_QUAD | FILE_LEQUAD | FILE_BEQUAD | FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE
        | FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE | FILE_QWDATE | FILE_BEQWDATE
        | FILE_LEQWDATE | FILE_OFFSET => v = p.q(),
        // C's comparisons, exact equality and NaN's unordering included.
        #[allow(clippy::float_cmp)]
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            let fl = m.value.f();
            let fv = p.f();
            return match m.reln {
                b'x' => 1,
                b'!' => i32::from(fl.is_nan() || fv.is_nan() || fv != fl),
                b'=' => i32::from(!(fl.is_nan() || fv.is_nan()) && fv == fl),
                b'>' => i32::from(fv > fl),
                b'<' => i32::from(fv < fl),
                r => {
                    ms.magerror(format!("cannot happen with float: invalid relation `{}'", char::from(r)).as_bytes());
                    -1
                }
            };
        }
        #[allow(clippy::float_cmp)]
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let dl = m.value.d();
            let dv = p.d();
            return match m.reln {
                b'x' => 1,
                b'!' => i32::from(dl.is_nan() || dv.is_nan() || dv != dl),
                b'=' => i32::from(!(dl.is_nan() || dv.is_nan()) && dv == dl),
                b'>' => i32::from(dv > dl),
                b'<' => i32::from(dv < dl),
                r => {
                    ms.magerror(format!("cannot happen with double: invalid relation `{}'", char::from(r)).as_bytes());
                    -1
                }
            };
        }
        FILE_DEFAULT | FILE_CLEAR => {
            l = 0;
            v = 0;
        }
        FILE_STRING | FILE_PSTRING | FILE_OCTAL => {
            l = 0;
            let data = value_with_tail(ms);
            v = file_strncmp(&m.value.0, &data, usize::from(m.vallen), MAXSTRING, m.str_flags());
        }
        FILE_BESTRING16 | FILE_LESTRING16 => {
            l = 0;
            let data = value_with_tail(ms);
            // Upstream compares 16-bit strings with no flags at all.
            v = file_strncmp(&m.value.0, &data, usize::from(m.vallen), MAXSTRING, 0);
        }
        FILE_SEARCH => {
            let Some(start) = ms.search.s else {
                return 0;
            };
            let slen = usize::from(m.vallen).min(MAXSTRING);
            l = 0;
            let region = s.get(start..start + ms.search.s_len).unwrap_or_default();
            if ms.flags & MAGIC_DEBUG != 0 {
                let xlen = ms.search.s_len.min(100);
                let mut w = b"search: [".to_vec();
                crate::print::file_showstr(&mut w, region.get(..xlen).unwrap_or(region), Some(xlen));
                w.extend_from_slice(if ms.search.s_len == xlen { b"] for [" } else { b"...] for [" });
                crate::print::file_showstr(&mut w, &m.value.0, Some(slen));
                debug(&w);
            }
            let pat = m.value.0.get(..slen).unwrap_or_default();
            if slen > 0 && m.str_flags() == 0 {
                // `memmem` within the range, or the whole region.
                let mut idx = m.str_range() as usize + slen;
                if m.str_range() == 0 || ms.search.s_len < idx {
                    idx = ms.search.s_len;
                }
                let hay = region.get(..idx).unwrap_or(region);
                let found = hay.windows(slen).position(|w| w == pat);
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(if found.is_some() { b"] found\n" } else { b"] not found\n" });
                }
                match found {
                    None => v = 1,
                    Some(i) => {
                        ms.search.offset += i;
                        ms.search.rm_len = ms.search.s_len - i;
                        v = 0;
                    }
                }
            } else {
                let mut vv = 0u64;
                let mut idx = 0usize;
                while m.str_range() == 0 || idx < m.str_range() as usize {
                    if slen + idx > ms.search.s_len {
                        vv = 1;
                        break;
                    }
                    let b2 = s.get(start + idx..).unwrap_or_default();
                    vv = file_strncmp(&m.value.0, b2, slen, ms.search.s_len - idx, m.str_flags());
                    if vv == 0 {
                        ms.search.offset += idx;
                        ms.search.rm_len = ms.search.s_len - idx;
                        break;
                    }
                    idx += 1;
                }
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(if vv == 0 { b"] found\n" } else { b"] not found\n" });
                }
                v = vv;
            }
        }
        FILE_REGEX => {
            let Some(start) = ms.search.s else {
                return 0;
            };
            let cached = rx.and_then(|c| c.get().cloned());
            let re = match cached {
                Some(r) => r,
                None => {
                    let Some(r) = alloc_regex(ms, m) else {
                        return -1;
                    };
                    let r = Rc::new(r);
                    if let Some(c) = rx {
                        let _set = c.set(Rc::clone(&r));
                    }
                    r
                }
            };
            l = 0;
            // A copy of the region less its last byte -- upstream's NUL goes
            // there -- searched as a C string.
            let slen = ms.search.s_len;
            let subject: &[u8] = if slen != 0 {
                s.get(start..start + slen - 1).unwrap_or_default()
            } else {
                b""
            };
            match file_regexec(&re, subject) {
                Ok(Some((so, eo))) => {
                    ms.search.s = Some(start + so);
                    ms.search.offset += so;
                    ms.search.rm_len = eo - so;
                    v = 0;
                }
                Ok(None) => v = 1,
                Err(_) => return -1,
            }
        }
        FILE_USE => return i32::from(ms.ms_value.q() != 0),
        FILE_NAME | FILE_INDIRECT => return 1,
        FILE_DER => {
            let matched = crate::der::der_cmp(ms, m, s);
            if matched == -1 {
                if ms.flags & MAGIC_DEBUG != 0 {
                    debug(b"EOF comparing DER entries\n");
                }
                return 0;
            }
            return matched;
        }
        FILE_GUID => {
            // `memcmp`: glibc's gives the difference of the first bytes that
            // differ, which only its sign and its being zero can matter to.
            l = 0;
            #[allow(clippy::cast_sign_loss)]
            {
                v = m.value.0[..16]
                    .iter()
                    .zip(&p.0[..16])
                    .find(|(a, b)| a != b)
                    .map_or(0, |(&a, &b)| (i64::from(a) - i64::from(b)) as u64);
            }
        }
        _ => {
            ms.magerror(format!("invalid type {} in magiccheck()", m.typ).as_bytes());
            return -1;
        }
    }

    let v = file_signextend(ms, m, v);
    #[allow(clippy::cast_possible_wrap)]
    let matched = match m.reln {
        b'x' => true,
        b'!' => v != l,
        b'=' => v == l,
        b'>' => {
            if m.flag & UNSIGNED != 0 {
                v > l
            } else {
                (v as i64) > (l as i64)
            }
        }
        b'<' => {
            if m.flag & UNSIGNED != 0 {
                v < l
            } else {
                (v as i64) < (l as i64)
            }
        }
        b'&' => v & l == l,
        b'^' => v & l != l,
        r => {
            ms.magerror(format!("cannot happen: invalid relation `{}'", char::from(r)).as_bytes());
            return -1;
        }
    };
    if ms.flags & MAGIC_DEBUG != 0 {
        debug(format!(" strength={}\n", crate::apprentice::file_magic_strength(m)).as_bytes());
    }
    i32::from(matched)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn strncmp_ignores_case_and_compacts_white_space_as_asked() {
        assert_eq!(file_strncmp(b"abc", b"abc", 3, 128, 0), 0);
        assert_ne!(file_strncmp(b"abc", b"abd", 3, 128, 0), 0);
        assert_eq!(file_strncmp(b"abc", b"ABC", 3, 128, STRING_IGNORE_LOWERCASE), 0);
        assert_eq!(file_strncmp(b"a b", b"a     b", 3, 128, STRING_COMPACT_WHITESPACE), 0);
        assert_ne!(file_strncmp(b"a b", b"ab", 3, 128, STRING_COMPACT_WHITESPACE), 0);
        assert_eq!(file_strncmp(b"a b", b"ab", 3, 128, STRING_COMPACT_OPTIONAL_WHITESPACE), 0);
        assert_eq!(file_strncmp(b"ab", b"ab cd", 2, 128, STRING_FULL_WORD), 0);
        assert_ne!(file_strncmp(b"ab", b"abcd", 2, 128, STRING_FULL_WORD), 0);
        // A difference is C's: the bytes subtracted as ints.
        assert_eq!(file_strncmp(b"b", b"a", 1, 128, 0), u64::MAX);
    }

    #[test]
    fn check_fmt_finds_a_string_conversion() {
        assert!(check_fmt(b"version %s"));
        assert!(check_fmt(b"%-10.3s"));
        assert!(!check_fmt(b"version %d"));
        assert!(!check_fmt(b"100% sure"));
        assert!(!check_fmt(b"no format"));
    }

    #[test]
    fn indirect_offsets_overflow_as_upstream_refuses() {
        let ms = Ms::new(0);
        let mut m = Magic::default();
        m.in_op = FILE_OPADD;
        assert_eq!(do_ops(&ms, &m, 10, 5), Ok(15));
        assert_eq!(do_ops(&ms, &m, i64::from(u32::MAX), 1), Err(()));
        assert_eq!(do_ops(&ms, &m, 10, -20), Ok(0xffff_fff6));
        assert!(oob(10, 8, 4));
        assert!(!oob(10, 6, 4));
        assert!(oob(10, -1, 1));
    }

    #[test]
    fn varexpand_reads_the_execute_bits() {
        let mut ms = Ms::new(0);
        ms.mode = 0o755;
        assert_eq!(varexpand(&ms, 512, b"${x?pie executable:shared object}").unwrap(), b"pie executable");
        ms.mode = 0o644;
        assert_eq!(varexpand(&ms, 512, b"a ${x?yes:no} b").unwrap(), b"a no b");
        assert_eq!(varexpand(&ms, 512, b"${y?a:b}"), None);
    }
}
