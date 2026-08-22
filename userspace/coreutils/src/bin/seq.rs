//! seq — print numbers from FIRST to LAST, in steps of INCREMENT.
//!
//! # What this used to be
//!
//! The shipped `seq` had no option parser: `-w`, `-f`, `-s`, `--help` and
//! `--version` were all read as operands, so `seq --help` answered `invalid
//! number: '--help'`. (Straight marks on purpose: that is what the code of
//! the day printed. Everything current in this file is curly, per §351.)
//! Past the command line it was `f64` throughout and disagreed with GNU in
//! ways that changed *output*, not just diagnostics:
//!
//! - The stopping test was `val <= last + f64::EPSILON`, on a value
//!   **accumulated** by repeated `val += increment`. Both halves are wrong.
//!   GNU computes `first + i * step` from scratch each time, so the error does
//!   not compound; and the epsilon is an absolute fudge on a relative quantity,
//!   so `seq 1e15 1 1e15+3` overshot while `seq 0 0.1 1e-300` did not reach.
//! - Numbers were printed by Rust's `Display`, which writes the shortest
//!   round-tripping decimal. GNU prints a `printf` conversion whose precision
//!   is taken from *how the operands were spelled*: `seq 1 0.5 3` is
//!   `1.0 1.5 2.0 2.5 3.0` there and `1 1.5 2 2.5 3` here.
//! - `-w` did not exist, so neither did the width arithmetic that makes
//!   `seq -w -1 1 3` print `-1 00 01 02 03` — a column width taken from the
//!   *widest* operand as written, with the sign counted in it.
//! - Arithmetic was `f64`. GNU's is `long double`, which on x86 is the 80-bit
//!   x87 format with a 64-bit significand, and the difference is visible:
//!   `seq 145.0612310077283783 1 145.0612310077283783` prints the operand back
//!   in 80-bit and prints `145.0612310077283786` in `f64`.
//!
//! # The arithmetic is 80-bit, in software
//!
//! [`coreutils::extfloat`] is that 80-bit float, certified against glibc's
//! `strtold` and `printf` over 12,278 cases with no exceptions. `seq` is its
//! first caller and it exists for `seq`'s sake: the claim `seq` needs is that an
//! operand reads as the value GNU read from it, and that a value prints as the
//! bytes GNU printed. Neither claim can be made by an `f64`, and neither can be
//! made by hardware `long double` from Rust, which has no such type.
//!
//! # Two integer fast paths, and why both are here
//!
//! When every operand is a plain run of decimal digits, GNU never converts to a
//! float at all: it counts in decimal *strings*, which is both faster and
//! exact past `2^64`. `seq 99999999999999999999 100000000000000000002` prints
//! three numbers that no float could tell apart. The path is entered twice —
//! once on the operands as typed, and once after conversion on operands like
//! `1e3` that are integers without looking like it — and the second entry is
//! also what makes `seq 1 inf` a counter rather than a rounding disaster.
//!
//! Both entries require a single-byte separator, no `-w`, and no `-f`, because
//! the fast path writes the digits itself rather than going through a
//! conversion. A step outside `(0, 200]` also disqualifies it: upstream
//! measured 200 as the point past which repeated decimal increment stops paying
//! for itself, and the limit is quoted in the manual, so it is observable.
//!
//! # The number past the end is sometimes printed
//!
//! `seq 0 0.000001 0.000003` would stop at `0.000002`: three ulps of rounding
//! in `first + 3 * step` put it a hair above `last`. GNU notices, and prints the
//! extra number when it *formats* to something that reads back as exactly `last`
//! and differs from what was just printed. That is transcribed here, re-parse
//! and all, because the alternative — comparing with a tolerance — changes which
//! numbers appear for inputs that have nothing to do with rounding.
//!
//! # Options stop at the first operand
//!
//! The optstring is `+f:s:w`, so `seq 1 --version` prints no version: `1` ends
//! option parsing and `--version` becomes an operand, which is then refused as
//! `invalid floating point argument: ‘--version’`. On top of that, each argument
//! is checked *before* getopt sees it for `-` followed by `.` or a digit, so
//! `seq -3 -1 -5` is a descending sequence rather than three bad options.
//!
//! # Checked against GNU
//!
//! `scripts/seq-diff.sh` runs both binaries over the same command lines and
//! compares stdout, stderr and the exit status separately.

use coreutils::errmsg::strerror;
use coreutils::extfloat::{self, ExtF80, Spec};
use coreutils::getopt::{self, Program};
use coreutils::quote::quote;
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

const SEQ: Program = Program::new("seq", 1);

const USAGE: &str = "usage: seq [OPTION]... LAST\n   \
                     or: seq [OPTION]... FIRST LAST\n   \
                     or: seq [OPTION]... FIRST INCREMENT LAST";

/// The long options, in GNU's declaration order — which is observable, because
/// `getopt_long` lists an ambiguous prefix's candidates in it. Measured with
/// `seq --=x`, whose empty prefix matches every entry:
///
/// ```text
/// seq: option '--=x' is ambiguous; possibilities: '--equal-width' '--format' '--separator' '--help' '--version'
/// ```
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("equal-width", Long::EqualWidth),
    ("format", Long::Format),
    ("separator", Long::Separator),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    EqualWidth,
    Format,
    Separator,
    Help,
    Version,
}

/// Upstream's `SEQ_FAST_STEP_LIMIT`: the largest step the decimal-string path
/// will take. Quoted in the texinfo manual, so it is part of the interface.
const FAST_STEP_LIMIT: u32 = 200;

/// What the options said.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Settings {
    equal_width: bool,
    format: Option<Vec<u8>>,
    separator: Vec<u8>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            equal_width: false,
            format: None,
            separator: b"\n".to_vec(),
        }
    }
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Run(Settings, Vec<Vec<u8>>),
    Help,
    Version,
}

