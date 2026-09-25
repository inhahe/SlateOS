//! `numfmt` — convert numbers from or to human-readable strings.
//!
//! ```text
//! Usage: numfmt [OPTION]... [NUMBER]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/numfmt.c`. `df`, `du` and `ls -l` output
//! is routinely piped through it (`--to=iec`, `--field`, `--header`), and there
//! was none: the `shuf` crate carried an unreachable `numfmt` personality that
//! nothing ever started.
//!
//! # The arithmetic is `long double`'s
//!
//! Upstream reads, scales, rounds and prints every number as an x87 `long
//! double`, and the digits it prints are the digits that arithmetic produces:
//! `--to=si 999999` is `1.0M` because `999.999` rounds up and is then divided
//! by 1000 again. So every step here is [`ExtF80`] -- multiplication, division,
//! the `(intmax_t)` casts `simple_round` is made of -- and the printing is
//! `extfloat`'s `%.*Lf` and `%Lg`, which are glibc's to the digit.
//!
//! # Upstream's quirks, reproduced
//!
//! These are visible in output, measured against GNU 9.4, and kept:
//!
//! * `5.` is `invalid number` (the fraction must have a digit), and `5..` is an
//!   `invalid suffix`;
//! * in `--format`, a `%%` before the directive counts as one byte of the
//!   prefix, so `a%%b%f` prints `a%%` and loses the `b`; one after it is
//!   printed as `%%`;
//! * `--from=iec-i` wants its `i` even on a number with no suffix;
//! * an invalid field is printed as it was read, less a `--suffix` that was
//!   trimmed from it before the number was found to be bad.
//!
//! One thing is not ported: `---debug`, the undocumented developer trace. It is
//! accepted and turns on `--debug`, and the trace itself is not written.
//!
//! # Checked against GNU
//!
//! `scripts/numfmt-diff.sh`.

use coreutils::extfloat::{ExtF80, Spec, render};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::setfields::{self, Range};
use coreutils::xnum::{Status, xstrtoimax, xstrtoumax};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const NUMFMT: Program = Program::new("numfmt", 1);

/// Upstream's status for a number it could not convert.
const EXIT_CONVERSION_WARNINGS: u8 = 2;

/// `LDBL_DIG`: digits a `long double` holds without loss, and so the most an
/// unscaled number may print.
const MAX_UNSCALED_DIGITS: u64 = 18;

/// 999Q: the largest value with a suffix to write it with.
const MAX_ACCEPTABLE_DIGITS: u64 = 33;

/// A rendered number between ASCII apostrophes, the way upstream's messages
/// print one: `value too large to be printed: '1.23457e+19'`.
///
/// Upstream writes these as `'%Lg'` in the format string rather than through
/// `quote()`, and this port prints the same bytes, because what stands between
/// the apostrophes is `printf`'s rendering of a `long double` -- digits, a
/// sign, a point, an exponent, `inf` or `nan` -- which can hold neither a
/// newline nor a quote to break out of the message with. They are added here
/// instead of around a placeholder in each format string so that that shape,
/// which `scripts/quote-names.py` refuses, keeps meaning "a name somebody
/// quoted by hand".
fn apostrophes(number: &str) -> String {
    let mut out = String::with_capacity(number.len().saturating_add(2));
    out.push('\'');
    out.push_str(number);
    out.push('\'');
    out
}

/// The suffix letters, from K (1) to Q (10).
const SUFFIXES: &[u8] = b"KMGTPEZYRQ";

/// Upstream's `longopts[]`, in declaration order. `-debug` is spelled
/// `---debug` on a command line.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("from", Takes::Required),
    ("from-unit", Takes::Required),
    ("to", Takes::Required),
    ("to-unit", Takes::Required),
    ("round", Takes::Required),
    ("padding", Takes::Required),
    ("suffix", Takes::Required),
    ("grouping", Takes::Nothing),
    ("delimiter", Takes::Required),
    ("field", Takes::Required),
    ("debug", Takes::Nothing),
    ("-debug", Takes::Nothing),
    ("header", Takes::Optional),
    ("format", Takes::Required),
    ("invalid", Takes::Required),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scale {
    None,
    /// `--from` only: K is 1000, Ki is 1024.
    Auto,
    Si,
    Iec,
    /// IEC, and the `i` is required.
    IecI,
}

impl Scale {
    fn base(self) -> u32 {
        match self {
            Scale::Iec | Scale::IecI => 1024,
            Scale::None | Scale::Auto | Scale::Si => 1000,
        }
    }
}

const SCALE_FROM: &[(&str, Scale)] = &[
    ("none", Scale::None),
    ("auto", Scale::Auto),
    ("si", Scale::Si),
    ("iec", Scale::Iec),
    ("iec-i", Scale::IecI),
];

