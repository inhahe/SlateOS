//! `infocmp`: ncurses 6.4's (`progs/infocmp.c`, 20240113), ported.
//!
//! Prints a terminal's compiled description back as source -- terminfo
//! (`-I`, `-L` for long names) or termcap (`-C`, `-K`) -- or compares two
//! or more: the capabilities that differ (`-d`), that they share (`-c`),
//! that neither has (`-n`), or the first rewritten to `use=` the others
//! (`-u`). `-F` compares two source files entry by entry; `-e` and `-E`
//! print C initializers; `-i` analyzes the init and reset strings.
//!
//! The work is `terminfo::compile`'s: the reader, the dumper
//! (`dump_entry.c`), the compiler for `-F`. This is `infocmp.c`'s `main`
//! and what it prints itself.
//!
//! Deliberately different: `-V` names SlateOS's coreutils rather than the
//! ncurses version (design-decisions §370).

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program};
use coreutils::ncurses as nc;
use coreutils::quote::os_bytes;
use coreutils::stdfd;
use terminfo::Kind;
use terminfo::compile::comp_parse::{Source, entry_match};
use terminfo::compile::dump::{
    self, Dumper, F_TCONVERR, F_TERMCAP, F_TERMINFO, F_VARIABLE, FAIL, S_DEFAULT, S_NOSORT,
    S_TERMCAP, S_TERMINFO, S_VARIABLE, repair_acsc,
};
use terminfo::compile::entry::{Entry, capcmp as nc_capcmp};
use terminfo::compile::expand::tic_expand;
use terminfo::compile::parse::Compiler;
use terminfo::compile::scan::{Scanner, cstr};
use terminfo::compile::tables::first_name;
use terminfo::compile::{Abort, Diagnostics};
use terminfo::names;
use terminfo::termtype::{ABSENT_BOOLEAN, CANCELLED_BOOLEAN, Str, TermType};
use terminfo::{BOOLCOUNT, NUMCOUNT, STRCOUNT};

coreutils::guard_std_fds!();

/// The parser. Its complaints name `argv[0]`, as glibc's `getopt` does, and
/// `usage` follows them.
const INFOCMP: Program = Program::new("infocmp", 1);

/// `MAX_STRING`: the longest string a comparison shows.
const MAX_STRING: usize = 1024;
/// `NAMESIZE`.
const NAMESIZE: usize = 256;
/// `BOOLWRITE`, `NUMWRITE`, `STRWRITE`: where the obsolete capabilities
/// begin, which comparisons skip without `-x`.
const BOOLWRITE: usize = 37;
const NUMWRITE: usize = 33;
const STRWRITE: usize = 394;
/// `PATH_MAX`.
const PATH_MAX: usize = 4096;
/// `MAX_TERMINFO_LENGTH`.
const MAX_TERMINFO_LENGTH: usize = 4096;
/// `acs_chars_index`.
const ACS_CHARS_INDEX: usize = 146;

/// The comparison modes.
const C_DEFAULT: i32 = 0;
const C_DIFFERENCE: i32 = 1;
const C_COMMON: i32 = 2;
const C_NAND: i32 = 3;
const C_USEALL: i32 = 4;

/// Standard error, unbuffered, as upstream's is.
struct Stderr;

impl Diagnostics for Stderr {
    fn emit(&mut self, bytes: &[u8]) {
        ulclosestream::stderr_write(bytes);
    }
}

/// A file name's bytes as the `OsString` the platform opens.
#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

/// A file name's bytes, as near as a host without byte paths comes.
#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// What ends the program early, its message written: the status.
struct Exit(u8);

impl From<Abort> for Exit {
    fn from(_: Abort) -> Self {
        Exit(1)
    }
}

/// The program's settings: `infocmp.c`'s statics.
struct Infocmp {
    progname: Vec<u8>,
    limited: bool,
    quiet: bool,
    literal: bool,
    bool_sep: &'static [u8],
    s_absent: &'static [u8],
    s_cancel: &'static [u8],
    itrace: u32,
    numbers: i32,
    outform: i32,
    compare: i32,
    ignorepads: bool,
    user_definable: bool,
}

impl Infocmp {
    /// `capcmp (idx, s, t)`: whether two strings differ -- ignoring padding
    /// with `-p`, but never in `acsc`.
    fn capcmp(&self, idx: usize, s: &Str, t: &Str) -> bool {
        match (s.valid(), t.valid()) {
            (None, None) => s != t,
            (None, _) | (_, None) => true,
            (Some(a), Some(b)) => {
                if idx == ACS_CHARS_INDEX || !self.ignorepads {
                    cstr(a) != cstr(b)
                } else {
                    nc_capcmp(Some(a), Some(b)) != 0
                }
            }
        }
    }

    /// `no_boolean`, `no_numeric`, `no_string`: a value shown as missing.
    fn same_marks(&self) -> bool {
        self.s_absent == self.s_cancel
    }

    fn no_boolean(&self, v: i8) -> bool {
        if self.same_marks() {
            !(v == 0 || v == 1)
        } else {
            v == ABSENT_BOOLEAN
        }
    }

    fn no_numeric(&self, v: i32) -> bool {
        if self.same_marks() { v < 0 } else { v == -1 }
    }

    fn no_string(&self, v: &Str) -> bool {
        if self.same_marks() {
            v.valid().is_none()
        } else {
            *v == Str::Absent
        }
    }

    /// `dump_boolean (val)`.
    fn dump_boolean(&self, v: i8) -> Vec<u8> {
        match v {
            ABSENT_BOOLEAN => self.s_absent.to_vec(),
            CANCELLED_BOOLEAN => self.s_cancel.to_vec(),
            0 => b"F".to_vec(),
            1 => b"T".to_vec(),
            _ => b"?".to_vec(),
        }
    }

    /// `dump_numeric (val, buf)`.
    fn dump_numeric(&self, v: i32) -> Vec<u8> {
        match v {
            -1 => self.s_absent.to_vec(),
            -2 => self.s_cancel.to_vec(),
            _ => v.to_string().into_bytes(),
        }
    }

    /// `TIC_EXPAND (s)`.
    fn tic_expand(&self, s: &[u8]) -> Vec<u8> {
        tic_expand(Some(s), self.outform == F_TERMINFO, self.numbers)
    }

    /// `dump_string (val, buf)`: `'%.*s'`, the value at most 1021 bytes.
    fn dump_string(&self, v: &Str) -> Vec<u8> {
        match v {
            Str::Absent => self.s_absent.to_vec(),
            Str::Cancelled => self.s_cancel.to_vec(),
            Str::Value(s) => {
                let e = self.tic_expand(s);
                let mut out = b"'".to_vec();
                out.extend_from_slice(e.get(..e.len().min(MAX_STRING - 3)).unwrap_or(&e));
                out.push(b'\'');
                out.truncate(MAX_STRING - 1);
                out
            }
        }
    }

