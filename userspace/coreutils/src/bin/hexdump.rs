//! `hexdump` -- display file contents in hexadecimal, decimal, octal, or
//! ascii: util-linux 2.39.3's, ported.
//!
//! ```text
//! hexdump [options] <file>...
//! ```
//!
//! A transcription of `text-utils/hexdump.c`, `hexdump-parse.c`,
//! `hexdump-display.c` and `hexdump-conv.c`. The program is a small
//! interpreter: each `-e` format (and each `-b -c -C -d -o -x`, which are
//! canned ones) is broken into format units -- `[reps/][bytes] "text"` --
//! and each unit into print units, one per conversion, whose `printf`
//! directives are rewritten for the width of the data they take. A block of
//! input as wide as the widest format is read and every format applied to
//! it in turn.
//!
//! What upstream does and this keeps:
//!
//! - A repetition count drops only the *last* trailing blank of the last
//!   repetition (`nospace`), which is why `-x` lines end in a digit.
//! - Identical blocks are squeezed to one `*` line unless `-v`; the state
//!   machine (`vflag`) is upstream's.
//! - Files are reopened onto standard input one after another, as
//!   `freopen` does, and a block may span two of them. `-s` skips whole
//!   regular files it is longer than, and seeks into the rest -- which fails
//!   on a pipe: `hexdump: stdin: Illegal seek`.
//! - `_L[...]` colour units and `-L`/`--color`, through `ulcolors`
//!   (util-linux's `lib/colors.c`).
//! - The error messages, each worded and placed as upstream's, including the
//!   ones that quote the format as it was mid-rewrite.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - Upstream reads past the end of a format in three malformed cases -- a
//!   trailing backslash, a `%` with nothing after its flags, and a `%_` at
//!   the end -- which is undefined behaviour in C. Here the format simply
//!   ends there.
//! - A read error on standard input names the argument before the first
//!   operand, as upstream's `_argv[-1]` does, only when there is one: with no
//!   arguments at all it names the program.

use std::ffi::OsString;
use std::io::SeekFrom;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Reopen};
use coreutils::stdio::StdioReader;
use cprintf::cfmt::{self, Value};
use cprintf::extfloat::{ExtF80, Spec};
use ulcolors::{ColorMode, Colors};

coreutils::guard_std_fds!();

const HEXDUMP: Program = Program::new("hexdump", 1);
const HD: Program = Program::new("hd", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "bcCde:f:L::n:os:vxhV";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("one-byte-octal", Takes::Nothing),
    ("one-byte-char", Takes::Nothing),
    ("canonical", Takes::Nothing),
    ("two-bytes-decimal", Takes::Nothing),
    ("two-bytes-octal", Takes::Nothing),
    ("two-bytes-hex", Takes::Nothing),
    ("format", Takes::Required),
    ("format-file", Takes::Required),
    ("color", Takes::Optional),
    ("length", Takes::Required),
    ("skip", Takes::Required),
    ("no-squeezing", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The flag bits of a print unit.
const F_ADDRESS: u32 = 0x001;
const F_BPAD: u32 = 0x002;
const F_C: u32 = 0x004;
const F_CHAR: u32 = 0x008;
const F_DBL: u32 = 0x010;
const F_INT: u32 = 0x020;
const F_P: u32 = 0x040;
const F_STR: u32 = 0x080;
const F_U: u32 = 0x100;
const F_UINT: u32 = 0x200;
const F_TEXT: u32 = 0x400;

/// The flag bits of a format unit.
const F_IGNORE: u32 = 0x01;
const F_SETREP: u32 = 0x02;

/// `spec`: what may stand between a `%` and its conversion.
const SPEC: &[u8] = b".#-+ 0123456789";

/// `hex_offt`, the address line every canned format starts with.
const HEX_OFFT: &[u8] = b"\"%07.7_Ax\n\"";

/// `-v`'s states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VFlag {
    All,
    Dup,
    First,
    Wait,
}

/// One colour condition of a print unit: `struct hexdump_clr`.
#[derive(Clone, Debug)]
struct Clr {
    /// The sequence that turns the colour on.
    seq: Vec<u8>,
    /// The offset it applies at, or -1 for any.
    offt: i64,
    /// How many bytes it covers.
    range: i32,
    /// The value to match, or -1.
    val: i32,
    /// The string to match.
    text: Option<Vec<u8>>,
    invert: bool,
}

/// A print unit: `struct hexdump_pr`.
#[derive(Clone, Debug, Default)]
struct Pr {
    flags: u32,
    bcnt: i32,
    fmt: Vec<u8>,
    /// Where in `fmt` the conversion character is.
    cchar: usize,
    colors: Option<Vec<Clr>>,
    /// The trailing blank the last repetition leaves out.
    nospace: Option<usize>,
}

/// A format unit: `struct hexdump_fu`.
#[derive(Clone, Debug)]
struct Fu {
    flags: u32,
    reps: i32,
    bcnt: i32,
    fmt: Vec<u8>,
    prs: Vec<Pr>,
}

/// A format string: `struct hexdump_fs`.
#[derive(Clone, Debug, Default)]
struct Fs {
    fus: Vec<Fu>,
    bcnt: i32,
}

/// `EIO`, for a read error that came without an `errno`.
const EIO: i32 = 5;

/// A run that must stop: `errx`/`err`'s message, after `hexdump: `.
#[derive(Debug)]
struct Fatal(Vec<u8>);

/// `isprint` in the C locale -- and in a UTF-8 one, where no single byte
/// above 0x7f is a printable character either.
fn is_print(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// The byte at `i`, or the terminating NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `skip_space`.
fn skip_space(s: &[u8], mut i: usize) -> usize {
    while cstrtol::isspace(at(s, i)) {
        i = i.saturating_add(1);
    }
    i
}

/// `next_number`: `strtol (str, &end, 10)` cut to an `int`, and where it
/// ended -- `None` if no digit or out of `long`'s range (`errno`).
fn next_number(s: &[u8], i: usize) -> Option<(i32, usize)> {
    let (v, used, overflow) = cstrtol::strtol_overflow(s.get(i..).unwrap_or_default(), 10);
    if used == 0 || overflow {
        return None;
    }
    // `*num = strtol (...)`: a `long` into an `int`, its low 32 bits.
    Some((cstrtol::low_i32(v), i.saturating_add(used)))
}

/// `escape`: the backslash escapes of a format's text, in place.
fn escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if c == b'\\' {
            i = i.saturating_add(1);
            let Some(&e) = s.get(i) else {
                // A trailing backslash: upstream copies the NUL and reads on
                // past it; what it printed stops there.
                break;
            };
            out.push(match e {
                b'a' => 0x07,
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 0x0b,
                other => other,
            });
        } else {
            out.push(c);
        }
        i = i.saturating_add(1);
    }
    out
}

/// `[a, b, ...]` joined.
fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// util-linux's `warn` (with a reason) and `warnx` (without): `PROG:
/// MESSAGE[: REASON]` on standard error, the message's bytes as they are --
/// a file name is printed raw, as upstream prints it -- and standard output
/// not flushed first, as `warn` does not.
fn warn_bytes(prog: &str, msg: &[u8], e: Option<&std::io::Error>) {
    let mut line = cat(&[prog.as_bytes(), b": ", msg]);
    if let Some(e) = e {
        line.extend_from_slice(b": ");
        line.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
    }
    line.push(b'\n');
    ulclosestream::stderr_write(&line);
}