const SCALE_TO: &[(&str, Scale)] = &[
    ("none", Scale::None),
    ("si", Scale::Si),
    ("iec", Scale::Iec),
    ("iec-i", Scale::IecI),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoundType {
    Ceiling,
    Floor,
    FromZero,
    ToZero,
    Nearest,
}

const ROUNDS: &[(&str, RoundType)] = &[
    ("up", RoundType::Ceiling),
    ("down", RoundType::Floor),
    ("from-zero", RoundType::FromZero),
    ("towards-zero", RoundType::ToZero),
    ("nearest", RoundType::Nearest),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Invalid {
    Abort,
    Fail,
    Warn,
    Ignore,
}

const INVALIDS: &[(&str, Invalid)] = &[
    ("abort", Invalid::Abort),
    ("fail", Invalid::Fail),
    ("warn", Invalid::Warn),
    ("ignore", Invalid::Ignore),
];

/// Everything the command line decided.
#[derive(Clone, Debug)]
struct Settings {
    scale_from: Scale,
    scale_to: Scale,
    round: RoundType,
    inval: Invalid,
    suffix: Option<Vec<u8>>,
    from_unit: u64,
    to_unit: u64,
    grouping: bool,
    /// `--padding`'s width; negative means left-aligned.
    padding: i64,
    fields: Option<Vec<Range>>,
    /// `-d`: `None` for the default, runs of blanks.
    delimiter: Option<u8>,
    line_delim: u8,
    debug: bool,
    header: u64,
    format: Option<Vec<u8>>,
    operands: Vec<OsString>,
}

#[cfg_attr(test, derive(Debug))]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

fn help_text() -> String {
    "\
Usage: numfmt [OPTION]... [NUMBER]...
Reformat NUMBER(s), or the numbers from standard input if none are specified.

Mandatory arguments to long options are mandatory for short options too.
      --debug          print warnings about invalid input
  -d, --delimiter=X    use X instead of whitespace for field delimiter
      --field=FIELDS   replace the numbers in these input fields (default=1);
                         see FIELDS below
      --format=FORMAT  use printf style floating-point FORMAT;
                         see FORMAT below for details
      --from=UNIT      auto-scale input numbers to UNITs; default is 'none';
                         see UNIT below
      --from-unit=N    specify the input unit size (instead of the default 1)
      --grouping       use locale-defined grouping of digits, e.g. 1,000,000
                         (which means it has no effect in the C/POSIX locale)
      --header[=N]     print (without converting) the first N header lines;
                         N defaults to 1 if not specified
      --invalid=MODE   failure mode for invalid numbers: MODE can be:
                         abort (default), fail, warn, ignore
      --padding=N      pad the output to N characters; positive N will
                         right-align; negative N will left-align;
                         padding is ignored if the output is wider than N;
                         the default is to automatically pad if a whitespace
                         is found
      --round=METHOD   use METHOD for rounding when scaling; METHOD can be:
                         up, down, from-zero (default), towards-zero, nearest
      --suffix=SUFFIX  add SUFFIX to output numbers, and accept optional
                         SUFFIX in input numbers
      --to=UNIT        auto-scale output numbers to UNITs; see UNIT below
      --to-unit=N      the output unit size (instead of the default 1)
  -z, --zero-terminated    line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

UNIT options:
  none       no auto-scaling is done; suffixes will trigger an error
  auto       accept optional single/two letter suffix:
               1K = 1000,
               1Ki = 1024,
               1M = 1000000,
               1Mi = 1048576,
  si         accept optional single letter suffix:
               1K = 1000,
               1M = 1000000,
               ...
  iec        accept optional single letter suffix:
               1K = 1024,
               1M = 1048576,
               ...
  iec-i      accept optional two-letter suffix:
               1Ki = 1024,
               1Mi = 1048576,
               ...

FIELDS supports cut(1) style field ranges:
  N    N'th field, counted from 1
  N-   from N'th field, to end of line
  N-M  from N'th to M'th field (inclusive)
  -M   from first to M'th field (inclusive)
  -    all fields
Multiple fields/ranges can be separated with commas

FORMAT must be suitable for printing one floating-point argument '%f'.
Optional quote (%'f) will enable --grouping (if supported by current locale).
Optional width value (%10f) will pad output. Optional zero (%010f) width
will zero pad the number. Optional negative values (%-10f) will left align.
Optional precision (%.1f) will override the input determined precision.

Exit status is 0 if all input numbers were successfully converted.
By default, numfmt will stop at the first conversion error with exit status 2.
With --invalid='fail' a warning is printed for each conversion error
and the exit status is 2.  With --invalid='warn' each conversion error is
diagnosed, but the exit status is 0.  With --invalid='ignore' conversion
errors are not diagnosed and the exit status is 0.

Examples:
  $ numfmt --to=si 1000
            -> \"1.0K\"
  $ numfmt --to=iec 2048
           -> \"2.0K\"
  $ numfmt --to=iec-i 4096
           -> \"4.0Ki\"
  $ echo 1K | numfmt --from=si
           -> \"1000\"
  $ echo 1K | numfmt --from=iec
           -> \"1024\"
  $ df -B1 | numfmt --header --field 2-4 --to=si
  $ ls -l  | numfmt --header --field 5 --to=iec
  $ ls -lh | numfmt --header --field 5 --from=iec --padding=10
  $ ls -lh | numfmt --header --field 5 --from=iec --format %10f
"
    .to_string()
}

/// A fatal diagnostic from the option loop: upstream's `error
/// (EXIT_FAILURE, …)`, which carries no referral.
fn fatal(message: String) -> getopt::Error {
    NUMFMT.usage(message)
}

/// Upstream's `unit_to_umax`: a unit size, where `K` is 1000 and `Ki` is 1024.
///
/// # Errors
///
/// `invalid unit size` for anything `xstrtoumax` rejects, and for zero.
fn unit_to_umax(text: &[u8]) -> Result<u64, getopt::Error> {
    let invalid = || fatal(format!("invalid unit size: {}", quote(text)));
    // A bare suffix letter would be read as 1024; `K` means 1000 here, so an
    // `B` is appended to ask for the decimal reading, and a trailing `i` --
    // after a letter -- is removed to leave the binary one.
    let mut adjusted = text.to_vec();
    let mut suffixes: &[u8] = SUFFIXES;
    if let Some(&last) = text.last()
        && !last.is_ascii_digit()
    {
        let before = text.len().checked_sub(2).and_then(|i| text.get(i));
        if last == b'i' && before.is_some_and(|b| !b.is_ascii_digit()) {
            adjusted.pop();
        } else {
            adjusted.push(b'B');
            suffixes = b"0KMGTPEZYRQ";
        }
    }
    match xstrtoumax(&adjusted, Some(suffixes)) {
        (n, Status::Ok) if n != 0 => Ok(n),
        _ => Err(invalid()),
    }
}

/// The option loop, then upstream's checks after it.
///
/// # Errors
///
/// Anything upstream refuses before it reads a number.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut s = Settings {
        scale_from: Scale::None,
        scale_to: Scale::None,
        round: RoundType::FromZero,
        inval: Invalid::Abort,
        suffix: None,
        from_unit: 1,
        to_unit: 1,
        grouping: false,
        padding: 0,
        fields: None,
        delimiter: None,
        line_delim: b'\n',
        debug: false,
        header: 0,
        format: None,
        operands: Vec::new(),
    };
    for item in NUMFMT.parse(args, "d:z", LONG_OPTIONS) {
        match item? {
            Opt::Long("from", Some(v)) => {
                s.scale_from = NUMFMT.argmatch(&os_bytes(&v), "--from", SCALE_FROM)?;
            }
            Opt::Long("from-unit", Some(v)) => s.from_unit = unit_to_umax(&os_bytes(&v))?,
            Opt::Long("to", Some(v)) => {
                s.scale_to = NUMFMT.argmatch(&os_bytes(&v), "--to", SCALE_TO)?;
            }
            Opt::Long("to-unit", Some(v)) => s.to_unit = unit_to_umax(&os_bytes(&v))?,
            Opt::Long("round", Some(v)) => {
                s.round = NUMFMT.argmatch(&os_bytes(&v), "--round", ROUNDS)?;
            }
            Opt::Long("grouping", _) => s.grouping = true,
            Opt::Long("padding", Some(v)) => {
                let text = os_bytes(&v);
                match xstrtoimax(&text, Some(b"")) {
                    (n, Status::Ok) if n != 0 && n != i64::MIN => s.padding = n,
                    _ => return Err(fatal(format!("invalid padding value {}", quote(&text)))),
                }
            }
            Opt::Long("field", Some(v)) => {
                if s.fields.is_some() {
                    return Err(fatal("multiple field specifications".to_string()));
                }
                s.fields = Some(setfields::set_fields(
                    NUMFMT,
                    &os_bytes(&v),
                    setfields::Flags {
                        allow_dash: true,
                        ..setfields::Flags::default()
                    },
                )?);
            }
            Opt::Short(b'd', Some(v)) | Opt::Long("delimiter", Some(v)) => {
                let text = os_bytes(&v);
                if text.len() > 1 {
                    return Err(fatal(
                        "the delimiter must be a single character".to_string(),
                    ));
                }
                // `-d ''` is the NUL byte.
                s.delimiter = Some(text.first().copied().unwrap_or(0));
            }
            Opt::Short(b'z', _) | Opt::Long("zero-terminated", _) => s.line_delim = 0,
            Opt::Long("suffix", Some(v)) => s.suffix = Some(os_bytes(&v).into_owned()),
            Opt::Long("debug" | "-debug", _) => s.debug = true,
            Opt::Long("header", value) => {
                s.header = match value {
                    None => 1,
                    Some(v) => {
                        let text = os_bytes(&v);
                        match xstrtoumax(&text, Some(b"")) {
                            (n, Status::Ok) if n != 0 => n,
                            _ => {
                                return Err(fatal(format!(
                                    "invalid header value {}",
                                    quote(&text)
                                )));
                            }
                        }
                    }
                };
            }
            Opt::Long("format", Some(v)) => s.format = Some(os_bytes(&v).into_owned()),
            Opt::Long("invalid", Some(v)) => {
                s.inval = NUMFMT.argmatch(&os_bytes(&v), "--invalid", INVALIDS)?;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => s.operands.push(x.clone()),
            // Unreachable: required values are always present, and every
            // name in the table is handled.
            Opt::Long(other, _) => {
                return Err(NUMFMT.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(NUMFMT.invalid_option(c)),
        }
    }
    if s.format.is_some() && s.grouping {
        return Err(fatal(
            "--grouping cannot be combined with --format".to_string(),
        ));
    }
    Ok(Request::Run(Box::new(s)))
}

/// What `--format` decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Format {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    grouping: bool,
    /// `%0Nf`.
    zero_padding_width: usize,
    /// `%Nf` or `%-Nf`, when given: it replaces `--padding`.
    padding: Option<i64>,
    /// `%.Nf`.
    precision: Option<usize>,
}

/// C's `strtol` in base 10 over `bytes`: white space, an optional sign,
/// digits. The value (saturated, with the overflow flagged) and how many bytes
/// it took; zero bytes when there was no number.
fn strtol(bytes: &[u8]) -> (i64, usize, bool) {
    let mut i = bytes
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .unwrap_or(bytes.len());
    let mut negative = false;
    if let Some(&sign @ (b'+' | b'-')) = bytes.get(i) {
        negative = sign == b'-';
        i = i.saturating_add(1);
    }
    let start = i;
    let mut value: i128 = 0;
    let mut overflow = false;
    while let Some(&d) = bytes.get(i).filter(|b| b.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(i128::from(d.wrapping_sub(b'0')));
        i = i.saturating_add(1);
    }
    if i == start {
        return (0, 0, false);
    }
    let signed = if negative {
        value.saturating_neg()
    } else {
        value
    };
    let clamped = match i64::try_from(signed) {
        Ok(v) => v,
        Err(_) => {
            overflow = true;
            if negative { i64::MIN } else { i64::MAX }
        }
    };
    (clamped, i, overflow)
}

/// Upstream's `parse_format_string`.
///
/// # Errors
///
/// The six ways upstream refuses a `--format`.
fn parse_format_string(
    fmt: &[u8],
    debug: bool,
    padding: i64,
) -> Result<(Format, Vec<String>), String> {
    let mut out = Format::default();
    let mut warnings = Vec::new();
    let at = |i: usize| fmt.get(i).copied().unwrap_or(0);
    // The prefix: every byte up to the first `%` not followed by `%`. A `%%`
    // advances two bytes and counts one -- upstream's own accounting, kept.
    let mut i = 0usize;
    let mut prefix_len = 0usize;
    while !(at(i) == b'%' && at(i.saturating_add(1)) != b'%') {
        if i >= fmt.len() {
            return Err(format!("format {} has no % directive", quote(fmt)));
        }
        prefix_len = prefix_len.saturating_add(1);
        i = i.saturating_add(if at(i) == b'%' { 2 } else { 1 });
    }
    i = i.saturating_add(1);
    // Flags: `'` and `0`, with spaces anywhere among them.
    let mut zero_padding = false;
    loop {
        let skip = fmt
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| b == b' ')
            .count();
        i = i.saturating_add(skip);
        if at(i) == b'\'' {
            out.grouping = true;
            i = i.saturating_add(1);
        } else if at(i) == b'0' {
            zero_padding = true;
            i = i.saturating_add(1);
        } else if skip == 0 {
            break;
        }
    }
    let (pad, used, overflow) = strtol(fmt.get(i..).unwrap_or_default());
    if overflow || pad == i64::MIN {
        return Err(format!("invalid format {} (width overflow)", quote(fmt)));
    }
    if used != 0 && pad != 0 {
        if debug && padding != 0 && !(zero_padding && pad > 0) {
            warnings.push("--format padding overriding --padding".to_string());
        }
        if pad < 0 || !zero_padding {
            out.padding = Some(pad);
        } else {
            out.zero_padding_width = usize::try_from(pad).unwrap_or(usize::MAX);
        }
    }
    i = i.saturating_add(used);
    if i >= fmt.len() {
        return Err(format!("format {} ends in %", quote(fmt)));
    }
    if at(i) == b'.' {
        i = i.saturating_add(1);
        let rest = fmt.get(i..).unwrap_or_default();
        let (prec, used, overflow) = strtol(rest);
        if overflow || prec < 0 || matches!(rest.first(), Some(b' ' | b'\t' | b'+')) {
            return Err(format!("invalid precision in format {}", quote(fmt)));
        }
        out.precision = Some(usize::try_from(prec).unwrap_or(0));
        i = i.saturating_add(used);
    }
    if at(i) != b'f' {
        return Err(format!(
            "invalid format {}, directive must be %[0]['][-][N][.][N]f",
            quote(fmt)
        ));
    }
    i = i.saturating_add(1);
    let suffix_pos = i;
    while i < fmt.len() {
        if at(i) == b'%' && at(i.saturating_add(1)) != b'%' {
            return Err(format!("format {} has too many % directives", quote(fmt)));
        }
        i = i.saturating_add(if at(i) == b'%' { 2 } else { 1 });
    }
    out.prefix = fmt.get(..prefix_len).unwrap_or_default().to_vec();
    out.suffix = fmt.get(suffix_pos..).unwrap_or_default().to_vec();
    Ok((out, warnings))
}

// --------------------------------------------------------- the arithmetic ---

fn ld(n: i64) -> ExtF80 {
    ExtF80::from_i64(n)
}

fn is_negative(v: ExtF80) -> bool {
    v.lt(ExtF80::ZERO)
}

fn abs(v: ExtF80) -> ExtF80 {
    if is_negative(v) { -v } else { v }
}

/// `a >= b`, as C compares.
fn ge(a: ExtF80, b: ExtF80) -> bool {
    matches!(
        a.partial_cmp(b),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    )
}

/// Upstream's `powerld`: `base` multiplied into itself `x - 1` times.
fn powerld(base: ExtF80, x: u32) -> ExtF80 {
    if x == 0 {
        return ExtF80::ONE;
    }
    let mut result = base;
    for _ in 1..x {
        result = result * base;
    }
    result
}

/// Upstream's `expld`: divide by `base` until below it, counting.
fn expld(val: ExtF80, base: u32) -> (ExtF80, u32) {
    let base = ld(i64::from(base));
    let mut val = val;
    let mut power = 0u32;
    if val.is_finite() {
        while ge(abs(val), base) {
            power = power.saturating_add(1);
            val = val / base;
        }
    }
    (val, power)
}

/// `(intmax_t) v`.
fn to_int(v: ExtF80) -> i64 {
    v.to_i64_trunc()
}

fn round_ceiling(val: ExtF80) -> i64 {
    let intval = to_int(val);
    if ld(intval).lt(val) {
        intval.wrapping_add(1)
    } else {
        intval
    }
}

fn round_floor(val: ExtF80) -> i64 {
    round_ceiling(-val).wrapping_neg()
}

/// Upstream's `simple_round`: bring the value within `intmax_t`, round the
/// remainder with integer casts, and put the whole back.
fn simple_round(val: ExtF80, how: RoundType) -> ExtF80 {
    let max = ld(i64::MAX);
    let intmax_mul = to_int(val / max);
    let val = val - max * ld(intmax_mul);
    let rval = match how {
        RoundType::Ceiling => round_ceiling(val),
        RoundType::Floor => round_floor(val),
        RoundType::FromZero => {
            if is_negative(val) {
                round_floor(val)
            } else {
                round_ceiling(val)
            }
        }
        RoundType::ToZero => to_int(val),
        RoundType::Nearest => {
            let half = ExtF80::ONE / ld(2);
            if is_negative(val) {
                to_int(val - half)
            } else {
                to_int(val + half)
            }
        }
    };
    max * ld(intmax_mul) + ld(rval)
}

// ------------------------------------------------------------ reading one ---

/// Upstream's `enum simple_strtod_error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sse {
    Ok,
    OkPrecisionLoss,
    Overflow,
    InvalidNumber,
    ValidButForbiddenSuffix,
    InvalidSuffix,
    MissingISuffix,
}

impl Sse {
    fn ok(self) -> bool {
        matches!(self, Sse::Ok | Sse::OkPrecisionLoss)
    }
}

/// Upstream's `simple_strtod_int`: an optional `-`, digits. Returns the status,
/// the value, whether it was negative, and where it stopped.
fn strtod_int(s: &[u8]) -> (Sse, ExtF80, bool, usize) {
    let mut i = 0usize;
    let negative = s.first() == Some(&b'-');
    if negative {
        i = 1;
    }
    let mut e = Sse::Ok;
    let mut val = ExtF80::ZERO;
    let mut digits = 0u64;
    let mut found = false;
    let ten = ld(10);
    while let Some(&c) = s.get(i).filter(|c| c.is_ascii_digit()) {
        let digit = c.wrapping_sub(b'0');
        found = true;
        if !val.is_zero() || digit != 0 {
            digits = digits.saturating_add(1);
        }
        if digits > MAX_UNSCALED_DIGITS {
            e = Sse::OkPrecisionLoss;
        }
        if digits > MAX_ACCEPTABLE_DIGITS {
            return (Sse::Overflow, val, negative, i);
        }
        val = val * ten + ld(i64::from(digit));
        i = i.saturating_add(1);
    }
    if !found && s.get(i) != Some(&b'.') {
        return (Sse::InvalidNumber, val, negative, i);
    }
    if negative {
        val = -val;
    }
    (e, val, negative, i)
}

/// Upstream's `simple_strtod_float`: `N[.N]`. Returns the status, the value,
/// the digits after the point, and where it stopped.
fn strtod_float(s: &[u8]) -> (Sse, ExtF80, usize, usize) {
    let (mut e, mut value, negative, mut end) = strtod_int(s);
    if !e.ok() {
        return (e, value, 0, end);
    }
    let mut precision = 0usize;
    if s.get(end) == Some(&b'.') {
        end = end.saturating_add(1);
        let rest = s.get(end..).unwrap_or_default();
        let (e2, frac, neg_frac, used) = strtod_int(rest);
        if !e2.ok() {
            return (e2, value, precision, end.saturating_add(used));
        }
        if e2 == Sse::OkPrecisionLoss {
            e = e2;
        }
        if neg_frac {
            return (
                Sse::InvalidNumber,
                value,
                precision,
                end.saturating_add(used),
            );
        }
        let frac = frac / powerld(ld(10), u32::try_from(used).unwrap_or(u32::MAX));
        value = if negative { value - frac } else { value + frac };
        precision = used;
        end = end.saturating_add(used);
    }
    (e, value, precision, end)
}

/// Upstream's `suffix_power`.
fn suffix_power(c: u8) -> u32 {
    SUFFIXES
        .iter()
        .position(|&s| s == c)
        .and_then(|i| u32::try_from(i.saturating_add(1)).ok())
        .unwrap_or(0)
}

/// Upstream's `simple_strtod_human`: a number and an optional suffix.
fn strtod_human(s: &[u8], scaling: Scale) -> (Sse, ExtF80, usize, usize) {
    let (e, mut value, mut precision, mut end) = strtod_float(s);
    if !e.ok() {
        return (e, value, precision, end);
    }
    let mut power = 0u32;
    let mut base = scaling.base();
    if end < s.len() {
        while matches!(s.get(end), Some(b' ' | b'\t')) {
            end = end.saturating_add(1);
        }
        // `strchr` finds the terminator too, so a string that ends here
        // counts as a valid suffix -- reachable only with `-d`, since the
        // default delimiter never leaves blanks in a field.
        let c = s.get(end).copied();
        if c.is_some_and(|c| !SUFFIXES.contains(&c)) {
            return (Sse::InvalidSuffix, value, precision, end);
        }
        if scaling == Scale::None {
            return (Sse::ValidButForbiddenSuffix, value, precision, end);
        }
        power = c.map_or(0, suffix_power);
        end = end.saturating_add(1);
        if scaling == Scale::Auto && s.get(end) == Some(&b'i') {
            base = 1024;
            end = end.saturating_add(1);
        }
        precision = 0;
    }
    if scaling == Scale::IecI {
        if s.get(end) == Some(&b'i') {
            end = end.saturating_add(1);
        } else {
            return (Sse::MissingISuffix, value, precision, end);
        }
    }
    value = value * powerld(ld(i64::from(base)), power);
    (e, value, precision, end.min(s.len()))
}

// ---------------------------------------------------------- the processor ---

/// A conversion failure in `--invalid=abort` mode: upstream's `error (2, …)`
/// has already been printed and the run stops.
#[derive(Debug)]
struct Abort;

/// Everything a run carries from field to field, including the one piece of
/// state upstream keeps in a global: the automatic padding width, set by each
/// field and read by the next.
struct Run<'a> {
    s: &'a Settings,
    fmt: Format,
    scale_from: Scale,
    padding_width: usize,
    padding_left: bool,
    auto_padding: bool,
    out: Vec<u8>,
    diags: Vec<String>,
}