    /// `use_predicate (type, idx)`: what `-u` shows of the first entry
    /// against those it is to `use=`.
    fn use_predicate(&self, entries: &[Entry], t0: &TermType, kind: Kind, idx: usize) -> i32 {
        match kind {
            Kind::Boolean => {
                if idx >= t0.booleans.len() {
                    return FAIL;
                }
                let mut is_set: i32 = 0;
                for ep in entries.iter().skip(1) {
                    if idx < ep.tterm.booleans.len() {
                        is_set = i32::from(ep.tterm.booleans.get(idx).copied().unwrap_or(0));
                        if is_set != 0 {
                            break;
                        }
                    }
                }
                if is_set != i32::from(t0.booleans.get(idx).copied().unwrap_or(0)) {
                    i32::from(is_set == 0)
                } else {
                    FAIL
                }
            }
            Kind::Number => {
                if idx >= t0.numbers.len() {
                    return FAIL;
                }
                let mut value = -1;
                for ep in entries.iter().skip(1) {
                    if let Some(&v) = ep.tterm.numbers.get(idx)
                        && v >= 0
                    {
                        value = v;
                        break;
                    }
                }
                if value != t0.numbers.get(idx).copied().unwrap_or(-1) {
                    i32::from(value != -1)
                } else {
                    FAIL
                }
            }
            Kind::String => {
                let termstr = t0.strings.get(idx).cloned().unwrap_or_default();
                if idx >= t0.strings.len() {
                    return FAIL;
                }
                let mut usestr = Str::Absent;
                for ep in entries.iter().skip(1) {
                    if let Some(v) = ep.tterm.strings.get(idx)
                        && *v != Str::Absent
                    {
                        usestr = v.clone();
                        break;
                    }
                }
                if usestr == Str::Cancelled && termstr == Str::Absent {
                    FAIL
                } else if usestr == Str::Cancelled && termstr == Str::Cancelled {
                    1
                } else if usestr == Str::Absent && termstr == Str::Absent {
                    FAIL
                } else if usestr == Str::Absent
                    || termstr == Str::Absent
                    || self.capcmp(idx, &usestr, &termstr)
                {
                    1
                } else {
                    FAIL
                }
            }
        }
    }
}

/// `useeq (e1, e2)`: whether two entries name the same `use=` clauses.
fn useeq(e1: &Entry, e2: &Entry) -> bool {
    if e1.nuses != e2.nuses {
        return false;
    }
    e1.uses.iter().take(e1.nuses).all(|u1| {
        e2.uses.iter().take(e2.nuses).any(|u2| {
            cstr(u1.name.as_deref().unwrap_or_default())
                == cstr(u2.name.as_deref().unwrap_or_default())
        })
    })
}

/// `entryeq (t1, t2)`: whether two (aligned) descriptions are the same.
fn entryeq(ic: &Infocmp, t1: &TermType, t2: &TermType) -> bool {
    (0..t1.booleans.len()).all(|i| t1.booleans.get(i) == t2.booleans.get(i))
        && (0..t1.numbers.len()).all(|i| t1.numbers.get(i) == t2.numbers.get(i))
        && (0..t1.strings.len()).all(|i| {
            !ic.capcmp(
                i,
                t1.strings.get(i).unwrap_or(&Str::Absent),
                t2.strings.get(i).unwrap_or(&Str::Absent),
            )
        })
}

/// `print_uses (ep, fp)`.
fn print_uses(ep: &Entry) -> Vec<u8> {
    if ep.nuses == 0 {
        return b"NULL".to_vec();
    }
    let names: Vec<&[u8]> = ep
        .uses
        .iter()
        .take(ep.nuses)
        .map(|u| cstr(u.name.as_deref().unwrap_or_default()))
        .collect();
    names.join(&b' ')
}

/// `canonical_name (source, target)`: the primary name, at most
/// `NAMESIZE - 1` bytes.
fn canonical_name(source: &[u8]) -> Vec<u8> {
    source
        .iter()
        .take(NAMESIZE - 1)
        .take_while(|&&c| c != b'|' && c != 0)
        .copied()
        .collect()
}

/// `compare_predicate (type, idx, name)`: one capability of the first entry
/// against the others', printed as the mode says.
#[allow(clippy::too_many_lines, reason = "upstream's compare_predicate")]
fn compare_predicate(
    ic: &Infocmp,
    entries: &[Entry],
    out: &mut Vec<u8>,
    kind: Option<Kind>,
    idx: usize,
    name: &[u8],
) {
    let Some(e1) = entries.first() else {
        return;
    };
    let others = entries.get(1..).unwrap_or_default();
    let line = |out: &mut Vec<u8>, parts: &[&[u8]]| {
        for p in parts {
            out.extend_from_slice(p);
        }
    };
    match kind {
        Some(Kind::Boolean) => {
            if !ic.user_definable && idx > BOOLWRITE {
                return;
            }
            let b1 = e1.tterm.booleans.get(idx).copied().unwrap_or(0);
            match ic.compare {
                C_DIFFERENCE => {
                    let b2 = others
                        .first()
                        .and_then(|e| e.tterm.booleans.get(idx))
                        .copied()
                        .unwrap_or(0);
                    if !(ic.no_boolean(b1) && ic.no_boolean(b2)) && b1 != b2 {
                        line(
                            out,
                            &[
                                b"\t",
                                name,
                                b": ",
                                &ic.dump_boolean(b1),
                                ic.bool_sep,
                                &ic.dump_boolean(b2),
                                b".\n",
                            ],
                        );
                    }
                }
                C_COMMON => {
                    if b1 != ABSENT_BOOLEAN
                        && others
                            .iter()
                            .all(|e| e.tterm.booleans.get(idx).copied().unwrap_or(0) == b1)
                    {
                        line(out, &[b"\t", name, b"= ", &ic.dump_boolean(b1), b".\n"]);
                    }
                }
                C_NAND => {
                    if b1 == ABSENT_BOOLEAN
                        && others
                            .iter()
                            .all(|e| e.tterm.booleans.get(idx).copied().unwrap_or(0) == b1)
                    {
                        line(out, &[b"\t!", name, b".\n"]);
                    }
                }
                _ => {}
            }
        }
        Some(Kind::Number) => {
            if !ic.user_definable && idx > NUMWRITE {
                return;
            }
            let n1 = e1.tterm.numbers.get(idx).copied().unwrap_or(-1);
            match ic.compare {
                C_DIFFERENCE => {
                    let n2 = others
                        .first()
                        .and_then(|e| e.tterm.numbers.get(idx))
                        .copied()
                        .unwrap_or(-1);
                    if !(ic.no_numeric(n1) && ic.no_numeric(n2)) && n1 != n2 {
                        line(
                            out,
                            &[
                                b"\t",
                                name,
                                b": ",
                                &ic.dump_numeric(n1),
                                b", ",
                                &ic.dump_numeric(n2),
                                b".\n",
                            ],
                        );
                    }
                }
                C_COMMON => {
                    if n1 != -1
                        && others
                            .iter()
                            .all(|e| e.tterm.numbers.get(idx).copied().unwrap_or(-1) == n1)
                    {
                        line(out, &[b"\t", name, b"= ", &ic.dump_numeric(n1), b".\n"]);
                    }
                }
                C_NAND => {
                    if n1 == -1
                        && others
                            .iter()
                            .all(|e| e.tterm.numbers.get(idx).copied().unwrap_or(-1) == n1)
                    {
                        line(out, &[b"\t!", name, b".\n"]);
                    }
                }
                _ => {}
            }
        }
        Some(Kind::String) => {
            if !ic.user_definable && idx > STRWRITE {
                return;
            }
            let s1 = e1.tterm.strings.get(idx).cloned().unwrap_or_default();
            match ic.compare {
                C_DIFFERENCE => {
                    let s2 = others
                        .first()
                        .and_then(|e| e.tterm.strings.get(idx))
                        .cloned()
                        .unwrap_or_default();
                    if !(ic.no_string(&s1) && ic.no_string(&s2)) && ic.capcmp(idx, &s1, &s2) {
                        let buf1 = ic.dump_string(&s1);
                        let buf2 = ic.dump_string(&s2);
                        if buf1 != buf2 {
                            line(out, &[b"\t", name, b": ", &buf1, b", ", &buf2, b".\n"]);
                        }
                    }
                }
                C_COMMON => {
                    if s1 != Str::Absent
                        && others.iter().all(|e| {
                            !ic.capcmp(idx, &s1, e.tterm.strings.get(idx).unwrap_or(&Str::Absent))
                        })
                    {
                        let shown = ic.tic_expand(s1.valid().unwrap_or_default());
                        line(out, &[b"\t", name, b"= '", &shown, b"'.\n"]);
                    }
                }
                C_NAND => {
                    if s1 == Str::Absent
                        && others
                            .iter()
                            .all(|e| e.tterm.strings.get(idx).is_none_or(|v| *v == s1))
                    {
                        line(out, &[b"\t!", name, b".\n"]);
                    }
                }
                _ => {}
            }
        }
        None => match ic.compare {
            // "unlike the other modes, this compares *all* use entries"
            C_DIFFERENCE => {
                if let Some(e2) = others.first()
                    && !useeq(e1, e2)
                {
                    line(
                        out,
                        &[b"\tuse: ", &print_uses(e1), b", ", &print_uses(e2), b".\n"],
                    );
                }
            }
            C_COMMON => {
                if e1.nuses != 0
                    && others
                        .iter()
                        .all(|e2| e2.nuses == e1.nuses && useeq(e1, e2))
                {
                    line(out, &[b"\tuse: ", &print_uses(e1), b".\n"]);
                }
            }
            C_NAND => {
                if e1.nuses == 0 && others.iter().all(|e2| e2.nuses == e1.nuses) {
                    out.extend_from_slice(b"\t!use.\n");
                }
            }
            _ => {}
        },
    }
}

