//! stat — display file or filesystem status.
//!
//! ```text
//! stat [OPTION]... FILE...
//!   -L, --dereference    follow symbolic links (default: report the link)
//!   -f, --file-system    report the filesystem the file lives on, not the file
//!   -c, --format=FORMAT  print FORMAT, then a newline, for each operand
//!       --printf=FORMAT  as -c, but interpret backslash escapes and add no
//!                        trailing newline
//!   -t, --terse          one line of bare numbers, for scripts
//!       --cached=MODE    accepted for compatibility; has no effect here
//!       --help  --version
//! ```
//!
//! # Why this is written against argv bytes and against GNU's own source
//!
//! `stat` is read by scripts more than by people: `size=$(stat -c %s "$f")` is
//! the single most common invocation of it anywhere, and its output is parsed.
//! Two consequences shape everything below.
//!
//! **A file name is bytes.** `main` takes `args_os`, every name is carried as
//! `&[u8]`, and a format string is `Vec<u8>` rather than `String`. The previous
//! version collected `Vec<String>`, so `stat "$f"` *panicked* — not failed,
//! panicked — for any name holding a byte that is not UTF-8, which this OS
//! permits (`design.txt`: every byte but `/` and NUL is legal in a name). The
//! format string is bytes for the same reason: GNU passes a non-UTF-8 format
//! through unchanged, and a `String` cannot hold one.
//!
//! **The width/precision layer is a real `printf`.** GNU implements `%-10s`,
//! `%#o`, `%04a`, `%.3Y` and the rest by rewriting each directive into a C
//! format string with a *per-specifier* set of allowed flags, so `%+d` prints
//! `2096` and not `+2096`. Scripts depend on the exact column widths that fall
//! out of this — the default human-readable block is itself written in those
//! directives. It is reimplemented here rather than approximated: see [`Conv`],
//! [`Kind`] and [`out_number`].
//!
//! # Two deliberate differences from GNU
//!
//! **`%N` is always quoted.** Upstream only consults `QUOTING_STYLE` when the
//! *user* supplied a format containing `%N`; the built-in human-readable block
//! contains `%N` too, but is rendered with the library default, which is
//! literal. A file named with an embedded newline therefore makes GNU's own
//! `File:` line contain a raw newline, and whoever chose the name can forge a
//! `Size:` or `Access:` line after it. Here `%N` is quoted in both paths,
//! defaulting to `shell-escape-always` exactly as `-c '%N'` already did.
//!
//! **`%C` prints `?` with no diagnostic.** There is no security-context
//! concept on this OS, so this is not a lookup that failed but a field that
//! does not exist — the case where GNU itself prints a silent `?` (filesystem
//! `%t` on a kernel without `f_type`). Upstream instead reports
//! `failed to get security context` and exits 1, which would make `stat` fail
//! on every file for a format nobody here can satisfy. `%C` is left out of
//! `--help` for the same reason. See `design-decisions.md`.
//!
//! Times are rendered in the machine's zone via `localtime`, from the same
//! `%Y-%m-%d %H:%M:%S.%N %z` template `nstrftime` is handed upstream.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{self, Style};
use localtime::{Zone, strftime};
use modechange::{S_IFBLK, S_IFCHR, S_IFMT, S_IFREG, file_type_name, mode_string};
use std::ffi::OsString;

/// Name and usage-exit status, for every diagnostic this program can print.
const STAT: Program = Program::new("stat", 1);

// ===========================================================================
// Platform-neutral model
// ===========================================================================
//
// Everything below the syscalls is expressed over these two plain structs
// rather than over `std::fs::Metadata`. `Metadata`'s unix accessors only exist
// behind `cfg(unix)`, and the build host is Windows — so anything written
// against them is invisible to `cargo test --workspace` and is, in practice,
// untested. That is how a `stat` with no argument parser survived.

/// The fields of `struct stat` this program can print.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StatInfo {
    dev: u64,
    ino: u64,
    /// Full mode word: file-type bits *and* permission bits.
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    rdev: u64,
    size: u64,
    blksize: u64,
    /// 512-byte blocks actually allocated, which is not `size / 512` for a
    /// sparse or a compressed file — that difference is the reason `%b` exists.
    blocks: u64,
    atime: i64,
    atime_nsec: u32,
    mtime: i64,
    mtime_nsec: u32,
    ctime: i64,
    ctime_nsec: u32,
    /// Creation time, when the filesystem records one. `None` is not an error:
    /// `%w` prints `-` and `%W` prints `0`, which is what GNU does for a
    /// filesystem that has no birth time to give.
    btime: Option<(i64, u32)>,
}

/// The fields of `struct statvfs` this program can print.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct FsInfo {
    fsid: u64,
    namelen: u64,
    bsize: u64,
    frsize: u64,
    blocks: u64,
    bfree: u64,
    bavail: u64,
    files: u64,
    ffree: u64,
}

// ===========================================================================
// Mode, type and device helpers
// ===========================================================================

/// `%F` for a regular file of zero length is `regular empty file`, which is
/// GNU's one special case and the one people grep for.
fn file_type_name_sized(mode: u32, size: u64) -> &'static str {
    if mode & S_IFMT == S_IFREG && size == 0 {
        "regular empty file"
    } else {
        file_type_name(mode)
    }
}

/// Major device number, in the encoding Linux has used since 2.6.
///
/// The obvious `rdev >> 8` is the *pre*-2.6 encoding and silently returns the
/// wrong number for any minor above 255 — which is most of them on a modern
/// system, `/dev/sda17` included.
const fn major(rdev: u64) -> u64 {
    ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff)
}

/// Minor device number; see [`major`] for why this is not `rdev & 0xff`.
const fn minor(rdev: u64) -> u64 {
    (rdev & 0xff) | ((rdev >> 12) & !0xff)
}

/// True for the two file types whose default block carries a `Device type:`
/// field. GNU renders those with a second, wider format string; see
/// [`default_format`].
const fn is_device(mode: u32) -> bool {
    let t = mode & S_IFMT;
    t == S_IFBLK || t == S_IFCHR
}

// ===========================================================================
// Time formatting
// ===========================================================================

/// `%x`, `%y`, `%z` and `%w`: `2024-06-15 12:30:45.123456789 -0400`.
///
/// The template is GNU's, character for character — upstream hands exactly
/// this string to `nstrftime`. Rendering it through the shared `localtime`
/// crate rather than a private calendar is what makes `stat`, `ls -l`, `date`
/// and `touch` agree about what time it is; the version this replaces printed
/// a hard-coded `+0000` because it predated that crate.
fn human_time(zone: &Zone, secs: i64, nsec: u32) -> Vec<u8> {
    strftime(b"%Y-%m-%d %H:%M:%S.%N %z", &zone.local(secs, nsec))
}

// ===========================================================================
// The printf layer
// ===========================================================================

/// The flag characters GNU accepts between `%` and the width, in its own
/// order. A character outside this set ends the directive's prefix, which is
/// how `%Hd` is told apart from `%-d`.
const PRINTF_FLAGS: &[u8] = b"'-+ #0I";

/// One directive's `printf` prefix: everything between the `%` and the
/// specifier letter.
///
/// Kept as parsed flags rather than as the raw bytes because the flags are
/// filtered *per specifier* before use — `%+d` is `%d` with the `+` dropped,
/// not an error and not a signed number. See [`filter`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Conv {
    /// `'` — thousands grouping. Parsed and dropped: the C locale groups
    /// nothing, and that is the only locale here.
    thousands: bool,
    /// `-` — pad on the right.
    left: bool,
    /// `+` — always print a sign.
    plus: bool,
    /// ` ` — a space where a `+` would go.
    space: bool,
    /// `#` — alternate form: a leading `0` for octal, `0x` for hex.
    alt: bool,
    /// `0` — pad with zeros rather than spaces. Ignored when a precision is
    /// given, as in C.
    zero: bool,
    /// `I` — locale digits. Parsed and dropped; it is in no specifier's
    /// allowed set upstream either.
    intl: bool,
    width: usize,
    /// `Some` when a `.` was present at all, even with no digits after it.
    /// `%.d` is precision zero in C, but `%.Y` is *nine* digits of fraction in
    /// `stat` — so the two facts are kept apart rather than folded together.
    precision: Option<usize>,
    /// Whether digits followed the `.`. Only [`out_epoch`] cares.
    precision_digits: bool,
}

/// Read a run of decimal digits, saturating rather than wrapping on a width
/// nobody could allocate anyway. Returns the value and how many bytes it took.
fn read_digits(b: &[u8]) -> (usize, usize) {
    let mut value: usize = 0;
    let mut n = 0;
    while let Some(&d) = b.get(n) {
        if !d.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(d.wrapping_sub(b'0')));
        n = n.saturating_add(1);
    }
    (value, n)
}

