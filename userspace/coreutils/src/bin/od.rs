//! od — dump files in octal and other formats.
//!
//! A GNU-coreutils-9.4-compatible reimplementation, transcribed from
//! `coreutils-9.4/src/od.c` and verified against the real `od` with
//! `scripts/od-diff.sh`.
//!
//! The hard parts, and why the code is shaped the way it is:
//!
//! * **Two option grammars.** `od` accepts both a modern one (`-t x1`,
//!   `--format=x1`) and the pre-POSIX traditional one (`od file [offset]`,
//!   `od -bc file +100`). A single traditional-looking operand can be an
//!   *offset*, and whether it is depends on how many operands there are and
//!   whether any modern option was seen. `finish()` and
//!   `traditional_operands()` reproduce upstream's three-case switch exactly,
//!   including its quirks.
//! * **Column layout is computed, not hardcoded.** The number of bytes per
//!   output line is the least common multiple of every format's datum size
//!   (rounded up toward 16), and each format's padding is spread across its
//!   fields by an integer-division rule that must match byte for byte.
//! * **Floating point uses gnulib's `ftoastr`**, not `%g`: the shortest
//!   `%.*g` precision that round-trips back to the same value. See
//!   `ftoastr_f32` / `ftoastr_f64` / `ftoastr_ext`.
//!
//! Deliberate divergences from GNU 9.4 are listed in `known-issues.md` under
//! `TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE`; briefly, `-w0` and `-w-4`
//! make GNU 9.4 `abort()` (fixed upstream after 9.4) and here print a normal
//! `invalid -w argument` diagnostic instead, and an allocation failure is
//! reported as `memory exhausted` rather than aborting.

use coreutils::errmsg::strerror;
use coreutils::extfloat::{self, ExtF80};
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, quote, quotef};
use coreutils::xnum::{self, Status};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::process::ExitCode;

const OD: Program = Program::new("od", 1);

const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("skip-bytes", Takes::Required),
    ("address-radix", Takes::Required),
    ("read-bytes", Takes::Required),
    ("format", Takes::Required),
    ("output-duplicates", Takes::Nothing),
    ("strings", Takes::Optional),
    ("traditional", Takes::Nothing),
    ("width", Takes::Optional),
    ("endian", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Suffixes accepted by `-j`/`-N`/`-S`. Matches upstream's
/// `xstrtoumax (..., "bEGKkMmPQRTYZ0")`; the `0` enables the `0x`/`0`
/// base prefixes rather than being a suffix of its own.
const MULTIPLIERS: Option<&[u8]> = Some(b"bEGKkMmPQRTYZ0");

const ENDIAN_ARGS: &[(&str, bool)] = &[("little", false), ("big", true)];

/// Names for control characters under `-a`, indexed by code point.
/// Entry 32 is `sp`, which is why the table is 33 long and not 32.
const CHARNAME: [&str; 33] = [
    "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "nl", "vt", "ff", "cr",
    "so", "si", "dle", "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub", "esc",
    "fs", "gs", "rs", "us", "sp",
];

// Digit counts for an N-byte integer in each radix, indexed by N.
// Transcribed from upstream's bytes_to_*_digits[] tables.
const OCT_DIGITS: [usize; 17] = [
    0, 3, 6, 8, 11, 14, 16, 19, 22, 25, 27, 30, 32, 35, 38, 41, 43,
];
const SDEC_DIGITS: [usize; 17] = [
    1, 4, 6, 8, 11, 13, 16, 18, 20, 23, 25, 28, 30, 33, 35, 37, 40,
];
const UDEC_DIGITS: [usize; 17] = [
    0, 3, 5, 8, 10, 13, 15, 17, 20, 22, 25, 27, 29, 32, 34, 37, 39,
];
const HEX_DIGITS: [usize; 17] = [
    0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32,
];

const DEFAULT_FORMAT: &[u8] = b"oS";

/// Read buffer size. Upstream uses stdio's default; the exact value is not
/// observable except through `-N` (see `Input::new`).
const READ_CAPACITY: usize = 65536;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn arg_bytes(arg: &OsString) -> Vec<u8> {
    os_bytes(arg.as_os_str()).into_owned()
}

fn integral_type_exists(size: usize) -> bool {
    matches!(size, 1 | 2 | 4 | 8)
}

fn fp_type_exists(size: usize) -> bool {
    matches!(size, 4 | 8 | 16)
}

fn isprint(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Least common multiple of every spec's datum size: the smallest line
/// length at which every requested format lands on a whole number of fields.
fn get_lcm(specs: &[Spec]) -> usize {
    specs.iter().fold(1, |l, s| l / gcd(l, s.size) * s.size)
}

// ---------------------------------------------------------------------------
// Failure reporting
// ---------------------------------------------------------------------------

/// A fatal usage error.
///
/// The messages are **bytes**, not `String`: two of `od`'s diagnostics embed a
/// raw byte from the command line (`invalid output address radix '%c'` and
/// `invalid character '%c' in type string`), and that byte can be NUL
/// (`od -A ''`) or non-UTF-8 (`od -A $'\xff'`). Carrying `String` here would
/// force lossy replacement and diverge from GNU on exactly those inputs.
struct Fail {
    messages: Vec<Vec<u8>>,
    referral: bool,
    status: i32,
}

impl Fail {
    fn one(message: Vec<u8>, referral: bool, status: i32) -> Self {
        Fail {
            messages: vec![message],
            referral,
            status,
        }
    }
}

impl From<getopt::Error> for Fail {
    fn from(e: getopt::Error) -> Self {
        Fail {
            referral: e.referral.is_some(),
            status: e.status,
            messages: vec![e.sentence.into_bytes()],
        }
    }
}

/// Write one `od: …` diagnostic line to stderr as raw bytes.
fn diagnose(body: &[u8]) {
    let mut line = Vec::with_capacity(body.len() + 5);
    line.extend_from_slice(b"od: ");
    line.extend_from_slice(body);
    line.push(b'\n');
    // A failed write to stderr has nowhere left to be reported.
    let _ = io::stderr().write_all(&line);
}

fn diagnose_str(s: &str) {
    diagnose(s.as_bytes());
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    SignedDecimal,
    UnsignedDecimal,
    Octal,
    Hexadecimal,
    FloatingPoint,
    NamedCharacter,
    Character,
}

/// One decoded `-t` conversion: how wide each datum is, how wide its column
/// is, and how much slack padding it is entitled to on a full line.
#[derive(Clone, Copy, Debug)]
struct Spec {
    fmt: Format,
    size: usize,
    field_width: usize,
    /// Filled in by `run()` once the line width is known.
    pad_width: usize,
    /// `z`: append the `>…<` printable-character gutter.
    trailer: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AddressStyle {
    None,
    Std,
    Paren,
    Label,
}

enum Request {
    Help,
    Version,
    Run(Box<Options>),
}

/// The outcome of argument parsing.
///
/// `printed` carries diagnostics that upstream had already emitted before it
/// reached `--help`/`--version`: a bad `-t` prints its complaint immediately
/// and only *later* causes a non-zero exit, so `od -t q --help` prints the
/// complaint and then exits 0 with the help text.
struct Parsed {
    printed: Vec<Vec<u8>>,
    request: Request,
}

struct Options {
    specs: Vec<Spec>,
    style: AddressStyle,
    base: u32,
    pad_len: usize,
    pseudo_offset: u64,
    skip: u64,
    limit: bool,
    max_bytes: u64,
    end_offset: u64,
    strings: bool,
    string_min: usize,
    abbreviate: bool,
    swap: bool,
    width_specified: bool,
    desired_width: u64,
    files: Vec<OsString>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            specs: Vec::new(),
            style: AddressStyle::Std,
            base: 8,
            pad_len: 7,
            pseudo_offset: 0,
            skip: 0,
            limit: false,
            max_bytes: 0,
            end_offset: 0,
            strings: false,
            string_min: 0,
            abbreviate: true,
            swap: false,
            width_specified: false,
            desired_width: 0,
            files: Vec::new(),
        }
    }
}

/// Parser state: `Options` plus the bookkeeping that only matters while
/// arguments are being consumed.
struct Draft {
    options: Options,
    /// Any modern option was seen, which disables traditional offset operands.
    modern: bool,
    traditional: bool,
    flag_pseudo_start: bool,
    pseudo_start: u64,
    /// Diagnostics printed at the point of the error but not yet fatal.
    pending: Vec<Vec<u8>>,
    ok: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            options: Options::default(),
            modern: false,
            traditional: false,
            flag_pseudo_start: false,
            pseudo_start: 0,
            pending: Vec::new(),
            ok: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Format string decoding
// ---------------------------------------------------------------------------

/// Upstream's `simple_strtoi`: read a decimal run, returning `None` on `int`
/// overflow (which becomes `invalid type string`) rather than saturating.
fn simple_strtoi(s: &[u8]) -> Option<(usize, &[u8])> {
    let mut sum: i32 = 0;
    let mut at = 0usize;
    while let Some(&c) = s.get(at) {
        if !c.is_ascii_digit() {
            break;
        }
        sum = sum.checked_mul(10)?.checked_add(i32::from(c - b'0'))?;
        at += 1;
    }
    Some((usize::try_from(sum).ok()?, s.get(at..).unwrap_or_default()))
}

/// Decode a whole `-t` argument, appending one `Spec` per conversion.
fn decode_format_string(s_orig: &[u8], specs: &mut Vec<Spec>) -> Result<(), Vec<u8>> {
    let mut s = s_orig;
    while !s.is_empty() {
        let (spec, rest) = decode_one_format(s_orig, s)?;
        specs.push(spec);
        s = rest;
    }
    Ok(())
}

/// Decode one conversion from the front of `s`.
///
/// `s_orig` is the whole `-t` argument, quoted into error messages exactly as
/// upstream does — the diagnostic names the argument, not the offending tail.
fn decode_one_format<'a>(s_orig: &[u8], s: &'a [u8]) -> Result<(Spec, &'a [u8]), Vec<u8>> {
    let invalid_type_string = || -> Vec<u8> {
        format!("invalid type string {}", quote(s_orig)).into_bytes()
    };

    let (&kind, mut rest) = s.split_first().ok_or_else(invalid_type_string)?;

    let (fmt, size, field_width) = match kind {
        b'd' | b'o' | b'u' | b'x' => {
            // C-type letters first; note upstream never validates that these
            // name a type that exists, because by construction they do.
            let size = match rest.first().copied() {
                Some(b'C') => {
                    rest = &rest[1..];
                    1
                }
                Some(b'S') => {
                    rest = &rest[1..];
                    2
                }
                Some(b'I') => {
                    rest = &rest[1..];
                    4
                }
                Some(b'L') => {
                    rest = &rest[1..];
                    8
                }
                _ => {
                    let (n, tail) = simple_strtoi(rest).ok_or_else(invalid_type_string)?;
                    if tail.len() == rest.len() {
                        // No digits consumed: the default is `int`.
                        4
                    } else {
                        rest = tail;
                        if n > 8 || !integral_type_exists(n) {
                            return Err(format!(
                                "invalid type string {};\nthis system doesn't provide a \
                                 {n}-byte integral type",
                                quote(s_orig)
                            )
                            .into_bytes());
                        }
                        n
                    }
                }
            };
            let fmt = match kind {
                b'd' => Format::SignedDecimal,
                b'o' => Format::Octal,
                b'u' => Format::UnsignedDecimal,
                _ => Format::Hexadecimal,
            };
            let width = match fmt {
                Format::SignedDecimal => SDEC_DIGITS.get(size).copied().unwrap_or(0),
                Format::UnsignedDecimal => UDEC_DIGITS.get(size).copied().unwrap_or(0),
                Format::Octal => OCT_DIGITS.get(size).copied().unwrap_or(0),
                _ => HEX_DIGITS.get(size).copied().unwrap_or(0),
            };
            (fmt, size, width)
        }
        b'f' => {
            let size = match rest.first().copied() {
                Some(b'F') => {
                    rest = &rest[1..];
                    4
                }
                Some(b'D') => {
                    rest = &rest[1..];
                    8
                }
                Some(b'L') => {
                    rest = &rest[1..];
                    16
                }
                _ => {
                    let (n, tail) = simple_strtoi(rest).ok_or_else(invalid_type_string)?;
                    if tail.len() == rest.len() {
                        8
                    } else {
                        rest = tail;
                        if n > 16 || !fp_type_exists(n) {
                            return Err(format!(
                                "invalid type string {};\nthis system doesn't provide a \
                                 {n}-byte floating point type",
                                quote(s_orig)
                            )
                            .into_bytes());
                        }
                        n
                    }
                }
            };
            let width = match size {
                4 => 15,
                8 => 24,
                _ => 29,
            };
            (Format::FloatingPoint, size, width)
        }
        b'a' => (Format::NamedCharacter, 1, 3),
        b'c' => (Format::Character, 1, 3),
        other => {
            let mut msg = b"invalid character '".to_vec();
            msg.push(other);
            msg.extend_from_slice(b"' in type string ");
            msg.extend_from_slice(quote(s_orig).as_bytes());
            return Err(msg);
        }
    };

    // Exactly one optional `z` suffix.
    let trailer = rest.first() == Some(&b'z');
    if trailer {
        rest = &rest[1..];
    }

    Ok((
        Spec {
            fmt,
            size,
            field_width,
            pad_width: 0,
            trailer,
        },
        rest,
    ))
}

// ---------------------------------------------------------------------------
// Option tables
// ---------------------------------------------------------------------------

/// `short_options[] = "A:aBbcDdeFfHhIij:LlN:OoS:st:vw::Xx"`.
fn short_takes(c: u8) -> Option<Takes> {
    Some(match c {
        b'A' | b'j' | b'N' | b'S' | b't' => Takes::Required,
        b'w' => Takes::Optional,
        b'a' | b'B' | b'b' | b'c' | b'D' | b'd' | b'e' | b'F' | b'f' | b'H' | b'h' | b'I'
        | b'i' | b'L' | b'l' | b'O' | b'o' | b's' | b'v' | b'x' | b'X' => Takes::Nothing,
        _ => return None,
    })
}

/// How an option names itself in an `xstrtol_fatal` diagnostic. Only the
/// argument-taking letters can reach one.
fn short_spelling(c: u8) -> &'static str {
    match c {
        b'A' => "-A",
        b'j' => "-j",
        b'N' => "-N",
        b'S' => "-S",
        b'w' => "-w",
        _ => "-t",
    }
}

