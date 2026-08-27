//! `sleep` -- pause for a length of time.
//!
//! # What was here before
//!
//! Forty-seven lines that took one argument and called `f64::from_str` on it.
//! Six defects, in the order a user would meet them:
//!
//! 1. **It read argv as `String`.** `env::args()` unwraps, so `sleep` died with
//!    a Rust panic on an argument holding a byte that is not UTF-8 -- before
//!    running a line this repository wrote. That is what brought the file into
//!    `scripts/argv-utf8.py`'s baseline, and the rest was found on the way.
//! 2. **One operand.** GNU sums them all: `sleep 1 2` pauses for three seconds.
//!    The old code read `args.first()` and *silently ignored the rest*, which
//!    its own test pinned as correct.
//! 3. **No suffixes.** `sleep 5m` is five minutes to GNU and was an error here.
//! 4. **No options at all.** `sleep --help` was an invalid time interval.
//! 5. **`inf` was refused.** GNU accepts it -- `strtod` reads the word -- and
//!    pauses forever. The old code called `is_finite()` an input check.
//! 6. **The wrong grammar.** `f64::from_str` is not `strtod`: it rejects the
//!    leading whitespace and the `0x` form that `strtod` accepts, so
//!    `sleep ' 1'` and `sleep 0x10` were errors. Both are measured to work.
//!
//! # The number is read the way `strtod` reads one
//!
//! Upstream is `xstrtod (argv[i], &p, &s, cl_strtod)`, and `cl_strtod` is the
//! C-locale `strtod`. [`coreutils::extfloat::strtold`] is that grammar, already
//! certified against glibc for `printf` and `seq`, and it reports how much of
//! the input it claimed -- which is the `p` upstream then inspects for a
//! suffix. [`ExtF80::to_f64`](coreutils::extfloat::ExtF80::to_f64) narrows it.
//!
//! That narrowing rounds a second time where glibc's `strtod` rounds once, so a
//! numeral sitting exactly on a `double`'s rounding boundary can land one ulp
//! from where glibc puts it. The difference is at most one part in 2^52 of a
//! duration -- for `sleep 1` a quarter of an attosecond -- against a `nanosleep`
//! whose own granularity is nine orders of magnitude coarser. Reading it twice,
//! once at 64 bits for the value and once at 53 for the grammar, would be two
//! parsers to keep in agreement in exchange for nothing observable.
//!
//! # Two shapes that look like bugs and are measured
//!
//! **`sleep --` exits 0 without pausing.** Upstream tests `argc == 1` -- were
//! there *arguments* -- and not whether any operand survived option parsing. So
//! a bare `sleep` is `missing operand` and `sleep --` is a zero-second pause.
//!
//! **`--help` wins wherever it appears.** `sleep 1 2 --help` prints the help and
//! exits 0 rather than pausing for three seconds, because `getopt_long` scans
//! the whole command line before the operand loop starts. `sleep abc --help`
//! likewise prints the help rather than complaining about `abc`.

use coreutils::extfloat;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

coreutils::guard_std_fds!();

/// Measured: `sleep abc; echo $?` prints 1.
const SLEEP: Program = Program::new("sleep", 1);

/// GNU `sleep`'s `getopt_long` string, exactly: it has no short options.
///
/// Empty rather than absent, so that `sleep -1` is `invalid option -- '1'`
/// rather than an operand -- which is what it is measured to be, and is the one
/// place a leading `-` does *not* mean a negative interval.
const SHORT_OPTIONS: &str = "";

/// GNU `sleep`'s `longopts[]`: `parse_gnu_standard_options_only`'s two, and
/// nothing of its own.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum Request {
    Help,
    Version,
    /// Pause for this many seconds -- the sum of every operand, already
    /// multiplied by its suffix. Non-negative, never NaN, possibly infinite.
    Pause(f64),
}