/// Parse the prefix of a directive: `after` starts just past the `%`.
///
/// Returns the prefix and its length in bytes, so the caller can find the
/// specifier letter. This is GNU's `format_code_offset` with the result
/// decoded instead of left as an offset into the original string.
fn parse_conv(after: &[u8]) -> (Conv, usize) {
    let mut conv = Conv::default();
    let mut i = 0;
    while let Some(&b) = after.get(i) {
        if !PRINTF_FLAGS.contains(&b) {
            break;
        }
        match b {
            b'\'' => conv.thousands = true,
            b'-' => conv.left = true,
            b'+' => conv.plus = true,
            b' ' => conv.space = true,
            b'#' => conv.alt = true,
            b'0' => conv.zero = true,
            _ => conv.intl = true,
        }
        i = i.saturating_add(1);
    }
    let (width, used) = read_digits(after.get(i..).unwrap_or_default());
    conv.width = width;
    i = i.saturating_add(used);
    if after.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        let (prec, used) = read_digits(after.get(i..).unwrap_or_default());
        conv.precision = Some(prec);
        conv.precision_digits = used > 0;
        i = i.saturating_add(used);
    }
    (conv, i)
}

/// Which C conversion a specifier ends up as, which is what decides its flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// `%s`-like: `%n`, `%N`, `%A`, `%F`, `%U`, `%G`, `%T`, `%m`, `%x`.
    Str,
    /// `%ld`: the filesystem's signed block and inode counts.
    Int,
    /// `%ju`: every unsigned field.
    Uint,
    /// `%jo`: `%a` alone.
    Octal,
    /// `%jx`: `%D`, `%f`, `%t`, `%T` (file), `%R`, `%i` (filesystem).
    Hex,
}

/// The flags GNU's `make_format` copies through for each conversion, verbatim
/// from `out_string` / `out_int` / `out_uint` / `out_uint_o` / `out_uint_x`.
///
/// Everything else is silently dropped — which is *observable*: `%+d` on an
/// unsigned field prints no sign, and `%#o` on the I/O block size prints no
/// leading zero, because `#` is not in `out_uint`'s set.
const fn allowed(kind: Kind) -> &'static [u8] {
    match kind {
        Kind::Str => b"-",
        Kind::Int => b"'-+ 0",
        Kind::Uint => b"'-0",
        Kind::Octal | Kind::Hex => b"-#0",
    }
}

/// Drop every flag this conversion does not accept.
fn filter(conv: Conv, kind: Kind) -> Conv {
    let ok = allowed(kind);
    Conv {
        thousands: conv.thousands && ok.contains(&b'\''),
        left: conv.left && ok.contains(&b'-'),
        plus: conv.plus && ok.contains(&b'+'),
        space: conv.space && ok.contains(&b' '),
        alt: conv.alt && ok.contains(&b'#'),
        zero: conv.zero && ok.contains(&b'0'),
        // `I` is in no set upstream; it is parsed only so that it does not end
        // the prefix and turn the rest of the directive into literal text.
        intl: false,
        width: conv.width,
        precision: conv.precision,
        precision_digits: conv.precision_digits,
    }
}

/// Pad `body` to `conv.width`, honouring `-` and (when `zero_at` is `Some`)
/// `0`. `zero_at` is the byte offset zeros must be inserted at — after a sign
/// and any `0x`, so that `%#010x` is `0x0000002a` and not `000000x2a`.
fn pad_to_width(conv: Conv, body: Vec<u8>, zero_at: Option<usize>) -> Vec<u8> {
    let fill = conv.width.saturating_sub(body.len());
    if fill == 0 {
        return body;
    }
    let mut out = Vec::with_capacity(conv.width);
    if conv.left {
        out.extend_from_slice(&body);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if let Some(at) = zero_at.filter(|_| conv.zero) {
        let at = at.min(body.len());
        out.extend_from_slice(body.get(..at).unwrap_or_default());
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(body.get(at..).unwrap_or_default());
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(&body);
    }
    out
}

/// `%s`: precision truncates, width pads with spaces and never with zeros.
///
/// Truncation is by *bytes*, as C's is. A name is bytes here and a `%.3n` that
/// split a multi-byte character would be no worse than one that did not — but
/// it would differ from GNU, and this output is parsed.
fn out_string(out: &mut Vec<u8>, conv: Conv, text: &[u8]) {
    let conv = filter(conv, Kind::Str);
    let body = match conv.precision {
        Some(p) => text.get(..p).unwrap_or(text).to_vec(),
        None => text.to_vec(),
    };
    out.extend_from_slice(&pad_to_width(conv, body, None));
}

/// Every numeric conversion, over an already-rendered magnitude.
///
/// `digits` is the absolute value in the target base with no sign, no prefix
/// and no padding; `negative` is its sign. Splitting it this way is what lets
/// one function serve `%ju`, `%jo`, `%jx` and `%ld` — the differences between
/// them are entirely in the flags, which [`filter`] has already applied.
fn out_number(out: &mut Vec<u8>, conv: Conv, kind: Kind, negative: bool, digits: &str) {
    let conv = filter(conv, kind);
    let bytes = digits.as_bytes();

    // Precision is a *minimum digit count*, and precision zero prints nothing
    // at all for a zero value — `stat -c %.0s` on an empty file is empty.
    let mut core: Vec<u8> = match conv.precision {
        Some(0) if digits == "0" => Vec::new(),
        Some(p) => {
            let mut v = Vec::with_capacity(p.max(bytes.len()));
            v.extend(std::iter::repeat_n(b'0', p.saturating_sub(bytes.len())));
            v.extend_from_slice(bytes);
            v
        }
        None => bytes.to_vec(),
    };

    // `#`: octal grows a leading zero only if it does not already have one,
    // which is why `%#.5a` on 0644 is `00644` and not `000644`.
    let mut prefix: Vec<u8> = Vec::new();
    if conv.alt {
        match kind {
            Kind::Octal if core.first() != Some(&b'0') => core.insert(0, b'0'),
            Kind::Hex if digits != "0" => prefix.extend_from_slice(b"0x"),
            _ => {}
        }
    }

    let sign: &[u8] = if negative {
        b"-"
    } else if conv.plus {
        b"+"
    } else if conv.space {
        b" "
    } else {
        b""
    };

    let mut body = Vec::with_capacity(
        sign.len()
            .saturating_add(prefix.len())
            .saturating_add(core.len()),
    );
    body.extend_from_slice(sign);
    body.extend_from_slice(&prefix);
    body.extend_from_slice(&core);
    // C ignores `0` when a precision is given; so does GNU here, so `%08.3s`
    // on a 5-byte file is five spaces and `005`.
    let zero_at = if conv.precision.is_some() {
        None
    } else {
        Some(sign.len().saturating_add(prefix.len()))
    };
    out.extend_from_slice(&pad_to_width(conv, body, zero_at));
}

/// `%X`, `%Y`, `%Z`, `%W`: seconds since the epoch, with an optional fraction.
///
/// The precision rule is `stat`'s own and not C's: **no** `.` means whole
/// seconds, a bare `.` means all nine digits, and `.N` means N. The awkward
/// part is a negative time with a non-zero fraction — `-1.5s` is stored as
/// `secs = -2, nsec = 500000000`, and must print as `-1.500`. That is a borrow
/// back out of the fraction, and it is why the seconds may round to zero and
/// still need a minus sign in front of them (`-0.500`).
fn out_epoch(out: &mut Vec<u8>, conv: Conv, secs: i64, nsec: u32) {
    let conv = filter(conv, Kind::Int);
    let precision = match conv.precision {
        None => 0,
        Some(_) if !conv.precision_digits => 9,
        Some(p) => p,
    };

    let mut sec = secs;
    let mut frac: u64 = 0;
    let mut minus_zero = false;
    if precision > 0 {
        // 10^(9 - min(precision, 9)); a precision past nine is padded with
        // literal zeros rather than invented digits.
        let kept = precision.min(9);
        let mut divisor: u64 = 1;
        for _ in kept..9 {
            divisor = divisor.saturating_mul(10);
        }
        frac = u64::from(nsec) / divisor;
        if secs < 0 && nsec != 0 {
            let modulus = 1_000_000_000_u64 / divisor;
            let lost = u64::from(nsec) % divisor != 0;
            frac = modulus.saturating_sub(frac).saturating_sub(u64::from(lost));
            if frac != 0 {
                sec = sec.saturating_add(1);
            }
            minus_zero = sec == 0;
        }
    }

    let negative = sec < 0 || minus_zero;
    let mut digits = sec.unsigned_abs().to_string();
    if precision > 0 {
        digits.push('.');
        let kept = precision.min(9);
        let text = frac.to_string();
        for _ in text.len()..kept {
            digits.push('0');
        }
        digits.push_str(&text);
        // A precision above nine: real nanoseconds, then zeros.
        for _ in 9..precision {
            digits.push('0');
        }
    }

    // The precision has been consumed by the fraction, so what is left is a
    // plain signed integer conversion whose width covers the whole thing.
    let padded = Conv {
        precision: None,
        precision_digits: false,
        ..conv
    };
    out_number(out, padded, Kind::Int, negative, &digits);
}

/// `%ju`.
fn uint(out: &mut Vec<u8>, conv: Conv, v: u64) {
    out_number(out, conv, Kind::Uint, false, &v.to_string());
}

/// `%ld`, for the filesystem counters GNU prints signed.
fn int(out: &mut Vec<u8>, conv: Conv, v: i64) {
    out_number(out, conv, Kind::Int, v < 0, &v.unsigned_abs().to_string());
}

/// `%jo`.
fn octal(out: &mut Vec<u8>, conv: Conv, v: u64) {
    out_number(out, conv, Kind::Octal, false, &format!("{v:o}"));
}

/// `%jx`.
fn hex(out: &mut Vec<u8>, conv: Conv, v: u64) {
    out_number(out, conv, Kind::Hex, false, &format!("{v:x}"));
}

// ===========================================================================
// Scanning a format string
// ===========================================================================

/// One item of a scanned format, in the order it was written.
///
/// A format is scanned once and rendered per operand. That is safe because
/// nothing in a directive depends on the file — and it is *observably* right
/// for the fatal case: GNU detects an invalid directive while printing, so the
/// literal text before it has already reached stdout when the error appears.
/// [`print_it`] reproduces that by stopping at [`Piece::Invalid`] rather than
/// by rejecting the format up front.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Piece {
    /// Literal output, escapes already resolved.
    Bytes(Vec<u8>),
    /// `%` followed by a prefix, an optional `H`/`L` modifier and a specifier.
    Directive { conv: Conv, modifier: u8, spec: u8 },
    /// A `%` prefix with nowhere to go: `%5%` or a trailing `%-`. Fatal, and
    /// carries the text the diagnostic quotes.
    Invalid(Vec<u8>),
    /// `\q` under `--printf`: a warning, then the character itself.
    BadEscape(u8),
    /// A `\` as the last byte of a `--printf` format: a warning, then a `\`.
    TrailingBackslash,
}