/// The pre-POSIX single-letter formats, as the `-t` string each expands to.
fn old_format(c: u8) -> Option<&'static [u8]> {
    Some(match c {
        b'a' => b"a".as_slice(),
        b'b' => b"o1".as_slice(),
        b'c' => b"c".as_slice(),
        b'D' => b"u4".as_slice(),
        b'd' => b"u2".as_slice(),
        b'F' | b'e' => b"fD".as_slice(),
        b'f' => b"fF".as_slice(),
        b'X' | b'H' => b"x4".as_slice(),
        b'i' => b"dI".as_slice(),
        b'I' | b'L' | b'l' => b"dL".as_slice(),
        b'O' => b"o4".as_slice(),
        b'B' | b'o' => b"o2".as_slice(),
        b's' => b"d2".as_slice(),
        b'h' | b'x' => b"x2".as_slice(),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------

fn set_address_radix(value: &[u8], draft: &mut Draft) -> Result<(), Fail> {
    let o = &mut draft.options;
    match value.first().copied().unwrap_or(0) {
        b'd' => {
            o.style = AddressStyle::Std;
            o.base = 10;
            o.pad_len = 7;
        }
        b'o' => {
            o.style = AddressStyle::Std;
            o.base = 8;
            o.pad_len = 7;
        }
        b'x' => {
            o.style = AddressStyle::Std;
            o.base = 16;
            o.pad_len = 6;
        }
        b'n' => {
            // Upstream leaves address_base alone here; nothing reads it.
            o.style = AddressStyle::None;
            o.pad_len = 0;
        }
        other => {
            let mut msg = b"invalid output address radix '".to_vec();
            msg.push(other);
            msg.extend_from_slice(b"'; it must be one character from [doxn]");
            return Err(Fail::one(msg, false, 1));
        }
    }
    Ok(())
}

/// `-j` / `-N` / `-S`: base 0 with the size suffixes.
fn number(value: &[u8], option: &str) -> Result<u64, Fail> {
    xnum::xstrtoumax_option(value, 0, MULTIPLIERS, option)
        .map_err(|m| Fail::one(m.into_bytes(), false, 1))
}

/// `-w`: base 10, no suffixes, and strictly positive.
///
/// GNU 9.4 writes `if (s_err != LONGINT_OK || w_tmp <= 0) xstrtol_fatal
/// (s_err, …)`, so a well-formed non-positive width reaches `xstrtol_fatal`
/// with `LONGINT_OK`, which `abort()`s. We print the diagnostic that upstream
/// evidently meant — and that later releases produce — instead of crashing.
fn set_width(value: Option<Vec<u8>>, spelling: &str, draft: &mut Draft) -> Result<(), Fail> {
    draft.options.width_specified = true;
    let Some(v) = value else {
        draft.options.desired_width = 32;
        return Ok(());
    };
    let (width, status) = xnum::xstrtoumax_base(&v, 10, Some(b""));
    if status != Status::Ok || width == 0 {
        let body = xnum::strtol_fatal(status, spelling, &v)
            .unwrap_or_else(|| format!("invalid {spelling} argument {}", quote(&v)));
        return Err(Fail::one(body.into_bytes(), false, 1));
    }
    draft.options.desired_width = width;
    Ok(())
}

/// `-t` and every traditional format letter. A bad type string is reported at
/// once but only becomes fatal after the whole command line has been read, so
/// `od -t q -t w` prints both complaints.
fn set_format(value: &[u8], draft: &mut Draft) {
    if let Err(message) = decode_format_string(value, &mut draft.options.specs) {
        draft.pending.push(message);
        draft.ok = false;
    }
}

fn apply(c: u8, spelling: &str, value: Option<Vec<u8>>, draft: &mut Draft) -> Result<(), Fail> {
    match c {
        b'A' => {
            draft.modern = true;
            set_address_radix(&value.unwrap_or_default(), draft)?;
        }
        b'j' => {
            draft.modern = true;
            draft.options.skip = number(&value.unwrap_or_default(), spelling)?;
        }
        b'N' => {
            draft.modern = true;
            draft.options.limit = true;
            draft.options.max_bytes = number(&value.unwrap_or_default(), spelling)?;
        }
        b'S' => {
            draft.modern = true;
            draft.options.string_min = match value {
                None => 3,
                Some(v) => usize::try_from(number(&v, spelling)?).unwrap_or(usize::MAX),
            };
            draft.options.strings = true;
        }
        b't' => {
            draft.modern = true;
            set_format(&value.unwrap_or_default(), draft);
        }
        b'v' => {
            draft.modern = true;
            draft.options.abbreviate = false;
        }
        b'w' => {
            draft.modern = true;
            set_width(value, spelling, draft)?;
        }
        // The traditional letters. None of them sets `modern`, which is what
        // keeps `od -bc file +100` reading `+100` as an offset.
        other => {
            if let Some(f) = old_format(other) {
                set_format(f, draft);
            }
        }
    }
    Ok(())
}

fn short_options(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    draft: &mut Draft,
) -> Result<(), Fail> {
    let mut at = 1usize;
    while let Some(&c) = bytes.get(at) {
        at += 1;
        let takes = short_takes(c).ok_or_else(|| Fail::from(OD.invalid_option(c)))?;
        let rest = bytes.get(at..).unwrap_or_default();
        let value = match takes {
            Takes::Nothing => None,
            Takes::Optional => {
                at = bytes.len();
                if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_vec())
                }
            }
            Takes::Required => {
                at = bytes.len();
                if rest.is_empty() {
                    let next = args
                        .get(*i)
                        .ok_or_else(|| Fail::from(OD.short_missing_argument(c)))?;
                    *i += 1;
                    Some(arg_bytes(next))
                } else {
                    Some(rest.to_vec())
                }
            }
        };
        apply(c, short_spelling(c), value, draft)?;
    }
    Ok(())
}