/// A failure that ends the run.
#[derive(Debug)]
enum Trouble {
    /// A command line that will not run. Carries its own status and whether the
    /// `Try 'seq --help'` referral follows, because upstream has both shapes:
    /// the operand diagnostics call `error (0, 0, …)` and then `usage`, while
    /// the four format diagnostics call `error (EXIT_FAILURE, 0, …)` and print
    /// one line.
    Refused(getopt::Error),
    Write(io::Error),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Refused(e) => {
                eprintln!("seq: {}", e.message());
                ExitCode::from(u8::try_from(e.status).unwrap_or(1))
            }
            Self::Write(e) => {
                eprintln!("seq: write error: {}", strerror(e));
                ExitCode::FAILURE
            }
        }
    }
}

impl From<getopt::Error> for Trouble {
    fn from(e: getopt::Error) -> Self {
        Trouble::Refused(e)
    }
}

impl From<io::Error> for Trouble {
    fn from(e: io::Error) -> Self {
        Trouble::Write(e)
    }
}

// ------------------------------------------------------------------ operands

/// The sentinel upstream uses for "this number cannot be written as a
/// fixed-point decimal", which is `INT_MAX` in an `int` field.
///
/// It is a sentinel rather than an `Option` because it is *compared* — `prec =
/// MAX (first.precision, step.precision)` — and the comparison relies on it
/// being the largest value, so that one unrepresentable operand poisons the
/// whole default format into `%Lg`.
const NO_PRECISION: i32 = i32::MAX;

/// A command-line operand: its value, and how it was spelled.
#[derive(Clone, Copy, Debug)]
struct Operand {
    value: ExtF80,
    /// The width it would occupy if printed in a form similar to its input
    /// form. `-.1` counts as `-0.1` and `1.` counts as `1`.
    width: usize,
    /// Digits after the decimal point, or [`NO_PRECISION`].
    precision: i32,
}

impl Operand {
    /// The `{1, 1, 0}` that upstream gives `first` and `step` before it knows
    /// whether the command line supplied them.
    fn one() -> Self {
        Operand {
            value: ExtF80::ONE,
            width: 1,
            precision: 0,
        }
    }
}

// ---------------------------------------------------------------------- main

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(trouble) => trouble.report(),
    }
}

fn run(args: &[OsString]) -> Result<ExitCode, Trouble> {
    let (settings, operands) = match parse_args(args)? {
        Request::Help => {
            println!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        }
        Request::Version => {
            println!("seq (SlateOS coreutils)");
            return Ok(ExitCode::SUCCESS);
        }
        Request::Run(settings, operands) => (settings, operands),
    };

    // The operand count is checked before the format is looked at, which is why
    // `seq -f %` is `missing operand` rather than `format '%' ends in %`.
    let n = operands.len();
    if n < 1 {
        return Err(SEQ.usage_referring("missing operand".to_string()).into());
    }
    if n > 3 {
        // `quote (argv[optind + 3])` — the *fourth* operand is the one named,
        // not the last.
        let extra = operands.get(3).map_or(&b""[..], Vec::as_slice);
        return Err(SEQ
            .usage_referring(format!("extra operand {}", quote(extra)))
            .into());
    }

    let format = match &settings.format {
        Some(fmt) => Some(long_double_format(fmt)?),
        None => None,
    };
    if format.is_some() && settings.equal_width {
        return Err(SEQ
            .usage_referring(
                "format string may not be specified when printing equal width strings".to_string(),
            )
            .into());
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if try_fast_from_strings(&mut out, &settings, &operands)? {
        out.flush()?;
        return Ok(ExitCode::SUCCESS);
    }

    // The scan order is the operands' own, and the zero-increment check sits
    // between the second and the third: `seq 1 0 x` names the increment, not
    // the unreadable last operand.
    let mut first = Operand::one();
    let mut step = Operand::one();
    let mut last = scan_arg(&operands[0])?;
    if n > 1 {
        first = last;
        last = scan_arg(&operands[1])?;
        if n > 2 {
            step = last;
            if step.value.is_zero() {
                return Err(SEQ
                    .usage_referring(format!(
                        "invalid Zero increment value: {}",
                        quote(&operands[1])
                    ))
                    .into());
            }
            last = scan_arg(&operands[2])?;
        }
    }

    if try_fast_from_values(&mut out, &settings, first, step, last)? {
        out.flush()?;
        return Ok(ExitCode::SUCCESS);
    }

    let format = match format {
        Some(format) => format,
        None => default_format(&settings, first, step, last),
    };
    print_numbers(&mut out, &format, &settings.separator, first, step, last)?;
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------ option parsing

fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut settings = Settings::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut at = 0usize;

    // The option loop, which ends at the first thing that is not an option —
    // that is what the `+` at the head of `"+f:s:w"` buys, and it is why
    // `seq 1 --version` prints no version.
    while let Some(arg) = args.get(at) {
        let bytes = arg_bytes(arg);

        // Checked *before* getopt sees the argument: a `-` followed by `.` or a
        // digit is a negative number, not a cluster of options.
        if bytes.first() == Some(&b'-') && matches!(bytes.get(1), Some(&b'.') | Some(b'0'..=b'9')) {
            break;
        }
        if bytes == b"--" {
            at = at.saturating_add(1);
            break;
        }
        if bytes == b"-" || bytes.first() != Some(&b'-') {
            break;
        }
        at = at.saturating_add(1);
        if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, args, &mut at, &mut settings)? {
                return Ok(request);
            }
        } else if let Some(request) = short_options(&bytes, args, &mut at, &mut settings)? {
            return Ok(request);
        }
    }

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        operands.push(arg_bytes(arg));
    }
    Ok(Request::Run(settings, operands))
}

/// One `--name`, `--name=value` or `--name value` argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, getopt::Error> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed = std::str::from_utf8(typed).map_err(|_| SEQ.unrecognized_option(whole))?;
    let (name, which) = SEQ.resolve_long(typed, whole, LONG_OPTIONS)?;

    match which {
        Long::Format | Long::Separator => {
            let value = match inline {
                Some(value) => value.to_vec(),
                None => {
                    let Some(separate) = args.get(*next) else {
                        return Err(SEQ.long_missing_argument(name));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                }
            };
            if which == Long::Format {
                settings.format = Some(value);
            } else {
                settings.separator = value;
            }
        }
        Long::EqualWidth | Long::Help | Long::Version => {
            if inline.is_some() {
                return Err(SEQ.long_unwanted_argument(name));
            }
            match which {
                Long::EqualWidth => settings.equal_width = true,
                Long::Help => return Ok(Some(Request::Help)),
                Long::Version => return Ok(Some(Request::Version)),
                Long::Format | Long::Separator => {}
            }
        }
    }
    Ok(None)
}