/// Transcribed from GNU 9.4, without the trailing block of URLs that no bin
/// here carries.
fn help_text() -> String {
    "\
Usage: sleep NUMBER[SUFFIX]...
  or:  sleep OPTION
Pause for NUMBER seconds.  SUFFIX may be 's' for seconds (the default),
'm' for minutes, 'h' for hours or 'd' for days.  NUMBER need not be an
integer.  Given two or more arguments, pause for the amount of time
specified by the sum of their values.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Upstream's `apply_suffix`: what one trailing letter multiplies by.
///
/// `None` is upstream's `multiplier == 0`, which is the *only* way a suffix is
/// rejected -- so an unknown letter and a second letter after a good one are
/// both `invalid time interval`, and neither says anything about suffixes.
fn suffix_multiplier(c: u8) -> Option<f64> {
    match c {
        b's' => Some(1.0),
        b'm' => Some(60.0),
        b'h' => Some(60.0 * 60.0),
        b'd' => Some(60.0 * 60.0 * 24.0),
        _ => None,
    }
}

/// One operand, as seconds.
///
/// `None` is upstream's four-way `||`, collapsed: no conversion at all, a
/// negative or NaN value, more than one character after the number, or a
/// trailing character that is not a suffix.
///
/// A range error is deliberately *not* a rejection. Upstream writes
/// `! (xstrtod (…) || errno == ERANGE)`, so `sleep 1e400` pauses forever and
/// `sleep 1e-400` pauses for no time at all, both successfully.
fn operand_seconds(arg: &[u8]) -> Option<f64> {
    let scanned = extfloat::strtold(arg);
    if scanned.consumed == 0 {
        return None;
    }
    let value = scanned.value.to_f64();
    // `0 <= s` upstream, which admits `-0.0` and refuses NaN. Written as two
    // tests rather than `!(value >= 0.0)` because the negation of a partial
    // order is the shape that reads as a typo.
    if value.is_nan() || value < 0.0 {
        return None;
    }
    let multiplier = match arg.get(scanned.consumed..) {
        None | Some([]) => 1.0,
        Some(&[c]) => suffix_multiplier(c)?,
        // Upstream's `*p && *(p+1)`: two or more characters left over is a
        // rejection before any suffix is looked at, so `1s2` never reaches
        // `apply_suffix`.
        Some(_) => return None,
    };
    Some(value * multiplier)
}

/// Parse `sleep`'s argv.
///
/// # Errors
///
/// No arguments at all, a getopt error, or any operand that is not a time
/// interval -- **every** bad operand is named, in the order they were given,
/// because upstream records a failure and carries on rather than stopping at
/// the first.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut seconds = 0.0f64;
    let mut bad: Vec<String> = Vec::new();

    // Upstream's `argc == 1`, which asks whether there were *arguments* and not
    // whether any operand survived. `sleep --` therefore pauses for zero
    // seconds and exits 0; see the module documentation.
    if args.is_empty() {
        return Err(SLEEP.usage_referring("missing operand".to_string()));
    }

    for item in SLEEP.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // Returned rather than recorded: upstream's option scan runs to
            // completion before the operand loop begins, so `--help` beats a
            // bad interval that precedes it. Measured: `sleep abc --help`
            // prints the help and exits 0.
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(arg) => match operand_seconds(&os_bytes(arg.as_os_str())) {
                Some(s) => seconds += s,
                None => bad.push(format!(
                    "invalid time interval {}",
                    quote(&os_bytes(arg.as_os_str()))
                )),
            },
            // Unreachable: the parser yields only names from the table above,
            // and there are no short options. Refusing rather than ignoring, so
            // a table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(SLEEP.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(SLEEP.invalid_option(other)),
        }
    }

    if bad.is_empty() {
        return Ok(Request::Pause(seconds));
    }
    // Every line but the first carries the `sleep: ` prefix itself: upstream
    // makes one `error (0, 0, …)` call per bad operand and only then calls
    // `usage (EXIT_FAILURE)`, so the referral follows the last of them. See
    // `Program::usage_referring`, which leaves that to the caller.
    Err(SLEEP.usage_referring(bad.join("\nsleep: ")))
}