fn long_option(
    bytes: &[u8],
    args: &[OsString],
    i: &mut usize,
    draft: &mut Draft,
) -> Result<Option<Request>, Fail> {
    let body = bytes.get(2..).unwrap_or_default();
    let (typed_bytes, inline) = match body.iter().position(|&c| c == b'=') {
        Some(p) => (
            body.get(..p).unwrap_or_default(),
            Some(body.get(p + 1..).unwrap_or_default().to_vec()),
        ),
        None => (body, None),
    };
    let typed = String::from_utf8_lossy(typed_bytes).into_owned();
    let (resolved, takes) = OD.resolve_long(&typed, bytes, LONG_OPTIONS)?;
    let value = match takes {
        Takes::Nothing => {
            if inline.is_some() {
                return Err(Fail::from(OD.long_unwanted_argument(resolved)));
            }
            None
        }
        Takes::Optional => inline,
        Takes::Required => match inline {
            Some(v) => Some(v),
            None => {
                let next = args
                    .get(*i)
                    .ok_or_else(|| Fail::from(OD.long_missing_argument(resolved)))?;
                *i += 1;
                Some(arg_bytes(next))
            }
        },
    };
    let spelling = format!("--{resolved}");
    match resolved {
        "help" => return Ok(Some(Request::Help)),
        "version" => return Ok(Some(Request::Version)),
        // Neither of these sets `modern`: --traditional exists to *enable* the
        // old grammar, and --endian has no bearing on operand interpretation.
        "traditional" => draft.traditional = true,
        "endian" => {
            let v = value.unwrap_or_default();
            draft.options.swap = OD.argmatch(&v, "--endian", ENDIAN_ARGS)?;
        }
        "skip-bytes" => apply(b'j', &spelling, value, draft)?,
        "address-radix" => apply(b'A', &spelling, value, draft)?,
        "read-bytes" => apply(b'N', &spelling, value, draft)?,
        "format" => apply(b't', &spelling, value, draft)?,
        "output-duplicates" => apply(b'v', &spelling, value, draft)?,
        "strings" => apply(b'S', &spelling, value, draft)?,
        "width" => apply(b'w', &spelling, value, draft)?,
        _ => {}
    }
    Ok(None)
}

fn parse_loop(
    args: &[OsString],
    posixly_correct: bool,
    draft: &mut Draft,
    operands: &mut Vec<OsString>,
) -> Result<Option<Request>, Fail> {
    let mut i = 0usize;
    let mut only_operands = false;
    while let Some(arg) = args.get(i) {
        i += 1;
        if only_operands || (posixly_correct && !operands.is_empty()) {
            operands.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);
        if bytes.as_slice() == b"--" {
            only_operands = true;
        } else if bytes.as_slice() == b"-" || bytes.first() != Some(&b'-') {
            operands.push(arg.clone());
        } else if bytes.starts_with(b"--") {
            if let Some(request) = long_option(&bytes, args, &mut i, draft)? {
                return Ok(Some(request));
            }
        } else {
            short_options(&bytes, args, &mut i, draft)?;
        }
    }
    Ok(None)
}

fn parse_args(args: &[OsString], posixly_correct: bool) -> Result<Parsed, Fail> {
    let mut draft = Draft::default();
    let mut operands: Vec<OsString> = Vec::new();
    match parse_loop(args, posixly_correct, &mut draft, &mut operands) {
        // A `-t` complaint is printed where it is found, so it precedes
        // whatever later option turned out to be fatal.
        Err(mut fail) => {
            let mut messages = std::mem::take(&mut draft.pending);
            messages.append(&mut fail.messages);
            fail.messages = messages;
            Err(fail)
        }
        Ok(Some(request)) => Ok(Parsed {
            printed: draft.pending,
            request,
        }),
        Ok(None) => {
            if !draft.ok {
                return Err(Fail {
                    messages: draft.pending,
                    referral: false,
                    status: 1,
                });
            }
            let printed = std::mem::take(&mut draft.pending);
            let options = finish(draft, operands)?;
            Ok(Parsed {
                printed,
                request: Request::Run(Box::new(options)),
            })
        }
    }
}

/// The pre-POSIX operand grammar:
///
/// ```text
/// od [file] [[+]offset[.][b] [[+]label[.][b]]]
/// ```
///
/// Transcribed case by case from upstream's `switch (n_files)`, including its
/// `argv` shuffling — in case 2 the file operand is *moved over* the offset
/// rather than the offset being removed, which is only observable in the
/// `extra operand` diagnostic.
fn traditional_operands(draft: &mut Draft, operands: &mut Vec<OsString>) {
    let raw: Vec<Vec<u8>> = operands.iter().map(arg_bytes).collect();
    let traditional = draft.traditional;
    match operands.len() {
        1 => {
            let leads_plus = raw[0].first() == Some(&b'+');
            if let Some(o1) = (traditional || leads_plus)
                .then(|| parse_old_offset(&raw[0]))
                .flatten()
            {
                draft.options.skip = o1;
                operands.clear();
            }
        }
        2 => {
            let second = &raw[1];
            let eligible = traditional
                || second.first() == Some(&b'+')
                || second.first().is_some_and(u8::is_ascii_digit);
            if let Some(o2) = eligible.then(|| parse_old_offset(second)).flatten() {
                if let Some(o1) = traditional.then(|| parse_old_offset(&raw[0])).flatten() {
                    draft.options.skip = o1;
                    draft.flag_pseudo_start = true;
                    draft.pseudo_start = o2;
                    operands.clear();
                } else {
                    draft.options.skip = o2;
                    operands.truncate(1);
                }
            }
        }
        3 => {
            if traditional
                && let Some(o1) = parse_old_offset(&raw[1])
                && let Some(o2) = parse_old_offset(&raw[2])
            {
                draft.options.skip = o1;
                draft.flag_pseudo_start = true;
                draft.pseudo_start = o2;
                operands.truncate(1);
            }
        }
        _ => {}
    }
}

/// A traditional `offset` or `label` operand.
///
/// The radix is decimal if the text contains a `.`, hexadecimal after `0x`,
/// octal otherwise. Note that the `.` is *not* stripped before conversion, so
/// the documented `offset[.]` form never actually parses — a genuine upstream
/// quirk, reproduced deliberately.
fn parse_old_offset(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let body = if s.first() == Some(&b'+') {
        s.get(1..).unwrap_or_default()
    } else {
        s
    };
    let radix = if body.contains(&b'.') {
        10
    } else if body.first() == Some(&b'0') && matches!(body.get(1), Some(b'x' | b'X')) {
        16
    } else {
        8
    };
    let (value, status) = xnum::xstrtoumax_base(body, radix, Some(b"Bb"));
    (status == Status::Ok).then_some(value)
}