impl Run<'_> {
    /// A conversion diagnostic: printed unless `--invalid=ignore`, and fatal
    /// in `--invalid=abort`.
    fn conversion_error(&mut self, message: String) -> Result<(), Abort> {
        if self.s.inval != Invalid::Ignore {
            self.diags.push(message);
        }
        if self.s.inval == Invalid::Abort {
            return Err(Abort);
        }
        Ok(())
    }

    /// Upstream's `parse_human_number`.
    fn parse_human_number(&mut self, s: &[u8]) -> Result<(Sse, ExtF80, usize), Abort> {
        let (e, value, precision, end) = strtod_human(s, self.scale_from);
        if !e.ok() {
            let message = match e {
                Sse::Overflow => format!("value too large to be converted: {}", quote(s)),
                Sse::InvalidNumber => format!("invalid number: {}", quote(s)),
                Sse::ValidButForbiddenSuffix => {
                    format!(
                        "rejecting suffix in input: {} (consider using --from)",
                        quote(s)
                    )
                }
                Sse::InvalidSuffix => format!("invalid suffix in input: {}", quote(s)),
                _ => format!("missing 'i' suffix in input: {} (e.g Ki/Mi/Gi)", quote(s)),
            };
            self.conversion_error(message)?;
            return Ok((e, value, precision));
        }
        if let Some(rest) = s.get(end..).filter(|r| !r.is_empty()) {
            let message = format!("invalid suffix in input {}: {}", quote(s), quote(rest));
            self.conversion_error(message)?;
            return Ok((Sse::InvalidSuffix, value, precision));
        }
        Ok((e, value, precision))
    }

    /// Upstream's `process_suffixed_number`. `text` is trimmed of the suffix
    /// in place, as upstream trims its buffer.
    fn process_suffixed_number(
        &mut self,
        text: &mut Vec<u8>,
        field: u64,
    ) -> Result<Option<(ExtF80, usize)>, Abort> {
        if let Some(suffix) = &self.s.suffix
            && text.len() > suffix.len()
            && text.ends_with(suffix)
        {
            text.truncate(text.len().saturating_sub(suffix.len()));
        }
        let start = text
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count();
        if self.auto_padding {
            self.padding_width = if start > 0 || field > 1 {
                text.len()
            } else {
                0
            };
        }
        let p = text.get(start..).unwrap_or_default().to_vec();
        let (e, mut val, precision) = self.parse_human_number(&p)?;
        if e == Sse::OkPrecisionLoss && self.s.debug {
            self.diags.push(format!(
                "large input value {}: possible precision loss",
                quote(&p)
            ));
        }
        if self.s.from_unit != 1 || self.s.to_unit != 1 {
            val = val * ExtF80::from_u64(self.s.from_unit) / ExtF80::from_u64(self.s.to_unit);
        }
        Ok(e.ok().then_some((val, precision)))
    }

    /// Upstream's `double_to_human`. `Err` carries upstream's "failed to
    /// prepare" message, which ends the run with status 1.
    fn double_to_human(&self, val: ExtF80, precision: usize) -> Result<Vec<u8>, String> {
        let spec = |prec: usize| Spec {
            zero: self.fmt.zero_padding_width > 0,
            width: self.fmt.zero_padding_width,
            ..Spec::fixed(prec)
        };
        let failed = |v: ExtF80| {
            format!(
                "failed to prepare value {} for printing",
                apostrophes(&render(&Spec::fixed(6), v))
            )
        };
        let to = self.s.scale_to;
        let prec_u32 = |p: usize| u32::try_from(p).unwrap_or(u32::MAX);
        if to == Scale::None {
            let scale = powerld(ld(10), prec_u32(precision));
            let val = simple_round(val * scale, self.s.round) / scale;
            let text = render(&spec(precision), val);
            if text.len() >= 128 {
                return Err(failed(val));
            }
            return Ok(text.into_bytes());
        }
        let base = to.base();
        let (mut val, mut power) = expld(val, base);
        let power_adjust = match self.fmt.precision {
            Some(user) => u32::try_from(user)
                .unwrap_or(u32::MAX)
                .min(power.saturating_mul(3)),
            None if abs(val).lt(ld(10)) => 1,
            None => 0,
        };
        let scale = powerld(ld(10), power_adjust);
        val = simple_round(val * scale, self.s.round) / scale;
        let base_ld = ld(i64::from(base));
        if ge(abs(val), base_ld) {
            val = val / base_ld;
            power = power.saturating_add(1);
        }
        let show_decimal_point = !val.is_zero() && abs(val).lt(ld(10)) && power > 0;
        let prec = self
            .fmt
            .precision
            .unwrap_or(usize::from(show_decimal_point));
        let mut text = render(&spec(prec), val);
        text.push_str(match power {
            0 => "",
            1..=10 => {
                let i = usize::try_from(power).unwrap_or(0).saturating_sub(1);
                // One ASCII letter, so the conversion cannot fail.
                std::str::from_utf8(SUFFIXES.get(i..=i).unwrap_or_default()).unwrap_or("")
            }
            _ => "(error)",
        });
        if text.len() >= 127 {
            return Err(failed(val));
        }
        let mut bytes = text.into_bytes();
        if to == Scale::IecI && power > 0 {
            bytes.push(b'i');
        }
        Ok(bytes)
    }

    /// Upstream's `prepare_padded_number` and `print_padded_number`.
    /// `Ok(false)` is a number that cannot be printed (already reported).
    fn print_number(
        &mut self,
        val: ExtF80,
        precision: usize,
    ) -> Result<Result<bool, String>, Abort> {
        let precision_used = self.fmt.precision.unwrap_or(precision);
        let (_, x) = expld(val, 10);
        let x = u64::from(x);
        let g = render(&Spec::general(), val);
        if self.s.scale_to == Scale::None
            && x.saturating_add(u64::try_from(precision_used).unwrap_or(u64::MAX))
                > MAX_UNSCALED_DIGITS
        {
            let message = if precision_used > 0 {
                let shown = apostrophes(&format!("{g}/{precision_used}"));
                format!("value/precision too large to be printed: {shown} (consider using --to)")
            } else {
                let shown = apostrophes(&g);
                format!("value too large to be printed: {shown} (consider using --to)")
            };
            self.conversion_error(message)?;
            return Ok(Ok(false));
        }
        if x > MAX_ACCEPTABLE_DIGITS.saturating_sub(1) {
            let shown = apostrophes(&g);
            self.conversion_error(format!(
                "value too large to be printed: {shown} (cannot handle values > 999Q)"
            ))?;
            return Ok(Ok(false));
        }
        let mut buf = match self.double_to_human(val, precision_used) {
            Ok(buf) => buf,
            Err(message) => return Ok(Err(message)),
        };
        if let Some(suffix) = &self.s.suffix {
            // `strncat` into a 128-byte buffer.
            let room = 127usize.saturating_sub(buf.len());
            buf.extend_from_slice(suffix.get(..room.min(suffix.len())).unwrap_or_default());
        }
        let padded = if self.padding_width > 0 && buf.len() < self.padding_width {
            let fill = vec![b' '; self.padding_width.saturating_sub(buf.len())];
            if self.padding_left {
                [buf, fill].concat()
            } else {
                [fill, buf].concat()
            }
        } else {
            buf
        };
        self.out.extend_from_slice(&self.fmt.prefix);
        self.out.extend_from_slice(&padded);
        self.out.extend_from_slice(&self.fmt.suffix);
        Ok(Ok(true))
    }

    /// Upstream's `process_field`.
    fn process_field(&mut self, text: &[u8], field: u64) -> Result<Result<bool, String>, Abort> {
        let included = match &self.s.fields {
            None => field == 1,
            Some(ranges) => ranges
                .iter()
                .any(|r| r.lo <= field && field <= r.hi && r.lo != u64::MAX),
        };
        if !included {
            self.out.extend_from_slice(text);
            return Ok(Ok(true));
        }
        let mut text = text.to_vec();
        let valid = match self.process_suffixed_number(&mut text, field)? {
            Some((val, precision)) => match self.print_number(val, precision)? {
                Ok(printed) => printed,
                Err(message) => return Ok(Err(message)),
            },
            None => false,
        };
        if !valid {
            self.out.extend_from_slice(&text);
        }
        Ok(Ok(valid))
    }

    /// Upstream's `process_line`: the fields, each followed by one separator.
    fn process_line(&mut self, line: &[u8], newline: bool) -> Result<Result<bool, String>, Abort> {
        // The line is a C string: it ends at its first NUL.
        let line = line.split(|&b| b == 0).next().unwrap_or_default();
        let mut valid = true;
        let mut at = 0usize;
        let mut field = 0u64;
        loop {
            field = field.saturating_add(1);
            let rest = line.get(at..).unwrap_or_default();
            let len = match self.s.delimiter {
                Some(d) => rest.iter().position(|&b| b == d).unwrap_or(rest.len()),
                None => {
                    let sep = |b: u8| b == b' ' || b == b'\t' || b == b'\n';
                    let lead = rest.iter().take_while(|&&b| sep(b)).count();
                    let body = rest.get(lead..).unwrap_or_default();
                    lead.saturating_add(body.iter().take_while(|&&b| !sep(b)).count())
                }
            };
            let text = rest.get(..len).unwrap_or_default().to_vec();
            let end = at.saturating_add(len);
            match self.process_field(&text, field)? {
                Ok(ok) => valid &= ok,
                Err(message) => return Ok(Err(message)),
            }
            if end < line.len() {
                self.out.push(self.s.delimiter.unwrap_or(b' '));
                at = end.saturating_add(1);
            } else {
                break;
            }
        }
        if newline {
            self.out.push(self.s.line_delim);
        }
        Ok(Ok(valid))
    }
}

fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(run(), 1)
}

fn run() -> std::process::ExitCode {
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::stdfd::{self, Stream};
    use std::io::{BufRead, Write};
    use std::process::ExitCode;

    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let s = match parse_args(&args) {
        Ok(Request::Run(s)) => *s,
        Ok(Request::Help) => {
            let mut out = Stream::stdout();
            // Deliberately unread: `close_stdout` reports a failed write.
            let _ = out.write_all(help_text().as_bytes());
            return stdfd::close_stdout("numfmt", out, ExitCode::SUCCESS);
        }
        Ok(Request::Version) => {
            let mut out = Stream::stdout();
            // Deliberately unread, as above.
            let _ = out.write_all(b"numfmt (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("numfmt", out, ExitCode::SUCCESS);
        }
        Err(e) => {
            NUMFMT.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    if s.debug
        && s.scale_from == Scale::None
        && s.scale_to == Scale::None
        && !s.grouping
        && s.padding == 0
        && s.format.is_none()
    {
        diag!("numfmt: no conversion option specified");
    }
    let mut fmt = Format::default();
    let mut padding = s.padding;
    if let Some(text) = &s.format {
        match parse_format_string(text, s.debug, padding) {
            Ok((parsed, warnings)) => {
                for w in warnings {
                    diag!("numfmt: {w}");
                }
                if let Some(p) = parsed.padding {
                    padding = p;
                }
                fmt = parsed;
            }
            Err(message) => {
                diag!("numfmt: {message}");
                return ExitCode::FAILURE;
            }
        }
    }
    let grouping = s.grouping || fmt.grouping;
    if grouping {
        if s.scale_to != Scale::None {
            diag!("numfmt: grouping cannot be combined with --to");
            return ExitCode::FAILURE;
        }
        if s.debug {
            // The C and POSIX locales -- the only ones here -- have no
            // thousands separator.
            diag!("numfmt: grouping has no effect in this locale");
        }
    }

    let mut r = Run {
        s: &s,
        scale_from: s.scale_from,
        padding_width: usize::try_from(padding.unsigned_abs()).unwrap_or(usize::MAX),
        padding_left: padding < 0,
        auto_padding: padding == 0 && s.delimiter.is_none(),
        fmt,
        out: Vec::new(),
        diags: Vec::new(),
    };
    let mut out = Stream::stdout();
    let mut valid = true;

    // What one call left: its diagnostics, and its output so far.
    let flush = |r: &mut Run, out: &mut Stream| {
        for message in r.diags.drain(..) {
            diag!("numfmt: {message}");
        }
        // Deliberately unread: `Stream` records a failed write, and the
        // `close_stdout` every exit path ends in reports it.
        let _ = out.write_all(&r.out);
        r.out.clear();
    };

    let outcome: Result<Result<(), String>, Abort> = (|| {
        if s.operands.is_empty() {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let mut line = Vec::new();
            let mut header = s.header;
            while header > 0 {
                header = header.saturating_sub(1);
                line.clear();
                match input.read_until(s.line_delim, &mut line) {
                    Ok(0) => break,
                    // `fputs`: the line up to its first NUL.
                    Ok(_) => r
                        .out
                        .extend_from_slice(line.split(|&b| b == 0).next().unwrap_or_default()),
                    Err(e) => return Ok(Err(format!("error reading input: {}", strerror(&e)))),
                }
            }
            loop {
                line.clear();
                match input.read_until(s.line_delim, &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let newline = line.last() == Some(&s.line_delim);
                        if newline {
                            line.pop();
                        }
                        match r.process_line(&line, newline)? {
                            Ok(ok) => valid &= ok,
                            Err(message) => return Ok(Err(message)),
                        }
                        flush(&mut r, &mut out);
                    }
                    Err(e) => return Ok(Err(format!("error reading input: {}", strerror(&e)))),
                }
            }
        } else {
            if s.debug && s.header > 0 {
                r.diags
                    .push("--header ignored with command-line input".to_string());
            }
            for operand in &s.operands {
                match r.process_line(&os_bytes(operand), true)? {
                    Ok(ok) => valid &= ok,
                    Err(message) => return Ok(Err(message)),
                }
                flush(&mut r, &mut out);
            }
        }
        Ok(Ok(()))
    })();

    flush(&mut r, &mut out);
    match outcome {
        Err(Abort) => {
            return stdfd::close_stdout("numfmt", out, ExitCode::from(EXIT_CONVERSION_WARNINGS));
        }
        Ok(Err(message)) => {
            diag!("numfmt: {message}");
            return stdfd::close_stdout("numfmt", out, ExitCode::FAILURE);
        }
        Ok(Ok(())) => {}
    }
    if s.debug && !valid {
        diag!("numfmt: failed to convert some of the input numbers");
    }
    let status = if !valid && matches!(s.inval, Invalid::Abort | Invalid::Fail) {
        ExitCode::from(EXIT_CONVERSION_WARNINGS)
    } else {
        ExitCode::SUCCESS
    };
    stdfd::close_stdout("numfmt", out, status)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn settings(args: &[&str]) -> Settings {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse_args(&args).unwrap() {
            Request::Run(s) => *s,
            other => panic!("{other:?}"),
        }
    }

    /// Output, diagnostics, and how the line ended early if it did: `abort`
    /// for `--invalid=abort`, or the message of a fatal error.
    fn convert(args: &[&str], input: &str) -> (String, Vec<String>, Option<String>) {
        let s = settings(args);
        let mut fmt = Format::default();
        let mut padding = s.padding;
        if let Some(text) = &s.format {
            let (parsed, _) = parse_format_string(text, false, padding).unwrap();
            if let Some(p) = parsed.padding {
                padding = p;
            }
            fmt = parsed;
        }
        let mut r = Run {
            s: &s,
            scale_from: s.scale_from,
            padding_width: usize::try_from(padding.unsigned_abs()).unwrap(),
            padding_left: padding < 0,
            auto_padding: padding == 0 && s.delimiter.is_none(),
            fmt,
            out: Vec::new(),
            diags: Vec::new(),
        };
        let aborted = match r.process_line(input.as_bytes(), true) {
            Err(Abort) => Some("abort".to_string()),
            Ok(Err(message)) => Some(message),
            Ok(Ok(_)) => None,
        };
        (String::from_utf8(r.out).unwrap(), r.diags, aborted)
    }

    #[test]
    fn scaling_to_si_and_iec() {
        assert_eq!(convert(&["--to=si"], "1000").0, "1.0K\n");
        assert_eq!(convert(&["--to=si"], "999999").0, "1.0M\n");
        assert_eq!(convert(&["--to=iec"], "2048").0, "2.0K\n");
        assert_eq!(convert(&["--to=iec-i"], "4096").0, "4.0Ki\n");
        assert_eq!(convert(&["--to=si"], "123456").0, "124K\n");
        assert_eq!(convert(&["--to=si", "--round=down"], "123456").0, "123K\n");
        assert_eq!(convert(&["--to=si"], "1").0, "1\n");
    }

    #[test]
    fn scaling_from_suffixes() {
        assert_eq!(convert(&["--from=si"], "1K").0, "1000\n");
        assert_eq!(convert(&["--from=iec"], "1K").0, "1024\n");
        assert_eq!(convert(&["--from=auto"], "1Ki").0, "1024\n");
        assert_eq!(convert(&["--from=auto"], "1.5M").0, "1500000\n");
        assert_eq!(convert(&["--from=iec-i"], "2Mi").0, "2097152\n");
    }

    #[test]
    fn invalid_numbers_abort_by_default() {
        let (out, diags, aborted) = convert(&[], "5.");
        assert_eq!((out.as_str(), aborted.as_deref()), ("", Some("abort")));
        assert_eq!(diags, vec!["invalid number: ‘5.’"]);
        let (_, diags, _) = convert(&[], "5..");
        assert_eq!(diags, vec!["invalid suffix in input: ‘5..’"]);
        let (_, diags, _) = convert(&[], "1K");
        assert_eq!(
            diags,
            vec!["rejecting suffix in input: ‘1K’ (consider using --from)"]
        );
    }

    #[test]
    fn warn_prints_the_field_as_it_was() {
        let (out, diags, aborted) = convert(&["--invalid=warn"], "x");
        assert_eq!((out.as_str(), aborted.as_deref()), ("x\n", None));
        assert_eq!(diags, vec!["invalid number: ‘x’"]);
        let (out, diags, _) = convert(&["--invalid=ignore"], "x");
        assert_eq!(out, "x\n");
        assert!(diags.is_empty());
    }

    /// Upstream's four messages that show a number between ASCII apostrophes,
    /// exactly -- not `quote()`'s curly quotes, which is what a hand-written
    /// `'{}'` is usually a bug for.
    #[test]
    fn numbers_in_messages_are_between_ascii_apostrophes() {
        let (_, diags, aborted) = convert(&[], "12345678901234567890");
        assert_eq!(aborted.as_deref(), Some("abort"));
        assert_eq!(
            diags,
            vec!["value too large to be printed: '1.23457e+19' (consider using --to)"]
        );
        let (_, diags, _) = convert(&["--format=%.20f"], "1");
        assert_eq!(
            diags,
            vec!["value/precision too large to be printed: '1/20' (consider using --to)"]
        );
        // Past 999Q only by scaling: a number *read* that long is refused
        // before it is printed, as "value too large to be converted". And only
        // with --to, or the check above ("consider using --to") speaks first.
        let (_, diags, _) = convert(&["--from=si", "--to=si"], "1000Q");
        assert_eq!(
            diags,
            vec!["value too large to be printed: '1e+33' (cannot handle values > 999Q)"]
        );
        let (out, _, aborted) = convert(&["--format=%0200f"], "1");
        assert_eq!(out, "");
        assert_eq!(
            aborted.as_deref(),
            Some("failed to prepare value '1.000000' for printing")
        );
    }

    #[test]
    fn fields_and_auto_padding() {
        assert_eq!(
            convert(&["--field=2", "--to=si"], "a 2000 b").0,
            "a 2.0K b\n"
        );
        // A field with blanks before it keeps its width.
        assert_eq!(convert(&["--to=si"], "   2000").0, "   2.0K\n");
        assert_eq!(
            convert(&["--field=-", "--to=si"], "1000 2000").0,
            "1.0K 2.0K\n"
        );
        assert_eq!(
            convert(&["-d:", "--field=2", "--to=si"], "a:2000:b").0,
            "a:2.0K:b\n"
        );
    }

    #[test]
    fn format_strings() {
        assert_eq!(convert(&["--format=%.2f"], "5").0, "5.00\n");
        assert_eq!(convert(&["--format=%08.2f"], "5").0, "00005.00\n");
        assert_eq!(convert(&["--format=%-6f|"], "5").0, "5     |\n");
        assert_eq!(convert(&["--format=a%%b%f"], "5").0, "a%%5\n");
        assert_eq!(convert(&["--format=%f%%"], "5").0, "5%%\n");
    }

    #[test]
    fn format_string_refusals() {
        let e = |f: &str| parse_format_string(f.as_bytes(), false, 0).unwrap_err();
        assert_eq!(e("abc"), "format ‘abc’ has no % directive");
        assert_eq!(e("%"), "format ‘%’ ends in %");
        assert_eq!(
            e("%d"),
            "invalid format ‘%d’, directive must be %[0]['][-][N][.][N]f"
        );
        assert_eq!(e("%f%f"), "format ‘%f%f’ has too many % directives");
        assert_eq!(e("%.-1f"), "invalid precision in format ‘%.-1f’");
    }

    #[test]
    fn unit_sizes() {
        assert_eq!(unit_to_umax(b"1").unwrap(), 1);
        assert_eq!(unit_to_umax(b"K").unwrap(), 1000);
        assert_eq!(unit_to_umax(b"Ki").unwrap(), 1024);
        assert_eq!(unit_to_umax(b"2M").unwrap(), 2_000_000);
        assert!(unit_to_umax(b"KiB").is_err());
        assert!(unit_to_umax(b"0").is_err());
        assert!(unit_to_umax(b"x").is_err());
    }

    #[test]
    fn rounding_directions() {
        for (how, v, want) in [
            (RoundType::Ceiling, "1.2", 2),
            (RoundType::Floor, "1.8", 1),
            (RoundType::FromZero, "-1.2", -2),
            (RoundType::ToZero, "-1.8", -1),
            (RoundType::Nearest, "1.5", 2),
            (RoundType::Nearest, "-1.5", -2),
        ] {
            let (_, val, _, _) = strtod_float(v.as_bytes());
            assert_eq!(simple_round(val, how).to_i64_trunc(), want, "{how:?} {v}");
        }
    }
}