/// `badfmt`.
fn badfmt(fmt: &[u8]) -> Fatal {
    Fatal(cat(&[b"bad format {", fmt, b"}"]))
}

/// `badcnt`.
fn badcnt(s: &[u8]) -> Fatal {
    Fatal(cat(&[b"bad byte count for conversion character ", s]))
}

/// `badsfmt`.
fn badsfmt() -> Fatal {
    Fatal(b"%s requires a precision or a byte count".to_vec())
}

/// `badconv`.
fn badconv(s: &[u8]) -> Fatal {
    Fatal(cat(&[b"bad conversion character %", s]))
}

/// `add_fmt`: one format string, broken into format units.
fn add_fmt(fmt: &[u8]) -> Result<Fs, Fatal> {
    // A C string: a NUL ends it.
    let fmt = fmt
        .iter()
        .position(|&b| b == 0)
        .map_or(fmt, |n| fmt.get(..n).unwrap_or_default());
    let mut fs = Fs::default();
    let mut p = 0usize;
    loop {
        p = skip_space(fmt, p);
        if at(fmt, p) == 0 {
            break;
        }
        let mut fu = Fu {
            flags: 0,
            reps: 1,
            bcnt: 0,
            fmt: Vec::new(),
            prs: Vec::new(),
        };
        if at(fmt, p).is_ascii_digit() {
            let (n, end) = next_number(fmt, p).ok_or_else(|| badfmt(fmt))?;
            if !cstrtol::isspace(at(fmt, end)) && at(fmt, end) != b'/' {
                return Err(badfmt(fmt));
            }
            fu.reps = n;
            fu.flags = F_SETREP;
            p = skip_space(fmt, end.saturating_add(1));
        }
        if at(fmt, p) == b'/' {
            p = skip_space(fmt, p.saturating_add(1));
        }
        if at(fmt, p).is_ascii_digit() {
            let (n, end) = next_number(fmt, p).ok_or_else(|| badfmt(fmt))?;
            if !cstrtol::isspace(at(fmt, end)) {
                return Err(badfmt(fmt));
            }
            fu.bcnt = n;
            p = skip_space(fmt, end.saturating_add(1));
        }
        if at(fmt, p) != b'"' {
            return Err(badfmt(fmt));
        }
        p = p.saturating_add(1);
        let savep = p;
        while at(fmt, p) != b'"' {
            if at(fmt, p) == 0 {
                return Err(badfmt(fmt));
            }
            p = p.saturating_add(1);
        }
        fu.fmt = escape(fmt.get(savep..p).unwrap_or_default());
        p = p.saturating_add(1);
        fs.fus.push(fu);
    }
    Ok(fs)
}

/// `addfile`: each line of a file, less leading blanks, as a format --
/// blank lines and `#` comments skipped.
fn addfile(name: &[u8], fss: &mut Vec<Fs>) -> Result<(), Fatal> {
    let text = std::fs::read(coreutils::quote::os_from_bytes(name)).map_err(|e| {
        Fatal(cat(&[
            b"can't read ",
            name,
            b": ",
            coreutils::errmsg::strerror(&e).as_bytes(),
        ]))
    })?;
    for line in text.split_inclusive(|&c| c == b'\n') {
        let n = line.iter().take_while(|&&b| cstrtol::isspace(b)).count();
        let fmt = line.get(n..).unwrap_or_default();
        if fmt.first().is_none_or(|&c| c == b'#' || c == 0) {
            continue;
        }
        fss.push(add_fmt(fmt)?);
    }
    Ok(())
}

/// `block_size`: the bytes one pass of a format string consumes.
fn block_size(fs: &Fs) -> Result<i32, Fatal> {
    let mut cursize: i32 = 0;
    for fu in &fs.fus {
        if fu.bcnt != 0 {
            cursize = cursize.wrapping_add(fu.bcnt.wrapping_mul(fu.reps));
            continue;
        }
        let (mut bcnt, mut prec) = (0i32, 0i32);
        let f = &fu.fmt;
        let mut i = 0usize;
        while at(f, i) != 0 {
            if at(f, i) != b'%' {
                i = i.saturating_add(1);
                continue;
            }
            // Skip the flags and field width, keeping a precision in case of
            // `%s`.
            loop {
                i = i.saturating_add(1);
                let c = at(f, i);
                if c == 0 || !SPEC.get(1..).unwrap_or_default().contains(&c) {
                    break;
                }
            }
            if at(f, i) == b'.' {
                i = i.saturating_add(1);
                if at(f, i).is_ascii_digit() {
                    let (n, end) = next_number(f, i).ok_or_else(|| badfmt(f))?;
                    prec = n;
                    i = end;
                }
            }
            let c = at(f, i);
            if c == 0 {
                return Err(badfmt(f));
            }
            if b"diouxX".contains(&c) {
                bcnt = bcnt.wrapping_add(4);
            } else if b"efgEG".contains(&c) {
                bcnt = bcnt.wrapping_add(8);
            } else if c == b's' {
                bcnt = bcnt.wrapping_add(prec);
            } else if c == b'c' {
                bcnt = bcnt.wrapping_add(1);
            } else if c == b'_' {
                i = i.saturating_add(1);
                let n = at(f, i);
                if n == 0 || b"cpu".contains(&n) {
                    bcnt = bcnt.wrapping_add(1);
                }
                if n == 0 {
                    break;
                }
            }
            i = i.saturating_add(1);
        }
        cursize = cursize.wrapping_add(bcnt.wrapping_mul(fu.reps));
    }
    Ok(cursize)
}