/// Everything upstream's `main` does between the option loop and opening the
/// first file.
fn finish(mut draft: Draft, mut operands: Vec<OsString>) -> Result<Options, Fail> {
    if draft.options.strings && !draft.options.specs.is_empty() {
        return Err(Fail::one(
            b"no type may be specified when dumping strings".to_vec(),
            false,
            1,
        ));
    }

    if !draft.modern || draft.traditional {
        traditional_operands(&mut draft, &mut operands);
        if draft.traditional && operands.len() > 1 {
            let extra = operands.get(1).map(arg_bytes).unwrap_or_default();
            return Err(Fail {
                messages: vec![
                    format!("extra operand {}", quote(&extra)).into_bytes(),
                    b"compatibility mode supports at most one file".to_vec(),
                ],
                referral: true,
                status: 1,
            });
        }
    }

    if draft.flag_pseudo_start {
        if draft.options.style == AddressStyle::None {
            draft.options.base = 8;
            draft.options.pad_len = 7;
            draft.options.style = AddressStyle::Paren;
        } else {
            draft.options.style = AddressStyle::Label;
        }
    }

    if draft.options.limit {
        let Some(end) = draft.options.skip.checked_add(draft.options.max_bytes) else {
            return Err(Fail::one(
                b"skip-bytes + read-bytes is too large".to_vec(),
                false,
                1,
            ));
        };
        draft.options.end_offset = end;
    }

    if draft.options.specs.is_empty()
        && let Err(message) = decode_format_string(DEFAULT_FORMAT, &mut draft.options.specs)
    {
        return Err(Fail::one(message, false, 1));
    }

    // The label counts from `pseudo_start` while the address counts from the
    // start of the data, so the two differ by the amount skipped. Wrapping is
    // upstream's own `pseudo_start - n_bytes_to_skip` on uintmax_t.
    draft.options.pseudo_offset = if draft.flag_pseudo_start {
        draft.pseudo_start.wrapping_sub(draft.options.skip)
    } else {
        0
    };

    draft.options.files = if operands.is_empty() {
        vec![OsString::from("-")]
    } else {
        operands
    };
    Ok(draft.options)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// A write sink that latches its first failure instead of propagating it.
///
/// `od` writes on nearly every line and checks only at the end, reporting
/// `write error` once; threading a `Result` through every column would say the
/// same thing with a great deal more noise.
struct Sink<W: Write> {
    inner: W,
    failed: bool,
    error: Option<io::Error>,
}

impl<W: Write> Sink<W> {
    fn new(inner: W) -> Self {
        Sink {
            inner,
            failed: false,
            error: None,
        }
    }

    fn put(&mut self, bytes: &[u8]) {
        if !self.failed
            && let Err(e) = self.inner.write_all(bytes)
        {
            self.failed = true;
            self.error = Some(e);
        }
    }

    fn pad(&mut self, mut n: usize) {
        const SPACES: [u8; 64] = [b' '; 64];
        while n > 0 {
            let take = n.min(SPACES.len());
            self.put(SPACES.get(..take).unwrap_or_default());
            n -= take;
        }
    }

    /// `printf("%*s", width, text)`.
    fn right(&mut self, width: usize, text: &[u8]) {
        self.pad(width.saturating_sub(text.len()));
        self.put(text);
    }
}

fn digits(address: u64, base: u32, pad_len: usize) -> Vec<u8> {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    let base = u64::from(base.max(2));
    let mut out = Vec::with_capacity(pad_len.max(1));
    let mut a = address;
    loop {
        let d = usize::try_from(a % base).unwrap_or(0);
        out.push(ALPHABET.get(d).copied().unwrap_or(b'0'));
        a /= base;
        if a == 0 {
            break;
        }
    }
    while out.len() < pad_len {
        out.push(b'0');
    }
    out.reverse();
    out
}

/// `format_address_std`. A `c` of NUL appends nothing: upstream builds the
/// string with `c` before the terminator, so `fputs` stops short of it.
fn address_std<W: Write>(sink: &mut Sink<W>, o: &Options, address: u64, c: u8) {
    sink.put(&digits(address, o.base, o.pad_len));
    if c != 0 {
        sink.put(&[c]);
    }
}

fn address_paren<W: Write>(sink: &mut Sink<W>, o: &Options, address: u64, c: u8) {
    sink.put(b"(");
    address_std(sink, o, address, b')');
    if c != 0 {
        sink.put(&[c]);
    }
}

fn format_address<W: Write>(sink: &mut Sink<W>, o: &Options, address: u64, c: u8) {
    match o.style {
        AddressStyle::None => {}
        AddressStyle::Std => address_std(sink, o, address, c),
        AddressStyle::Paren => address_paren(sink, o, address, c),
        AddressStyle::Label => {
            address_std(sink, o, address, b' ');
            address_paren(sink, o, address.wrapping_add(o.pseudo_offset), c);
        }
    }
}

fn two(b: &[u8]) -> [u8; 2] {
    let mut a = [0u8; 2];
    let n = b.len().min(2);
    if let (Some(dst), Some(src)) = (a.get_mut(..n), b.get(..n)) {
        dst.copy_from_slice(src);
    }
    a
}

fn four(b: &[u8]) -> [u8; 4] {
    let mut a = [0u8; 4];
    let n = b.len().min(4);
    if let (Some(dst), Some(src)) = (a.get_mut(..n), b.get(..n)) {
        dst.copy_from_slice(src);
    }
    a
}

fn eight(b: &[u8]) -> [u8; 8] {
    let mut a = [0u8; 8];
    let n = b.len().min(8);
    if let (Some(dst), Some(src)) = (a.get_mut(..n), b.get(..n)) {
        dst.copy_from_slice(src);
    }
    a
}

fn unsigned(b: &[u8]) -> u64 {
    u64::from_le_bytes(eight(b))
}

fn signed(b: &[u8], size: usize) -> i64 {
    match size {
        1 => i64::from(b.first().copied().unwrap_or(0).cast_signed()),
        2 => i64::from(i16::from_le_bytes(two(b))),
        4 => i64::from(i32::from_le_bytes(four(b))),
        _ => i64::from_le_bytes(eight(b)),
    }
}

fn render_g(precision: usize, v: ExtF80) -> String {
    let spec = extfloat::Spec {
        precision: Some(precision),
        ..extfloat::Spec::general()
    };
    extfloat::render(&spec, v)
}

/// gnulib's `ftoastr` for `float`: the shortest `%.*g` that reads back equal.
///
/// The precision floor is 1 rather than `FLT_DIG` for subnormals, where the
/// available significand is short enough that a long precision only adds
/// noise. NaN never compares equal to itself, so it always runs to the bound —
/// which is fine, because `%g` spells it `nan` at every precision.
fn ftoastr_f32(x: f32) -> String {
    let v = ExtF80::from_f32(x);
    let mut prec = if x.abs() < f32::MIN_POSITIVE { 1 } else { 6 };
    loop {
        let s = render_g(prec, v);
        if prec >= 9 || s.parse::<f32>().is_ok_and(|back| back == x) {
            return s;
        }
        prec += 1;
    }
}

fn ftoastr_f64(x: f64) -> String {
    let v = ExtF80::from_f64(x);
    let mut prec = if x.abs() < f64::MIN_POSITIVE { 1 } else { 15 };
    loop {
        let s = render_g(prec, v);
        if prec >= 17 || s.parse::<f64>().is_ok_and(|back| back == x) {
            return s;
        }
        prec += 1;
    }
}

/// The same for x87 `long double`. Only the first 10 of the 16 bytes carry the
/// value; the rest is padding the ABI never defines.
fn ftoastr_ext(bytes: &[u8]) -> String {
    let mut raw = [0u8; 10];
    let n = bytes.len().min(10);
    if let (Some(dst), Some(src)) = (raw.get_mut(..n), bytes.get(..n)) {
        dst.copy_from_slice(src);
    }
    let v = ExtF80::from_x87_bytes(raw);

    let mut abs_raw = raw;
    if let Some(top) = abs_raw.get_mut(9) {
        *top &= 0x7f;
    }
    let abs = ExtF80::from_x87_bytes(abs_raw);
    // LDBL_MIN: the smallest normal, exponent 1 with the explicit integer bit set.
    let ldbl_min = ExtF80::from_x87_bytes([0, 0, 0, 0, 0, 0, 0, 0x80, 0x01, 0x00]);

    let mut prec = if abs.lt(ldbl_min) { 1 } else { 18 };
    loop {
        let s = render_g(prec, v);
        // `strtold`, not `xstrtold`: gnulib's ftoastr compares the value and
        // never looks at errno, and every subnormal — the whole reason the
        // precision floor drops to 1 — sets ERANGE on the way back in. Reading
        // that as "did not round-trip" would run the loop out to 21 digits and
        // print `3.6451995318824746025e-4951` where GNU prints `4e-4951`.
        let back = extfloat::strtold(s.as_bytes());
        if prec >= 21 || back.value.eq_value(v) {
            return s;
        }
        prec += 1;
    }
}

/// One datum, rendered exactly as the corresponding `printf` in upstream would.
fn render_datum(spec: &Spec, datum: &[u8], swap: bool) -> String {
    let mut bytes = datum.to_vec();
    if swap && spec.size > 1 {
        bytes.reverse();
    }
    match spec.fmt {
        Format::NamedCharacter => {
            let c = bytes.first().copied().unwrap_or(0) & 0x7f;
            if c == 127 {
                "del".to_owned()
            } else if c <= 0o40 {
                CHARNAME
                    .get(usize::from(c))
                    .copied()
                    .unwrap_or_default()
                    .to_owned()
            } else {
                char::from(c).to_string()
            }
        }
        Format::Character => {
            let c = bytes.first().copied().unwrap_or(0);
            match c {
                b'\0' => "\\0".to_owned(),
                0x07 => "\\a".to_owned(),
                0x08 => "\\b".to_owned(),
                0x0c => "\\f".to_owned(),
                b'\n' => "\\n".to_owned(),
                b'\r' => "\\r".to_owned(),
                b'\t' => "\\t".to_owned(),
                0x0b => "\\v".to_owned(),
                _ if isprint(c) => char::from(c).to_string(),
                _ => format!("{c:03o}"),
            }
        }
        // `%*.No`/`%*.Nx`: the precision zero-fills to the full digit count.
        Format::Octal => format!("{:0>width$o}", unsigned(&bytes), width = spec.field_width),
        Format::Hexadecimal => format!("{:0>width$x}", unsigned(&bytes), width = spec.field_width),
        // `%*u`/`%*d`: no precision, so no zero fill.
        Format::UnsignedDecimal => unsigned(&bytes).to_string(),
        Format::SignedDecimal => signed(&bytes, spec.size).to_string(),
        Format::FloatingPoint => match spec.size {
            4 => ftoastr_f32(f32::from_le_bytes(four(&bytes))),
            8 => ftoastr_f64(f64::from_le_bytes(eight(&bytes))),
            _ => ftoastr_ext(&bytes),
        },
    }
}

/// Print `fields - blank` columns of one format, distributing `pad_width`
/// across them.
///
/// The share each column gets is `pad * (i - 1) / fields` subtracted from what
/// remains, so the slack lands on the leftmost columns first. Getting this
/// integer division wrong shifts entire lines, which is why it is transcribed
/// rather than reinvented.
fn print_fields<W: Write>(
    sink: &mut Sink<W>,
    spec: &Spec,
    fields: usize,
    blank: usize,
    block: &[u8],
    swap: bool,
) {
    let mut pad_remaining = spec.pad_width;
    let mut at = 0usize;
    let mut i = fields;
    while i > blank {
        let next_pad = spec.pad_width * (i - 1) / fields;
        let adjusted = pad_remaining - next_pad + spec.field_width;
        let datum = block.get(at..at.saturating_add(spec.size)).unwrap_or_default();
        sink.right(adjusted, render_datum(spec, datum, swap).as_bytes());
        at = at.saturating_add(spec.size);
        pad_remaining = next_pad;
        i -= 1;
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// An enum rather than `Box<dyn Read>` so that `skip` can seek a real file.
enum Source {
    Stdin(BufReader<io::Stdin>),
    File(BufReader<File>),
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Source::Stdin(r) => r.read(buf),
            Source::File(r) => r.read(buf),
        }
    }
}

/// The operand list read as one continuous byte stream.
///
/// A failure to open one file is reported and skipped; a failure to *read* one
/// is held in `pending` and reported when the file is closed, which is where
/// upstream reports it too (`check_and_close (errno)`).
struct Input {
    list: Vec<OsString>,
    next: usize,
    stream: Option<Source>,
    name: Vec<u8>,
    pending: Option<io::Error>,
    capacity: usize,
}

impl Input {
    fn new(list: Vec<OsString>, capacity: usize) -> Self {
        Input {
            list,
            next: 0,
            stream: None,
            name: Vec::new(),
            pending: None,
            capacity,
        }
    }

    /// Open operands until one opens or the list runs out. Returns false if any
    /// operand failed to open.
    fn open_next(&mut self) -> bool {
        let mut ok = true;
        loop {
            let Some(arg) = self.list.get(self.next).cloned() else {
                return ok;
            };
            self.next += 1;
            let raw = arg_bytes(&arg);
            if raw.as_slice() == b"-" {
                self.name = b"standard input".to_vec();
                self.stream = Some(Source::Stdin(BufReader::with_capacity(
                    self.capacity,
                    io::stdin(),
                )));
                return ok;
            }
            self.name = raw;
            match File::open(&arg) {
                Ok(f) => {
                    self.stream = Some(Source::File(BufReader::with_capacity(self.capacity, f)));
                    return ok;
                }
                Err(e) => {
                    let mut msg = quotef(&self.name).into_bytes();
                    msg.extend_from_slice(b": ");
                    msg.extend_from_slice(strerror(&e).as_bytes());
                    diagnose(&msg);
                    ok = false;
                }
            }
        }
    }

    fn check_and_close(&mut self, out_failed: bool) -> bool {
        let mut ok = true;
        if self.stream.take().is_some()
            && let Some(e) = self.pending.take()
        {
            let mut msg = quotef(&self.name).into_bytes();
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(strerror(&e).as_bytes());
            diagnose(&msg);
            ok = false;
        }
        if out_failed {
            diagnose(b"write error");
            ok = false;
        }
        ok
    }

    /// One `read`, retrying `EINTR`. A hard error is latched and read as EOF,
    /// which is what `fread` reports to `od`.
    fn read_some(&mut self, buf: &mut [u8]) -> usize {
        let outcome = match self.stream.as_mut() {
            None => return 0,
            Some(s) => loop {
                match s.read(buf) {
                    Ok(n) => break Ok(n),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => break Err(e),
                }
            },
        };
        match outcome {
            Ok(n) => n,
            Err(e) => {
                self.pending.get_or_insert(e);
                0
            }
        }
    }

    /// `fread` semantics: keep reading until the request is satisfied or the
    /// stream ends.
    fn fill(&mut self, buf: &mut [u8]) -> usize {
        let want = buf.len();
        let mut got = 0usize;
        while got < want {
            let n = match buf.get_mut(got..want) {
                Some(dst) => self.read_some(dst),
                None => 0,
            };
            if n == 0 {
                break;
            }
            got += n;
        }
        got
    }

    /// Read `n` bytes into `block`, crossing into later files as needed.
    fn read_block(&mut self, n: usize, block: &mut [u8], out_failed: bool) -> (usize, bool) {
        let mut ok = true;
        let mut got = 0usize;
        while self.stream.is_some() {
            let n_read = match block.get_mut(got..n) {
                Some(dst) => self.fill(dst),
                None => 0,
            };
            let needed = n - got;
            got += n_read;
            if n_read == needed {
                break;
            }
            ok &= self.check_and_close(out_failed);
            ok &= self.open_next();
        }
        (got, ok)
    }

    fn read_char(&mut self, out_failed: bool) -> (Option<u8>, bool) {
        let mut ok = true;
        while self.stream.is_some() {
            let mut one = [0u8; 1];
            if self.fill(&mut one) == 1 {
                return (one.first().copied(), ok);
            }
            ok &= self.check_and_close(out_failed);
            ok &= self.open_next();
        }
        (None, ok)
    }

    /// The length of the current stream if it is a regular file.
    fn regular_size(&self) -> Option<u64> {
        match self.stream.as_ref() {
            Some(Source::File(r)) => r
                .get_ref()
                .metadata()
                .ok()
                .filter(std::fs::Metadata::is_file)
                .map(|m| m.len()),
            _ => None,
        }
    }

    fn seek_forward(&mut self, n: u64) -> io::Result<()> {
        match self.stream.as_mut() {
            Some(Source::File(r)) => {
                let offset = i64::try_from(n)
                    .map_err(|_| io::Error::from(ErrorKind::InvalidInput))?;
                r.seek(SeekFrom::Current(offset)).map(|_| ())
            }
            _ => Err(io::Error::from(ErrorKind::Unsupported)),
        }
    }

    /// Upstream's fallback for streams that cannot be sized or seeked.
    ///
    /// A read error here leaves `n` at zero *and* leaves the file open, so the
    /// error is never reported — an upstream quirk, reproduced rather than
    /// corrected so the two agree on the exit status.
    fn skip_by_reading(&mut self, mut n: u64, ok: &mut bool) -> u64 {
        let mut buf = vec![0u8; 8192];
        while n > 0 {
            let want = usize::try_from(n).unwrap_or(buf.len()).min(buf.len());
            let got = match buf.get_mut(..want) {
                Some(dst) => self.fill(dst),
                None => 0,
            };
            n -= u64::try_from(got).unwrap_or(0);
            if got != want {
                if self.pending.is_some() {
                    *ok = false;
                    return 0;
                }
                break;
            }
        }
        n
    }

    /// `None` means the combined input ran out before `n` bytes were skipped,
    /// which is fatal.
    fn skip(&mut self, mut n: u64, out_failed: bool) -> Option<bool> {
        let mut ok = true;
        if n == 0 {
            return Some(true);
        }
        while self.stream.is_some() {
            match self.regular_size() {
                Some(len) if len < n => n -= len,
                Some(_) => {
                    if self.seek_forward(n).is_err() {
                        // Fall back to reading: a seek that fails on a regular
                        // file is exotic, and reading still gets it right.
                        n = self.skip_by_reading(n, &mut ok);
                    } else {
                        n = 0;
                    }
                }
                None => n = self.skip_by_reading(n, &mut ok),
            }
            if n == 0 {
                break;
            }
            ok &= self.check_and_close(out_failed);
            ok &= self.open_next();
        }
        if n != 0 {
            return None;
        }
        Some(ok)
    }
}

/// `write_block`'s two `static` flags, made explicit.
struct BlockState {
    first: bool,
    prev_pair_equal: bool,
}

// ---------------------------------------------------------------------------
// The dump loops
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "upstream's write_block plus the statics it kept in file scope"
)]
fn write_block<W: Write>(
    sink: &mut Sink<W>,
    o: &Options,
    specs: &[Spec],
    bytes_per_block: usize,
    current_offset: u64,
    n_bytes: usize,
    prev: &[u8],
    curr: &[u8],
    state: &mut BlockState,
) {
    let duplicate = o.abbreviate
        && !state.first
        && n_bytes == bytes_per_block
        && prev.get(..bytes_per_block) == curr.get(..bytes_per_block);
    if duplicate {
        // The first repeat prints `*`; every further one prints nothing.
        if !state.prev_pair_equal {
            sink.put(b"*\n");
            state.prev_pair_equal = true;
        }
    } else {
        state.prev_pair_equal = false;
        for (i, spec) in specs.iter().enumerate() {
            let fields_per_block = bytes_per_block / spec.size;
            let blank_fields = (bytes_per_block - n_bytes) / spec.size;
            if i == 0 {
                format_address(sink, o, current_offset, 0);
            } else {
                sink.pad(o.pad_len);
            }
            print_fields(sink, spec, fields_per_block, blank_fields, curr, o.swap);
            if spec.trailer {
                let extra = spec
                    .pad_width
                    .saturating_mul(blank_fields)
                    .checked_div(fields_per_block)
                    .unwrap_or(0);
                sink.pad(blank_fields * spec.field_width + extra);
                sink.put(b"  >");
                for &c in curr.get(..n_bytes).unwrap_or_default() {
                    sink.put(&[if isprint(c) { c } else { b'.' }]);
                }
                sink.put(b"<");
            }
            sink.put(b"\n");
        }
    }
    state.first = false;
}