// ---- Init string analysis -------------------------------------------------

/// `std_caps`: sequences X.364, iBCS2, ISO 2022 and DEC define.
const STD_CAPS: &[(&[u8], &str)] = &[
    (b"\x1bc", "RIS"),
    (b"\x1b7", "SC"),
    (b"\x1b8", "RC"),
    (b"\x1b[r", "RSR"),
    (b"\x1b[m", "SGR0"),
    (b"\x1b[2J", "ED2"),
    (b"\x1b(0", "ISO DEC G0"),
    (b"\x1b(A", "ISO UK G0"),
    (b"\x1b(B", "ISO US G0"),
    (b"\x1b)0", "ISO DEC G1"),
    (b"\x1b)A", "ISO UK G1"),
    (b"\x1b)B", "ISO US G1"),
    (b"\x1b=", "DECPAM"),
    (b"\x1b>", "DECPNM"),
    (b"\x1b<", "DECANSI"),
    (b"\x1b[!p", "DECSTR"),
    (b"\x1b F", "S7C1T"),
];

/// `std_modes`: ECMA modes.
const STD_MODES: &[(&str, &str)] = &[("2", "AM"), ("4", "IRM"), ("12", "SRM"), ("20", "LNM")];

/// `private_modes`: DEC modes.
const PRIVATE_MODES: &[(&str, &str)] = &[
    ("1", "CKM"),
    ("2", "ANM"),
    ("3", "COLM"),
    ("4", "SCLM"),
    ("5", "SCNM"),
    ("6", "OM"),
    ("7", "AWM"),
    ("8", "ARM"),
];

/// `ecma_highlights`: SGR parameters.
const ECMA_HIGHLIGHTS: &[(&str, &str)] = &[
    ("0", "NORMAL"),
    ("1", "+BOLD"),
    ("2", "+DIM"),
    ("3", "+ITALIC"),
    ("4", "+UNDERLINE"),
    ("5", "+BLINK"),
    ("6", "+FASTBLINK"),
    ("7", "+REVERSE"),
    ("8", "+INVISIBLE"),
    ("9", "+DELETED"),
    ("10", "MAIN-FONT"),
    ("11", "ALT-FONT-1"),
    ("12", "ALT-FONT-2"),
    ("13", "ALT-FONT-3"),
    ("14", "ALT-FONT-4"),
    ("15", "ALT-FONT-5"),
    ("16", "ALT-FONT-6"),
    ("17", "ALT-FONT-7"),
    ("18", "ALT-FONT-1"),
    ("19", "ALT-FONT-1"),
    ("20", "FRAKTUR"),
    ("21", "DOUBLEUNDER"),
    ("22", "-DIM"),
    ("23", "-ITALIC"),
    ("24", "-UNDERLINE"),
    ("25", "-BLINK"),
    ("26", "-FASTBLINK"),
    ("27", "-REVERSE"),
    ("28", "-INVISIBLE"),
    ("29", "-DELETED"),
];

/// The byte at `i`, NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `skip_csi (cap)`: the length of a CSI at the start.
fn skip_csi(cap: &[u8]) -> usize {
    if at(cap, 0) == 0x1b && at(cap, 1) == b'[' {
        2
    } else if at(cap, 0) == 0o233 {
        1
    } else {
        0
    }
}

/// `strspn (s, "0123456789;")`.
fn param_span(s: &[u8]) -> usize {
    s.iter()
        .take_while(|c| c.is_ascii_digit() || **c == b';')
        .count()
}

/// `lookup_params (table, dst, src)`: each `;`-separated parameter (empty
/// ones skipped, as `strtok` skips them) named from the table, or kept.
fn lookup_params(table: &[(&str, &str)], head: &[u8], src: &[u8]) -> Option<Vec<u8>> {
    let tokens: Vec<&[u8]> = src
        .split(|&c| c == b';')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut dst = head.to_vec();
    for ep in tokens {
        let found = table.iter().find(|(from, _)| {
            let f = from.as_bytes();
            ep.starts_with(f) && !at(ep, f.len()).is_ascii_digit()
        });
        match found {
            Some((_, to)) => dst.extend_from_slice(to.as_bytes()),
            None => dst.extend_from_slice(ep),
        }
        dst.push(b';');
    }
    dst.pop();
    Some(dst)
}