/// `rewrite_rules`: each format unit's print units, the directives rewritten
/// for the data's width; then the last unit repeated to fill the block.
fn rewrite_rules(
    fs: &mut Fs,
    blocksize: i32,
    colors: &Colors,
    endfu: &mut Option<(usize, usize)>,
    fs_index: usize,
) -> Result<(), Fatal> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Sokay {
        NotOkay,
        UseBcnt,
        UsePrec,
    }
    let fu_count = fs.fus.len();
    for (fi, fu) in fs.fus.iter_mut().enumerate() {
        let mut nconv: u32 = 0;
        let fmt = fu.fmt.clone();
        let mut fmtp = 0usize;
        let mut prec = 0i32;
        while at(&fmt, fmtp) != 0 {
            let mut pr = Pr::default();
            let mut p1 = fmtp;
            while at(&fmt, p1) != 0 && at(&fmt, p1) != b'%' {
                p1 = p1.saturating_add(1);
            }
            if at(&fmt, p1) == 0 {
                pr.fmt = fmt.get(fmtp..).unwrap_or_default().to_vec();
                pr.flags = F_TEXT;
                fu.prs.push(pr);
                break;
            }
            let sokay;
            if fu.bcnt != 0 {
                sokay = Sokay::UseBcnt;
                p1 = p1.saturating_add(1);
                while at(&fmt, p1) != 0 && SPEC.contains(&at(&fmt, p1)) {
                    p1 = p1.saturating_add(1);
                }
            } else {
                loop {
                    p1 = p1.saturating_add(1);
                    let c = at(&fmt, p1);
                    if c == 0 || !SPEC.get(1..).unwrap_or_default().contains(&c) {
                        break;
                    }
                }
                if at(&fmt, p1) == b'.' && at(&fmt, p1.saturating_add(1)).is_ascii_digit() {
                    sokay = Sokay::UsePrec;
                    let (n, end) =
                        next_number(&fmt, p1.saturating_add(1)).ok_or_else(|| badfmt(&fmt))?;
                    prec = n;
                    p1 = end;
                } else {
                    if at(&fmt, p1) == b'.' {
                        p1 = p1.saturating_add(1);
                    }
                    sokay = Sokay::NotOkay;
                }
            }
            let mut p2 = p1.saturating_add(1);
            let c0 = at(&fmt, p1);
            // The conversion string the directive ends with.
            let mut cs: Vec<u8> = if c0 == 0 { Vec::new() } else { vec![c0] };
            let one = |len: usize| {
                fmt.get(p1..p1.saturating_add(len))
                    .unwrap_or_default()
                    .to_vec()
            };
            match c0 {
                b'c' => {
                    pr.flags = F_CHAR;
                    match fu.bcnt {
                        0 | 1 => pr.bcnt = 1,
                        _ => return Err(badcnt(&one(1))),
                    }
                }
                b'd' | b'i' | b'o' | b'u' | b'x' | b'X' => {
                    pr.flags = if matches!(c0, b'd' | b'i') {
                        F_INT
                    } else {
                        F_UINT
                    };
                    cs = vec![b'l', b'l', c0];
                    match fu.bcnt {
                        0 => pr.bcnt = 4,
                        1 | 2 | 4 | 8 => pr.bcnt = fu.bcnt,
                        _ => return Err(badcnt(&one(1))),
                    }
                }
                b'e' | b'f' | b'g' | b'E' | b'G' => {
                    pr.flags = F_DBL;
                    match fu.bcnt {
                        0 => pr.bcnt = 8,
                        4 | 8 => pr.bcnt = fu.bcnt,
                        _ => return Err(badcnt(&one(1))),
                    }
                }
                b's' => {
                    pr.flags = F_STR;
                    pr.bcnt = match sokay {
                        Sokay::NotOkay => return Err(badsfmt()),
                        Sokay::UseBcnt => fu.bcnt,
                        Sokay::UsePrec => prec,
                    };
                }
                b'_' => {
                    p2 = p2.saturating_add(1);
                    let c1 = at(&fmt, p1.saturating_add(1));
                    match c1 {
                        b'A' | b'a' => {
                            if c1 == b'A' {
                                *endfu = Some((fs_index, fi));
                                fu.flags |= F_IGNORE;
                            }
                            pr.flags = F_ADDRESS;
                            p2 = p2.saturating_add(1);
                            let c2 = at(&fmt, p1.saturating_add(2));
                            if c2 == 0 || b"dox".contains(&c2) {
                                cs = vec![b'l', b'l'];
                                if c2 != 0 {
                                    cs.push(c2);
                                }
                            } else {
                                return Err(badconv(&one(3)));
                            }
                        }
                        b'c' | b'p' | b'u' => {
                            pr.flags = match c1 {
                                b'c' => F_C,
                                b'p' => F_P,
                                _ => F_U,
                            };
                            if c1 == b'p' {
                                cs = vec![b'c'];
                            }
                            match fu.bcnt {
                                0 | 1 => pr.bcnt = 1,
                                _ => return Err(badcnt(&one(2))),
                            }
                        }
                        _ => return Err(badconv(&one(2))),
                    }
                }
                _ => return Err(badconv(&one(1))),
            }

            // Colour units: `_L[...]`.
            if at(&fmt, p2) == b'_' && at(&fmt, p2.saturating_add(1)) == b'L' {
                let rest = fmt.get(p2..).unwrap_or_default();
                let close = rest.iter().rposition(|&c| c == b']');
                if colors.wanted() {
                    let open = rest.iter().position(|&c| c == b'[');
                    match (open, close) {
                        (Some(a), Some(b)) => {
                            let start = p2.saturating_add(a).saturating_add(1);
                            let end = p2.saturating_add(b);
                            let inner = fmt.get(start..end).unwrap_or_default();
                            pr.colors = color_fmt(inner, pr.bcnt)?;
                            p2 = end.saturating_add(1);
                        }
                        // `badconv (p2)`, `p2` being `strrchr`'s answer: the
                        // text from the last `]`, or glibc's `(null)`.
                        (_, Some(b)) => return Err(badconv(rest.get(b..).unwrap_or_default())),
                        (_, None) => return Err(badconv(b"(null)")),
                    }
                } else {
                    let Some(b) = close else {
                        return Err(badconv(b"_L"));
                    };
                    p2 = p2.saturating_add(b).saturating_add(1);
                }
            }

            pr.fmt = cat(&[fmt.get(fmtp..p1).unwrap_or_default(), &cs]);
            pr.cchar = p1.saturating_sub(fmtp);
            fmtp = p2;
            if pr.flags & F_ADDRESS == 0 && fu.bcnt != 0 {
                if nconv > 0 {
                    return Err(Fatal(
                        b"byte count with multiple conversion characters".to_vec(),
                    ));
                }
                nconv = nconv.saturating_add(1);
            }
            fu.prs.push(pr);
        }
        if fu.bcnt == 0 {
            fu.bcnt = fu.prs.iter().fold(0i32, |n, pr| n.wrapping_add(pr.bcnt));
        }
    }
    let fs_bcnt = fs.bcnt;
    for (fi, fu) in fs.fus.iter_mut().enumerate() {
        if fi.saturating_add(1) == fu_count
            && fs_bcnt < blocksize
            && fu.flags & F_SETREP == 0
            && fu.bcnt != 0
        {
            fu.reps = fu.reps.wrapping_add(
                blocksize
                    .wrapping_sub(fs_bcnt)
                    .checked_div(fu.bcnt)
                    .unwrap_or(0),
            );
        }
        if fu.reps > 1 {
            if let Some(pr) = fu.prs.last_mut() {
                let len = pr.fmt.len();
                if len > 0 && pr.fmt.last().copied().is_some_and(cstrtol::isspace) {
                    pr.nospace = Some(len.saturating_sub(1));
                }
            }
        }
    }
    Ok(())
}