/// The pause, saturating rather than panicking.
///
/// `Duration` tops out around 5.8e11 years and `sleep inf` is measured to pause
/// forever, so the two ends have to meet somewhere. Upstream's `xnanosleep`
/// clamps each `nanosleep` to `time_t`'s maximum and loops, which is "forever"
/// by a longer route; `Duration::MAX` is the same answer without the loop.
/// `Duration::from_secs_f64` would panic on both the infinity and any finite
/// value past the ceiling, which for a pause is the worst of the three
/// behaviours.
fn pause_for(seconds: f64) -> Duration {
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Decided before stdout exists: a usage error reaches upstream's
    // `atexit (close_stdout)` with nothing buffered, so `sleep abc >&-` prints
    // only the interval complaint.
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            SLEEP.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"sleep (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Pause(seconds) => {
            // After the write verdict is *not* where this goes: upstream sleeps
            // and then runs `close_stdout`, and a `sleep 5 >&-` that reported
            // the closed descriptor five seconds early would be observably
            // different.
            thread::sleep(pause_for(seconds));
            ExitCode::SUCCESS
        }
    };
    stdfd::close_stdout("sleep", out, earned)
}

/// The funnel. A diagnostic that could not be written turns the earned status
/// into failure, which is what upstream's `atexit (close_stdout)` does on every
/// exit path at once. See [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn pause(args: &[&str]) -> f64 {
        match parse_args(&argv(args)).unwrap() {
            Request::Pause(s) => s,
            other => panic!("wanted a pause, got {other:?}"),
        }
    }

    fn refuse(args: &[&str]) -> String {
        parse_args(&argv(args)).unwrap_err().message()
    }

    #[test]
    fn an_integer_and_a_fraction_are_both_seconds() {
        assert_eq!(pause(&["5"]), 5.0);
        assert_eq!(pause(&["0.25"]), 0.25);
        assert_eq!(pause(&["0"]), 0.0);
    }

    /// The defect that made the old file's `extra_args_ignored_takes_first`
    /// test wrong: GNU sums the operands.
    #[test]
    fn several_operands_are_summed() {
        assert_eq!(pause(&["1", "2"]), 3.0);
        assert_eq!(pause(&["0.5", "0.25", "0.25"]), 1.0);
    }

    #[test]
    fn a_suffix_multiplies() {
        assert_eq!(pause(&["2s"]), 2.0);
        assert_eq!(pause(&["2m"]), 120.0);
        assert_eq!(pause(&["2h"]), 7200.0);
        assert_eq!(pause(&["1d"]), 86_400.0);
        assert_eq!(pause(&["1m", "30s"]), 90.0);
    }

    /// `strtod`'s grammar, not `f64::from_str`'s: leading whitespace and a hex
    /// numeral are both accepted. Measured -- `sleep 0x10` exits 0 after
    /// sixteen seconds.
    #[test]
    fn the_grammar_is_strtods() {
        assert_eq!(pause(&[" 1"]), 1.0);
        assert_eq!(pause(&["0x10"]), 16.0);
        assert_eq!(pause(&["1e1"]), 10.0);
        assert_eq!(pause(&["+3"]), 3.0);
    }

    /// `inf` is a valid interval upstream and pauses forever; the old code
    /// refused it as "not finite".
    #[test]
    fn infinity_is_a_valid_interval() {
        assert!(pause(&["inf"]).is_infinite());
        assert!(pause(&["INFINITY"]).is_infinite());
        assert_eq!(pause_for(f64::INFINITY), Duration::MAX);
    }

    /// A range error is not a rejection: upstream tolerates `errno == ERANGE`
    /// explicitly, so both ends of the `double` range are ordinary answers.
    #[test]
    fn a_value_out_of_range_is_accepted_at_both_ends() {
        assert!(pause(&["1e400"]).is_infinite());
        assert_eq!(pause(&["1e-400"]), 0.0);
    }

    #[test]
    fn a_bare_invocation_is_a_missing_operand() {
        assert_eq!(
            refuse(&[]),
            "missing operand\nTry 'sleep --help' for more information."
        );
    }

    /// Upstream asks whether there were *arguments*, not whether any operand
    /// survived, so this is a zero-second pause and not `missing operand`.
    /// Measured: `sleep --; echo $?` prints 0 at once.
    #[test]
    fn a_double_dash_alone_is_a_zero_second_pause() {
        assert_eq!(pause(&["--"]), 0.0);
    }

    /// After `--` it is an operand however it is spelled -- measured:
    /// `sleep -- --help` says `invalid time interval ‘--help’`.
    #[test]
    fn a_double_dash_turns_an_option_into_an_operand() {
        assert_eq!(
            refuse(&["--", "--help"]),
            "invalid time interval \u{2018}--help\u{2019}\n\
             Try 'sleep --help' for more information."
        );
    }

    #[test]
    fn a_value_that_is_not_a_number_is_refused() {
        for arg in ["abc", "", "-", "nan", "1x", "1s2", "1e1x"] {
            assert!(
                refuse(&[arg]).starts_with("invalid time interval "),
                "{arg} was accepted"
            );
        }
    }

    /// `0 <= s` upstream, so a negative interval is refused -- but only where
    /// it can be *reached*, since a bare `-1` is an option. `-0` is not
    /// negative by that test and is allowed.
    #[test]
    fn a_negative_interval_is_refused_but_negative_zero_is_not() {
        assert!(refuse(&["--", "-1"]).starts_with("invalid time interval "));
        assert_eq!(pause(&["--", "-0"]), 0.0);
    }

    /// Upstream records each failure and carries on, so every bad operand is
    /// named and the referral follows the last. Measured, exactly.
    #[test]
    fn every_bad_operand_is_named() {
        assert_eq!(
            refuse(&["abc", "def"]),
            "invalid time interval \u{2018}abc\u{2019}\n\
             sleep: invalid time interval \u{2018}def\u{2019}\n\
             Try 'sleep --help' for more information."
        );
    }

    /// A leading `-` is getopt's, not `strtod`'s: `sleep -1` is an unknown
    /// option and says so.
    #[test]
    fn a_leading_dash_is_an_option() {
        assert_eq!(
            refuse(&["-1"]),
            "invalid option -- '1'\nTry 'sleep --help' for more information."
        );
        assert_eq!(
            refuse(&["--bogus"]),
            "unrecognized option '--bogus'\nTry 'sleep --help' for more information."
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        // Unambiguous prefixes, as glibc resolves them.
        assert_eq!(parse_args(&argv(&["--hel"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--v"])).unwrap(), Request::Version);
    }

    /// The scan finishes before the operands are judged, so `--help` wins
    /// wherever it is -- even after an interval that would have paused, and
    /// even after one that would have failed.
    #[test]
    fn help_wins_over_operands_that_precede_it() {
        assert_eq!(
            parse_args(&argv(&["1", "2", "--help"])).unwrap(),
            Request::Help
        );
        assert_eq!(
            parse_args(&argv(&["abc", "--help"])).unwrap(),
            Request::Help
        );
    }

    /// An operand that is not UTF-8 must reach the diagnostic escaped, not
    /// replaced -- and must not panic on the way, which is the defect that
    /// brought this file into the baseline.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_operand_is_escaped_not_corrupted() {
        use std::os::unix::ffi::OsStringExt;
        let e = parse_args(&[OsString::from_vec(b"1\xffs".to_vec())]).unwrap_err();
        assert!(
            e.message()
                .starts_with("invalid time interval \u{2018}1\\377s\u{2019}"),
            "got {:?}",
            e.message()
        );
    }

    #[test]
    fn a_finite_pause_is_not_clamped() {
        assert_eq!(pause_for(1.5), Duration::from_millis(1500));
        assert_eq!(pause_for(0.0), Duration::ZERO);
        // Past `Duration`'s ceiling, and so clamped rather than panicking.
        assert_eq!(pause_for(1e30), Duration::MAX);
    }

    #[test]
    fn the_help_text_is_upstreams_wording() {
        let text = help_text();
        assert!(text.starts_with("Usage: sleep NUMBER[SUFFIX]...\n  or:  sleep OPTION\n"));
        assert!(text.contains("'m' for minutes, 'h' for hours or 'd' for days."));
        assert!(text.ends_with("output version information and exit\n"));
    }
}