/// `None` reports an allocation failure, which upstream turns into
/// `xalloc_die`'s `memory exhausted`.
fn dump<W: Write>(
    sink: &mut Sink<W>,
    o: &Options,
    specs: &[Spec],
    input: &mut Input,
    bytes_per_block: usize,
    l_c_m: usize,
) -> Option<bool> {
    let mut curr: Vec<u8> = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    curr.try_reserve_exact(bytes_per_block).ok()?;
    prev.try_reserve_exact(bytes_per_block).ok()?;
    curr.resize(bytes_per_block, 0);
    prev.resize(bytes_per_block, 0);

    let mut state = BlockState {
        first: true,
        prev_pair_equal: false,
    };
    let mut current_offset = o.skip;
    let mut ok = true;
    let mut n_bytes_read = 0usize;

    while ok {
        let n_needed = if o.limit {
            if current_offset >= o.end_offset {
                n_bytes_read = 0;
                break;
            }
            let remaining = o.end_offset - current_offset;
            usize::try_from(remaining)
                .unwrap_or(bytes_per_block)
                .min(bytes_per_block)
        } else {
            bytes_per_block
        };
        let (n, read_ok) = input.read_block(n_needed, &mut curr, sink.failed);
        ok &= read_ok;
        n_bytes_read = n;
        if n_bytes_read < bytes_per_block {
            break;
        }
        write_block(
            sink,
            o,
            specs,
            bytes_per_block,
            current_offset,
            n_bytes_read,
            &prev,
            &curr,
            &mut state,
        );
        if sink.failed {
            ok = false;
        }
        current_offset = current_offset.wrapping_add(u64::try_from(n_bytes_read).unwrap_or(0));
        // The buffer just written becomes the one the next block is compared
        // against; the next read lands in the other.
        std::mem::swap(&mut curr, &mut prev);
    }

    if n_bytes_read > 0 {
        // Zero-fill up to a whole number of the widest datum, so the last
        // partial line has something defined to render in its final field.
        let bytes_to_write = l_c_m * n_bytes_read.div_ceil(l_c_m);
        if let Some(tail) = curr.get_mut(n_bytes_read..bytes_to_write.min(bytes_per_block)) {
            tail.fill(0);
        }
        write_block(
            sink,
            o,
            specs,
            bytes_per_block,
            current_offset,
            n_bytes_read,
            &prev,
            &curr,
            &mut state,
        );
        current_offset = current_offset.wrapping_add(u64::try_from(n_bytes_read).unwrap_or(0));
    }

    format_address(sink, o, current_offset, b'\n');

    if o.limit && current_offset >= o.end_offset {
        ok &= input.check_and_close(sink.failed);
    }
    Some(ok)
}