/// An `unsigned long` stored in an `int`: its low 32 bits.
fn as_int(v: u64) -> i32 {
    let b = v.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// `color_fmt`: `[!]colour[:string|:0xhex|:0oct][@offt[-end]],...` -- `None`
/// when a colour is not a known name, as upstream's `NULL`.
fn color_fmt(cfmt: &[u8], bcnt: i32) -> Result<Option<Vec<Clr>>, Fatal> {
    let fmt = cfmt;
    let mut list: Vec<Clr> = Vec::new();
    let mut cur = Clr {
        seq: Vec::new(),
        offt: 0,
        range: 0,
        val: 0,
        text: None,
        invert: false,
    };
    let mut i = 0usize;
    let mut cur_at = 0usize;
    list.push(cur.clone());
    while at(cfmt, i) != 0 {
        if at(cfmt, i) == b'!' {
            cur.invert = true;
            i = i.saturating_add(1);
        }
        let name_len = cfmt
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| !b":@,".contains(c))
            .count();
        let name = cfmt.get(i..i.saturating_add(name_len)).unwrap_or_default();
        i = i.saturating_add(name_len);
        let Some(seq) = ulcolors::sequence_from_colorname(name) else {
            return Ok(None);
        };
        cur.seq = seq.to_vec();

        if at(cfmt, i) == b':' {
            i = i.saturating_add(1);
            if at(cfmt, i) == b'0' {
                let hex = matches!(at(cfmt, i.saturating_add(1)), b'x' | b'X');
                let (v, used, overflow) = if hex {
                    let (v, u, o) =
                        cstrtol::strtoull(cfmt.get(i.saturating_add(2)..).unwrap_or_default(), 16);
                    (v, u.saturating_add(2), o)
                } else {
                    cstrtol::strtoull(cfmt.get(i..).unwrap_or_default(), 8)
                };
                // `end == cfmt` -- compared with where the hex digits would
                // have started, not where the scan did.
                if overflow || used == 0 {
                    return Err(badfmt(fmt));
                }
                cur.val = as_int(v);
                i = i.saturating_add(if hex && used == 2 { 2 } else { used });
            } else {
                cur.val = -1;
                let rest = cfmt.get(i..).unwrap_or_default();
                let fmt_end = rest.iter().position(|&c| c == b',').unwrap_or(rest.len());
                let field = rest.get(..fmt_end).unwrap_or_default();
                let text = match field.iter().rposition(|&c| c == b'@') {
                    Some(at_pos) => {
                        let end = if at_pos.saturating_add(1) < field.len() {
                            at_pos
                        } else {
                            at_pos.saturating_add(1)
                        };
                        field.get(..end).unwrap_or_default().to_vec()
                    }
                    None => field.to_vec(),
                };
                i = i.saturating_add(text.len());
                cur.text = Some(text);
            }
        } else {
            cur.val = -1;
        }

        cur.range = bcnt;
        let mut clones: Vec<Clr> = Vec::new();
        if at(cfmt, i) == b'@' {
            i = i.saturating_add(1);
            let (v, used, overflow) = cstrtol::strtoull(cfmt.get(i..).unwrap_or_default(), 10);
            if overflow {
                return Err(badfmt(fmt));
            }
            cur.offt = i64::from_le_bytes(v.to_le_bytes());
            i = i.saturating_add(used);
            if at(cfmt, i) == b'-' {
                i = i.saturating_add(1);
                let (end, used, overflow) =
                    cstrtol::strtoull(cfmt.get(i..).unwrap_or_default(), 10);
                if overflow {
                    return Err(badfmt(fmt));
                }
                i = i.saturating_add(used);
                cur.range = as_int(
                    end.wrapping_sub(u64::from_le_bytes(cur.offt.to_le_bytes()))
                        .wrapping_add(1),
                );
                if cur.range < 0 {
                    return Err(badcnt(b"_L"));
                }
                while cur.range > bcnt {
                    let mut hc = cur.clone();
                    hc.range = bcnt;
                    clones.push(hc);
                    cur.offt = cur.offt.wrapping_add(i64::from(bcnt));
                    cur.range = cur.range.wrapping_sub(bcnt);
                }
            }
        } else {
            cur.offt = -1;
        }

        if let Some(t) = &cur.text {
            if i32::try_from(t.len()).ok() != Some(cur.range) {
                return Err(badcnt(b"_L"));
            }
        }
        if let Some(slot) = list.get_mut(cur_at) {
            *slot = cur.clone();
        }
        list.extend(clones);

        if at(cfmt, i) == b',' {
            i = i.saturating_add(1);
            cur = Clr {
                seq: Vec::new(),
                offt: 0,
                range: 0,
                val: 0,
                text: None,
                invert: false,
            };
            cur_at = list.len();
            list.push(cur.clone());
        }
    }
    Ok(Some(list))
}

/// One `printf` of a print unit's directive with one argument: the text
/// before the `%`, then the conversion.
fn c_printf(fmt: &[u8], value: Value<'_>) -> Vec<u8> {
    let Some(pct) = fmt.iter().position(|&c| c == b'%') else {
        return fmt.to_vec();
    };
    let mut out = fmt.get(..pct).unwrap_or_default().to_vec();
    let mut i = pct.saturating_add(1);
    let mut spec = Spec {
        minus: false,
        plus: false,
        space: false,
        hash: false,
        zero: false,
        width: 0,
        precision: None,
        conv: b's',
    };
    while let Some(&c) = fmt.get(i) {
        match c {
            b'-' => spec.minus = true,
            b'+' => spec.plus = true,
            b' ' => spec.space = true,
            b'#' => spec.hash = true,
            b'0' => spec.zero = true,
            _ => break,
        }
        i = i.saturating_add(1);
    }
    while let Some(&d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
        spec.width = spec
            .width
            .saturating_mul(10)
            .saturating_add(usize::from(d.wrapping_sub(b'0')));
        i = i.saturating_add(1);
    }
    if fmt.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        let mut p = 0usize;
        while let Some(&d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
            p = p
                .saturating_mul(10)
                .saturating_add(usize::from(d.wrapping_sub(b'0')));
            i = i.saturating_add(1);
        }
        spec.precision = Some(p);
    }
    while matches!(
        fmt.get(i),
        Some(b'l' | b'h' | b'L' | b'q' | b'j' | b'z' | b't')
    ) {
        i = i.saturating_add(1);
    }
    let Some(&conv) = fmt.get(i) else {
        // A directive with no conversion: glibc writes it as it stands.
        out.extend_from_slice(fmt.get(pct..).unwrap_or_default());
        return out;
    };
    spec.conv = conv;
    out.extend_from_slice(&cfmt::render(&spec, value));
    out.extend_from_slice(fmt.get(i.saturating_add(1)..).unwrap_or_default());
    out
}

/// `fstat (fileno (stdin))`: a regular file's size, or `None` for anything
/// else.
fn regular_size() -> std::io::Result<Option<u64>> {
    let meta = stdfd::metadata(0)?;
    Ok(meta.is_file().then_some(meta.len()))
}

/// The run: upstream's `struct hexdump` and the file-scope state of
/// `hexdump-display.c`.
struct Dump<'o> {
    fss: Vec<Fs>,
    blocksize: usize,
    exitval: u8,
    /// `-n`: -1 for all.
    length: i64,
    /// `-s`.
    skip: i64,
    vflag: VFlag,
    /// `endfu`: the `_A` unit, by format string and unit.
    endfu: Option<(usize, usize)>,
    address: i64,
    eaddress: i64,
    curp: Vec<u8>,
    savp: Vec<u8>,
    started: bool,
    ateof: bool,
    files: Vec<Vec<u8>>,
    next_file: usize,
    done: u32,
    /// What `_argv[-1]` names when reading standard input.
    stdin_name: Vec<u8>,
    /// The name of the file now on standard input.
    current: Vec<u8>,
    /// `stdin`, which every file is reopened onto in turn.
    input: &'o mut StdioReader,
    /// `fileno (stdin) == -1`: a failed `freopen` closed it.
    input_closed: bool,
    /// `errno` as the last failed read left it, which `warn` reports when
    /// `ferror (stdin)` is found later.
    read_errno: Option<i32>,
    colors: Colors,
    prog: &'static str,
    out: &'o mut ulclosestream::Stdout,
}