/// `analyze_string (name, cap, tp)`: an init string as the capabilities
/// and standard sequences it is made of.
#[allow(clippy::too_many_lines, reason = "upstream's analyze_string")]
fn analyze_string(ic: &Infocmp, out: &mut Vec<u8>, name: &str, cap_index: usize, tp: &TermType) {
    let Some(cap) = tp.strings.get(cap_index).and_then(Str::valid) else {
        return;
    };
    let cap = cstr(cap).to_vec();
    let tp_lines = tp.numbers.get(2).copied().unwrap_or(-1);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b": ");
    let isrs = |n: &[u8]| n.starts_with(b"is") || n.starts_with(b"rs");

    let mut k = 0usize;
    while k < cap.len() {
        let sp = cap.get(k..).unwrap_or_default();
        let mut len = 0usize;
        let mut expansion: Option<Vec<u8>> = None;

        // "first, check other capabilities in this entry"
        for i in 0..STRCOUNT {
            let Some(sname) = names::STRNAMES.get(i) else {
                continue;
            };
            let sname = sname.as_bytes();
            if sname.starts_with(b"kf") {
                continue;
            }
            let Some(cp) = tp.strings.get(i).and_then(Str::valid) else {
                continue;
            };
            let cp = cstr(cp);
            if cp.is_empty() || i == cap_index {
                continue;
            }
            len = cp.len();
            let buf2 = sp.get(..len.min(sp.len())).unwrap_or(sp);
            if nc_capcmp(Some(cp), Some(buf2)) != 0 {
                continue;
            }
            // "identical pairs of initialization and reset strings don't
            // just refer to each other" -- by where each lies in the table,
            // which a compiled entry has in index order.
            if (isrs(name.as_bytes()) || isrs(sname)) && cap_index < i {
                continue;
            }
            expansion = Some(sname.to_vec());
            break;
        }

        // "now check the standard capabilities"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            for (from, to) in STD_CAPS {
                // Upstream's table holds `from` in four bytes: a sequence of
                // four (`\E[2J`, `\E[!p`) has no NUL there, and `strlen`
                // runs on into `to` -- measured: those two never match.
                let from: Vec<u8> = if from.len() >= 4 {
                    [*from, to.as_bytes()].concat()
                } else {
                    from.to_vec()
                };
                let adj = if csi != 0 { 2 } else { 0 };
                len = from.len();
                if csi != 0 && skip_csi(&from) != csi {
                    continue;
                }
                if len > adj
                    && sp
                        .get(csi..)
                        .is_some_and(|r| r.starts_with(from.get(adj..).unwrap_or_default()))
                {
                    expansion = Some(to.as_bytes().to_vec());
                    len = len.wrapping_sub(adj).wrapping_add(csi);
                    break;
                }
            }
        }

        // "now check for standard-mode sequences"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 {
                len = param_span(sp.get(csi..).unwrap_or_default());
                let next = csi.wrapping_add(len);
                if len != 0
                    && len < MAX_TERMINFO_LENGTH
                    && (at(sp, next) == b'h' || at(sp, next) == b'l')
                {
                    let head: &[u8] = if at(sp, next) == b'h' {
                        b"ECMA+"
                    } else {
                        b"ECMA-"
                    };
                    expansion =
                        lookup_params(STD_MODES, head, sp.get(csi..next).unwrap_or_default());
                }
            }
        }

        // "now check for private-mode sequences"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 && at(sp, csi) == b'?' {
                len = param_span(sp.get(csi.wrapping_add(1)..).unwrap_or_default());
                let next = csi.wrapping_add(1).wrapping_add(len);
                if len != 0
                    && len < MAX_TERMINFO_LENGTH
                    && (at(sp, next) == b'h' || at(sp, next) == b'l')
                {
                    let head: &[u8] = if at(sp, next) == b'h' {
                        b"DEC+"
                    } else {
                        b"DEC-"
                    };
                    expansion = lookup_params(
                        PRIVATE_MODES,
                        head,
                        sp.get(csi.wrapping_add(1)..next).unwrap_or_default(),
                    );
                }
            }
        }

        // "now check for ECMA highlight sequences"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 {
                let span = param_span(sp.get(csi..).unwrap_or_default());
                if span != 0 {
                    len = span;
                    let next = csi.wrapping_add(len);
                    if len < MAX_TERMINFO_LENGTH && at(sp, next) == b'm' {
                        len = len.wrapping_add(csi.wrapping_add(1));
                        expansion = lookup_params(
                            ECMA_HIGHLIGHTS,
                            b"SGR:",
                            sp.get(csi..next).unwrap_or_default(),
                        );
                    }
                }
            }
        }

        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 && at(sp, csi) == b'm' {
                len = csi.wrapping_add(1);
                expansion = Some(b"SGR:NORMAL".to_vec());
            }
        }

        // "now check for scroll region reset"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 {
                if at(sp, csi) == b'r' {
                    expansion = Some(b"RSR".to_vec());
                    len = 1;
                } else {
                    let buf2 = format!("1;{tp_lines}r").into_bytes();
                    len = buf2.len();
                    if sp.get(csi..).is_some_and(|r| r.starts_with(&buf2)) {
                        expansion = Some(b"RSR".to_vec());
                    }
                }
                len = len.wrapping_add(csi);
            }
        }

        // "now check for home-down"
        if expansion.is_none() {
            let csi = skip_csi(sp);
            if csi != 0 {
                let buf2 = format!("{tp_lines};1H").into_bytes();
                len = buf2.len();
                if sp.get(csi..).is_some_and(|r| r.starts_with(&buf2)) {
                    expansion = Some(b"LL".to_vec());
                } else {
                    let buf2 = format!("{tp_lines}H").into_bytes();
                    len = buf2.len();
                    if sp.get(csi..).is_some_and(|r| r.starts_with(&buf2)) {
                        expansion = Some(b"LL".to_vec());
                    }
                }
                len = len.wrapping_add(csi);
            }
        }

        // "now look at the expansion we got, if any"
        match expansion {
            Some(e) => {
                out.push(b'{');
                out.extend_from_slice(&e);
                out.push(b'}');
                k = k.wrapping_add(len.max(1));
            }
            None => {
                // "couldn't match anything"
                out.extend_from_slice(&ic.tic_expand(&[at(sp, 0)]));
                k = k.wrapping_add(1);
            }
        }
    }
    out.push(b'\n');
}

// ---- C initializers --------------------------------------------------------

/// `any_initializer (fmt, type)`: the first name, its non-alphanumerics `_`,
/// and the suffix.
fn any_initializer(names: &[u8], suffix: &str) -> Vec<u8> {
    let mut v: Vec<u8> = names
        .iter()
        .take_while(|&&c| c != b'|' && c != 0)
        .map(|&c| if c.is_ascii_alphanumeric() { c } else { b'_' })
        .collect();
    v.extend_from_slice(suffix.as_bytes());
    v
}

/// The name an extended capability or a standard one has in the C tables.
fn ext_name(t: &TermType, kind: Kind, i: usize) -> Vec<u8> {
    let (count, base, table): (usize, usize, &[&str]) = match kind {
        Kind::Boolean => (BOOLCOUNT, 0, &names::BOOLNAMES),
        Kind::Number => (NUMCOUNT, t.ext_booleans, &names::NUMNAMES),
        Kind::String => (
            STRCOUNT,
            t.ext_booleans.saturating_add(t.ext_numbers),
            &names::STRNAMES,
        ),
    };
    if i >= count {
        t.ext_name(i.wrapping_sub(count).wrapping_add(base))
            .to_vec()
    } else {
        table
            .get(i)
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default()
    }
}

/// `dump_initializers (term)`: the entry as C data.
fn dump_initializers(out: &mut Vec<u8>, names0: &[u8], term: &TermType) {
    let mut p = |s: &[u8]| out.extend_from_slice(s);
    p(b"\nstatic char ");
    p(&any_initializer(names0, "_alias_data"));
    p(b"[] = \"");
    p(cstr(names0));
    p(b"\";\n\n");
    for (n, v) in term.strings.iter().enumerate() {
        if let Some(v) = v.valid() {
            let mut buf = b"\"".to_vec();
            for &c in cstr(v) {
                if (MAX_STRING - 5).saturating_sub(buf.len()) <= 2 {
                    break;
                }
                if (0x20..0x7f).contains(&c) && c != b'\\' && c != b'"' {
                    buf.push(c);
                } else {
                    buf.extend_from_slice(format!("\\{c:03o}").as_bytes());
                }
            }
            buf.push(b'"');
            let var = any_initializer(
                names0,
                &format!(
                    "_s_{}",
                    String::from_utf8_lossy(&ext_name(term, Kind::String, n))
                ),
            );
            p(format!("static char {:<20}[] = ", String::from_utf8_lossy(&var)).as_bytes());
            p(&buf);
            p(b";\n");
        }
    }
    p(b"\n");
    p(b"static char ");
    p(&any_initializer(names0, "_bool_data"));
    p(b"[] = {\n");
    for (n, &v) in term.booleans.iter().enumerate() {
        let str_ = match v {
            1 => "TRUE",
            0 => "FALSE",
            ABSENT_BOOLEAN => "ABSENT_BOOLEAN",
            CANCELLED_BOOLEAN => "CANCELLED_BOOLEAN",
            _ => "",
        };
        p(format!(
            "\t/* {n:3}: {:<8} */\t{str_},\n",
            String::from_utf8_lossy(&ext_name(term, Kind::Boolean, n))
        )
        .as_bytes());
    }
    p(b"};\n");
    p(b"static short ");
    p(&any_initializer(names0, "_number_data"));
    p(b"[] = {\n");
    for (n, &v) in term.numbers.iter().enumerate() {
        let str_ = match v {
            -1 => "ABSENT_NUMERIC".to_owned(),
            -2 => "CANCELLED_NUMERIC".to_owned(),
            _ => v.to_string(),
        };
        p(format!(
            "\t/* {n:3}: {:<8} */\t{str_},\n",
            String::from_utf8_lossy(&ext_name(term, Kind::Number, n))
        )
        .as_bytes());
    }
    p(b"};\n");
    p(b"static char * ");
    p(&any_initializer(names0, "_string_data"));
    p(b"[] = {\n");
    for (n, v) in term.strings.iter().enumerate() {
        let sname = ext_name(term, Kind::String, n);
        let str_ = match v {
            Str::Absent => b"ABSENT_STRING".to_vec(),
            Str::Cancelled => b"CANCELLED_STRING".to_vec(),
            Str::Value(_) => {
                any_initializer(names0, &format!("_s_{}", String::from_utf8_lossy(&sname)))
            }
        };
        p(format!(
            "\t/* {n:3}: {:<8} */\t{},\n",
            String::from_utf8_lossy(&sname),
            String::from_utf8_lossy(&str_)
        )
        .as_bytes());
    }
    p(b"};\n");
    if term.booleans.len() != BOOLCOUNT
        || term.numbers.len() != NUMCOUNT
        || term.strings.len() != STRCOUNT
    {
        p(b"static char * ");
        p(&any_initializer(names0, "_string_ext_data"));
        p(b"[] = {\n");
        for n in BOOLCOUNT..term.booleans.len() {
            p(format!(
                "\t/* {n:3}: bool */\t\"{}\",\n",
                String::from_utf8_lossy(&ext_name(term, Kind::Boolean, n))
            )
            .as_bytes());
        }
        for n in NUMCOUNT..term.numbers.len() {
            p(format!(
                "\t/* {n:3}: num */\t\"{}\",\n",
                String::from_utf8_lossy(&ext_name(term, Kind::Number, n))
            )
            .as_bytes());
        }
        for n in STRCOUNT..term.strings.len() {
            p(format!(
                "\t/* {n:3}: str */\t\"{}\",\n",
                String::from_utf8_lossy(&ext_name(term, Kind::String, n))
            )
            .as_bytes());
        }
        p(b"};\n");
    }
}