fn dump_strings<W: Write>(sink: &mut Sink<W>, o: &Options, input: &mut Input) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut address = o.skip;
    let mut ok = true;
    let min = u64::try_from(o.string_min).unwrap_or(u64::MAX);

    'line: loop {
        // Upstream's `tryline:` label, which the inner loops jump back to; the
        // limit test is inside it and so is retried on every restart.
        loop {
            if o.limit && (o.end_offset < min || o.end_offset - min <= address) {
                break 'line;
            }
            buf.clear();
            let mut restart = false;
            for _ in 0..o.string_min {
                let (c, k) = input.read_char(sink.failed);
                ok &= k;
                address = address.wrapping_add(1);
                let Some(c) = c else { return ok };
                if !isprint(c) {
                    restart = true;
                    break;
                }
                buf.push(c);
            }
            if !restart {
                break;
            }
        }

        // A run of printable bytes: keep going until NUL (print it) or a
        // non-printable byte (abandon it).
        let mut restart = false;
        while !o.limit || address < o.end_offset {
            let (c, k) = input.read_char(sink.failed);
            ok &= k;
            address = address.wrapping_add(1);
            let Some(c) = c else { return ok };
            if c == 0 {
                break;
            }
            if !isprint(c) {
                restart = true;
                break;
            }
            buf.push(c);
        }
        if restart {
            continue 'line;
        }

        let start = address
            .wrapping_sub(u64::try_from(buf.len()).unwrap_or(0))
            .wrapping_sub(1);
        format_address(sink, o, start, b' ');
        for &c in &buf {
            match c {
                0x07 => sink.put(b"\\a"),
                0x08 => sink.put(b"\\b"),
                0x0c => sink.put(b"\\f"),
                b'\n' => sink.put(b"\\n"),
                b'\r' => sink.put(b"\\r"),
                b'\t' => sink.put(b"\\t"),
                0x0b => sink.put(b"\\v"),
                _ => sink.put(&[c]),
            }
        }
        sink.put(b"\n");
    }

    ok &= input.check_and_close(sink.failed);
    ok
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// The line length `od` aims for when the formats allow it.
const DEFAULT_BYTES_PER_BLOCK: usize = 16;