/// `\a`, `\b`, `\e`, … — GNU's `print_esc_char` table. `None` means the
/// character is not an escape at all and earns a warning.
const fn escape_char(c: u8) -> Option<u8> {
    match c {
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'e' => Some(0x1B),
        b'f' => Some(0x0C),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0B),
        b'"' => Some(b'"'),
        b'\\' => Some(b'\\'),
        _ => None,
    }
}

/// Break a format into [`Piece`]s.
///
/// `interpret` is `--printf` rather than `-c`. `stat_mode` is the file form
/// rather than `-f`, and decides only one thing: whether `H` and `L` are
/// modifiers. They are not in `-f` mode, so `stat -f -c '%Hd'` prints `?d` —
/// a `?` for the unknown specifier `H`, then a literal `d`.
fn scan(format: &[u8], interpret: bool, stat_mode: bool) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut literal: Vec<u8> = Vec::new();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !literal.is_empty() {
                pieces.push(Piece::Bytes(std::mem::take(&mut literal)));
            }
        };
    }

    while let Some(&b) = format.get(i) {
        match b {
            b'%' => {
                let after = format.get(i.saturating_add(1)..).unwrap_or_default();
                let (conv, prefix_len) = parse_conv(after);
                match after.get(prefix_len) {
                    // `%%` is a literal percent, but only with an empty
                    // prefix: `%5%` asked for something that does not exist.
                    // A `%` prefix that runs into the end of the format is the
                    // same mistake with the specifier missing entirely.
                    None | Some(b'%') => {
                        if prefix_len > 0 {
                            flush!();
                            let mut text = vec![b'%'];
                            text.extend_from_slice(after.get(..prefix_len).unwrap_or_default());
                            if let Some(&c) = after.get(prefix_len) {
                                text.push(c);
                            }
                            pieces.push(Piece::Invalid(text));
                            return pieces;
                        }
                        literal.push(b'%');
                        // Two bytes for `%%`, one for a trailing `%`.
                        i = i.saturating_add(if after.get(prefix_len).is_some() {
                            2
                        } else {
                            1
                        });
                    }
                    Some(&c) => {
                        flush!();
                        let next = after.get(prefix_len.saturating_add(1)).copied();
                        let modifier_applies = stat_mode
                            && (c == b'H' || c == b'L')
                            && matches!(next, Some(b'd' | b'r'));
                        if modifier_applies {
                            pieces.push(Piece::Directive {
                                conv,
                                modifier: c,
                                spec: next.unwrap_or(b'?'),
                            });
                            i = i.saturating_add(prefix_len).saturating_add(3);
                        } else {
                            // `%Hs` is a `?` for `H` and then a literal `s`:
                            // the modifier letter becomes the specifier, and
                            // no specifier matches it.
                            pieces.push(Piece::Directive {
                                conv,
                                modifier: 0,
                                spec: c,
                            });
                            i = i.saturating_add(prefix_len).saturating_add(2);
                        }
                    }
                }
            }
            b'\\' if interpret => {
                let next = format.get(i.saturating_add(1)).copied();
                match next {
                    None => {
                        flush!();
                        pieces.push(Piece::TrailingBackslash);
                        i = i.saturating_add(1);
                    }
                    Some(d) if d.is_ascii_digit() && d < b'8' => {
                        let mut value: u32 = 0;
                        let mut n = 0;
                        while n < 3 {
                            match format.get(i.saturating_add(1).saturating_add(n)) {
                                Some(&o) if (b'0'..b'8').contains(&o) => {
                                    value = value
                                        .saturating_mul(8)
                                        .saturating_add(u32::from(o.wrapping_sub(b'0')));
                                    n = n.saturating_add(1);
                                }
                                _ => break,
                            }
                        }
                        literal.push(u8::try_from(value & 0xff).unwrap_or(0));
                        i = i.saturating_add(1).saturating_add(n);
                    }
                    // `\x` is an escape only when a hex digit follows it.
                    // `\xzz` is the *unrecognized* escape `\x`, warning and
                    // all — a rule worth keeping, since `\x` with no digits is
                    // far more often a typo than an intended `x`.
                    Some(b'x')
                        if format
                            .get(i.saturating_add(2))
                            .is_some_and(u8::is_ascii_hexdigit) =>
                    {
                        let mut value: u32 = 0;
                        let mut n = 0;
                        while n < 2 {
                            match format.get(i.saturating_add(2).saturating_add(n)) {
                                Some(&h) if h.is_ascii_hexdigit() => {
                                    let d = char::from(h).to_digit(16).unwrap_or(0);
                                    value = value.saturating_mul(16).saturating_add(d);
                                    n = n.saturating_add(1);
                                }
                                _ => break,
                            }
                        }
                        literal.push(u8::try_from(value & 0xff).unwrap_or(0));
                        i = i.saturating_add(2).saturating_add(n);
                    }
                    Some(c) => {
                        match escape_char(c) {
                            Some(e) => literal.push(e),
                            None => {
                                flush!();
                                pieces.push(Piece::BadEscape(c));
                            }
                        }
                        i = i.saturating_add(2);
                    }
                }
            }
            _ => {
                literal.push(b);
                i = i.saturating_add(1);
            }
        }
    }
    flush!();
    pieces
}

/// Whether a scanned format asks for a given specifier.
///
/// Used to decide what to *look up*: the account database is only read for a
/// format containing `%U` or `%G`, and the mount table is only walked for
/// `%m`. `stat -c %s` on a thousand files should not open `/etc/passwd`.
fn wants(pieces: &[Piece], spec: u8) -> bool {
    pieces
        .iter()
        .any(|p| matches!(p, Piece::Directive { spec: s, .. } if *s == spec))
}

// ===========================================================================
// Rendering one file's directives
// ===========================================================================

/// Diagnostics, and whether any of them was an error.
///
/// A *warning* — an unrecognized escape, a trailing backslash, a bad
/// `QUOTING_STYLE` — leaves the exit status alone; that was measured, not
/// assumed. An *error* sets it to 1 while still letting the remaining
/// operands run.
#[derive(Default)]
struct Diags {
    fail: bool,
}

impl Diags {
    fn error(&mut self, message: &str) {
        diag!("stat: {message}");
        self.fail = true;
    }

    #[allow(clippy::unused_self)]
    fn warn(&self, message: &str) {
        diag!("stat: {message}");
    }
}

/// Everything about one file that a directive can ask for, resolved before
/// rendering starts so that the renderer itself performs no I/O.
struct FileFacts {
    st: StatInfo,
    /// The operand exactly as typed. Never the canonical path: `stat` reports
    /// on what it was asked about.
    name: Vec<u8>,
    /// `readlink`, for a symlink only. `Err` carries the diagnostic `%N` prints.
    link: Option<Result<Vec<u8>, String>>,
    /// `%m`. `Err(Some(_))` is a diagnostic to print before the `?`.
    mount: Result<Vec<u8>, Option<String>>,
    /// Account names, absent when the id has no entry — `%U` then prints
    /// `UNKNOWN`, which is what a script greps for.
    user: Option<Vec<u8>>,
    group: Option<Vec<u8>>,
    /// How `%N` quotes. See the module docs for why this is never `Literal`
    /// by default.
    style: Style,
}

/// Everything `-f` can ask for.
struct FsFacts {
    fs: FsInfo,
    name: Vec<u8>,
}