/// `dump_termtype (term)`: the `TERMTYPE` initializer.
fn dump_termtype(out: &mut Vec<u8>, names0: &[u8], term: &TermType) {
    let name = |s: &str| String::from_utf8_lossy(&any_initializer(names0, s)).into_owned();
    let mut p = |s: String| out.extend_from_slice(s.as_bytes());
    p(format!("\t{{\n\t\t{},\n", name("_alias_data")));
    p("\t\t(char *)0,\t/* pointer to string table */\n".to_owned());
    p(format!("\t\t{},\n", name("_bool_data")));
    p(format!("\t\t{},\n", name("_number_data")));
    p(format!("\t\t{},\n", name("_string_data")));
    p("#if NCURSES_XNAMES\n".to_owned());
    p("\t\t(char *)0,\t/* pointer to extended string table */\n".to_owned());
    let ext = term.booleans.len() != BOOLCOUNT
        || term.numbers.len() != NUMCOUNT
        || term.strings.len() != STRCOUNT;
    p(format!(
        "\t\t{},\t/* ...corresponding names */\n",
        if ext {
            name("_string_ext_data")
        } else {
            "(char **)0".to_owned()
        }
    ));
    p(format!(
        "\t\t{},\t\t/* count total Booleans */\n",
        term.booleans.len()
    ));
    p(format!(
        "\t\t{},\t\t/* count total Numbers */\n",
        term.numbers.len()
    ));
    p(format!(
        "\t\t{},\t\t/* count total Strings */\n",
        term.strings.len()
    ));
    p(format!(
        "\t\t{},\t\t/* count extensions to Booleans */\n",
        term.booleans.len().saturating_sub(BOOLCOUNT)
    ));
    p(format!(
        "\t\t{},\t\t/* count extensions to Numbers */\n",
        term.numbers.len().saturating_sub(NUMCOUNT)
    ));
    p(format!(
        "\t\t{},\t\t/* count extensions to Strings */\n",
        term.strings.len().saturating_sub(STRCOUNT)
    ));
    p("#endif /* NCURSES_XNAMES */\n".to_owned());
    p("\t}\n".to_owned());
}

// ---- Main sequence ---------------------------------------------------------

/// The `-` options, two to a line, as `usage` prints them.
const OPTIONS: &[&str] = &[
    "  -0    print single-row",
    "  -1    print single-column",
    "  -C    use termcap-names",
    "  -D    print database locations",
    "  -E    format output as C tables",
    "  -F    compare terminfo-files",
    "  -G    format %{number} to %'char'",
    "  -I    use terminfo-names",
    "  -K    use termcap-names and BSD syntax",
    "  -L    use long names",
    "  -R subset (see manpage)",
    "  -T    eliminate size limits (test)",
    "  -U    do not post-process entries",
    "  -V    print version",
    "  -W    wrap long strings per -w[n]",
    "  -a    with -F, list commented-out caps",
    "  -c    list common capabilities",
    "  -d    list different capabilities",
    "  -e    format output for C initializer",
    "  -f    with -1, format complex strings",
    "  -g    format %'char' to %{number}",
    "  -i    analyze initialization/reset",
    "  -l    output terminfo names",
    "  -n    list capabilities in neither",
    "  -p    ignore padding specifiers",
    "  -Q number  dump compiled description",
    "  -q    brief listing, removes headers",
    "  -r    with -C, output in termcap form",
    "  -r    with -F, resolve use-references",
    "  -s [d|i|l|c] sort fields",
    "  -t    suppress commented-out capabilities",
    "  -u    produce source with 'use='",
    "  -v number  (verbose)",
    "  -w number  (width)",
    "  -x    unknown capabilities are user-defined",
];

/// `usage ()`.
fn usage() -> Exit {
    let mut m =
        b"Usage: infocmp [options] [-A directory] [-B directory] [termname...]\n\nOptions:\n"
            .to_vec();
    let last = OPTIONS.len();
    let left = last.div_ceil(2);
    for n in 0..left {
        let a = OPTIONS.get(n).copied().unwrap_or("");
        match OPTIONS.get(n.wrapping_add(left)) {
            Some(b) => m.extend_from_slice(format!("{:<40.40}{b}\n", a).as_bytes()),
            None => m.extend_from_slice(format!("{a}\n").as_bytes()),
        }
    }
    ulclosestream::stderr_write(&m);
    Exit(1)
}

/// `optarg_to_number ()`: a number, all of the argument, base 0.
fn optarg_to_number(arg: &[u8]) -> Result<i32, Exit> {
    let (value, used) = cstrtol::strtol(arg, 0);
    if used == 0 || used != arg.len() {
        let mut m = b"Expected a number, not \"".to_vec();
        m.extend_from_slice(arg);
        m.extend_from_slice(b"\"\n");
        ulclosestream::stderr_write(&m);
        return Err(Exit(1));
    }
    Ok(cstrtol::low_i32(value))
}

/// `terminal_env ()`: `$TERM`, or the end.
fn terminal_env(progname: &[u8]) -> Result<Vec<u8>, Exit> {
    match std::env::var_os("TERM") {
        Some(t) => Ok(os_bytes(&t).into_owned()),
        None => {
            let mut m = progname.to_vec();
            m.extend_from_slice(b": environment variable TERM not set\n");
            ulclosestream::stderr_write(&m);
            Err(Exit(1))
        }
    }
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let mut diag = Stderr;
    let status = match run(&argv, &mut out, &mut diag) {
        Ok(code) | Err(Exit(code)) => code,
    };
    out.flush_at_exit();
    ExitCode::from(status)
}