fn finish_run<W: Write>(sink: &mut Sink<W>, ok: bool) -> ExitCode {
    if let Err(e) = sink.inner.flush() {
        sink.failed = true;
        sink.error.get_or_insert(e);
    }
    if let Some(e) = sink.error.take() {
        diagnose_str(&format!("write error: {}", strerror(&e)));
        return ExitCode::from(1);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run(o: &Options) -> ExitCode {
    let stdout = io::stdout();
    let mut sink = Sink::new(BufWriter::new(stdout.lock()));

    // `-N` without `-S` makes upstream turn input buffering off, so that no
    // more of the input is consumed than is dumped. That is observable when
    // the input is a pipe another process keeps reading.
    let capacity = if o.limit && !o.strings {
        1
    } else {
        READ_CAPACITY
    };
    let mut input = Input::new(o.files.clone(), capacity);

    let mut ok = input.open_next();
    if input.stream.is_none() {
        return finish_run(&mut sink, ok);
    }
    match input.skip(o.skip, sink.failed) {
        None => {
            diagnose(b"cannot skip past end of combined input");
            return ExitCode::from(1);
        }
        Some(k) => ok &= k,
    }
    if input.stream.is_none() {
        return finish_run(&mut sink, ok);
    }

    let mut specs = o.specs.clone();
    let l_c_m = get_lcm(&specs);

    let bytes_per_block = if o.width_specified {
        if o.desired_width != 0 && o.desired_width.is_multiple_of(u64::try_from(l_c_m).unwrap_or(1))
        {
            usize::try_from(o.desired_width).unwrap_or(l_c_m)
        } else {
            diagnose_str(&format!(
                "warning: invalid width {}; using {l_c_m} instead",
                o.desired_width
            ));
            l_c_m
        }
    } else if l_c_m < DEFAULT_BYTES_PER_BLOCK {
        l_c_m * (DEFAULT_BYTES_PER_BLOCK / l_c_m)
    } else {
        l_c_m
    };

    // Every format's columns are stretched to the widest format's line, so a
    // multi-format dump keeps its columns aligned down the page.
    let mut width_per_block = 0usize;
    for s in &specs {
        width_per_block = width_per_block.max((s.field_width + 1) * (bytes_per_block / s.size));
    }
    for s in &mut specs {
        s.pad_width = width_per_block - s.field_width * (bytes_per_block / s.size);
    }

    if o.strings {
        ok &= dump_strings(&mut sink, o, &mut input);
    } else {
        match dump(&mut sink, o, &specs, &mut input, bytes_per_block, l_c_m) {
            None => {
                diagnose(b"memory exhausted");
                return ExitCode::from(1);
            }
            Some(k) => ok &= k,
        }
    }
    finish_run(&mut sink, ok)
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let posixly_correct = std::env::var_os("POSIXLY_CORRECT").is_some();
    match parse_args(&args, posixly_correct) {
        Ok(parsed) => {
            for message in &parsed.printed {
                diagnose(message);
            }
            match parsed.request {
                Request::Help => {
                    print!("{}", help_text());
                    ExitCode::SUCCESS
                }
                Request::Version => {
                    println!("od (SlateOS coreutils) 0.1.0");
                    ExitCode::SUCCESS
                }
                Request::Run(options) => run(&options),
            }
        }
        Err(fail) => {
            for message in &fail.messages {
                diagnose(message);
            }
            if fail.referral {
                let _ = io::stderr().write_all(b"Try 'od --help' for more information.\n");
            }
            ExitCode::from(u8::try_from(fail.status).unwrap_or(1))
        }
    }
}

/// GNU's `--help`, byte for byte, minus the trailing block of URLs naming the
/// GNU project's own bug addresses.
///
/// The ragged indentation is upstream's: `--help` and `--version` come from a
/// shared macro and so line up with each other rather than with `od`'s own
/// option block.
fn help_text() -> String {
    "\
Usage: od [OPTION]... [FILE]...
  or:  od [-abcdfilosx]... [FILE] [[+]OFFSET[.][b]]
  or:  od --traditional [OPTION]... [FILE] [[+]OFFSET[.][b] [+][LABEL][.][b]]

Write an unambiguous representation, octal bytes by default,
of FILE to standard output.  With more than one FILE argument,
concatenate them in the listed order to form the input.

With no FILE, or when FILE is -, read standard input.

If first and second call formats both apply, the second format is assumed
if the last operand begins with + or (if there are 2 operands) a digit.
An OFFSET operand means -j OFFSET.  LABEL is the pseudo-address
at first byte printed, incremented when dump is progressing.
For OFFSET and LABEL, a 0x or 0X prefix indicates hexadecimal;
suffixes may be . for octal and b for multiply by 512.

Mandatory arguments to long options are mandatory for short options too.
  -A, --address-radix=RADIX   output format for file offsets; RADIX is one
                                of [doxn], for Decimal, Octal, Hex or None
      --endian={big|little}   swap input bytes according the specified order
  -j, --skip-bytes=BYTES      skip BYTES input bytes first
  -N, --read-bytes=BYTES      limit dump to BYTES input bytes
  -S BYTES, --strings[=BYTES]  show only NUL terminated strings
                                of at least BYTES (3) printable characters
  -t, --format=TYPE           select output format or formats
  -v, --output-duplicates     do not use * to mark line suppression
  -w[BYTES], --width[=BYTES]  output BYTES bytes per output line;
                                32 is implied when BYTES is not specified
      --traditional           accept arguments in third form above
      --help        display this help and exit
      --version     output version information and exit


Traditional format specifications may be intermixed; they accumulate:
  -a   same as -t a,  select named characters, ignoring high-order bit
  -b   same as -t o1, select octal bytes
  -c   same as -t c,  select printable characters or backslash escapes
  -d   same as -t u2, select unsigned decimal 2-byte units
  -f   same as -t fF, select floats
  -i   same as -t dI, select decimal ints
  -l   same as -t dL, select decimal longs
  -o   same as -t o2, select octal 2-byte units
  -s   same as -t d2, select decimal 2-byte units
  -x   same as -t x2, select hexadecimal 2-byte units


TYPE is made up of one or more of these specifications:
  a          named character, ignoring high-order bit
  c          printable character or backslash escape
  d[SIZE]    signed decimal, SIZE bytes per integer
  f[SIZE]    floating point, SIZE bytes per float
  o[SIZE]    octal, SIZE bytes per integer
  u[SIZE]    unsigned decimal, SIZE bytes per integer
  x[SIZE]    hexadecimal, SIZE bytes per integer

SIZE is a number.  For TYPE in [doux], SIZE may also be C for
sizeof(char), S for sizeof(short), I for sizeof(int) or L for
sizeof(long).  If TYPE is f, SIZE may also be F for sizeof(float), D
for sizeof(double) or L for sizeof(long double).

Adding a z suffix to any type displays printable characters at the end of
each output line.


BYTES is hex with 0x or 0X prefix, and may have a multiplier suffix:
  b    512
  KB   1000
  K    1024
  MB   1000*1000
  M    1024*1024
and so on for G, T, P, E, Z, Y, R, Q.
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.
"
    .to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// `scripts/od-diff.sh` is the real conformance check: it runs this binary and
// GNU od over the same command lines and compares stdout, stderr and status.
// What is worth unit-testing here is the handful of routines whose behaviour is
// hard to *localise* from a harness failure — a wrong column pad shifts a whole
// line, and a wrong `ftoastr` precision differs by one character in one field.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn decode(s: &str) -> Vec<Spec> {
        let mut specs = Vec::new();
        decode_format_string(s.as_bytes(), &mut specs).unwrap();
        specs
    }

    fn err(s: &str) -> String {
        let mut specs = Vec::new();
        let e = decode_format_string(s.as_bytes(), &mut specs).unwrap_err();
        String::from_utf8_lossy(&e).into_owned()
    }

    #[test]
    fn default_sizes_follow_the_conversion() {
        // d/o/u/x default to sizeof(int); f defaults to sizeof(double).
        assert_eq!(decode("d")[0].size, 4);
        assert_eq!(decode("o")[0].size, 4);
        assert_eq!(decode("u")[0].size, 4);
        assert_eq!(decode("x")[0].size, 4);
        assert_eq!(decode("f")[0].size, 8);
        // a and c are always one byte three columns wide.
        assert_eq!((decode("a")[0].size, decode("a")[0].field_width), (1, 3));
        assert_eq!((decode("c")[0].size, decode("c")[0].field_width), (1, 3));
    }

    #[test]
    fn size_letters_map_to_c_types() {
        assert_eq!(decode("dC")[0].size, 1);
        assert_eq!(decode("dS")[0].size, 2);
        assert_eq!(decode("dI")[0].size, 4);
        assert_eq!(decode("dL")[0].size, 8);
        assert_eq!(decode("fF")[0].size, 4);
        assert_eq!(decode("fD")[0].size, 8);
        assert_eq!(decode("fL")[0].size, 16);
    }

    #[test]
    fn field_widths_match_the_digit_tables() {
        // Widths are the printed column, not the datum size: an unsigned byte
        // needs 3 columns, a 4-byte hex value 8, an 8-byte octal value 22.
        assert_eq!(decode("u1")[0].field_width, UDEC_DIGITS[1]);
        assert_eq!(decode("d2")[0].field_width, SDEC_DIGITS[2]);
        assert_eq!(decode("o8")[0].field_width, OCT_DIGITS[8]);
        assert_eq!(decode("x4")[0].field_width, HEX_DIGITS[4]);
        // Measured against glibc's %g output for each float size.
        assert_eq!(decode("fF")[0].field_width, 15);
        assert_eq!(decode("fD")[0].field_width, 24);
        assert_eq!(decode("fL")[0].field_width, 29);
    }

    #[test]
    fn several_specs_may_be_concatenated() {
        let specs = decode("x1c");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].size, 1);
        assert!(matches!(specs[0].fmt, Format::Hexadecimal));
        assert!(matches!(specs[1].fmt, Format::Character));
    }

    #[test]
    fn z_suffix_sets_the_trailer_on_its_own_spec_only() {
        let specs = decode("xzc");
        assert!(specs[0].trailer);
        assert!(!specs[1].trailer);
        // Only one z is consumed, so a second is an unrelated (invalid) letter.
        assert_eq!(
            err("xzz"),
            "invalid character 'z' in type string 'xzz'".to_owned()
        );
    }

    #[test]
    fn unsupported_widths_name_the_missing_type() {
        assert_eq!(
            err("d3"),
            "invalid type string 'd3';\nthis system doesn't provide a 3-byte integral type".to_owned()
        );
        assert_eq!(
            err("f3"),
            "invalid type string 'f3';\nthis system doesn't provide a 3-byte floating point type"
                .to_owned()
        );
        // The quoted string is always the *whole* original, not the tail.
        assert!(err("cd9").starts_with("invalid type string 'cd9';"));
    }

    #[test]
    fn overflowing_size_is_an_invalid_type_string() {
        // simple_strtoi overflows int rather than saturating, which upstream
        // reports as a bad type string with no second line.
        assert_eq!(err("d99999999999"), "invalid type string 'd99999999999'");
    }

    #[test]
    fn old_offsets_pick_their_radix_from_the_text() {
        assert_eq!(parse_old_offset(b"10"), Some(8)); // octal by default
        assert_eq!(parse_old_offset(b"+10"), Some(8)); // one leading + is dropped
        assert_eq!(parse_old_offset(b"0x10"), Some(16));
        assert_eq!(parse_old_offset(b"0X10"), Some(16));
        assert_eq!(parse_old_offset(b"10b"), Some(8 * 512)); // b = 512-byte blocks
        assert_eq!(parse_old_offset(b""), None);
        assert_eq!(parse_old_offset(b"9"), None); // not an octal digit
        // The documented `offset.` decimal form does not actually parse: the
        // dot selects base 10 but is never stripped. Upstream does the same.
        assert_eq!(parse_old_offset(b"10."), None);
    }

    fn old_operands(traditional: bool, args: &[&str]) -> (Draft, Vec<String>) {
        let mut draft = Draft {
            traditional,
            ..Draft::default()
        };
        let mut operands: Vec<OsString> = args.iter().map(OsString::from).collect();
        traditional_operands(&mut draft, &mut operands);
        let left = operands
            .iter()
            .map(|o| o.to_string_lossy().into_owned())
            .collect();
        (draft, left)
    }

    #[test]
    fn a_lone_plus_offset_is_an_offset_even_without_dash_dash_traditional() {
        let (draft, left) = old_operands(false, &["+20"]);
        assert_eq!(draft.options.skip, 0o20);
        assert!(left.is_empty());
        // Without the +, it is a file name.
        let (draft, left) = old_operands(false, &["20"]);
        assert_eq!(draft.options.skip, 0);
        assert_eq!(left, vec!["20".to_owned()]);
    }

    #[test]
    fn traditional_takes_file_then_offset_then_label() {
        let (draft, left) = old_operands(true, &["f", "20", "0x40"]);
        assert_eq!(draft.options.skip, 0o20);
        assert!(draft.flag_pseudo_start);
        assert_eq!(draft.pseudo_start, 0x40);
        assert_eq!(left, vec!["f".to_owned()]);
    }

    #[test]
    fn two_operands_without_traditional_are_file_and_offset() {
        let (draft, left) = old_operands(false, &["f", "20"]);
        assert_eq!(draft.options.skip, 0o20);
        assert!(!draft.flag_pseudo_start);
        assert_eq!(left, vec!["f".to_owned()]);
        // A second operand that is not an offset leaves both alone, so the
        // caller reports "extra operand".
        let (draft, left) = old_operands(false, &["f", "g"]);
        assert_eq!(draft.options.skip, 0);
        assert_eq!(left, vec!["f".to_owned(), "g".to_owned()]);
    }

    #[test]
    fn traditional_alone_is_offset_and_label() {
        let (draft, left) = old_operands(true, &["20", "0x40"]);
        assert_eq!(draft.options.skip, 0o20);
        assert!(draft.flag_pseudo_start);
        assert_eq!(draft.pseudo_start, 0x40);
        assert!(left.is_empty());
    }

    fn render(fmt: &str, datum: &[u8]) -> String {
        let spec = decode(fmt)[0];
        render_datum(&spec, datum, false)
    }

    #[test]
    fn integers_zero_fill_only_in_octal_and_hex() {
        assert_eq!(render("o1", &[7]), "007");
        assert_eq!(render("x2", &[0x34, 0x12]), "1234");
        assert_eq!(render("x2", &[1, 0]), "0001");
        // %*u and %*d carry no precision, so they are space-padded by the
        // column code instead of zero-filled here.
        assert_eq!(render("u1", &[7]), "7");
        assert_eq!(render("d1", &[0xff]), "-1");
        assert_eq!(render("d2", &[0xff, 0xff]), "-1");
        assert_eq!(render("u2", &[0xff, 0xff]), "65535");
    }

    #[test]
    fn character_formats_escape_and_name() {
        assert_eq!(render("c", b"A"), "A");
        assert_eq!(render("c", &[0]), "\\0");
        assert_eq!(render("c", b"\n"), "\\n");
        assert_eq!(render("c", &[0x1b]), "033"); // no \e escape in od
        assert_eq!(render("c", &[0xff]), "377");
        assert_eq!(render("a", b"A"), "A");
        assert_eq!(render("a", &[0]), "nul");
        assert_eq!(render("a", &[0x20]), "sp");
        assert_eq!(render("a", &[0x7f]), "del");
        // -t a ignores the high-order bit, so 0xc1 prints as 'A'.
        assert_eq!(render("a", &[0xc1]), "A");
    }

    #[test]
    fn swapping_reverses_the_datum_but_never_single_bytes() {
        let x2 = decode("x2")[0];
        assert_eq!(render_datum(&x2, &[0x34, 0x12], true), "3412");
        let c = decode("c")[0];
        assert_eq!(render_datum(&c, b"A", true), "A");
    }

    #[test]
    fn floats_use_the_shortest_round_tripping_precision() {
        // gnulib's ftoastr: %.15g first, widening only when it fails to
        // re-parse equal. 0.1 needs 17 digits; 1.5 needs 2.
        assert_eq!(ftoastr_f64(1.5), "1.5");
        // 0.1 already round-trips at %.15g, so it stays short; the value one
        // ulp away from it does not, and widens to seventeen digits.
        assert_eq!(ftoastr_f64(0.1), "0.1");
        assert_eq!(ftoastr_f64(f64::from_bits(0.1_f64.to_bits() + 1)), "0.10000000000000002");
        assert_eq!(ftoastr_f64(-0.0), "-0");
        assert_eq!(ftoastr_f64(f64::INFINITY), "inf");
        assert_eq!(ftoastr_f64(f64::NEG_INFINITY), "-inf");
        assert_eq!(ftoastr_f32(1.5), "1.5");
        assert_eq!(ftoastr_f32(0.1), "0.1");
        // Every double must survive the round trip that chose its precision.
        for x in [1.0_f64, std::f64::consts::PI, 1e300, 5e-324, 2.5e-10] {
            let s = ftoastr_f64(x);
            assert_eq!(s.parse::<f64>().unwrap(), x, "round trip of {x} via {s}");
        }
    }

    #[test]
    fn extended_floats_read_ten_of_their_sixteen_bytes() {
        // 1.0L: significand 0x8000000000000000, exponent 0x3fff.
        let mut one = [0u8; 16];
        one[7] = 0x80;
        one[8] = 0xff;
        one[9] = 0x3f;
        assert_eq!(ftoastr_ext(&one), "1");
        // The sign lives in bit 7 of byte 9.
        let mut minus = one;
        minus[9] |= 0x80;
        assert_eq!(ftoastr_ext(&minus), "-1");
        // Zero is all-zero; the padding bytes are ignored.
        let mut zero = [0u8; 16];
        zero[10] = 0xaa;
        assert_eq!(ftoastr_ext(&zero), "0");
    }

    fn columns(fmt: &str, block: &[u8], fields: usize, blank: usize, pad: usize) -> String {
        let mut spec = decode(fmt)[0];
        spec.pad_width = pad;
        let mut sink = Sink::new(Vec::new());
        print_fields(&mut sink, &spec, fields, blank, block, false);
        String::from_utf8(sink.inner).unwrap()
    }

    #[test]
    fn slack_padding_is_spread_across_the_columns() {
        // The real -t x1 line: sixteen 2-digit fields in a 48-column block, so
        // the 16 columns of slack come out as exactly one space each.
        assert_eq!(columns("x1", &[0xab; 16], 16, 0, 16), " ab".repeat(16));
        // No slack at all packs the fields with no separator whatsoever.
        assert_eq!(columns("x1", &[0xab; 4], 4, 0, 0), "abababab");
        // When the slack does not divide evenly it is spread by the running
        // `pad * (i - 1) / fields` remainder, not banked on one side: two
        // columns over four fields land on the first and third.
        assert_eq!(columns("x1", &[0xab; 4], 4, 0, 2), " abab abab");
    }

    #[test]
    fn blank_fields_are_dropped_from_the_right() {
        // A short final block prints only the fields it has, and the widths of
        // the ones it does print are unchanged.
        let full = columns("x1", &[1, 2, 3, 4], 4, 0, 2);
        let short = columns("x1", &[1, 2], 4, 2, 2);
        assert!(full.starts_with(&short), "{short:?} is not a prefix of {full:?}");
    }

    #[test]
    fn addresses_are_padded_to_the_radix_width() {
        assert_eq!(digits(0, 8, 7), b"0000000".to_vec());
        assert_eq!(digits(8, 8, 7), b"0000010".to_vec());
        assert_eq!(digits(255, 16, 6), b"0000ff".to_vec());
        assert_eq!(digits(255, 10, 7), b"0000255".to_vec());
        // A value wider than the pad is not truncated.
        assert_eq!(digits(u64::MAX, 16, 6), b"ffffffffffffffff".to_vec());
        // -A n pads to nothing at all.
        assert_eq!(digits(8, 8, 0), b"10".to_vec());
    }

    #[test]
    fn the_line_length_is_the_lcm_of_every_datum_size() {
        assert_eq!(get_lcm(&decode("x1")), 1);
        assert_eq!(get_lcm(&decode("x1x2")), 2);
        assert_eq!(get_lcm(&decode("x2x4")), 4);
        assert_eq!(get_lcm(&decode("x2fL")), 16);
        // A 4- and an 8-byte type still line up on 8, not 32.
        assert_eq!(get_lcm(&decode("x4x8")), 8);
    }
}