/// One `-fsw` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, getopt::Error> {
    let cluster = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = cluster.get(at) {
        match c {
            b'w' => settings.equal_width = true,
            b'f' | b's' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument. Taking the
                // whole next argument is why `seq -s -1 3` separates with the
                // two bytes `-1` rather than counting down.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(SEQ.short_missing_argument(c));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                if c == b'f' {
                    settings.format = Some(value);
                } else {
                    settings.separator = value;
                }
                return Ok(None);
            }
            _ => return Err(SEQ.invalid_option(c)),
        }
        at = at.saturating_add(1);
    }
    Ok(None)
}

// ------------------------------------------------------------ reading numbers

/// Read one operand, and measure how it was written.
///
/// The measurement is the whole reason this is not just a call to `xstrtold`:
/// the default output format's precision and `-w`'s column width are both taken
/// from the *spelling* of the operands, not from their values, so `seq 1.0 2.0`
/// prints `1.0 2.0` while `seq 1 2` prints `1 2`.
fn scan_arg(arg: &[u8]) -> Result<Operand, getopt::Error> {
    let Some(value) = extfloat::xstrtold(arg) else {
        return Err(SEQ.usage_referring(format!("invalid floating point argument: {}", quote(arg))));
    };
    if value.is_nan() {
        return Err(SEQ.usage_referring(format!(
            "invalid {} argument: {}",
            quote(b"not-a-number"),
            quote(arg)
        )));
    }

    // Spaces and `+` are consumed by the conversion but never printed, so they
    // are not part of the width either.
    let mut s = arg;
    while matches!(s.first(), Some(&c) if is_c_space(c) || c == b'+') {
        s = s.get(1..).unwrap_or_default();
    }

    let mut width = 0usize;
    let mut precision = NO_PRECISION;

    let decimal_point = s.iter().position(|&c| c == b'.');
    // `strchr (arg, 'p')` — lowercase only, which is upstream's test for a hex
    // float and misses `0X1P+0`. That operand therefore gets precision 0 and,
    // because it still contains an `X`, no width at all.
    if decimal_point.is_none() && !s.contains(&b'p') {
        precision = 0;
    }

    // Widths are measured for decimal spellings only. A hex float's printed
    // form has no relation to how it was typed, so upstream leaves the width at
    // 0 and lets `%Lf`'s natural width stand.
    if !s.iter().any(|&c| c == b'x' || c == b'X') && value.is_finite() {
        let mut fraction_len = 0usize;
        width = s.len();

        if let Some(dp) = decimal_point {
            let after = s.get(dp.saturating_add(1)..).unwrap_or_default();
            fraction_len = after
                .iter()
                .position(|&c| c == b'e' || c == b'E')
                .unwrap_or(after.len());
            if let Ok(p) = i32::try_from(fraction_len) {
                precision = p;
            }
            // `#.` prints as `#`, so it is one byte narrower; `.#` and `-.#`
            // print as `0.#` and `-0.#`, so they are one byte wider.
            let previous = dp.checked_sub(1).and_then(|i| s.get(i).copied());
            if fraction_len == 0 {
                width = width.wrapping_sub(1);
            } else if !previous.is_some_and(|c| c.is_ascii_digit()) {
                width = width.wrapping_add(1);
            }
        }

        // A lowercase `e` anywhere beats an uppercase one earlier in the
        // string, because upstream looks for the two in that order.
        let e = s
            .iter()
            .position(|&c| c == b'e')
            .or_else(|| s.iter().position(|&c| c == b'E'));
        if let Some(e) = e {
            let exponent = strtol(s.get(e.saturating_add(1)..).unwrap_or_default());
            let mut exponent = exponent.max(-i64::MAX);
            precision = if exponent < 0 {
                saturating_add_i64(precision, exponent.saturating_neg())
            } else {
                saturating_add_i64(precision, -i64::from(precision).min(exponent))
            };
            // The `e...` is read but never written, so it leaves the width.
            width = width.wrapping_sub(s.len().wrapping_sub(e));
            if exponent < 0 {
                match decimal_point {
                    // `1.e5` lost a byte to the `#.` rule above, but the
                    // fraction is coming back as leading zeros, so give it back.
                    Some(dp) => {
                        if e == dp.saturating_add(1) {
                            width = width.wrapping_add(1);
                        }
                    }
                    // `1e-5` prints as `0.00001`: a radix point appears where
                    // the spelling had none.
                    None => width = width.wrapping_add(1),
                }
                exponent = exponent.saturating_neg();
            } else {
                // `1.5e1` prints as `15`, and the radix point goes away.
                if decimal_point.is_some() && precision == 0 && fraction_len != 0 {
                    width = width.wrapping_sub(1);
                }
                exponent = exponent.saturating_sub(
                    i64::try_from(fraction_len)
                        .unwrap_or(i64::MAX)
                        .min(exponent),
                );
            }
            width = width.wrapping_add(usize::try_from(exponent).unwrap_or(usize::MAX));
        }
    }

    Ok(Operand {
        value,
        width,
        precision,
    })
}

/// `isspace` in the C locale, which includes the vertical tab that
/// `u8::is_ascii_whitespace` leaves out.
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `strtol (s, nullptr, 10)`, saturating at the ends of the range as it does.
///
/// The string has already been through `strtold`, so it is a sign and digits;
/// this only has to agree about how many digits are too many.
fn strtol(s: &[u8]) -> i64 {
    let (neg, digits) = match s.first() {
        Some(&b'-') => (true, s.get(1..).unwrap_or_default()),
        Some(&b'+') => (false, s.get(1..).unwrap_or_default()),
        _ => (false, s),
    };
    let mut magnitude: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            break;
        }
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(i64::from(c - b'0'));
    }
    if neg { -magnitude } else { magnitude }
}