/// What the dumper has printed, onto standard output.
fn drain(d: &mut Dumper, out: &mut ulclosestream::Stdout) {
    let text = std::mem::take(&mut d.stdout);
    out.write(&text);
}

/// Upstream's `main`, up to its `ExitProgram`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, in one piece so it reads against it"
)]
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout, diag: &mut Stderr) -> Result<u8, Exit> {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let progname = nc::rootname(&argv0).to_vec();
    let mut scan = Scanner::new(diag);
    scan.strict_bsd = false;

    let mut ic = Infocmp {
        progname: progname.clone(),
        limited: true,
        quiet: false,
        literal: false,
        bool_sep: b":",
        s_absent: b"NULL",
        s_cancel: b"NULL",
        itrace: 0,
        numbers: 0,
        outform: F_TERMINFO,
        compare: C_DEFAULT,
        ignorepads: false,
        user_definable: false,
    };
    let mut mwidth = 60;
    let mut mheight = 65535;
    let mut tversion: Option<Vec<u8>> = None;
    let mut sortmode = S_DEFAULT;
    let mut firstdir: Option<Vec<u8>> = None;
    let mut restdir: Option<Vec<u8>> = None;
    let mut formatted = false;
    let mut filecompare = false;
    let mut initdump = 0;
    let mut init_analyze = false;
    let mut suppress_untranslatable = false;
    let mut quickdump = 0;
    let mut wrap_strings = false;
    let mut disable_period = false;

    let words = argv.get(1..).unwrap_or_default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in INFOCMP
        .parse(words, "01A:aB:CcDdEeFfGgIiKLlnpQ:qR:rs:TtUuVv:Ww:x", &[])
        .short_only(true)
    {
        let opt = match item {
            Err(e) => {
                nc::getopt_complaint(&argv0, &e.sentence);
                return Err(usage());
            }
            Ok(o) => o,
        };
        let arg = |v: &Option<OsString>| {
            v.as_ref()
                .map(|v| os_bytes(v).into_owned())
                .unwrap_or_default()
        };
        match opt {
            Opt::Short(b'0', _) => {
                mwidth = 65535;
                mheight = 1;
            }
            Opt::Short(b'1', _) => mwidth = 0,
            Opt::Short(b'A', v) => firstdir = Some(arg(&v)),
            Opt::Short(b'a', _) => {
                disable_period = true;
                ic.user_definable = true;
            }
            Opt::Short(b'B', v) => restdir = Some(arg(&v)),
            Opt::Short(b'K', _) => {
                scan.strict_bsd = true;
                ic.outform = F_TERMCAP;
                tversion = Some(b"BSD".to_vec());
                if sortmode == S_DEFAULT {
                    sortmode = S_TERMCAP;
                }
            }
            Opt::Short(b'C', _) => {
                ic.outform = F_TERMCAP;
                tversion = Some(b"BSD".to_vec());
                if sortmode == S_DEFAULT {
                    sortmode = S_TERMCAP;
                }
            }
            Opt::Short(b'D', _) => {
                for path in terminfo::search_list(&terminfo::Env::from_process()) {
                    out.write(&path);
                    out.write(b"\n");
                }
                return Ok(0);
            }
            Opt::Short(b'c', _) => ic.compare = C_COMMON,
            Opt::Short(b'd', _) => ic.compare = C_DIFFERENCE,
            Opt::Short(b'E', _) => initdump |= 2,
            Opt::Short(b'e', _) => initdump |= 1,
            Opt::Short(b'F', _) => filecompare = true,
            Opt::Short(b'f', _) => formatted = true,
            Opt::Short(b'G', _) => ic.numbers = 1,
            Opt::Short(b'g', _) => ic.numbers = -1,
            Opt::Short(b'I', _) => {
                ic.outform = F_TERMINFO;
                if sortmode == S_DEFAULT {
                    sortmode = S_VARIABLE;
                }
                tversion = None;
            }
            Opt::Short(b'i', _) => init_analyze = true,
            Opt::Short(b'L', _) => {
                ic.outform = F_VARIABLE;
                if sortmode == S_DEFAULT {
                    sortmode = S_VARIABLE;
                }
            }
            Opt::Short(b'l', _) => ic.outform = F_TERMINFO,
            Opt::Short(b'n', _) => ic.compare = C_NAND,
            Opt::Short(b'p', _) => ic.ignorepads = true,
            Opt::Short(b'Q', v) => quickdump = optarg_to_number(&arg(&v))?,
            Opt::Short(b'q', _) => {
                ic.quiet = true;
                ic.s_absent = b"-";
                ic.s_cancel = b"@";
                ic.bool_sep = b", ";
            }
            Opt::Short(b'R', v) => tversion = Some(arg(&v)),
            Opt::Short(b'r', _) => tversion = None,
            Opt::Short(b's', v) => {
                sortmode = match arg(&v).first() {
                    Some(b'd') => S_NOSORT,
                    Some(b'i') => S_TERMINFO,
                    Some(b'l') => S_VARIABLE,
                    Some(b'c') => S_TERMCAP,
                    _ => {
                        let mut m = progname.clone();
                        m.extend_from_slice(b": unknown sort mode\n");
                        ulclosestream::stderr_write(&m);
                        return Err(Exit(1));
                    }
                };
            }
            Opt::Short(b'T', _) => ic.limited = false,
            Opt::Short(b't', _) => {
                disable_period = false;
                suppress_untranslatable = true;
            }
            Opt::Short(b'U', _) => ic.literal = true,
            Opt::Short(b'u', _) => ic.compare = C_USEALL,
            Opt::Short(b'V', _) => {
                out.write(&nc::version_line(&progname));
                return Ok(0);
            }
            Opt::Short(b'v', v) => {
                ic.itrace = u32::try_from(optarg_to_number(&arg(&v))?).unwrap_or(u32::MAX);
            }
            Opt::Short(b'W', _) => wrap_strings = true,
            Opt::Short(b'w', v) => mwidth = optarg_to_number(&arg(&v))?,
            Opt::Short(b'x', _) => ic.user_definable = true,
            Opt::Operand(o) => operands.push(os_bytes(o).into_owned()),
            Opt::Short(..) | Opt::Long(..) => return Err(usage()),
        }
    }
    scan.disable_period = disable_period;

    // "by default, sort by terminfo name"
    if sortmode == S_DEFAULT {
        sortmode = S_TERMINFO;
    }
    // "make sure we have at least one terminal name to work with"
    if operands.is_empty() {
        operands.push(terminal_env(&progname)?);
    }
    // "if user is after a comparison, make sure we have two entries"
    if ic.compare != C_DEFAULT && operands.len() <= 1 {
        operands.push(terminal_env(&progname)?);
    }
    // "exactly one terminal name with no options means display it; exactly
    // two terminal names with no options means do -d"
    if ic.compare == C_DEFAULT {
        match operands.len() {
            1 => {}
            2 => ic.compare = C_DIFFERENCE,
            _ => {
                let mut m = progname.clone();
                m.extend_from_slice(b": too many names to compare\n");
                ulclosestream::stderr_write(&m);
                return Err(Exit(1));
            }
        }
    }

    let mut d = Dumper::new(&progname, ic.user_definable);
    d.init(
        &mut scan,
        tversion.as_deref(),
        ic.outform,
        sortmode,
        wrap_strings,
        mwidth,
        mheight,
        ic.itrace,
        formatted,
        false,
        quickdump,
    );

    if filecompare {
        if ic.compare == C_USEALL {
            ulclosestream::stderr_write(b"Sorry, -u doesn't work with -F\n");
        } else if ic.compare == C_DEFAULT {
            ulclosestream::stderr_write(b"Use `tic -[CI] <file>' for this.\n");
        } else if operands.len() != 2 {
            ulclosestream::stderr_write(b"File comparison needs exactly two file arguments.\n");
        } else {
            let status = file_comparison(&ic, &mut d, scan, &operands, out);
            drain(&mut d, out);
            return status;
        }
        return Ok(0);
    }

    // "grab the entries"
    let env = terminfo::Env::from_process();
    let mut entries: Vec<Entry> = Vec::new();
    let mut tfiles: Vec<Vec<u8>> = Vec::new();
    for (termcount, name) in operands.iter().enumerate() {
        let directory = if termcount != 0 { &restdir } else { &firstdir };
        let (found, tfile) = if let Some(dir) = directory {
            let mut tfile = dir.clone();
            tfile.push(b'/');
            tfile.push(name.first().copied().unwrap_or(0));
            tfile.push(b'/');
            tfile.extend_from_slice(name);
            let tfile = cstr(&tfile)
                .get(..PATH_MAX.min(cstr(&tfile).len()))
                .unwrap_or_default()
                .to_vec();
            if ic.itrace != 0 {
                let mut m = progname.clone();
                m.extend_from_slice(b": reading entry ");
                m.extend_from_slice(name);
                m.extend_from_slice(b" from file ");
                m.extend_from_slice(&tfile);
                m.push(b'\n');
                ulclosestream::stderr_write(&m);
            }
            (
                terminfo::read_file_entry(&tfile, ic.user_definable).ok_or(0),
                tfile,
            )
        } else {
            if ic.itrace != 0 {
                let mut m = progname.clone();
                m.extend_from_slice(b": reading entry ");
                m.extend_from_slice(name);
                m.extend_from_slice(b" from database\n");
                ulclosestream::stderr_write(&m);
            }
            terminfo::read_entry_file(name, &env, None, ic.user_definable)
        };
        let Ok(mut tterm) = found else {
            let mut m = progname.clone();
            m.extend_from_slice(b": couldn't open terminfo file ");
            m.extend_from_slice(&tfile);
            m.extend_from_slice(b".\n");
            ulclosestream::stderr_write(&m);
            return Err(Exit(1));
        };
        repair_acsc(&mut tterm);
        entries.push(Entry {
            tterm,
            ..Entry::default()
        });
        tfiles.push(tfile);
    }

    // "User-defined capabilities in different terminal descriptions may
    // have the same name/type but different indices. Line up the names to
    // use comparable indices."
    if entries.len() > 1 {
        for c in 1..entries.len() {
            let (first, rest) = entries.split_at_mut(c);
            if let (Some(e0), Some(ec)) = (first.first_mut(), rest.first_mut()) {
                ec.tterm.align(&mut e0.tterm);
            }
        }
    }

    let names0 = entries
        .first()
        .map(|e| e.tterm.term_names.clone())
        .unwrap_or_default();
    if initdump != 0 {
        let mut text = Vec::new();
        if let Some(e0) = entries.first() {
            if initdump & 1 != 0 {
                dump_termtype(&mut text, &names0, &e0.tterm);
            }
            if initdump & 2 != 0 {
                dump_initializers(&mut text, &names0, &e0.tterm);
            }
        }
        out.write(&text);
    } else if init_analyze {
        let mut text = Vec::new();
        if let Some(e0) = entries.first() {
            for (name, idx) in [
                ("is1", 48),
                ("is2", 49),
                ("is3", 50),
                ("rs1", 122),
                ("rs2", 123),
                ("rs3", 124),
                ("smcup", 28),
                ("rmcup", 40),
                ("smkx", 89),
                ("rmkx", 88),
            ] {
                analyze_string(&ic, &mut text, name, idx, &e0.tterm);
            }
        }
        out.write(&text);
    } else {
        match ic.compare {
            C_DEFAULT => {
                if ic.itrace != 0 {
                    let mut m = progname.clone();
                    m.extend_from_slice(b": about to dump ");
                    m.extend_from_slice(operands.first().map_or(&[][..], Vec::as_slice));
                    m.push(b'\n');
                    ulclosestream::stderr_write(&m);
                }
                if !ic.quiet {
                    let mut m = b"#\tReconstructed via infocmp from file: ".to_vec();
                    m.extend_from_slice(tfiles.first().map_or(&[][..], Vec::as_slice));
                    m.push(b'\n');
                    out.write(&m);
                }
                if let Some(e0) = entries.first_mut() {
                    d.dump_entry(
                        &mut scan,
                        &mut e0.tterm,
                        suppress_untranslatable,
                        ic.limited,
                        ic.numbers,
                        None,
                    );
                }
                drain(&mut d, out);
                let len = d.show_entry();
                drain(&mut d, out);
                if ic.itrace != 0 {
                    let mut m = progname.clone();
                    m.extend_from_slice(format!(": length {len}\n").as_bytes());
                    ulclosestream::stderr_write(&m);
                }
            }
            C_DIFFERENCE | C_COMMON | C_NAND => {
                show_comparing(&ic, out, &operands);
                compare(&ic, &mut d, &entries, out);
            }
            _ => {
                // C_USEALL
                if ic.itrace != 0 {
                    let mut m = progname.clone();
                    m.extend_from_slice(b": dumping use entry\n");
                    ulclosestream::stderr_write(&m);
                }
                let (first, rest) = entries.split_at_mut(1);
                if let Some(e0) = first.first_mut() {
                    let others: Vec<Entry> = std::iter::once(Entry::default())
                        .chain(rest.iter().cloned())
                        .collect();
                    let pred = |t: &TermType, kind: Kind, idx: usize| {
                        ic.use_predicate(&others, t, kind, idx)
                    };
                    d.dump_entry(
                        &mut scan,
                        &mut e0.tterm,
                        suppress_untranslatable,
                        ic.limited,
                        ic.numbers,
                        Some(&pred),
                    );
                }
                for name in operands.iter().skip(1) {
                    let infodump = !(ic.outform == F_TERMCAP || ic.outform == F_TCONVERR);
                    d.dump_uses(&mut scan, Some(name), infodump);
                }
                drain(&mut d, out);
                let len = d.show_entry();
                drain(&mut d, out);
                if ic.itrace != 0 {
                    let mut m = progname.clone();
                    m.extend_from_slice(format!(": length {len}\n").as_bytes());
                    ulclosestream::stderr_write(&m);
                }
            }
        }
    }
    Ok(0)
}