impl Dump<'_> {
    /// `warn`: `PROG: MESSAGE: REASON`.
    fn warn(&self, msg: &[u8], e: &std::io::Error) {
        warn_bytes(self.prog, msg, Some(e));
    }

    /// `next`: the next file on standard input -- or standard input itself
    /// when there are no files -- skipping as `-s` asks.
    fn next(&mut self) -> Result<bool, Fatal> {
        loop {
            let statok;
            if let Some(name) = self.files.get(self.next_file).cloned() {
                let path = coreutils::quote::os_from_bytes(&name);
                let reopened = if self.input_closed {
                    // `freopen` on a stream an earlier failure left with no
                    // descriptor (`fileno` -1): a plain open, onto the lowest
                    // free number -- 0, which that failure closed. It closes
                    // nothing when it fails, so its own errno stands:
                    // `nosuch2: No such file or directory`, not `Bad file
                    // descriptor`.
                    std::fs::File::open(&path).and_then(|f| stdfd::move_to(f, 0))
                } else {
                    stdfd::freopen(&path, Reopen::Read, 0)
                };
                if let Err(e) = reopened {
                    self.warn(&name, &e);
                    self.exitval = 1;
                    // `freopen` closed the stream before it failed to open
                    // the file: its buffer is gone, and so is descriptor 0.
                    *self.input = StdioReader::stdin();
                    self.input_closed = true;
                    self.next_file = self.next_file.saturating_add(1);
                    continue;
                }
                *self.input = StdioReader::stdin();
                self.input_closed = false;
                self.current = name;
                statok = true;
                self.done = 1;
            } else {
                let was = self.done;
                self.done = self.done.saturating_add(1);
                if was != 0 {
                    return Ok(false);
                }
                statok = false;
            }
            if self.skip != 0 {
                let fname = if statok {
                    self.current.clone()
                } else {
                    b"stdin".to_vec()
                };
                self.doskip(&fname, statok)?;
            }
            if self.next_file < self.files.len() {
                self.next_file = self.next_file.saturating_add(1);
            }
            if self.skip == 0 {
                return Ok(true);
            }
        }
    }

    /// `doskip`.
    fn doskip(&mut self, fname: &[u8], statok: bool) -> Result<(), Fatal> {
        let fatal = |e: &std::io::Error| {
            Fatal(cat(&[
                fname,
                b": ",
                coreutils::errmsg::strerror(e).as_bytes(),
            ]))
        };
        if statok {
            match regular_size() {
                Err(e) => return Err(fatal(&e)),
                Ok(Some(size)) => {
                    let size = i64::try_from(size).unwrap_or(i64::MAX);
                    if self.skip > size {
                        self.skip = self.skip.wrapping_sub(size);
                        self.address = self.address.wrapping_add(size);
                        return Ok(());
                    }
                }
                Ok(None) => {}
            }
        }
        // `fseek (stdin, skip, SEEK_SET)`: a negative offset is `EINVAL`.
        let to = u64::try_from(self.skip).map_err(|_| std::io::Error::from_raw_os_error(22));
        to.and_then(|to| self.input.seek(SeekFrom::Start(to)))
            .map_err(|e| fatal(&e))?;
        self.address = self.address.wrapping_add(self.skip);
        self.skip = 0;
        Ok(())
    }

    /// `get`: the next block, or `None` at the end -- squeezing duplicates
    /// as `-v` has it.
    fn get(&mut self) -> Result<bool, Fatal> {
        let bs = self.blocksize;
        if !self.started {
            self.started = true;
            self.ateof = true;
            self.curp = vec![0; bs];
            self.savp = vec![0; bs];
        } else {
            std::mem::swap(&mut self.curp, &mut self.savp);
            self.address = self.address.wrapping_add(i64::try_from(bs).unwrap_or(0));
        }
        let mut need = bs;
        let mut nread = 0usize;
        loop {
            if self.length == 0 || (self.ateof && !self.next()?) {
                if need == bs {
                    return Ok(false);
                }
                if need == 0
                    && self.vflag != VFlag::All
                    && self.curp.get(..nread) == self.savp.get(..nread)
                {
                    if self.vflag != VFlag::Dup {
                        self.out.write(b"*\n");
                    }
                    return Ok(false);
                }
                if let Some(tail) = self.curp.get_mut(nread..) {
                    tail.fill(0);
                }
                self.eaddress = self.address.wrapping_add(i64::try_from(nread).unwrap_or(0));
                return Ok(true);
            }
            if self.input_closed {
                warn_bytes(self.prog, b"all input file arguments failed", None);
                return Ok(false);
            }
            let want = if self.length == -1 {
                need
            } else {
                need.min(usize::try_from(self.length).unwrap_or(usize::MAX))
            };
            let mut chunk = vec![0u8; want];
            let (n, failed) = self.input.fread(&mut chunk);
            if let Some(e) = failed {
                self.read_errno = Some(e.raw_os_error().unwrap_or(EIO));
            }
            if let Some(dst) = self.curp.get_mut(nread..nread.saturating_add(n)) {
                dst.copy_from_slice(chunk.get(..n).unwrap_or_default());
            }
            if n == 0 {
                if self.input.has_error() {
                    let e = std::io::Error::from_raw_os_error(self.read_errno.unwrap_or(EIO));
                    let name = if self.files.is_empty() {
                        self.stdin_name.clone()
                    } else {
                        self.current.clone()
                    };
                    self.warn(&name, &e);
                }
                self.ateof = true;
                continue;
            }
            self.ateof = false;
            if self.length != -1 {
                self.length = self.length.wrapping_sub(i64::try_from(n).unwrap_or(0));
            }
            need = need.saturating_sub(n);
            if need == 0 {
                if self.vflag == VFlag::All || self.vflag == VFlag::First || self.curp != self.savp
                {
                    if self.vflag == VFlag::Dup || self.vflag == VFlag::First {
                        self.vflag = VFlag::Wait;
                    }
                    return Ok(true);
                }
                if self.vflag == VFlag::Wait {
                    self.out.write(b"*\n");
                }
                self.vflag = VFlag::Dup;
                self.address = self.address.wrapping_add(i64::try_from(bs).unwrap_or(0));
                need = bs;
                nread = 0;
            } else {
                nread = nread.saturating_add(n);
            }
        }
    }

    /// `color_cond`: the colour this unit is printed in, if any.
    fn color_cond(&self, pr: &Pr, bp: &[u8], bcnt: i32) -> Option<Vec<u8>> {
        let list = pr.colors.as_ref()?;
        for clr in list {
            let mut offt = clr.offt;
            if offt < 0 {
                offt = self.address;
            }
            if offt < self.address
                || offt.wrapping_add(i64::from(clr.range))
                    > self.address.wrapping_add(i64::from(bcnt))
            {
                continue;
            }
            let rel = usize::try_from(offt.wrapping_sub(self.address)).unwrap_or(0);
            let range = usize::try_from(clr.range).unwrap_or(0);
            let matched = if let Some(t) = &clr.text {
                pr.flags != F_ADDRESS && bp.get(rel..rel.saturating_add(range)) == t.get(..range)
            } else if clr.val != -1 {
                if pr.flags == F_ADDRESS {
                    i64::from(clr.val) == self.address
                } else {
                    let mut v = [0u8; 4];
                    for (k, slot) in v.iter_mut().enumerate().take(range.min(4)) {
                        *slot = bp.get(rel.saturating_add(k)).copied().unwrap_or(0);
                    }
                    i32::from_le_bytes(v) == clr.val
                }
            } else {
                return Some(clr.seq.clone());
            };
            if matched ^ clr.invert {
                return Some(clr.seq.clone());
            }
        }
        None
    }

    /// `print`: one print unit over the bytes at `bp`, its colour around it.
    fn print(&mut self, pr: &Pr, bp: &[u8], fmt: &[u8]) {
        let color = if pr.colors.is_some() {
            self.color_cond(pr, bp, pr.bcnt)
        } else {
            None
        };
        if let Some(c) = &color {
            let on = self.colors.enable(c).to_vec();
            self.out.write(&on);
        }
        let b0 = bp.first().copied().unwrap_or(0);
        let with = |conv: u8| {
            let mut f = fmt.to_vec();
            if let Some(slot) = f.get_mut(pr.cchar) {
                *slot = conv;
            }
            f
        };
        let text = match pr.flags {
            F_ADDRESS => {
                let conv = fmt.last().copied().unwrap_or(b'x');
                if conv == b'd' {
                    c_printf(fmt, Value::Signed(self.address))
                } else {
                    c_printf(
                        fmt,
                        Value::Unsigned(u64::from_le_bytes(self.address.to_le_bytes())),
                    )
                }
            }
            F_BPAD => c_printf(fmt, Value::Text(b"")),
            F_C => conv_c(&with, b0),
            F_CHAR => c_printf(fmt, Value::Byte(b0)),
            F_DBL => {
                let v = match pr.bcnt {
                    4 => {
                        let b: [u8; 4] = bytes_at(bp);
                        ExtF80::from_f32(f32::from_le_bytes(b))
                    }
                    _ => {
                        let b: [u8; 8] = bytes_at(bp);
                        ExtF80::from_f64(f64::from_le_bytes(b))
                    }
                };
                c_printf(fmt, Value::Float(v))
            }
            F_INT => {
                let v: i64 = match pr.bcnt {
                    1 => i64::from(i8::from_le_bytes(bytes_at(bp))),
                    2 => i64::from(i16::from_le_bytes(bytes_at(bp))),
                    4 => i64::from(i32::from_le_bytes(bytes_at(bp))),
                    _ => i64::from_le_bytes(bytes_at(bp)),
                };
                c_printf(fmt, Value::Signed(v))
            }
            F_P => c_printf(fmt, Value::Byte(if is_print(b0) { b0 } else { b'.' })),
            F_STR => {
                let end = bp.iter().position(|&c| c == 0).unwrap_or(bp.len());
                c_printf(fmt, Value::Text(bp.get(..end).unwrap_or_default()))
            }
            F_TEXT => fmt.to_vec(),
            F_U => conv_u(&with, b0),
            F_UINT => {
                let v: u64 = match pr.bcnt {
                    1 => u64::from(b0),
                    2 => u64::from(u16::from_le_bytes(bytes_at(bp))),
                    4 => u64::from(u32::from_le_bytes(bytes_at(bp))),
                    _ => u64::from_le_bytes(bytes_at(bp)),
                };
                c_printf(fmt, Value::Unsigned(v))
            }
            _ => Vec::new(),
        };
        self.out.write(&text);
        if color.is_some() {
            let off = self.colors.disable();
            self.out.write(off);
        }
    }

    /// `display`: every block through every format string.
    fn display(&mut self) -> Result<(), Fatal> {
        let mut fss = std::mem::take(&mut self.fss);
        while self.get()? {
            let block = self.curp.clone();
            let saveaddress = self.address;
            for fs in &mut fss {
                let mut bp = 0usize;
                for fu in &mut fs.fus {
                    if fu.flags & F_IGNORE != 0 {
                        break;
                    }
                    let mut cnt = fu.reps;
                    while cnt > 0 {
                        for pr in &mut fu.prs {
                            if self.eaddress != 0
                                && self.address >= self.eaddress
                                && pr.flags & (F_TEXT | F_BPAD) == 0
                            {
                                bpad(pr);
                            }
                            let here = block.get(bp..).unwrap_or_default();
                            let fmt = match pr.nospace {
                                Some(ns) if cnt == 1 => {
                                    pr.fmt.get(..ns).unwrap_or_default().to_vec()
                                }
                                _ => pr.fmt.clone(),
                            };
                            let snapshot = pr.clone();
                            self.print(&snapshot, here, &fmt);
                            let step = usize::try_from(pr.bcnt).unwrap_or(0);
                            self.address = self.address.wrapping_add(i64::from(pr.bcnt));
                            bp = bp.saturating_add(step);
                        }
                        cnt = cnt.saturating_sub(1);
                    }
                }
                self.address = saveaddress;
            }
        }
        if let Some((fsi, fui)) = self.endfu {
            if self.eaddress == 0 {
                if self.address == 0 {
                    self.fss = fss;
                    return Ok(());
                }
                self.eaddress = self.address;
            }
            if let Some(fu) = fss.get(fsi).and_then(|fs| fs.fus.get(fui)) {
                for pr in &fu.prs {
                    let color = if self.colors.wanted() && pr.colors.is_some() {
                        self.color_cond(pr, &[], pr.bcnt)
                    } else {
                        None
                    };
                    if let Some(c) = &color {
                        let on = self.colors.enable(c).to_vec();
                        self.out.write(&on);
                    }
                    match pr.flags {
                        F_ADDRESS => {
                            let conv = pr.fmt.last().copied().unwrap_or(b'x');
                            let text = if conv == b'd' {
                                c_printf(&pr.fmt, Value::Signed(self.eaddress))
                            } else {
                                c_printf(
                                    &pr.fmt,
                                    Value::Unsigned(u64::from_le_bytes(
                                        self.eaddress.to_le_bytes(),
                                    )),
                                )
                            };
                            self.out.write(&text);
                        }
                        F_TEXT => self.out.write(&pr.fmt),
                        _ => {}
                    }
                    if color.is_some() {
                        let off = self.colors.disable();
                        self.out.write(off);
                    }
                }
            }
        }
        self.fss = fss;
        Ok(())
    }
}