/// Which of the two directive tables a format is being rendered against.
enum Target<'a> {
    File(&'a FileFacts),
    Fs(&'a FsFacts),
}

/// GNU's `print_stat`, directive for directive.
fn render_stat(
    out: &mut Vec<u8>,
    diags: &mut Diags,
    conv: Conv,
    modifier: u8,
    spec: u8,
    f: &FileFacts,
    zone: &Zone,
) {
    let st = &f.st;
    // `%Hd`/`%Ld` and `%Hr`/`%Lr` split a device number into its two halves.
    let split = |whole: u64| match modifier {
        b'H' => major(whole),
        b'L' => minor(whole),
        _ => whole,
    };
    match spec {
        b'n' => out_string(out, conv, &f.name),
        b'N' => {
            out_string(out, conv, &f.style.quote(&f.name));
            match &f.link {
                Some(Ok(target)) => {
                    out.extend_from_slice(b" -> ");
                    // The width applies to *each* half, not to the pair. That
                    // is upstream's behaviour and it is what keeps `ls`-like
                    // columns of `%-30N` lined up on both sides of the arrow.
                    out_string(out, conv, &f.style.quote(target));
                }
                Some(Err(message)) => diags.error(message),
                None => {}
            }
        }
        b'd' => uint(out, conv, split(st.dev)),
        b'D' => hex(out, conv, st.dev),
        b'i' => uint(out, conv, st.ino),
        b'a' => octal(out, conv, u64::from(st.mode & 0o7777)),
        b'A' => out_string(out, conv, mode_string(st.mode).as_bytes()),
        b'f' => hex(out, conv, u64::from(st.mode)),
        b'F' => out_string(out, conv, file_type_name_sized(st.mode, st.size).as_bytes()),
        b'h' => uint(out, conv, st.nlink),
        b'u' => uint(out, conv, u64::from(st.uid)),
        b'U' => out_string(out, conv, f.user.as_deref().unwrap_or(b"UNKNOWN")),
        b'g' => uint(out, conv, u64::from(st.gid)),
        b'G' => out_string(out, conv, f.group.as_deref().unwrap_or(b"UNKNOWN")),
        b'm' => match &f.mount {
            Ok(point) => out_string(out, conv, point),
            Err(message) => {
                if let Some(message) = message {
                    diags.error(message);
                }
                out.push(b'?');
            }
        },
        b's' => uint(out, conv, st.size),
        b'r' => uint(out, conv, split(st.rdev)),
        b'R' => hex(out, conv, st.rdev),
        b't' => hex(out, conv, major(st.rdev)),
        b'T' => hex(out, conv, minor(st.rdev)),
        // The unit `%b` counts in, which is 512 whatever the filesystem's own
        // block size is. It is a constant, and printing it is how a script
        // turns `%b` into bytes without hardcoding the number.
        b'B' => uint(out, conv, 512),
        b'b' => uint(out, conv, st.blocks),
        b'o' => uint(out, conv, st.blksize),
        b'w' => match st.btime {
            Some((secs, nsec)) => out_string(out, conv, &human_time(zone, secs, nsec)),
            None => out_string(out, conv, b"-"),
        },
        b'W' => {
            let (secs, nsec) = st.btime.filter(|&(s, _)| s >= 0).unwrap_or((0, 0));
            out_epoch(out, conv, secs, nsec);
        }
        b'x' => out_string(out, conv, &human_time(zone, st.atime, st.atime_nsec)),
        b'X' => out_epoch(out, conv, st.atime, st.atime_nsec),
        b'y' => out_string(out, conv, &human_time(zone, st.mtime, st.mtime_nsec)),
        b'Y' => out_epoch(out, conv, st.mtime, st.mtime_nsec),
        b'z' => out_string(out, conv, &human_time(zone, st.ctime, st.ctime_nsec)),
        b'Z' => out_epoch(out, conv, st.ctime, st.ctime_nsec),
        // `%C` — see the module docs: an absent field, not a failed lookup.
        _ => out.push(b'?'),
    }
}

/// GNU's `print_statfs`, directive for directive.
fn render_statfs(out: &mut Vec<u8>, conv: Conv, spec: u8, f: &FsFacts) {
    let fs = &f.fs;
    match spec {
        b'n' => out_string(out, conv, &f.name),
        b'i' => hex(out, conv, fs.fsid),
        b'l' => uint(out, conv, fs.namelen),
        // `statvfs` carries no filesystem-type field, so there is nothing to
        // print. GNU prints `?` here too on a kernel whose `statfs` lacks
        // `f_type`; this is that same case, not a new one.
        b't' => out.push(b'?'),
        b'T' => out_string(out, conv, b"UNKNOWN"),
        // Signed, as upstream prints them: these come from a kernel field wide
        // enough to look negative, and a script comparing them must see the
        // same text GNU shows.
        b'b' => int(out, conv, fs.blocks as i64),
        b'f' => int(out, conv, fs.bfree as i64),
        b'a' => int(out, conv, fs.bavail as i64),
        b'd' => int(out, conv, fs.ffree as i64),
        b's' => uint(out, conv, fs.bsize),
        b'S' => uint(out, conv, if fs.frsize == 0 { fs.bsize } else { fs.frsize }),
        b'c' => uint(out, conv, fs.files),
        _ => out.push(b'?'),
    }
}

/// Render a scanned format for one operand.
///
/// # Errors
///
/// The text of an invalid directive, which the caller reports and then exits
/// on — after writing whatever `out` already holds, because that is what GNU's
/// print-then-die does.
fn print_it(
    pieces: &[Piece],
    target: &Target<'_>,
    zone: &Zone,
    out: &mut Vec<u8>,
    diags: &mut Diags,
) -> Result<(), Vec<u8>> {
    for piece in pieces {
        match piece {
            Piece::Bytes(b) => out.extend_from_slice(b),
            Piece::Directive {
                conv,
                modifier,
                spec,
            } => match target {
                Target::File(f) => render_stat(out, diags, *conv, *modifier, *spec, f, zone),
                Target::Fs(f) => render_statfs(out, *conv, *spec, f),
            },
            Piece::Invalid(text) => return Err(text.clone()),
            Piece::BadEscape(c) => {
                diags.warn(&format!(
                    "warning: unrecognized escape '\\{}'",
                    char::from(*c)
                ));
                out.push(*c);
            }
            Piece::TrailingBackslash => {
                diags.warn("warning: backslash at end of format");
                out.push(b'\\');
            }
        }
    }
    Ok(())
}

// ===========================================================================
// Formats
// ===========================================================================

/// `--terse`. GNU's constant carries a trailing `%C` on an SELinux build; ours
/// does not, because `%C` here is always `?` and a column of question marks in
/// a machine-read line is worse than no column.
const TERSE_FILE: &[u8] = b"%n %s %b %f %u %g %D %i %h %t %T %X %Y %Z %W %o\n";

/// `--terse --file-system`, verbatim from upstream.
const TERSE_FS: &[u8] = b"%n %i %l %t %s %S %b %f %a %c %d\n";

/// The format used when the caller gave none.
///
/// `device` selects the wider file block that carries `Device type:`; it is
/// chosen per operand, from the file's own type, which is why two formats are
/// scanned up front rather than one.
///
/// These are byte-for-byte GNU 9.x. They are worth copying exactly rather than
/// laying out by eye: the human-readable block is the part of `stat` that
/// people screenshot into bug reports, and every column in it — `%-10s`,
/// `%04a`, `%10.10A`, `%5u`, `%8U` — is a directive the printf layer above has
/// to get right. Any difference here is a difference in that layer.
fn default_format(fs: bool, terse: bool, device: bool) -> Vec<u8> {
    if fs {
        return if terse {
            TERSE_FS.to_vec()
        } else {
            b"  File: \"%n\"\n\
              \x20   ID: %-8i Namelen: %-7l Type: %T\n\
              Block size: %-10s Fundamental block size: %S\n\
              Blocks: Total: %-10b Free: %-10f Available: %a\n\
              Inodes: Total: %-10c Free: %d\n"
                .to_vec()
        };
    }
    if terse {
        return TERSE_FILE.to_vec();
    }
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"  File: %N\n");
    out.extend_from_slice(b"  Size: %-10s\tBlocks: %-10b IO Block: %-6o %F\n");
    if device {
        out.extend_from_slice(b"Device: %Hd,%Ld\tInode: %-10i  Links: %-5h Device type: %Hr,%Lr\n");
    } else {
        out.extend_from_slice(b"Device: %Hd,%Ld\tInode: %-10i  Links: %h\n");
    }
    out.extend_from_slice(b"Access: (%04a/%10.10A)  Uid: (%5u/%8U)   Gid: (%5g/%8G)\n");
    out.extend_from_slice(b"Access: %x\nModify: %y\nChange: %z\n Birth: %w\n");
    out
}

/// How `%N` quotes, from `QUOTING_STYLE`.
///
/// The default is `shell-escape-always`, which is gnulib's when `stat` asks
/// the environment — and, unlike upstream, it is the default in the built-in
/// block too. See the module docs. An unusable value warns and is ignored;
/// upstream warns identically and, measured, does not change the exit status.
fn quoting_style_from_env(diags: &Diags) -> Style {
    let Some(value) = std::env::var_os("QUOTING_STYLE") else {
        return Style::ShellEscapeAlways;
    };
    let bytes = quote::os_bytes(&value);
    STAT.argmatch(&bytes, "--quoting-style", Style::WORDS)
        .unwrap_or_else(|_| {
            diags.warn(&format!(
                "ignoring invalid value of environment variable QUOTING_STYLE: {}",
                quote::quote(&bytes)
            ));
            Style::ShellEscapeAlways
        })
}