/// `show_comparing (names)`.
fn show_comparing(ic: &Infocmp, out: &mut ulclosestream::Stdout, names: &[Vec<u8>]) {
    if ic.itrace != 0 {
        let what = match ic.compare {
            C_DIFFERENCE | C_NAND => Some("dumping differences"),
            C_COMMON => Some("dumping common capabilities"),
            _ => None,
        };
        if let Some(what) = what {
            let mut m = ic.progname.clone();
            m.extend_from_slice(format!(": {what}\n").as_bytes());
            ulclosestream::stderr_write(&m);
        }
    }
    let mut it = names.iter();
    if let Some(first) = it.next() {
        let mut m = b"comparing ".to_vec();
        m.extend_from_slice(first);
        if let Some(second) = it.next() {
            m.extend_from_slice(b" to ");
            m.extend_from_slice(second);
            for n in it {
                m.extend_from_slice(b", ");
                m.extend_from_slice(n);
            }
        }
        m.extend_from_slice(b".\n");
        out.write(&m);
    }
}

/// `compare_entry (compare_predicate, &entries->tterm, quiet)`.
fn compare(ic: &Infocmp, d: &mut Dumper, entries: &[Entry], out: &mut ulclosestream::Stdout) {
    let Some(e0) = entries.first() else {
        return;
    };
    let t0 = e0.tterm.clone();
    let mut hook = |dd: &mut Dumper, kind: Option<Kind>, idx: usize, name: &[u8]| {
        compare_predicate(ic, entries, &mut dd.stdout, kind, idx, name);
    };
    d.compare_entry(&mut hook, &t0, ic.quiet);
    drain(d, out);
}