/// `int += long`, saturating rather than wrapping.
///
/// Upstream this is a plain `+=` onto an `int`, which for an exponent past two
/// billion is signed overflow — undefined, and unreproducible by construction.
/// Saturating lands on [`NO_PRECISION`], which sends the default format to
/// `%Lg`; that is the same answer upstream gives for every operand it cannot
/// write as a fixed-point decimal, which is what an operand with a
/// two-billion-digit expansion is.
fn saturating_add_i64(base: i32, delta: i64) -> i32 {
    i64::from(base)
        .saturating_add(delta)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

// ---------------------------------------------------------- the print format

/// A `-f FORMAT`, split into the bytes before the directive, the directive, and
/// the bytes after it.
#[derive(Clone, Debug)]
struct Format {
    /// Already decoded: a `%%` in the format is one `%` here, which is what
    /// makes this length upstream's `layout.prefix_len` — a count of *output*
    /// bytes, used to find where the number starts in a formatted line.
    prefix: Vec<u8>,
    spec: Spec,
    suffix: Vec<u8>,
}

impl Format {
    /// The bare conversion the default format is, with nothing around it.
    fn bare(spec: Spec) -> Self {
        Format {
            prefix: Vec::new(),
            spec,
            suffix: Vec::new(),
        }
    }

    fn render(&self, v: ExtF80) -> Vec<u8> {
        let mut out = self.prefix.clone();
        out.extend_from_slice(extfloat::render(&self.spec, v).as_bytes());
        out.extend_from_slice(&self.suffix);
        out
    }
}

/// Validate `-f`'s argument and split it up, with upstream's four diagnostics.
///
/// All four are `error (EXIT_FAILURE, 0, …)`, so they print one line and carry
/// no `Try 'seq --help'` referral — unlike every other refusal in this file.
fn long_double_format(fmt: &[u8]) -> Result<Format, getopt::Error> {
    let mut prefix: Vec<u8> = Vec::new();
    let mut i = 0usize;
    loop {
        let c = fmt.get(i).copied();
        if c == Some(b'%') && fmt.get(i.saturating_add(1)) != Some(&b'%') {
            break;
        }
        let Some(c) = c else {
            return Err(SEQ.usage(format!("format {} has no % directive", quote(fmt))));
        };
        // A `%%` contributes one `%` and consumes two bytes, which is why the
        // prefix's length is the length of the text printed rather than of the
        // format.
        prefix.push(c);
        i = i.saturating_add(if c == b'%' { 2 } else { 1 });
    }

    let start = i;
    i = i.saturating_add(1);
    while matches!(fmt.get(i), Some(c) if b"-+#0 '".contains(c)) {
        i = i.saturating_add(1);
    }
    while matches!(fmt.get(i), Some(c) if c.is_ascii_digit()) {
        i = i.saturating_add(1);
    }
    if fmt.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while matches!(fmt.get(i), Some(c) if c.is_ascii_digit()) {
            i = i.saturating_add(1);
        }
    }
    if fmt.get(i) == Some(&b'L') {
        i = i.saturating_add(1);
    }
    let Some(&conv) = fmt.get(i) else {
        return Err(SEQ.usage(format!("format {} ends in %", quote(fmt))));
    };
    if !b"efgaEFGA".contains(&conv) {
        return Err(SEQ.usage(format!(
            "format {} has unknown %{} directive",
            quote(fmt),
            directive_char(conv)
        )));
    }
    i = i.saturating_add(1);
    let Some((spec, used)) = Spec::parse(fmt.get(start..i).unwrap_or_default()) else {
        // Unreachable: the scan above accepts exactly what `Spec::parse` does.
        return Err(SEQ.usage(format!(
            "format {} has unknown %{} directive",
            quote(fmt),
            directive_char(conv)
        )));
    };
    debug_assert_eq!(used, i.saturating_sub(start));

    let mut suffix: Vec<u8> = Vec::new();
    loop {
        let c = fmt.get(i).copied();
        if c == Some(b'%') && fmt.get(i.saturating_add(1)) != Some(&b'%') {
            return Err(SEQ.usage(format!("format {} has too many % directives", quote(fmt))));
        }
        let Some(c) = c else { break };
        suffix.push(c);
        i = i.saturating_add(if c == b'%' { 2 } else { 1 });
    }

    Ok(Format {
        prefix,
        spec,
        suffix,
    })
}

/// The unknown directive's own character, as GNU's `%c` would write it —
/// except that a byte which is not printable is escaped instead.
///
/// glibc writes the raw byte, so `seq -f '%<LF>'` makes GNU print a diagnostic
/// with a newline in the middle of it, and a second line that `seq` never
/// wrote. This is the same argument, and the same fix, as
/// [`coreutils::getopt`]'s: for every directive a person would actually type
/// the two are byte-identical.
fn directive_char(c: u8) -> String {
    if (0x20..0x7f).contains(&c) {
        (c as char).to_string()
    } else {
        format!("\\{:03o}", c)
    }
}

/// The format to use when `-f` gave none.
///
/// Two decisions, in this order: whether the operands can be written as
/// fixed-point decimals at all — one that cannot sends everything to `%Lg` —
/// and then, under `-w`, how wide the column has to be.
fn default_format(settings: &Settings, first: Operand, step: Operand, last: Operand) -> Format {
    let prec = first.precision.max(step.precision);

    if prec != NO_PRECISION && last.precision != NO_PRECISION {
        if settings.equal_width {
            // Every width below is measured from the operand *as written*, then
            // corrected for the difference between how it was written and how
            // it is about to be printed.
            let widen = |width: usize, precision: i32| {
                width.wrapping_add_signed(isize::try_from(prec - precision).unwrap_or(0))
            };
            let mut first_width = widen(first.width, first.precision);
            let mut last_width = widen(last.width, last.precision);
            if last.precision != 0 && prec == 0 {
                last_width = last_width.wrapping_sub(1); // no room for a `.`
            }
            if last.precision == 0 && prec != 0 {
                last_width = last_width.wrapping_add(1); // room for a `.`
            }
            if first.precision == 0 && prec != 0 {
                first_width = first_width.wrapping_add(1);
            }
            let width = first_width.max(last_width);
            if let Ok(w) = i32::try_from(width) {
                return Format::bare(Spec::zero_padded(
                    usize::try_from(w).unwrap_or(0),
                    usize::try_from(prec).unwrap_or(0),
                ));
            }
        } else {
            return Format::bare(Spec::fixed(usize::try_from(prec).unwrap_or(0)));
        }
    }

    Format::bare(Spec::general())
}