// ===========================================================================
// Argument parsing
// ===========================================================================

const SHORT_OPTIONS: &str = "c:fLt";

const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("dereference", Takes::Nothing),
    ("file-system", Takes::Nothing),
    ("format", Takes::Required),
    ("printf", Takes::Required),
    ("terse", Takes::Nothing),
    ("cached", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--cached`'s words. Validated and then ignored: there is no attribute cache
/// to steer here, so all three modes describe the same behaviour. Rejecting a
/// misspelling is still worth doing — a script that says `--cached=nevr` on
/// another system gets an error there and must get one here too.
const CACHED_MODES: &[(&str, u8)] = &[("default", 0), ("never", 1), ("always", 2)];

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Settings {
    dereference: bool,
    filesystem: bool,
    terse: bool,
    /// The format and whether backslash escapes are interpreted — `--printf`
    /// rather than `-c`. `-c` also appends a newline per operand and
    /// `--printf` does not, which is the same distinction.
    format: Option<(Vec<u8>, bool)>,
    files: Vec<OsString>,
}

/// Parse stat's argv.
///
/// # Errors
///
/// An unknown option, a missing option value, or no operands at all.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut settings = Settings::default();
    for opt in STAT.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match opt? {
            Opt::Short(b'L', _) | Opt::Long("dereference", _) => settings.dereference = true,
            Opt::Short(b'f', _) | Opt::Long("file-system", _) => settings.filesystem = true,
            Opt::Short(b't', _) | Opt::Long("terse", _) => settings.terse = true,
            Opt::Short(b'c', value) | Opt::Long("format", value) => {
                let text = value.unwrap_or_default();
                settings.format = Some((quote::os_bytes(&text).into_owned(), false));
            }
            Opt::Long("printf", value) => {
                let text = value.unwrap_or_default();
                settings.format = Some((quote::os_bytes(&text).into_owned(), true));
            }
            Opt::Long("cached", value) => {
                let text = value.unwrap_or_default();
                STAT.argmatch(&quote::os_bytes(&text), "--cached", CACHED_MODES)?;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => settings.files.push(word.clone()),
            // The tables above list nothing else; a short or long option that
            // reached here would already have been rejected by the parser.
            Opt::Short(_, _) | Opt::Long(_, _) => {}
        }
    }
    if settings.files.is_empty() {
        return Err(STAT.usage_referring("missing operand".to_string()));
    }
    Ok(Request::Run(settings))
}

/// GNU's `usage()`, with two honest edits: `%C` is gone (it is always `?`
/// here — see the module docs), and `--cached` says what it actually does.
fn help_text() -> String {
    format!(
        "\
Usage: stat [OPTION]... FILE...
Display file or file system status.

Mandatory arguments to long options are mandatory for short options too.
  -L, --dereference     follow links
  -f, --file-system     display file system status instead of file status
      --cached=MODE     accepted for compatibility and validated, but has no
                          effect: nothing here caches attributes. See MODE below
  -c  --format=FORMAT   use the specified FORMAT instead of the default;
                          output a newline after each use of FORMAT
      --printf=FORMAT   like --format, but interpret backslash escapes,
                          and do not output a mandatory trailing newline;
                          if you want a newline, include \\n in FORMAT
  -t, --terse           print the information in terse form
      --help        display this help and exit
      --version     output version information and exit

The MODE argument of --cached can be: always, never, or default.

The valid format sequences for files (without --file-system):

  %a   permission bits in octal (note '#' and '0' printf flags)
  %A   permission bits and file type in human readable form
  %b   number of blocks allocated (see %B)
  %B   the size in bytes of each block reported by %b
  %d   device number in decimal (st_dev)
  %D   device number in hex (st_dev)
  %Hd  major device number in decimal
  %Ld  minor device number in decimal
  %f   raw mode in hex
  %F   file type
  %g   group ID of owner
  %G   group name of owner
  %h   number of hard links
  %i   inode number
  %m   mount point
  %n   file name
  %N   quoted file name with dereference if symbolic link
  %o   optimal I/O transfer size hint
  %s   total size, in bytes
  %r   device type in decimal (st_rdev)
  %R   device type in hex (st_rdev)
  %Hr  major device type in decimal, for character/block device special files
  %Lr  minor device type in decimal, for character/block device special files
  %t   major device type in hex, for character/block device special files
  %T   minor device type in hex, for character/block device special files
  %u   user ID of owner
  %U   user name of owner
  %w   time of file birth, human-readable; - if unknown
  %W   time of file birth, seconds since Epoch; 0 if unknown
  %x   time of last access, human-readable
  %X   time of last access, seconds since Epoch
  %y   time of last data modification, human-readable
  %Y   time of last data modification, seconds since Epoch
  %z   time of last status change, human-readable
  %Z   time of last status change, seconds since Epoch

Valid format sequences for file systems:

  %a   free blocks available to non-superuser
  %b   total data blocks in file system
  %c   total file nodes in file system
  %d   free file nodes in file system
  %f   free blocks in file system
  %i   file system ID in hex
  %l   maximum length of filenames
  %n   file name
  %s   block size (for faster transfers)
  %S   fundamental block size (for block counts)
  %t   file system type in hex
  %T   file system type in human readable form

--terse is equivalent to the following FORMAT:
    {}
--terse --file-system is equivalent to the following FORMAT:
    {}
",
        String::from_utf8_lossy(TERSE_FILE).trim_end(),
        String::from_utf8_lossy(TERSE_FS).trim_end(),
    )
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    diag!("stat: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    imp::main()
}

#[cfg(unix)]
mod imp {
    use super::{
        Diags, FileFacts, FsFacts, FsInfo, Piece, Request, StatInfo, Target, default_format,
        help_text, is_device, parse_args, print_it, quoting_style_from_env, scan, wants,
    };
    use coreutils::canon::{self, Mode, RealFs};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::pathname::dir_name;
    use coreutils::quote::{self, Style, quote_os, quoteaf, quoteaf_os};
    use localtime::Zone;
    use modechange::{S_IFDIR, S_IFLNK, S_IFMT};
    use pwdb::Db;
    use std::ffi::{CString, OsString};
    use std::fs;
    use std::io::{self, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::process::ExitCode;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The layout the posix crate's `statvfs` writes.
    #[repr(C)]
    #[derive(Default)]
    struct PosixStatvfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
    }

    // SAFETY: `statvfs` is provided by the posix crate with exactly this C
    // signature. It returns 0 on success and -1 with `errno` set on failure.
    unsafe extern "C" {
        fn statvfs(path: *const u8, buf: *mut PosixStatvfs) -> i32;
    }

    /// Split a `SystemTime` into the epoch seconds and nanoseconds `%W`/`%w`
    /// want, keeping a pre-epoch instant a *negative second plus a positive
    /// fraction* — the same representation `struct timespec` uses, and the one
    /// `out_epoch` knows how to borrow out of.
    fn epoch_parts(t: SystemTime) -> (i64, u32) {
        match t.duration_since(UNIX_EPOCH) {
            Ok(d) => (
                i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                d.subsec_nanos(),
            ),
            Err(e) => {
                let d = e.duration();
                let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
                let nanos = d.subsec_nanos();
                if nanos == 0 {
                    (secs.saturating_neg(), 0)
                } else {
                    (
                        secs.saturating_neg().saturating_sub(1),
                        1_000_000_000_u32.saturating_sub(nanos),
                    )
                }
            }
        }
    }

    fn info_from(meta: &fs::Metadata) -> StatInfo {
        StatInfo {
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            nlink: meta.nlink(),
            uid: meta.uid(),
            gid: meta.gid(),
            rdev: meta.rdev(),
            size: meta.size(),
            blksize: meta.blksize(),
            blocks: meta.blocks(),
            atime: meta.atime(),
            atime_nsec: u32::try_from(meta.atime_nsec()).unwrap_or(0),
            mtime: meta.mtime(),
            mtime_nsec: u32::try_from(meta.mtime_nsec()).unwrap_or(0),
            ctime: meta.ctime(),
            ctime_nsec: u32::try_from(meta.ctime_nsec()).unwrap_or(0),
            btime: meta.created().ok().map(epoch_parts),
        }
    }

    /// `%m`: the mount point the file lives on.
    ///
    /// There is no mount table to consult, so this is the walk that does not
    /// need one — climb parent directories while `st_dev` stays the same, and
    /// stop at the first parent on a different device. `/` ends the walk by
    /// being its own parent. Checked against GNU's answers for `/tmp/x` (`/`),
    /// `/proc/1/cmdline` (`/proc`) and `/dev/null` (`/dev`).
    ///
    /// The canonicalization that comes first is upstream's, kept for its
    /// diagnostic alone: `stat -c %m` on an unreachable path must say *why*
    /// rather than print a bare `?`. It is skipped for a symlink reported as
    /// itself, since that link's own path is what the question is about.
    fn mount_point(name: &[u8], st: &StatInfo, follow: bool) -> Result<Vec<u8>, Option<String>> {
        if (follow || st.mode & S_IFMT != S_IFLNK)
            && let Err(e) = canon::canonicalize(&RealFs, name, Mode::Existing)
        {
            return Err(Some(format!(
                "failed to canonicalize {}: {}",
                quoteaf(name),
                strerror(&e)
            )));
        }

        // A directory is its own starting point; anything else asks about the
        // directory holding it, which is where GNU starts too.
        let start: Vec<u8> = if st.mode & S_IFMT == S_IFDIR {
            name.to_vec()
        } else {
            dir_name(name).to_vec()
        };
        let mut dir = canon::canonicalize(&RealFs, &start, Mode::Existing).map_err(|_| None)?;
        let mut dev = fs::metadata(quote::os_from_bytes(&dir))
            .map_err(|_| None)?
            .dev();

        loop {
            let parent = dir_name(&dir).to_vec();
            if parent == dir {
                return Ok(dir);
            }
            let Ok(meta) = fs::metadata(quote::os_from_bytes(&parent)) else {
                return Ok(dir);
            };
            if meta.dev() != dev {
                return Ok(dir);
            }
            dev = meta.dev();
            dir = parent;
        }
    }

    /// Collect everything a directive can ask about one file.
    fn gather(
        name: &[u8],
        settings_deref: bool,
        pieces: &[Piece],
        db: Option<&Db>,
        style: Style,
        diags: &mut Diags,
    ) -> Option<FileFacts> {
        // `-` is standard input, not a file called `-`. A pipeline that says
        // `... | stat -c %s -` is asking about the descriptor it was handed.
        let meta = if name == b"-" {
            // SAFETY: descriptor 0 is open for the lifetime of the process.
            // `ManuallyDrop` is what keeps this borrow from closing it — a
            // `File` built from a raw fd owns it, and dropping it here would
            // shut stdin for everything that runs afterwards.
            let stdin = ManuallyDrop::new(unsafe { fs::File::from_raw_fd(0) });
            match stdin.metadata() {
                Ok(m) => m,
                Err(e) => {
                    diags.error(&format!("cannot stat standard input: {}", strerror(&e)));
                    return None;
                }
            }
        } else {
            let os = quote::os_from_bytes(name);
            let got = if settings_deref {
                fs::metadata(&os)
            } else {
                fs::symlink_metadata(&os)
            };
            match got {
                Ok(m) => m,
                Err(e) => {
                    diags.error(&format!(
                        "cannot stat {}: {}",
                        quoteaf_os(&os),
                        strerror(&e)
                    ));
                    return None;
                }
            }
        };

        let st = info_from(&meta);

        let link = if st.mode & S_IFMT == S_IFLNK && wants(pieces, b'N') {
            Some(
                fs::read_link(quote::os_from_bytes(name))
                    .map(|t| quote::os_bytes(t.as_os_str()).into_owned())
                    .map_err(|e| {
                        format!(
                            "cannot read symbolic link {}: {}",
                            quoteaf(name),
                            strerror(&e)
                        )
                    }),
            )
        } else {
            None
        };

        let mount = if wants(pieces, b'm') {
            mount_point(name, &st, settings_deref)
        } else {
            Err(None)
        };

        Some(FileFacts {
            user: db
                .and_then(|d| d.user_by_uid(st.uid))
                .map(|u| u.name.clone()),
            group: db
                .and_then(|d| d.group_by_gid(st.gid))
                .map(|g| g.name.clone()),
            st,
            name: name.to_vec(),
            link,
            mount,
            style,
        })
    }

    fn gather_fs(name: &[u8], diags: &mut Diags) -> Option<FsFacts> {
        let os = quote::os_from_bytes(name);
        let Ok(cpath) = CString::new(name) else {
            diags.error(&format!(
                "cannot read file system information for {}: path contains a NUL byte",
                quoteaf_os(&os)
            ));
            return None;
        };
        let mut raw = PosixStatvfs::default();
        // SAFETY: `cpath` is a valid NUL-terminated C string that outlives the
        // call, and `raw` is a valid, writable buffer of the declared layout.
        let ret = unsafe { statvfs(cpath.as_ptr().cast::<u8>(), &raw mut raw) };
        if ret != 0 {
            diags.error(&format!(
                "cannot read file system information for {}: {}",
                quoteaf_os(&os),
                strerror(&io::Error::last_os_error())
            ));
            return None;
        }
        Some(FsFacts {
            fs: FsInfo {
                fsid: raw.f_fsid,
                namelen: raw.f_namemax,
                bsize: raw.f_bsize,
                frsize: raw.f_frsize,
                blocks: raw.f_blocks,
                bfree: raw.f_bfree,
                bavail: raw.f_bavail,
                files: raw.f_files,
                ffree: raw.f_ffree,
            },
            name: name.to_vec(),
        })
    }

    pub fn main() -> ExitCode {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Help) => {
                print!("{}", help_text());
                return ExitCode::SUCCESS;
            }
            Ok(Request::Version) => {
                println!("stat (SlateOS coreutils) 0.1.0");
                return ExitCode::SUCCESS;
            }
            Ok(Request::Run(settings)) => settings,
            Err(e) => {
                diag!("stat: {e}");
                return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
            }
        };

        let stat_mode = !settings.filesystem;
        let (fmt, fmt2, interpret) = match &settings.format {
            Some((f, interpret)) => (f.clone(), f.clone(), *interpret),
            None => (
                default_format(settings.filesystem, settings.terse, false),
                default_format(settings.filesystem, settings.terse, true),
                false,
            ),
        };
        // `-c` ends each operand's output with a newline; `--printf` does not,
        // and neither do the built-in formats, which already carry their own.
        let trailing: &[u8] = match &settings.format {
            Some((_, false)) => b"\n",
            _ => b"",
        };

        let pieces = scan(&fmt, interpret, stat_mode);
        let pieces2 = scan(&fmt2, interpret, stat_mode);

        let mut diags = Diags::default();
        let style = if wants(&pieces, b'N') || wants(&pieces2, b'N') {
            quoting_style_from_env(&diags)
        } else {
            Style::ShellEscapeAlways
        };
        // Reading the account database is not free, and most invocations do
        // not name a user. `%U`/`%G` are the only two directives that need it.
        let needs_db = b"UG"
            .iter()
            .any(|&s| wants(&pieces, s) || wants(&pieces2, s));
        let db = if needs_db && stat_mode {
            Some(Db::load())
        } else {
            None
        };
        let zone = Zone::from_env();

        let stdout = io::stdout();
        let mut out = stdout.lock();
        let mut invalid: Option<Vec<u8>> = None;

        for file in &settings.files {
            let name = quote::os_bytes(file).into_owned();
            let mut buf: Vec<u8> = Vec::new();

            let outcome = if settings.filesystem {
                if name == b"-" {
                    diags.error(&format!(
                        "using {} to denote standard input does not work in file system mode",
                        quote_os(file)
                    ));
                    continue;
                }
                let Some(facts) = gather_fs(&name, &mut diags) else {
                    continue;
                };
                print_it(&pieces, &Target::Fs(&facts), &zone, &mut buf, &mut diags)
            } else {
                let Some(facts) = gather(
                    &name,
                    settings.dereference,
                    &pieces,
                    db.as_ref(),
                    style,
                    &mut diags,
                ) else {
                    continue;
                };
                // A device file gets the wider block with `Device type:` in
                // it — chosen from the file, so a list of operands may use
                // both formats.
                let chosen = if is_device(facts.st.mode) {
                    &pieces2
                } else {
                    &pieces
                };
                print_it(chosen, &Target::File(&facts), &zone, &mut buf, &mut diags)
            };

            let stop = match outcome {
                Ok(()) => {
                    buf.extend_from_slice(trailing);
                    false
                }
                Err(text) => {
                    invalid = Some(text);
                    true
                }
            };

            // A closed downstream reader — `stat * | head -1` — is an ordinary
            // end to a pipeline, not a failure. Anything else is data the
            // caller will never see and must not be reported as success.
            if let Err(e) = out.write_all(&buf) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    return ExitCode::from(u8::from(diags.fail));
                }
                diag!("stat: write error: {}", strerror(&e));
                return ExitCode::from(1);
            }
            if stop {
                break;
            }
        }

        if let Err(e) = out.flush()
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            diag!("stat: write error: {}", strerror(&e));
            return ExitCode::from(1);
        }

        if let Some(text) = invalid {
            diag!("stat: {}: invalid directive", quote::quote(&text));
            return ExitCode::from(1);
        }
        ExitCode::from(u8::from(diags.fail))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // Every expected string below was measured against GNU coreutils 9.4
    // under WSL before it was written down. `scripts/` holds no probe for
    // this; the probes were scratch scripts, and the numbers they produced are
    // what these assertions are.

    /// `2024-06-15 12:30:45.123456789 UTC`, the timestamp the probes used.
    const T: i64 = 1_718_454_645;
    const NS: u32 = 123_456_789;

    fn facts() -> FileFacts {
        FileFacts {
            st: StatInfo {
                dev: 2049, // 8,1
                ino: 12345,
                mode: 0o100_644,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                size: 5,
                blksize: 4096,
                blocks: 8,
                atime: T,
                atime_nsec: NS,
                mtime: T,
                mtime_nsec: NS,
                ctime: T,
                ctime_nsec: NS,
                btime: None,
            },
            name: b"plain".to_vec(),
            link: None,
            mount: Err(None),
            user: Some(b"alice".to_vec()),
            group: Some(b"users".to_vec()),
            style: Style::ShellEscapeAlways,
        }
    }

    /// Render a `-c` format against [`facts`], in UTC.
    fn render(fmt: &str) -> String {
        render_with(fmt, &facts())
    }

    fn render_with(fmt: &str, f: &FileFacts) -> String {
        let pieces = scan(fmt.as_bytes(), false, true);
        let mut out = Vec::new();
        let mut diags = Diags::default();
        print_it(
            &pieces,
            &Target::File(f),
            &Zone::utc(),
            &mut out,
            &mut diags,
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    }

    fn render_fs(fmt: &str) -> String {
        let facts = FsFacts {
            fs: FsInfo {
                fsid: 0x68867c45465b201c,
                namelen: 255,
                bsize: 4096,
                frsize: 4096,
                blocks: 1000,
                bfree: 400,
                bavail: 300,
                files: 500,
                ffree: 200,
            },
            name: b".".to_vec(),
        };
        let pieces = scan(fmt.as_bytes(), false, false);
        let mut out = Vec::new();
        let mut diags = Diags::default();
        print_it(
            &pieces,
            &Target::Fs(&facts),
            &Zone::utc(),
            &mut out,
            &mut diags,
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    }

    // ------------------------------------------------------------ prefixes ---

    #[test]
    fn prefix_parses_flags_width_and_precision() {
        let (c, len) = parse_conv(b"-010.3Y");
        assert_eq!(len, 6);
        assert!(c.left && c.zero);
        assert_eq!(c.width, 10);
        assert_eq!(c.precision, Some(3));
        assert!(c.precision_digits);
    }

    #[test]
    fn a_bare_dot_is_a_precision_with_no_digits() {
        let (c, len) = parse_conv(b".Y");
        assert_eq!(len, 1);
        assert_eq!(c.precision, Some(0));
        assert!(!c.precision_digits);
    }

    #[test]
    fn zero_is_a_flag_so_a_width_never_starts_with_one() {
        let (c, _) = parse_conv(b"010s");
        assert!(c.zero);
        assert_eq!(c.width, 10);
    }

    #[test]
    fn a_prefix_ends_at_the_first_byte_that_is_not_one() {
        // `%Hd`: `H` is not a flag, so the prefix is empty and `H` is next.
        let (c, len) = parse_conv(b"Hd");
        assert_eq!(len, 0);
        assert_eq!(c, Conv::default());
    }

    // -------------------------------------------------------- flag filtering ---

    #[test]
    fn a_flag_the_conversion_does_not_accept_is_dropped() {
        // Measured: `[%+d][% d][%'d][%Id]` on a device number all print the
        // bare number, because `%d` is an *unsigned* conversion upstream.
        assert_eq!(render("[%+d][% d][%'d][%Id]"), "[2049][2049][2049][2049]");
        // `#` is not in `out_uint`'s set, so the I/O block size grows no `0`.
        assert_eq!(render("[%#o]"), "[4096]");
        // …but it is in `out_uint_o`'s, and `%a` is the only octal.
        assert_eq!(render("[%#a][%0a]"), "[0644][644]");
    }

    #[test]
    fn hex_alternate_form_prefixes_only_a_nonzero_value() {
        assert_eq!(render("[%#D][%#R]"), "[0x801][0]");
    }

    // ------------------------------------------------------------- widths ---

    #[test]
    fn strings_pad_with_spaces_and_truncate_to_the_precision() {
        // Measured on a name of `abcdefgh`.
        let mut f = facts();
        f.name = b"abcdefgh".to_vec();
        assert_eq!(
            render_with("[%.3n][%.0n][%.n][%10.3n][%-10.3n]", &f),
            "[abc][][][       abc][abc       ]"
        );
    }

    #[test]
    fn numbers_treat_precision_as_a_minimum_digit_count() {
        assert_eq!(
            render("[%.3s][%.0s][%.10i][%#.5a]"),
            "[005][5][0000012345][00644]"
        );
    }

    #[test]
    fn precision_zero_erases_a_zero_value() {
        let mut f = facts();
        f.st.size = 0;
        assert_eq!(render_with("[%.0s]", &f), "[]");
    }

    #[test]
    fn the_zero_flag_is_ignored_when_a_precision_is_given() {
        assert_eq!(render("[%08.3s][%-8.3s]"), "[     005][005     ]");
    }

    // -------------------------------------------------------------- epochs ---

    #[test]
    fn epoch_precision_defaults_to_whole_seconds_and_a_bare_dot_means_nine() {
        assert_eq!(
            render("%.3Y|%.0Y|%.Y|%Y"),
            "1718454645.123|1718454645|1718454645.123456789|1718454645"
        );
    }

    #[test]
    fn epoch_width_covers_the_fraction_too() {
        assert_eq!(
            render("[%20.3Y][%-20.3Y][%020.3Y]"),
            "[      1718454645.123][1718454645.123      ][0000001718454645.123]"
        );
    }

    #[test]
    fn a_negative_time_borrows_out_of_its_fraction() {
        // -1.5s is stored as (-2, 500000000); GNU prints -1.500, not -2.500.
        let mut f = facts();
        f.st.mtime = -2;
        f.st.mtime_nsec = 500_000_000;
        assert_eq!(
            render_with("[%Y][%.3Y][%.9Y]", &f),
            "[-2][-1.500][-1.500000000]"
        );
        // -0.5s rounds the seconds to zero and must keep the sign.
        f.st.mtime = -1;
        assert_eq!(render_with("[%Y][%.3Y][%.1Y]", &f), "[-1][-0.500][-0.5]");
    }

    #[test]
    fn an_unknown_birth_time_is_a_dash_and_a_zero() {
        assert_eq!(render("[%w][%W]"), "[-][0]");
    }

    // ---------------------------------------------------------- directives ---

    #[test]
    fn the_human_readable_block_matches_gnus_layout() {
        let fmt = String::from_utf8(default_format(false, false, false)).unwrap();
        assert_eq!(
            render(&fmt),
            "  File: 'plain'\n\
             \x20 Size: 5         \tBlocks: 8          IO Block: 4096   regular file\n\
             Device: 8,1\tInode: 12345       Links: 1\n\
             Access: (0644/-rw-r--r--)  Uid: ( 1000/   alice)   Gid: ( 1000/   users)\n\
             Access: 2024-06-15 12:30:45.123456789 +0000\n\
             Modify: 2024-06-15 12:30:45.123456789 +0000\n\
             Change: 2024-06-15 12:30:45.123456789 +0000\n\
             \x20Birth: -\n"
        );
    }

    #[test]
    fn a_symlink_gets_an_arrow_and_the_width_applies_to_both_halves() {
        let mut f = facts();
        f.st.mode = 0o120_777;
        f.name = b"l k".to_vec();
        f.link = Some(Ok(b"a b".to_vec()));
        assert_eq!(render_with("[%N]", &f), "['l k' -> 'a b']");
        // Each half is padded to 10 on its own; the space before `->` is the
        // literal one in the arrow, not part of either field.
        assert_eq!(render_with("[%-10N]", &f), "['l k'      -> 'a b'     ]");
    }

    #[test]
    fn an_unknown_id_prints_the_word_scripts_grep_for() {
        let mut f = facts();
        f.user = None;
        f.group = None;
        assert_eq!(render_with("%U %G", &f), "UNKNOWN UNKNOWN");
    }

    #[test]
    fn an_empty_regular_file_has_its_own_type_name() {
        let mut f = facts();
        f.st.size = 0;
        assert_eq!(render_with("%F", &f), "regular empty file");
        assert_eq!(render("%F"), "regular file");
    }

    #[test]
    fn device_numbers_use_the_modern_encoding() {
        let mut f = facts();
        f.st.mode = 0o020_666;
        f.st.rdev = 0x0103; // 1,3 — /dev/null
        assert_eq!(render_with("%Hr,%Lr|%t %T|%r", &f), "1,3|1 3|259");
        assert!(is_device(f.st.mode));
    }

    #[test]
    fn an_unknown_specifier_is_a_question_mark() {
        // Including `%C`: there is no security context here, and this is the
        // one directive where that is a silent absence rather than a failure.
        assert_eq!(render("[%Q][%C]"), "[?][?]");
    }

    #[test]
    fn a_modifier_on_something_that_is_not_a_device_number_is_not_a_modifier() {
        // Measured: `%Hs` prints `?s` — a `?` for `H`, then a literal `s`.
        assert_eq!(render("%Hs|%H"), "?s|?");
        // …and in filesystem mode `H` is never a modifier at all.
        assert_eq!(render_fs("%Hd"), "?d");
        // …while in file mode it is.
        assert_eq!(render("%Hd %Ld"), "8 1");
    }

    // ------------------------------------------------------- filesystem mode ---

    #[test]
    fn filesystem_directives_render() {
        assert_eq!(
            render_fs("[%i][%l][%t][%T][%b][%f][%a][%c][%d][%s][%S][%n]"),
            "[68867c45465b201c][255][?][UNKNOWN][1000][400][300][500][200][4096][4096][.]"
        );
    }

    /// The `-f` block is the one format built with Rust's `\` line-continuation,
    /// which eats the newline *and* the following line's indentation — so the four
    /// leading spaces of the `    ID:` line have to be restored with an `\x20`
    /// escape. That is exactly the kind of thing that breaks silently, so pin the
    /// whole block, column for column, against GNU 9.4's own output shape.
    #[test]
    fn the_filesystem_default_block_keeps_its_columns() {
        let fmt = String::from_utf8(default_format(true, false, false)).unwrap();
        assert_eq!(
            render_fs(&fmt),
            concat!(
                "  File: \".\"\n",
                "    ID: 68867c45465b201c Namelen: 255     Type: UNKNOWN\n",
                "Block size: 4096       Fundamental block size: 4096\n",
                "Blocks: Total: 1000       Free: 400        Available: 300\n",
                "Inodes: Total: 500        Free: 200\n",
            )
        );
    }

    #[test]
    fn a_fundamental_block_size_of_zero_falls_back_to_the_block_size() {
        let facts = FsFacts {
            fs: FsInfo {
                bsize: 4096,
                frsize: 0,
                ..FsInfo::default()
            },
            name: b".".to_vec(),
        };
        let pieces = scan(b"%S", false, false);
        let mut out = Vec::new();
        let mut diags = Diags::default();
        print_it(
            &pieces,
            &Target::Fs(&facts),
            &Zone::utc(),
            &mut out,
            &mut diags,
        )
        .unwrap();
        assert_eq!(out, b"4096");
    }

    // ------------------------------------------------------------- scanning ---

    #[test]
    fn a_percent_prefix_with_no_specifier_is_fatal() {
        assert_eq!(
            scan(b"x%5%y", false, true),
            vec![Piece::Bytes(b"x".to_vec()), Piece::Invalid(b"%5%".to_vec())]
        );
        assert_eq!(
            scan(b"x%-", false, true),
            vec![Piece::Bytes(b"x".to_vec()), Piece::Invalid(b"%-".to_vec())]
        );
    }

    #[test]
    fn a_bare_double_percent_is_a_literal_one() {
        assert_eq!(
            scan(b"a%%b", false, true),
            vec![Piece::Bytes(b"a%b".to_vec())]
        );
        assert_eq!(scan(b"a%", false, true), vec![Piece::Bytes(b"a%".to_vec())]);
    }

    #[test]
    fn escapes_are_literal_under_c_and_interpreted_under_printf() {
        assert_eq!(
            scan(br"a\nb", false, true),
            vec![Piece::Bytes(br"a\nb".to_vec())]
        );
        assert_eq!(
            scan(br"a\nb", true, true),
            vec![Piece::Bytes(b"a\nb".to_vec())]
        );
    }

    #[test]
    fn octal_and_hex_escapes() {
        assert_eq!(
            scan(br"\x41\101\0", true, true),
            vec![Piece::Bytes(b"AA\0".to_vec())]
        );
        // `\x` with no hex digit after it is the unrecognized escape `\x`,
        // not the letter `x` — which is what makes a typo visible.
        assert_eq!(
            scan(br"\xzz", true, true),
            vec![Piece::BadEscape(b'x'), Piece::Bytes(b"zz".to_vec())]
        );
    }

    #[test]
    fn an_unknown_escape_and_a_trailing_backslash_warn() {
        assert_eq!(
            scan(br"a\qb", true, true),
            vec![
                Piece::Bytes(b"a".to_vec()),
                Piece::BadEscape(b'q'),
                Piece::Bytes(b"b".to_vec())
            ]
        );
        assert_eq!(
            scan(br"end\", true, true),
            vec![Piece::Bytes(b"end".to_vec()), Piece::TrailingBackslash]
        );
    }

    #[test]
    fn wants_finds_only_directives() {
        let pieces = scan(b"literal N %s %N", false, true);
        assert!(wants(&pieces, b'N'));
        assert!(wants(&pieces, b's'));
        assert!(!wants(&pieces, b'U'));
    }

    // -------------------------------------------------------------- formats ---

    #[test]
    fn terse_formats_are_upstreams() {
        assert_eq!(default_format(false, true, false), TERSE_FILE);
        assert_eq!(default_format(true, true, false), TERSE_FS);
        // Both end in exactly one newline: `stat -t f` prints one line.
        assert!(TERSE_FILE.ends_with(b"\n") && !TERSE_FILE.ends_with(b"\n\n"));
    }

    #[test]
    fn the_device_block_carries_a_device_type_field() {
        let plain = default_format(false, false, false);
        let device = default_format(false, false, true);
        assert!(!plain.windows(12).any(|w| w == b"Device type:"));
        assert!(device.windows(12).any(|w| w == b"Device type:"));
    }

    // -------------------------------------------------------------- parsing ---

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn clustered_and_attached_options_parse() {
        // `stat -c%s f` is the single most common invocation of this program,
        // and the version this replaces treated all three words as file names.
        let Ok(Request::Run(s)) = parse_args(&args(&["-c%s", "f"])) else {
            panic!("expected a run")
        };
        assert_eq!(s.format, Some((b"%s".to_vec(), false)));
        assert_eq!(s.files, args(&["f"]));

        let Ok(Request::Run(s)) = parse_args(&args(&["-Lt", "f"])) else {
            panic!("expected a run")
        };
        assert!(s.dereference && s.terse);

        let Ok(Request::Run(s)) = parse_args(&args(&["-tc", "%s", "f"])) else {
            panic!("expected a run")
        };
        assert!(s.terse);
        assert_eq!(s.format, Some((b"%s".to_vec(), false)));
    }

    #[test]
    fn printf_is_told_apart_from_format() {
        let Ok(Request::Run(s)) = parse_args(&args(&[r"--printf=%s\n", "f"])) else {
            panic!("expected a run")
        };
        assert_eq!(s.format, Some((br"%s\n".to_vec(), true)));
    }

    #[test]
    fn options_may_follow_an_operand_and_double_dash_ends_them() {
        let Ok(Request::Run(s)) = parse_args(&args(&["f", "-c%s"])) else {
            panic!("expected a run")
        };
        assert_eq!(s.files, args(&["f"]));
        assert_eq!(s.format, Some((b"%s".to_vec(), false)));

        let Ok(Request::Run(s)) = parse_args(&args(&["--", "-c"])) else {
            panic!("expected a run")
        };
        assert_eq!(s.files, args(&["-c"]));
        assert!(s.format.is_none());
    }

    /// The whole point of the rewrite: neither of these is representable as a
    /// `String`, and the previous version *panicked* on the first.
    ///
    /// Byte-exactness is asserted only on unix. The build host is Windows,
    /// where an `OsString` is UTF-16 and `os_from_bytes` has to go through a
    /// lossy conversion — so this test can prove the plumbing carries whatever
    /// it is given, but not that the bytes are the same ones, which is a fact
    /// about the host and not about `stat`.
    #[test]
    fn a_non_utf8_operand_and_format_survive() {
        let name = quote::os_from_bytes(b"bad\xffname");
        let fmt = quote::os_from_bytes(b"-cA\xffB %s");
        let Ok(Request::Run(s)) = parse_args(&[fmt, name.clone()]) else {
            panic!("expected a run")
        };
        assert_eq!(s.files, vec![name]);
        let (format, interpret) = s.format.unwrap();
        assert!(!interpret);
        #[cfg(unix)]
        assert_eq!(format, b"A\xffB %s");
        #[cfg(not(unix))]
        assert!(format.ends_with(b"B %s"));
    }

    #[test]
    fn cached_is_validated_even_though_it_does_nothing() {
        assert!(parse_args(&args(&["--cached=never", "f"])).is_ok());
        assert!(parse_args(&args(&["--cached=n", "f"])).is_ok());
        let e = parse_args(&args(&["--cached=bogus", "f"])).unwrap_err();
        assert!(e.message().contains("invalid argument"), "{}", e.message());
        assert_eq!(e.status, 1);
    }

    #[test]
    fn no_operand_is_an_error_that_refers_to_help() {
        let e = parse_args(&[]).unwrap_err();
        assert!(e.message().contains("missing operand"), "{}", e.message());
        assert_eq!(e.status, 1);
    }

    #[test]
    fn help_and_version_win_over_everything() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse_args(&args(&["--version"])), Ok(Request::Version));
    }

    #[test]
    fn help_mentions_every_option_it_accepts() {
        let help = help_text();
        for (name, _) in LONG_OPTIONS {
            assert!(help.contains(&format!("--{name}")), "help omits --{name}");
        }
        for flag in SHORT_OPTIONS.chars().filter(char::is_ascii_alphabetic) {
            assert!(help.contains(&format!("-{flag}")), "help omits -{flag}");
        }
    }
}