/// `file_comparison (argc, argv)`: two source files' entries matched by
/// name, and those that differ compared.
#[allow(clippy::too_many_lines, reason = "upstream's file_comparison")]
fn file_comparison(
    ic: &Infocmp,
    d: &mut Dumper,
    scan: Scanner<'_>,
    files: &[Vec<u8>],
    out: &mut ulclosestream::Stdout,
) -> Result<u8, Exit> {
    // The compiler takes over the scanner and its messages.
    let mut diag2 = Stderr;
    let (strict_bsd, disable_period) = (scan.strict_bsd, scan.disable_period);
    drop(scan);
    let mut c = Compiler::new(&mut diag2);
    c.user_definable = ic.user_definable;
    c.env = terminfo::Env::from_process();
    c.scan.strict_bsd = strict_bsd;
    c.scan.disable_period = disable_period;
    c.tracing = ic.itrace << 13;
    d.init(
        &mut c.scan,
        None,
        dump::F_LITERAL,
        S_TERMINFO,
        false,
        0,
        65535,
        ic.itrace,
        false,
        false,
        0,
    );
    let mut heads: Vec<Vec<Entry>> = Vec::new();
    for file in files.iter().take(2) {
        let Ok(f) = std::fs::File::open(os_from_bytes(file)) else {
            let mut m = b"Can't open ".to_vec();
            m.extend_from_slice(file);
            return Err(c.scan.err_abort(&m).into());
        };
        c.entries.clear();
        c.scan.set_source(Some(file));
        c.read_entry_source(Source::File(Box::new(f), Some(0)), true, ic.literal)?;
        if ic.itrace != 0 {
            ulclosestream::stderr_write(format!("Resolving file {}...\n", heads.len()).as_bytes());
        }
        if !c.resolve_uses2(!ic.limited, ic.literal)? {
            let mut m = b"There are unresolved use entries in ".to_vec();
            m.extend_from_slice(file);
            m.extend_from_slice(b":\n");
            for qp in &c.entries {
                if qp.nuses != 0 {
                    m.extend_from_slice(&qp.tterm.term_names);
                    m.push(b'\n');
                }
            }
            ulclosestream::stderr_write(&m);
            return Err(Exit(1));
        }
        heads.push(std::mem::take(&mut c.entries));
    }
    if ic.itrace != 0 {
        ulclosestream::stderr_write(b"Entries are now in core...\n");
    }
    let (mut h0, mut h1) = match (heads.first().cloned(), heads.get(1).cloned()) {
        (Some(a), Some(b)) => (a, b),
        _ => return Ok(0),
    };

    // "The entry-matching loop."
    let mut cross0: Vec<Vec<usize>> = vec![Vec::new(); h0.len()];
    let mut cross1: Vec<Vec<usize>> = vec![Vec::new(); h1.len()];
    for (qi, qp) in h0.iter().enumerate() {
        for (ri, rp) in h1.iter().enumerate() {
            if entry_match(&qp.tterm.term_names, &rp.tterm.term_names) {
                if let Some(v) = cross0.get_mut(qi) {
                    v.push(ri);
                }
                if let Some(v) = cross1.get_mut(ri) {
                    v.push(qi);
                }
            }
        }
    }
    if ic.itrace != 0 {
        ulclosestream::stderr_write(b"Name matches are done...\n");
    }
    // `MAX_CROSSLINKS`: the links kept, of those counted.
    const MAX_CROSSLINKS: usize = 16;
    let report = |which: (u8, u8),
                  list: &[Entry],
                  cross: &[Vec<usize>],
                  other: &[Entry],
                  f: (&[u8], &[u8])| {
        for (qi, qp) in list.iter().enumerate() {
            let links = cross.get(qi).map_or(&[][..], Vec::as_slice);
            if links.len() > 1 {
                let mut m = first_name(&qp.tterm.term_names);
                m.extend_from_slice(format!(" in file {} (", which.0 as char).as_bytes());
                m.extend_from_slice(f.0);
                m.extend_from_slice(
                    format!(
                        ") has {} matches in file {} (",
                        links.len(),
                        which.1 as char
                    )
                    .as_bytes(),
                );
                m.extend_from_slice(f.1);
                m.extend_from_slice(b"):\n");
                for &l in links.iter().take(MAX_CROSSLINKS) {
                    m.push(b'\t');
                    m.extend_from_slice(&first_name(
                        &other
                            .get(l)
                            .map(|e| e.tterm.term_names.clone())
                            .unwrap_or_default(),
                    ));
                    m.push(b'\n');
                }
                ulclosestream::stderr_write(&m);
            }
        }
    };
    let f0 = files.first().map_or(&[][..], Vec::as_slice);
    let f1 = files.get(1).map_or(&[][..], Vec::as_slice);
    report((b'1', b'2'), &h0, &cross0, &h1, (f0, f1));
    report((b'2', b'1'), &h1, &cross1, &h0, (f1, f0));

    let mut text = Vec::new();
    text.extend_from_slice(b"In file 1 (");
    text.extend_from_slice(f0);
    text.extend_from_slice(b") only:\n");
    for (qi, qp) in h0.iter().enumerate() {
        if cross0.get(qi).is_none_or(Vec::is_empty) {
            text.push(b'\t');
            text.extend_from_slice(&first_name(&qp.tterm.term_names));
            text.push(b'\n');
        }
    }
    text.extend_from_slice(b"In file 2 (");
    text.extend_from_slice(f1);
    text.extend_from_slice(b") only:\n");
    for (ri, rp) in h1.iter().enumerate() {
        if cross1.get(ri).is_none_or(Vec::is_empty) {
            text.push(b'\t');
            text.extend_from_slice(&first_name(&rp.tterm.term_names));
            text.push(b'\n');
        }
    }
    text.extend_from_slice(b"The following entries are equivalent:\n");
    for qi in 0..h0.len() {
        let links = cross0.get(qi).cloned().unwrap_or_default();
        if links.len() == 1
            && let Some(&ri) = links.first()
        {
            let (Some(qp), Some(rp)) = (h0.get_mut(qi), h1.get_mut(ri)) else {
                continue;
            };
            repair_acsc(&mut qp.tterm);
            repair_acsc(&mut rp.tterm);
            qp.tterm.align(&mut rp.tterm);
            if entryeq(ic, &qp.tterm, &rp.tterm) && useeq(qp, rp) {
                text.extend_from_slice(&canonical_name(&qp.tterm.term_names));
                text.extend_from_slice(b" = ");
                text.extend_from_slice(&canonical_name(&rp.tterm.term_names));
                text.push(b'\n');
            }
        }
    }
    text.extend_from_slice(b"Differing entries:\n");
    out.write(&text);
    for qi in 0..h0.len() {
        let links = cross0.get(qi).cloned().unwrap_or_default();
        if links.len() == 1
            && let Some(&ri) = links.first()
        {
            let (Some(qp), Some(rp)) = (h0.get_mut(qi), h1.get_mut(ri)) else {
                continue;
            };
            // "sorry - we have to do this on each pass"
            qp.tterm.align(&mut rp.tterm);
            if !(entryeq(ic, &qp.tterm, &rp.tterm) && useeq(qp, rp)) {
                let pair = vec![qp.clone(), rp.clone()];
                let names = vec![
                    canonical_name(&qp.tterm.term_names),
                    canonical_name(&rp.tterm.term_names),
                ];
                show_comparing(ic, out, &names);
                compare(ic, d, &pair, out);
            }
        }
    }
    Ok(0)
}