/// The first `N` bytes at `bp`, the rest zero if there are fewer.
fn bytes_at<const N: usize>(bp: &[u8]) -> [u8; N] {
    let mut b = [0u8; N];
    for (k, slot) in b.iter_mut().enumerate() {
        *slot = bp.get(k).copied().unwrap_or(0);
    }
    b
}

/// `bpad`: a unit past the end of the data, printed as blanks of its width:
/// its conversion made `%s` of nothing, its flags but `.` dropped.
fn bpad(pr: &mut Pr) {
    pr.flags = F_BPAD;
    pr.fmt.truncate(pr.cchar);
    pr.fmt.push(b's');
    if let Some(pct) = pr.fmt.iter().position(|&c| c == b'%') {
        let start = pct.saturating_add(1);
        let mut end = start;
        while pr.fmt.get(end).is_some_and(|c| b" -0+#".contains(c)) {
            end = end.saturating_add(1);
        }
        pr.fmt.drain(start..end);
        pr.cchar = pr.fmt.len().saturating_sub(1);
    }
    pr.nospace = pr.nospace.filter(|&n| n < pr.fmt.len());
}

/// `conv_c`: `%_c`.
fn conv_c(with: &dyn Fn(u8) -> Vec<u8>, c: u8) -> Vec<u8> {
    let named: Option<&[u8]> = match c {
        0 => Some(b"\\0"),
        0x07 => Some(b"\\a"),
        0x08 => Some(b"\\b"),
        0x0c => Some(b"\\f"),
        b'\n' => Some(b"\\n"),
        b'\r' => Some(b"\\r"),
        b'\t' => Some(b"\\t"),
        0x0b => Some(b"\\v"),
        _ => None,
    };
    if let Some(s) = named {
        return c_printf(&with(b's'), Value::Text(s));
    }
    if is_print(c) {
        c_printf(&with(b'c'), Value::Byte(c))
    } else {
        let s = format!("{c:03o}");
        c_printf(&with(b's'), Value::Text(s.as_bytes()))
    }
}