// -------------------------------------------------------------- the sequence

fn print_numbers(
    out: &mut impl Write,
    format: &Format,
    separator: &[u8],
    first: Operand,
    step: Operand,
    last: Operand,
) -> Result<(), Trouble> {
    let descending = step.value.lt(ExtF80::ZERO);
    let past = |x: ExtF80| {
        if descending {
            x.lt(last.value)
        } else {
            last.value.lt(x)
        }
    };

    let mut out_of_range = past(first.value);
    if out_of_range {
        return Ok(());
    }

    let mut x = first.value;
    // The index is a float, and the value is `first + i * step` computed afresh
    // every time rather than accumulated: repeated addition compounds its
    // rounding error, and `seq 1 0.1 2` would drift off the tenths.
    let mut i = ExtF80::ONE;

    loop {
        let x0 = x;
        out.write_all(&format.render(x))?;
        if out_of_range {
            break;
        }
        x = first.value + i * step.value;
        i = i + ExtF80::ONE;
        out_of_range = past(x);

        if out_of_range && !prints_as_last(format, x, x0, last.value) {
            break;
        }
        out.write_all(separator)?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

/// Whether the number just past the end should be printed anyway.
///
/// `seq 0 0.000001 0.000003` is the case this exists for: `first + 3 * step` is
/// a few ulps above `last`, so the loop would stop one number early. It is
/// printed when it *formats* to text that reads back as exactly `last` — and
/// when that text differs from the previous number's, so a sequence that has
/// already printed the value does not print it twice.
///
/// The comparison is between the formatted strings with the format's suffix
/// removed, and the value is re-read from past the format's prefix, which is
/// how a `-f` with text around the number still works.
fn prints_as_last(format: &Format, x: ExtF80, x0: ExtF80, last: ExtF80) -> bool {
    let rendered = format.render(x);
    let body = rendered
        .get(..rendered.len().saturating_sub(format.suffix.len()))
        .unwrap_or_default();
    let number = body.get(format.prefix.len()..).unwrap_or_default();
    let Some(read_back) = extfloat::xstrtold(number) else {
        return false;
    };
    if !read_back.eq_value(last) {
        return false;
    }
    let previous = format.render(x0);
    let previous = previous
        .get(..previous.len().saturating_sub(format.suffix.len()))
        .unwrap_or_default();
    previous != body
}

// ------------------------------------------------------------ the fast paths

/// The first entry: every operand is a run of decimal digits, so no conversion
/// is needed at all and the count is exact past what any float could hold.
fn try_fast_from_strings(
    out: &mut impl Write,
    settings: &Settings,
    operands: &[Vec<u8>],
) -> Result<bool, Trouble> {
    let n = operands.len();
    let mut fast_step: u64 = 1;
    if n == 3 {
        // A step that is not a small positive integer disqualifies the path.
        // Note that this reads the *second* operand, which is the increment.
        let Some(value) = operands
            .get(1)
            .filter(|s| all_digits_p(s))
            .and_then(|s| extfloat::xstrtold(s))
        else {
            return Ok(false);
        };
        let Some(small) = small_positive_step(value) else {
            return Ok(false);
        };
        fast_step = small;
    }

    let digits = |i: usize| operands.get(i).is_some_and(|s| all_digits_p(s));
    if !(digits(0)
        && (n == 1 || digits(1))
        && (n < 3 || digits(2))
        && !settings.equal_width
        && settings.format.is_none()
        && settings.separator.len() == 1)
    {
        return Ok(false);
    }

    let one = b"1".to_vec();
    let from = if n == 1 { &one } else { &operands[0] };
    let to = &operands[n.saturating_sub(1)];
    Ok(seq_fast(out, from, to, fast_step, settings.separator[0])?)
}

/// The second entry, after conversion: operands like `1e3` are integers that do
/// not look like it, and `inf` as the last operand turns this into the counter
/// that `seq 1 inf` is expected to be.
fn try_fast_from_values(
    out: &mut impl Write,
    settings: &Settings,
    first: Operand,
    step: Operand,
    last: Operand,
) -> Result<bool, Trouble> {
    if !(first.precision == 0
        && step.precision == 0
        && last.precision == 0
        && first.value.is_finite()
        && !first.value.lt(ExtF80::ZERO)
        && !last.value.lt(ExtF80::ZERO)
        && !settings.equal_width
        && settings.format.is_none()
        && settings.separator.len() == 1)
    {
        return Ok(false);
    }
    let Some(fast_step) = small_positive_step(step.value) else {
        return Ok(false);
    };

    // `%0.Lf`: every digit and no radix point. The values reached here are
    // integral by construction — a precision of 0 is what says so.
    let integer = Spec::zero_padded(0, 0);
    let from = extfloat::render(&integer, first.value).into_bytes();
    let to = if last.value.is_finite() {
        extfloat::render(&integer, last.value).into_bytes()
    } else {
        b"inf".to_vec()
    };
    // `-0` reaches here, since it is not less than zero, and the decimal
    // counter has no sign to count with.
    if from.first() == Some(&b'-') || to.first() == Some(&b'-') {
        return Ok(false);
    }
    Ok(seq_fast(out, &from, &to, fast_step, settings.separator[0])?)
}

/// `0 < v && v <= 200`, and the integer it is.
fn small_positive_step(v: ExtF80) -> Option<u64> {
    if !ExtF80::ZERO.lt(v) || ExtF80::from_u32(FAST_STEP_LIMIT).lt(v) {
        return None;
    }
    // Integral by construction on both call sites, so `%.0Lf` is exact and the
    // digits are the value.
    let digits = extfloat::render(&Spec::fixed(0), v);
    digits.parse::<u64>().ok()
}

/// Count from `a` to `b` in decimal, writing the digits directly.
///
/// Returns whether it did the work. `b < a` is not an error — it means the
/// general path should have its turn, which for a descending range is where the
/// answer (nothing at all) comes from.
fn seq_fast(
    out: &mut impl Write,
    a: &[u8],
    b: &[u8],
    step: u64,
    separator: u8,
) -> io::Result<bool> {
    let unbounded = b == b"inf";
    // Without this, the naive length-then-bytes comparison would call `000`
    // larger than `99`.
    let a = trim_leading_zeros(a);
    let b = trim_leading_zeros(b);

    let mut p = a.to_vec();
    if !unbounded && cmp_digits(&p, b) == std::cmp::Ordering::Greater {
        return Ok(false);
    }
    out.write_all(&p)?;
    loop {
        incr(&mut p, step);
        if !unbounded && cmp_digits(&p, b) == std::cmp::Ordering::Greater {
            break;
        }
        out.write_all(&[separator])?;
        out.write_all(&p)?;
    }
    out.write_all(b"\n")?;
    Ok(true)
}

/// Add `step` to a decimal digit string in place.
fn incr(p: &mut Vec<u8>, step: u64) {
    let mut carry = step;
    for i in (0..p.len()).rev() {
        if carry == 0 {
            break;
        }
        let sum = u64::from(p[i].wrapping_sub(b'0')).saturating_add(carry);
        p[i] = b'0'.wrapping_add(u8::try_from(sum % 10).unwrap_or(0));
        carry = sum / 10;
    }
    while carry > 0 {
        p.insert(0, b'0'.wrapping_add(u8::try_from(carry % 10).unwrap_or(0)));
        carry /= 10;
    }
}

/// Compare two runs of decimal digits with no leading zeros: the longer is the
/// larger, and equal lengths compare bytewise.
fn cmp_digits(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Drop leading zeros, but leave one if that is all there was.
fn trim_leading_zeros(s: &[u8]) -> &[u8] {
    let first_nonzero = s.iter().position(|&c| c != b'0');
    match first_nonzero {
        Some(at) => s.get(at..).unwrap_or_default(),
        None if s.is_empty() => s,
        // All zeros: keep the last one, so `seq 000` is `0`-based and not empty.
        None => s.get(s.len().saturating_sub(1)..).unwrap_or_default(),
    }
}

/// At least one digit and nothing else. No sign, no radix point, no exponent.
fn all_digits_p(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    /// Run a command line and collect what it wrote to standard output.
    fn out(v: &[&str]) -> String {
        let request = parse_args(&args(v)).expect("parsed");
        let Request::Run(settings, operands) = request else {
            panic!("not a run: {v:?}")
        };
        let mut buf: Vec<u8> = Vec::new();
        if try_fast_from_strings(&mut buf, &settings, &operands).unwrap() {
            return String::from_utf8(buf).unwrap();
        }
        let mut first = Operand::one();
        let mut step = Operand::one();
        let mut last = scan_arg(&operands[0]).expect("first operand");
        if operands.len() > 1 {
            first = last;
            last = scan_arg(&operands[1]).expect("second operand");
            if operands.len() > 2 {
                step = last;
                last = scan_arg(&operands[2]).expect("third operand");
            }
        }
        if try_fast_from_values(&mut buf, &settings, first, step, last).unwrap() {
            return String::from_utf8(buf).unwrap();
        }
        let format = match &settings.format {
            Some(fmt) => long_double_format(fmt).expect("format"),
            None => default_format(&settings, first, step, last),
        };
        print_numbers(&mut buf, &format, &settings.separator, first, step, last).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// The message a command line is refused with, referral and all.
    fn refusal(v: &[&str]) -> String {
        let request = match parse_args(&args(v)) {
            Ok(request) => request,
            Err(e) => return e.message(),
        };
        let Request::Run(settings, operands) = request else {
            panic!("not a run: {v:?}")
        };
        if operands.is_empty() {
            return SEQ.usage_referring("missing operand".to_string()).message();
        }
        if operands.len() > 3 {
            return SEQ
                .usage_referring(format!("extra operand {}", quote(&operands[3])))
                .message();
        }
        if let Some(fmt) = &settings.format {
            match long_double_format(fmt) {
                Ok(_) => {}
                Err(e) => return e.message(),
            }
            if settings.equal_width {
                return SEQ
                    .usage_referring(
                        "format string may not be specified when printing equal width strings"
                            .to_string(),
                    )
                    .message();
            }
        }
        for (i, operand) in operands.iter().enumerate() {
            if let Err(e) = scan_arg(operand) {
                return e.message();
            }
            if i == 1 && operands.len() == 3 && scan_arg(operand).unwrap().value.is_zero() {
                return SEQ
                    .usage_referring(format!("invalid Zero increment value: {}", quote(operand)))
                    .message();
            }
        }
        panic!("not refused: {v:?}")
    }

    // ------------------------------------------------------------ the basics

    #[test]
    fn the_three_operand_forms() {
        assert_eq!(out(&["5"]), "1\n2\n3\n4\n5\n");
        assert_eq!(out(&["3", "7"]), "3\n4\n5\n6\n7\n");
        assert_eq!(out(&["0", "2", "10"]), "0\n2\n4\n6\n8\n10\n");
        assert_eq!(out(&["7", "7"]), "7\n");
    }

    #[test]
    fn a_range_that_runs_the_wrong_way_prints_nothing() {
        // Not even a newline: upstream returns before the terminator.
        assert_eq!(out(&["10", "5"]), "");
        assert_eq!(out(&["1", "-1", "5"]), "");
        assert_eq!(out(&["5", "1", "1"]), "");
    }

    #[test]
    fn a_negative_operand_is_not_an_option() {
        assert_eq!(out(&["-3", "-1", "-5"]), "-3\n-4\n-5\n");
        assert_eq!(out(&["-.5", "1", "1"]), "-0.5\n0.5\n");
        assert_eq!(out(&["-2", "2"]), "-2\n-1\n0\n1\n2\n");
    }

    // ------------------------------------------- precision from the spelling

    #[test]
    fn the_precision_comes_from_how_the_operands_were_written() {
        // The step's precision, not the value's, is what sets the format.
        assert_eq!(out(&["1", "0.5", "3"]), "1.0\n1.5\n2.0\n2.5\n3.0\n");
        assert_eq!(out(&["1", "3"]), "1\n2\n3\n");
        // `1.` reads as `1` and `.5` as `0.5`, which is a width claim as well
        // as a precision one.
        assert_eq!(out(&["1.", "3"]), "1\n2\n3\n");
        assert_eq!(out(&[".5", "1", "2.5"]), "0.5\n1.5\n2.5\n");
        // The *last* operand's precision does not widen the format; only
        // `first` and `step` do.
        assert_eq!(out(&["1", "3.00"]), "1\n2\n3\n");
    }

    #[test]
    fn an_exponent_is_read_but_never_written() {
        assert_eq!(
            out(&["1e1"]),
            (1..=10).fold(String::new(), |mut s, n| {
                s.push_str(&format!("{n}\n"));
                s
            })
        );
        assert_eq!(out(&["1.5e1", "16"]), "15\n16\n");
        // A negative exponent turns into digits after the point.
        assert_eq!(out(&["1e-1", "1e-1", "3e-1"]), "0.1\n0.2\n0.3\n");
    }

    #[test]
    fn a_hex_operand_has_no_width_and_no_precision() {
        // `0x1p+0` is 1, and the `p` keeps precision at the sentinel, which
        // sends the whole format to `%Lg`.
        assert_eq!(out(&["0x1p+0", "0x1p+0", "0x3p+0"]), "1\n2\n3\n");
        // Upstream looks for a *lowercase* `p`, so the uppercase spelling gets
        // precision 0 — and still no width, because of the `X`.
        assert_eq!(out(&["0X1P+0", "0X1P+0", "0X3P+0"]), "1\n2\n3\n");
    }

    // ---------------------------------------------------------- equal width

    #[test]
    fn equal_width_pads_to_the_widest_operand_as_written() {
        // The measured case: the `-1` is three bytes wide counting its sign,
        // and every later number is padded to it.
        assert_eq!(out(&["-w", "-1", "1", "3"]), "-1\n00\n01\n02\n03\n");
        assert_eq!(out(&["-w", "1.5", "0.5", "3"]), "1.5\n2.0\n2.5\n3.0\n");
        assert_eq!(out(&["-w", "0.1", "1"]), "0.1\n");
        assert_eq!(out(&["-w", "8", "11"]), "08\n09\n10\n11\n");
        // A step with more places than the endpoints widens both.
        assert_eq!(out(&["-w", "1", "0.5", "2"]), "1.0\n1.5\n2.0\n");
    }

    // ------------------------------------------------------------- the format

    #[test]
    fn a_format_may_carry_text_on_either_side() {
        assert_eq!(
            out(&["-f", "x%%y%.2fz%%w", "1", "3"]),
            "x%y1.00z%w\nx%y2.00z%w\nx%y3.00z%w\n"
        );
        assert_eq!(out(&["-f", "%5.2Lf", "1", "3"]), " 1.00\n 2.00\n 3.00\n");
        assert_eq!(out(&["-f", "%g", "1", "3"]), "1\n2\n3\n");
    }

    #[test]
    fn the_four_format_diagnostics_carry_no_referral() {
        // `error (EXIT_FAILURE, 0, …)`, which prints one line and stops — so
        // none of these ends in `Try 'seq --help'`.
        for (line, message) in [
            (
                &["-f", "abc", "1", "3"][..],
                "format ‘abc’ has no % directive",
            ),
            (&["-f", "%", "3"][..], "format ‘%’ ends in %"),
            (&["-f", "%L", "3"][..], "format ‘%L’ ends in %"),
            (
                &["-f", "%d", "1", "3"][..],
                "format ‘%d’ has unknown %d directive",
            ),
            (
                &["-f", "%f%f", "1", "3"][..],
                "format ‘%f%f’ has too many % directives",
            ),
            (
                &["-f", "%%", "1", "3"][..],
                "format ‘%%’ has no % directive",
            ),
        ] {
            assert_eq!(refusal(line), message, "{line:?}");
        }
        // A `%%` in the suffix is text, not a second directive.
        assert_eq!(out(&["-f", "%f%%", "1", "1"]), "1.000000%\n");
    }

    #[test]
    fn a_directive_character_cannot_forge_a_second_line() {
        let e = long_double_format(b"%\n").unwrap_err();
        assert_eq!(e.sentence, r"format ‘%\n’ has unknown %\012 directive");
        assert_eq!(e.sentence.lines().count(), 1);
        assert_eq!(e.referral, None);
        assert_eq!(e.status, 1);
    }

    // ------------------------------------------------------------ the refusals

    #[test]
    fn the_operand_diagnostics_do_carry_the_referral() {
        let referral = "\nTry 'seq --help' for more information.";
        assert_eq!(refusal(&[]), format!("missing operand{referral}"));
        assert_eq!(
            refusal(&["1", "2", "3", "4"]),
            format!("extra operand ‘4’{referral}")
        );
        assert_eq!(
            refusal(&["1", "0", "5"]),
            format!("invalid Zero increment value: ‘0’{referral}")
        );
        assert_eq!(
            refusal(&["abc"]),
            format!("invalid floating point argument: ‘abc’{referral}")
        );
        assert_eq!(
            refusal(&["nan"]),
            format!("invalid ‘not-a-number’ argument: ‘nan’{referral}")
        );
        assert_eq!(
            refusal(&["-w", "-f", "%f", "1", "3"]),
            format!(
                "format string may not be specified when printing equal width strings{referral}"
            )
        );
    }

    #[test]
    fn getopt_speaks_for_itself() {
        assert_eq!(
            refusal(&["-x"]),
            "invalid option -- 'x'\nTry 'seq --help' for more information."
        );
        assert_eq!(
            refusal(&["--zzz"]),
            "unrecognized option '--zzz'\nTry 'seq --help' for more information."
        );
        assert_eq!(
            refusal(&["-f"]),
            "option requires an argument -- 'f'\nTry 'seq --help' for more information."
        );
        assert_eq!(
            refusal(&["--format"]),
            "option '--format' requires an argument\nTry 'seq --help' for more information."
        );
        // The declaration order is the table's order, and it is not
        // alphabetical: `--equal-width` comes first because it is declared
        // first, and `--help`/`--version` come last.
        assert_eq!(
            refusal(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--equal-width' '--format' \
             '--separator' '--help' '--version'\nTry 'seq --help' for more information."
        );
    }

    #[test]
    fn options_stop_at_the_first_operand() {
        // `+` in the optstring: `--version` after an operand is an operand.
        assert_eq!(
            parse_args(&args(&["1", "--version"])).unwrap(),
            Request::Run(
                Settings::default(),
                vec![b"1".to_vec(), b"--version".to_vec()]
            )
        );
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
        // A `--` is consumed and ends the options.
        assert_eq!(
            parse_args(&args(&["--", "3"])).unwrap(),
            Request::Run(Settings::default(), vec![b"3".to_vec()])
        );
        // A lone `-` is an operand, and a bad one.
        assert_eq!(
            refusal(&["-"]),
            "invalid floating point argument: ‘-’\nTry 'seq --help' for more information."
        );
    }

    #[test]
    fn a_separator_is_taken_whole_from_the_next_argument() {
        assert_eq!(out(&["-s", "-1", "3"]), "1-12-13\n");
        assert_eq!(out(&["-s", "", "1", "3"]), "123\n");
        assert_eq!(out(&["-s", ", ", "1", "3"]), "1, 2, 3\n");
        assert_eq!(out(&["--separator=|", "1", "3"]), "1|2|3\n");
    }

    // ---------------------------------------------------------- the fast path

    #[test]
    fn plain_digits_are_counted_in_decimal_and_not_in_a_float() {
        // Past 2^64, where no float can tell the three apart.
        assert_eq!(
            out(&["99999999999999999999", "100000000000000000002"]),
            "99999999999999999999\n100000000000000000000\n\
             100000000000000000001\n100000000000000000002\n"
        );
        // Leading zeros are trimmed, and an all-zero operand keeps one digit.
        assert_eq!(out(&["007", "010"]), "7\n8\n9\n10\n");
        assert_eq!(out(&["000", "2"]), "0\n1\n2\n");
        // The step limit: 200 is fast, 201 goes the general way and must give
        // the same answer.
        assert_eq!(out(&["0", "200", "400"]), "0\n200\n400\n");
        assert_eq!(out(&["0", "201", "402"]), "0\n201\n402\n");
    }

    #[test]
    fn the_second_fast_entry_takes_integers_that_do_not_look_like_it() {
        assert_eq!(out(&["1e0", "1e0", "3e0"]), "1\n2\n3\n");
        // A descending range still prints nothing, by way of the general path.
        assert_eq!(out(&["3e0", "1e0"]), "");
    }

    // ------------------------------------------------- the number past the end

    #[test]
    fn the_number_past_the_end_is_printed_when_it_reads_back_as_the_end() {
        // The case upstream's comment names. Without the re-parse this stops at
        // `0.000002`.
        assert_eq!(
            out(&["0", "0.000001", "0.000003"]),
            "0.000000\n0.000001\n0.000002\n0.000003\n"
        );
        assert_eq!(
            out(&["0.000001", "0.000001", "0.000003"]),
            "0.000001\n0.000002\n0.000003\n"
        );
        // And it is not printed twice when the sequence already reached it.
        assert_eq!(out(&["1", "1", "3"]), "1\n2\n3\n");
    }

    // ------------------------------------------------------------- the pieces

    #[test]
    fn scan_arg_measures_width_and_precision() {
        let m = |s: &str| {
            let o = scan_arg(s.as_bytes()).unwrap();
            (o.width, o.precision)
        };
        assert_eq!(m("1"), (1, 0));
        assert_eq!(m("-1"), (2, 0));
        assert_eq!(m("1."), (1, 0));
        assert_eq!(m(".5"), (3, 1));
        assert_eq!(m("-.5"), (4, 1));
        assert_eq!(m("0.50"), (4, 2));
        // Spaces and a leading `+` are read but never printed.
        assert_eq!(m(" +1"), (1, 0));
        // `1e5` prints as `100000`.
        assert_eq!(m("1e5"), (6, 0));
        assert_eq!(m("1.5e1"), (2, 0));
        assert_eq!(m("1e-5"), (7, 5));
        assert_eq!(m("1.5e-3"), (6, 4));
        assert_eq!(m("1.e5"), (6, 0));
        // A hex float is measured not at all.
        assert_eq!(m("0x1p+0"), (0, NO_PRECISION));
    }

    #[test]
    fn incr_adds_in_decimal_and_grows_the_string() {
        let mut p = b"9".to_vec();
        incr(&mut p, 1);
        assert_eq!(p, b"10");
        let mut p = b"999".to_vec();
        incr(&mut p, 2);
        assert_eq!(p, b"1001");
        let mut p = b"0".to_vec();
        incr(&mut p, 200);
        assert_eq!(p, b"200");
        let mut p = b"99999999999999999999".to_vec();
        incr(&mut p, 1);
        assert_eq!(p, b"100000000000000000000");
    }

    #[test]
    fn digits_compare_by_length_first() {
        use std::cmp::Ordering;
        assert_eq!(cmp_digits(b"99", b"100"), Ordering::Less);
        assert_eq!(cmp_digits(b"100", b"99"), Ordering::Greater);
        assert_eq!(cmp_digits(b"100", b"100"), Ordering::Equal);
        assert_eq!(trim_leading_zeros(b"000"), b"0");
        assert_eq!(trim_leading_zeros(b"007"), b"7");
        assert_eq!(trim_leading_zeros(b"70"), b"70");
    }

    #[test]
    fn all_digits_is_digits_and_nothing_else() {
        assert!(all_digits_p(b"0"));
        assert!(all_digits_p(b"0123456789"));
        assert!(!all_digits_p(b""));
        assert!(!all_digits_p(b"-1"));
        assert!(!all_digits_p(b"1.0"));
        assert!(!all_digits_p(b"1e5"));
        assert!(!all_digits_p(b" 1"));
    }
}