/// `conv_u`: `%_u`.
fn conv_u(with: &dyn Fn(u8) -> Vec<u8>, c: u8) -> Vec<u8> {
    const LIST: [&[u8]; 32] = [
        b"nul", b"soh", b"stx", b"etx", b"eot", b"enq", b"ack", b"bel", b"bs", b"ht", b"lf", b"vt",
        b"ff", b"cr", b"so", b"si", b"dle", b"dc1", b"dc2", b"dc3", b"dc4", b"nak", b"syn", b"etb",
        b"can", b"em", b"sub", b"esc", b"fs", b"gs", b"rs", b"us",
    ];
    if c <= 0x1f {
        c_printf(
            &with(b's'),
            Value::Text(LIST.get(usize::from(c)).copied().unwrap_or_default()),
        )
    } else if c == 0x7f {
        c_printf(&with(b's'), Value::Text(b"del"))
    } else if is_print(c) {
        c_printf(&with(b'c'), Value::Byte(c))
    } else {
        c_printf(&with(b'x'), Value::Unsigned(u64::from(c)))
    }
}

/// Upstream's `usage()`, for the name it was invoked as.
fn usage(prog: &str) -> String {
    format!(
        "\nUsage:\n {prog} [options] <file>...\n\n\
         Display file contents in hexadecimal, decimal, octal, or ascii.\n\n\
         Options:\n \
         -b, --one-byte-octal      one-byte octal display\n \
         -c, --one-byte-char       one-byte character display\n \
         -C, --canonical           canonical hex+ASCII display\n \
         -d, --two-bytes-decimal   two-byte decimal display\n \
         -o, --two-bytes-octal     two-byte octal display\n \
         -x, --two-bytes-hex       two-byte hexadecimal display\n \
         -L, --color[=<mode>]      interpret color formatting specifiers\n\
         \x20                            {}\n \
         -e, --format <format>     format string to be used for displaying data\n \
         -f, --format-file <file>  file that contains format strings\n \
         -n, --length <length>     interpret only length bytes of input\n \
         -s, --skip <offset>       skip offset bytes from the beginning\n \
         -v, --no-squeezing        output identical lines\n\n \
         -h, --help                display this help\n \
         -V, --version             display version\n\n\
         Arguments:\n \
         <length> and <offset> arguments may be followed by the suffixes for\n   \
         GiB, TiB, PiB, EiB, ZiB, and YiB (the \"iB\" is optional)\n\n\
         For more details see hexdump(1).\n",
        ulcolors::usage_colors_default()
    )
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv0 = std::env::args_os().next().unwrap_or_default();
    let base = {
        let b = os_bytes(&argv0).into_owned();
        let start = b
            .iter()
            .rposition(|&c| c == b'/')
            .map_or(0, |p| p.saturating_add(1));
        b.get(start..).unwrap_or_default().to_vec()
    };
    let (program, prog) = if base == b"hd" {
        (HD, "hd")
    } else {
        (HEXDUMP, "hexdump")
    };
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut stdout = ulclosestream::Stdout::new(1);
    let mut input = StdioReader::stdin();
    let status = match run(&argv, program, prog, &mut stdout, &mut input) {
        Ok(code) => code,
        Err(Fatal(message)) => {
            warn_bytes(prog, &message, None);
            1
        }
    };
    let (code, exited) = stdout.close_exits(status, prog.as_bytes());
    if !exited {
        // `exit`'s own cleanup, after `close_stdout`: standard input, if it
        // was read, is given back what was read ahead of what was used --
        // so `{ hexdump -n 5; cat; } < file` leaves `cat` the rest.
        input.exit_sync();
    }
    ExitCode::from(code)
}

fn run(
    argv: &[OsString],
    program: Program,
    prog: &'static str,
    out: &mut ulclosestream::Stdout,
    input: &mut StdioReader,
) -> Result<u8, Fatal> {
    let mut fss: Vec<Fs> = Vec::new();
    let mut length: i64 = -1;
    let mut skip: i64 = 0;
    let mut vflag = VFlag::First;
    let mut colormode = ColorMode::Undef;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in program.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                // getopt's own complaint, then `errtryhelp`.
                ulclosestream::stderr_write(format!("{prog}: {}\n", e.sentence).as_bytes());
                ulclosestream::stderr_write(
                    format!("Try '{prog} --help' for more information.\n").as_bytes(),
                );
                return Ok(1);
            }
        };
        let canned = |f: &[u8], fss: &mut Vec<Fs>| -> Result<(), Fatal> {
            fss.push(add_fmt(HEX_OFFT)?);
            fss.push(add_fmt(f)?);
            Ok(())
        };
        match opt {
            Opt::Short(b'b', _) | Opt::Long("one-byte-octal", _) => {
                canned(b"\"%07.7_ax \" 16/1 \"%03o \" \"\\n\"", &mut fss)?;
            }
            Opt::Short(b'c', _) | Opt::Long("one-byte-char", _) => {
                canned(b"\"%07.7_ax \" 16/1 \"%3_c \" \"\\n\"", &mut fss)?;
            }
            Opt::Short(b'C', _) | Opt::Long("canonical", _) => {
                fss.push(add_fmt(b"\"%08.8_Ax\n\"")?);
                fss.push(add_fmt(
                    b"\"%08.8_ax  \" 8/1 \"%02x \" \"  \" 8/1 \"%02x \" ",
                )?);
                fss.push(add_fmt(b"\"  |\" 16/1 \"%_p\" \"|\\n\"")?);
            }
            Opt::Short(b'd', _) | Opt::Long("two-bytes-decimal", _) => {
                canned(b"\"%07.7_ax \" 8/2 \"  %05u \" \"\\n\"", &mut fss)?;
            }
            Opt::Short(b'e', v) | Opt::Long("format", v) => {
                fss.push(add_fmt(&os_bytes(&v.unwrap_or_default()))?);
            }
            Opt::Short(b'f', v) | Opt::Long("format-file", v) => {
                addfile(&os_bytes(&v.unwrap_or_default()), &mut fss)?;
            }
            Opt::Short(b'L', v) | Opt::Long("color", v) => {
                colormode = ColorMode::Auto;
                if let Some(v) = v {
                    colormode = ulcolors::colormode_or_err(&os_bytes(&v))
                        .map_err(|bad| Fatal(cat(&[b"unsupported color mode: '", &bad, b"'"])))?;
                }
            }
            Opt::Short(b'n', v) | Opt::Long("length", v) => {
                length = size_arg(&v.unwrap_or_default(), "failed to parse length")?;
            }
            Opt::Short(b'o', _) | Opt::Long("two-bytes-octal", _) => {
                canned(b"\"%07.7_ax \" 8/2 \" %06o \" \"\\n\"", &mut fss)?;
            }
            Opt::Short(b's', v) | Opt::Long("skip", v) => {
                skip = size_arg(&v.unwrap_or_default(), "failed to parse offset")?;
            }
            Opt::Short(b'v', _) | Opt::Long("no-squeezing", _) => vflag = VFlag::All,
            Opt::Short(b'x', _) | Opt::Long("two-bytes-hex", _) => {
                canned(b"\"%07.7_ax \" 8/2 \"   %04x \" \"\\n\"", &mut fss)?;
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                out.write(usage(prog).as_bytes());
                return Ok(0);
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                out.write(format!("{prog} from SlateOS coreutils 0.1.0\n").as_bytes());
                return Ok(0);
            }
            Opt::Operand(v) => operands.push(os_bytes(v).into_owned()),
            Opt::Short(..) | Opt::Long(..) => {
                ulclosestream::stderr_write(
                    format!("Try '{prog} --help' for more information.\n").as_bytes(),
                );
                return Ok(1);
            }
        }
    }
    if fss.is_empty() {
        if prog == "hd" {
            fss.push(add_fmt(b"\"%08.8_Ax\n\"")?);
            fss.push(add_fmt(
                b"\"%08.8_ax  \" 8/1 \"%02x \" \"  \" 8/1 \"%02x \" ",
            )?);
            fss.push(add_fmt(b"\"  |\" 16/1 \"%_p\" \"|\\n\"")?);
        } else {
            fss.push(add_fmt(HEX_OFFT)?);
            fss.push(add_fmt(b"\"%07.7_ax \" 8/2 \"%04x \" \"\\n\"")?);
        }
    }
    let colors = Colors::init(colormode, b"hexdump", stdfd::is_tty(1));

    let mut blocksize: i32 = 0;
    for fs in &mut fss {
        fs.bcnt = block_size(fs)?;
        if fs.bcnt > blocksize {
            blocksize = fs.bcnt;
        }
    }
    let mut endfu = None;
    for (k, fs) in fss.iter_mut().enumerate() {
        rewrite_rules(fs, blocksize, &colors, &mut endfu, k)?;
    }

    // `_argv[-1]` while standard input is read: the argument before the
    // first operand -- the last option, or the program itself.
    let stdin_name = argv
        .last()
        .map_or_else(|| prog.as_bytes().to_vec(), |a| os_bytes(a).into_owned());
    let mut dump = Dump {
        fss,
        blocksize: usize::try_from(blocksize).unwrap_or(0),
        exitval: 0,
        length,
        skip,
        vflag,
        endfu,
        address: 0,
        eaddress: 0,
        curp: Vec::new(),
        savp: Vec::new(),
        started: false,
        ateof: true,
        files: operands,
        next_file: 0,
        done: 0,
        stdin_name,
        current: Vec::new(),
        input,
        input_closed: false,
        read_errno: None,
        colors,
        prog,
        out,
    };
    dump.display()?;
    Ok(dump.exitval)
}

/// `strtosize_or_err (arg, errmsg)`, as `-n` and `-s` read their values.
fn size_arg(arg: &OsString, errmsg: &str) -> Result<i64, Fatal> {
    let bytes = os_bytes(arg);
    let v = ulstrutils::parse_size(&bytes)
        .map_err(|e| Fatal(ulstrutils::size_error_message(errmsg, arg, e).into_bytes()))?;
    // An `off_t` / `ssize_t` from a `uintmax_t`: the same 64 bits.
    Ok(i64::from_le_bytes(v.to_le_bytes()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_format_into_units() {
        let fs = add_fmt(b"\"%07.7_ax \" 8/2 \"%04x \" \"\\n\"").unwrap();
        assert_eq!(fs.fus.len(), 3);
        assert_eq!((fs.fus[1].reps, fs.fus[1].bcnt), (8, 2));
        assert_eq!(fs.fus[2].fmt, b"\n");
        assert!(add_fmt(b"8/2 %04x").is_err());
        assert!(add_fmt(b"\"unterminated").is_err());
    }

    #[test]
    fn escapes_and_a_trailing_backslash() {
        assert_eq!(escape(b"a\\tb\\\\c\\q"), b"a\tb\\cq");
        assert_eq!(escape(b"ab\\"), b"ab");
    }

    #[test]
    fn block_sizes() {
        let fs = add_fmt(b"\"%07.7_ax \" 8/2 \"%04x \" \"\\n\"").unwrap();
        assert_eq!(block_size(&fs).unwrap(), 16);
        let fs = add_fmt(b"\"  |\" 16/1 \"%_p\" \"|\\n\"").unwrap();
        assert_eq!(block_size(&fs).unwrap(), 16);
        let fs = add_fmt(b"\"%5.3s\"").unwrap();
        assert_eq!(block_size(&fs).unwrap(), 3);
    }

    #[test]
    fn printf_directives() {
        assert_eq!(c_printf(b"%07.7llx", Value::Unsigned(0x10)), b"0000010");
        assert_eq!(c_printf(b"x%04llx", Value::Unsigned(0xab)), b"x00ab");
        assert_eq!(c_printf(b"  %05llu", Value::Unsigned(7)), b"  00007");
        assert_eq!(c_printf(b"%3c", Value::Byte(b'a')), b"  a");
        assert_eq!(c_printf(b"%7.7s", Value::Text(b"")), b"       ");
    }

    #[test]
    fn padding_units_past_the_end() {
        // A conversion's format never holds its trailing blank: that is a
        // text unit of its own, which `nospace` trims.
        let mut pr = Pr {
            flags: F_UINT,
            bcnt: 2,
            fmt: b"%04llx".to_vec(),
            cchar: 3,
            colors: None,
            nospace: None,
        };
        bpad(&mut pr);
        assert_eq!(pr.fmt, b"%4s");
        assert_eq!(pr.flags, F_BPAD);
    }
}
