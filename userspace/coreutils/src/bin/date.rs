//! date — print a date and time.
//!
//! ```text
//! date [-u] [-d @EPOCH | -r FILE] [-R | -I[SPEC] | --rfc-3339=SPEC | +FORMAT]
//! ```
//!
//! # It used to answer a different question from the one it was asked
//!
//! This program read the clock, formatted it one way, and printed it —
//! whatever the command line said:
//!
//! ```text
//! $ date -d @0
//! ours   Fri Sep 11 23:45:44 UTC 2026     <- the current time
//! GNU    Thu Jan  1 00:00:00 UTC 1970
//! ```
//!
//! Both exited 0. `scripts/date-diff.sh` scored it **3 of 121**, and the three
//! were the cases where the answer happens not to depend on the arguments.
//! That is worse than an unimplemented option and worse than the refusing stub
//! §1006 forbids, because a refusal at least tells the caller it did not get
//! what it asked for; `date -d @0 +%s` in a script did not fail, it returned
//! today.
//!
//! # Why this is mostly wiring
//!
//! The hard part of `date` is the formatter, and the tree already has one:
//! [`localtime::strftime`] implements the whole specifier set — `%a %A %b %B
//! %c %C %d %D %e %F %g %G %h %H %I %j %k %l %m %M %n %N %p %P %r %R %s %S %t
//! %T %u %U %V %w %W %x %X %y %Y %z %Z %%` — against a [`localtime::Tm`] that
//! knows its zone. So this file decides *which instant*, *which zone* and
//! *which format string*, and hands all three to code that already works.
//!
//! The previous version carried its own `unix_secs_to_datetime`, which is the
//! fourth copy of that arithmetic this tree has had to remove.
//!
//! # What is refused rather than approximated
//!
//! * **`-d` with anything but `@EPOCH`.** GNU's `-d` accepts a small natural
//!   language — `yesterday`, `2 weeks ago`, `2021-03-04 05:06:07 +0200`. That
//!   is a parser, not a format string, and guessing at it would reintroduce
//!   exactly the defect above in a subtler form: a date that is plausible and
//!   wrong. `@EPOCH` is the form scripts use and is unambiguous.
//! * **`-s`/`--set`**, which sets the system clock.
//! * **`-f`/`--file`, `--debug`, `--resolution`.**
//!
//! Each says so. The table below still carries all sixteen of GNU's long
//! options, because the table is what decides whether an abbreviation is
//! ambiguous — `uname` paid for that lesson, where a missing name made every
//! abbreviation of it resolve to some *other* option.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote_os};
use coreutils::stdfd;
use localtime::{Tm, Zone, strftime};
use std::ffi::OsString;
use std::fs;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

/// GNU `date` exits 1 on a usage error.
const DATE: Program = Program::new("date", 1);

/// GNU `date`'s `getopt_long` string. `-I` takes an *optional* argument, which
/// is why `date -I` and `date -Iseconds` are both valid and `date -I seconds`
/// is not.
const SHORT_OPTIONS: &str = "d:f:I::r:Rs:u";

/// GNU `date`'s `longopts[]`, in its declaration order, read from the binary
/// with `date --=x` rather than from the manual.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("date", Takes::Required),
    ("debug", Takes::Nothing),
    ("file", Takes::Required),
    ("iso-8601", Takes::Optional),
    ("reference", Takes::Required),
    ("resolution", Takes::Nothing),
    ("rfc-email", Takes::Nothing),
    ("rfc-822", Takes::Nothing),
    ("rfc-2822", Takes::Nothing),
    ("rfc-3339", Takes::Required),
    ("set", Takes::Required),
    ("uct", Takes::Nothing),
    ("utc", Takes::Nothing),
    ("universal", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The long names that are ONE option wearing three spellings.
///
/// Named `ALIASES` and not `LONG_ALIASES`: `scripts/getopt-ambiguity-check.py`
/// matches the former exactly, so a table declared under the other spelling is
/// invisible to it. `chmod.rs` and `chown.rs` both use `LONG_ALIASES` and are
/// therefore unchecked on this axis -- recorded in `known-issues.md`.
///
/// GNU's `struct option` carries a `val`, and `getopt_long` judges ambiguity by
/// that rather than by the name: two spellings sharing a `val` are one option,
/// so a prefix matching both of them RESOLVES instead of failing. `--uct`,
/// `--utc` and `--universal` share one, which is why `date --u` works in GNU
/// and would be "ambiguous" against a faithful-looking name-only table.
///
/// Found by `scripts/getopt-ambiguity-check.py` refusing the push, not by
/// reading: *"date: `--u` we say ambiguous, GNU resolves it; matches ['uct',
/// 'utc', 'universal']: a missing ALIASES entry"*. The same shape as `rmdir`'s
/// `--path`/`--parents`, which is the case that gate was written for.
const ALIASES: &[(&str, &str)] = &[("uct", "utc"), ("universal", "utc")];

/// GNU's default output, measured: `Sun Sep  9 01:46:40 UTC 2001`.
const DEFAULT_FORMAT: &[u8] = b"%a %b %e %H:%M:%S %Z %Y";
/// `-R`, measured: `Sun, 09 Sep 2001 01:46:40 +0000`.
const RFC_EMAIL_FORMAT: &[u8] = b"%a, %d %b %Y %H:%M:%S %z";

/// Which instant to print.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum When {
    Now,
    /// `-d @N`.
    Epoch(i64),
    /// `-r FILE`: the file's modification time, as `(secs, nanos)`.
    File(OsString),
}

/// What to print it as.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Shape {
    Default,
    /// A `+FORMAT` operand, without its leading `+`.
    Custom(Vec<u8>),
    RfcEmail,
    /// `-I[SPEC]` and `--rfc-3339=SPEC` share a rendering and differ by the
    /// character between the date and the time: `T` for ISO, a space for 3339.
    Iso(Precision, u8),
}

/// How much of the time `-I`/`--rfc-3339` prints.
// `Copy` and `PartialEq` unconditionally: the formatter compares and copies
// these outside the test build too. Only `Debug` is test-only.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Precision {
    Date,
    Hours,
    Minutes,
    Seconds,
    Ns,
}

impl Precision {
    /// The spelling GNU accepts, or `None`.
    fn parse(spec: &[u8], allow_hm: bool) -> Option<Self> {
        match spec {
            b"date" => Some(Self::Date),
            b"hours" if allow_hm => Some(Self::Hours),
            b"minutes" if allow_hm => Some(Self::Minutes),
            b"seconds" => Some(Self::Seconds),
            b"ns" => Some(Self::Ns),
            _ => None,
        }
    }
}

/// Build the format string for `-I`/`--rfc-3339`.
///
/// The zone suffix is `%:z` — the colon form, `+00:00` — which is what both
/// spellings use and which is *not* what plain `%z` produces (`+0000`).
/// Measured: `date -Iseconds` gives `2001-09-09T01:46:40+00:00`.
fn iso_format(p: Precision, sep: u8) -> Vec<u8> {
    let mut f = b"%Y-%m-%d".to_vec();
    if p == Precision::Date {
        return f;
    }
    f.push(sep);
    f.extend_from_slice(match p {
        Precision::Hours => b"%H".as_slice(),
        Precision::Minutes => b"%H:%M".as_slice(),
        Precision::Seconds => b"%H:%M:%S".as_slice(),
        // ISO uses a comma before the fraction and RFC 3339 a full stop; the
        // caller's separator tells them apart, since only ISO passes `T`.
        Precision::Ns if sep == b'T' => b"%H:%M:%S,%N".as_slice(),
        _ => b"%H:%M:%S.%N".as_slice(),
    });
    f.extend_from_slice(b"%:z");
    f
}

/// A parsed command line.
#[cfg_attr(test, derive(Debug))]
struct Config {
    when: When,
    shape: Shape,
    utc: bool,
}

fn help_text() -> String {
    "\
Usage: date [OPTION]... [+FORMAT]
Display the current time in the given FORMAT.

  -d, --date=STRING          display time described by STRING (only @EPOCH here)
  -I[FMT], --iso-8601[=FMT]  output date/time in ISO 8601 format
  -R, --rfc-email            output date and time in RFC 5322 format
      --rfc-3339=FMT         output date/time in RFC 3339 format
  -r, --reference=FILE       display last modification time of FILE
  -u, --utc, --universal     print Coordinated Universal Time (UTC)
      --help                 display this help and exit
      --version              output version information and exit
"
    .to_string()
}

/// Parse `date`'s argv.
///
/// # Errors
///
/// An unknown option, an ambiguous abbreviation, a second `+FORMAT`, an operand
/// that is not a format, or one of the options this does not implement.
fn parse_args(args: &[OsString]) -> Result<Config, String> {
    let mut cfg = Config {
        when: When::Now,
        shape: Shape::Default,
        utc: false,
    };
    let mut seen_format = false;

    for item in DATE.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item.map_err(|e| e.message())? {
            Opt::Long("utc" | "universal" | "uct", _) | Opt::Short(b'u', _) => cfg.utc = true,
            Opt::Long("rfc-email" | "rfc-822" | "rfc-2822", _) | Opt::Short(b'R', _) => {
                cfg.shape = Shape::RfcEmail;
            }
            Opt::Long("date", v) | Opt::Short(b'd', v) => {
                cfg.when = parse_when(v.unwrap_or_default())?;
            }
            Opt::Long("reference", v) | Opt::Short(b'r', v) => {
                cfg.when = When::File(v.unwrap_or_default());
            }
            Opt::Long("iso-8601", v) | Opt::Short(b'I', v) => {
                let spec = v.unwrap_or_else(|| OsString::from("date"));
                let bytes = os_bytes(&spec);
                let p = Precision::parse(&bytes, true).ok_or_else(|| {
                    DATE.usage_referring(format!(
                        "invalid argument {} for '--iso-8601'",
                        quote_os(&spec)
                    ))
                    .message()
                })?;
                cfg.shape = Shape::Iso(p, b'T');
            }
            Opt::Long("rfc-3339", v) => {
                let spec = v.unwrap_or_default();
                let bytes = os_bytes(&spec);
                // GNU's `--rfc-3339` does NOT accept `hours` or `minutes`;
                // `-I` does. Measured, not assumed.
                let p = Precision::parse(&bytes, false).ok_or_else(|| {
                    DATE.usage_referring(format!(
                        "invalid argument {} for '--rfc-3339'",
                        quote_os(&spec)
                    ))
                    .message()
                })?;
                cfg.shape = Shape::Iso(p, b' ');
            }
            // Refused rather than approximated -- see the module docs.
            Opt::Long(name @ ("set" | "file" | "debug" | "resolution"), _) => {
                return Err(DATE
                    .usage_referring(format!("option '--{name}' is not implemented"))
                    .message());
            }
            Opt::Short(c @ (b's' | b'f'), _) => {
                return Err(DATE
                    .usage_referring(format!("option '-{}' is not implemented", c as char))
                    .message());
            }
            Opt::Long("help", _) => return Err(HELP_SENTINEL.to_string()),
            Opt::Long("version", _) => return Err(VERSION_SENTINEL.to_string()),
            Opt::Long(other, _) => {
                return Err(DATE
                    .usage_referring(format!("option '--{other}' is unhandled"))
                    .message());
            }
            Opt::Short(other, _) => return Err(DATE.invalid_option(other).message()),
            Opt::Operand(arg) => {
                let bytes = os_bytes(arg);
                let Some(fmt) = bytes.strip_prefix(b"+") else {
                    return Err(DATE
                        .usage_referring(format!("extra operand {}", quote_os(arg)))
                        .message());
                };
                if seen_format {
                    return Err(DATE
                        .usage_referring(format!("extra operand {}", quote_os(arg)))
                        .message());
                }
                seen_format = true;
                cfg.shape = Shape::Custom(fmt.to_vec());
            }
        }
    }
    Ok(cfg)
}

/// `--help` and `--version` travel back through the error channel because they
/// are the only two non-error early exits, and a second success variant would
/// have to be threaded through every arm above to carry them.
const HELP_SENTINEL: &str = "\u{1}help";
const VERSION_SENTINEL: &str = "\u{1}version";

/// `-d`'s argument. Only `@EPOCH` is accepted; see the module docs for why the
/// rest is refused rather than guessed at.
fn parse_when(value: OsString) -> Result<When, String> {
    let bytes = os_bytes(&value);
    if let Some(rest) = bytes.strip_prefix(b"@")
        && let Ok(text) = std::str::from_utf8(rest)
        && let Ok(secs) = text.parse::<i64>()
    {
        return Ok(When::Epoch(secs));
    }
    Err(DATE
        .usage_referring(format!(
            "invalid date {} -- only @SECONDS-SINCE-EPOCH is supported here",
            quote_os(&value)
        ))
        .message())
}

/// The instant a [`When`] names, as `(seconds, nanoseconds)`.
fn resolve(when: &When) -> Result<(i64, u32), String> {
    match when {
        When::Now => {
            let d = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "date: the system clock is before the epoch".to_string())?;
            let secs = i64::try_from(d.as_secs())
                .map_err(|_| "date: the system clock is out of range".to_string())?;
            Ok((secs, d.subsec_nanos()))
        }
        When::Epoch(s) => Ok((*s, 0)),
        When::File(path) => {
            let meta = fs::metadata(path)
                .map_err(|e| format!("date: {}: {}", quote_os(path), strerror(&e)))?;
            let mtime = meta
                .modified()
                .map_err(|e| format!("date: {}: {}", quote_os(path), strerror(&e)))?;
            match mtime.duration_since(UNIX_EPOCH) {
                Ok(d) => {
                    let secs = i64::try_from(d.as_secs())
                        .map_err(|_| "date: that timestamp is out of range".to_string())?;
                    Ok((secs, d.subsec_nanos()))
                }
                // Before the epoch: representable, and the duration is the
                // distance backwards.
                Err(e) => {
                    let d = e.duration();
                    let secs = i64::try_from(d.as_secs())
                        .map_err(|_| "date: that timestamp is out of range".to_string())?;
                    Ok((-secs, 0))
                }
            }
        }
    }
}

fn format_for(shape: &Shape) -> Vec<u8> {
    match shape {
        Shape::Default => DEFAULT_FORMAT.to_vec(),
        Shape::RfcEmail => RFC_EMAIL_FORMAT.to_vec(),
        Shape::Custom(f) => f.clone(),
        Shape::Iso(p, sep) => iso_format(*p, *sep),
    }
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(m) if m == HELP_SENTINEL => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Err(m) if m == VERSION_SENTINEL => {
            println!("date (SlateOS coreutils)");
            return ExitCode::SUCCESS;
        }
        Err(m) => {
            diag!("{}", m);
            return ExitCode::FAILURE;
        }
    };

    let (secs, nanos) = match resolve(&cfg.when) {
        Ok(v) => v,
        Err(m) => {
            diag!("{}", m);
            return ExitCode::FAILURE;
        }
    };

    let zone = if cfg.utc {
        Zone::utc()
    } else {
        Zone::from_env()
    };
    let tm: Tm = zone.local(secs, nanos);
    let out = strftime(&format_for(&cfg.shape), &tm);

    // Written as bytes: a format string may contain any byte, and a `%` that
    // `strftime` does not recognise is passed through unchanged, so the result
    // is not necessarily UTF-8 even when the format was.
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&out)
        .and_then(|()| stdout.write_all(b"\n"))
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn iso_formats_match_the_measured_gnu_strings() {
        // Measured with `date -I… -d @1000000000` against GNU 9.4, TZ=UTC:
        //   -I          2001-09-09
        //   -Iseconds   2001-09-09T01:46:40+00:00
        //   -Ins        2001-09-09T01:46:40,000000000+00:00
        assert_eq!(iso_format(Precision::Date, b'T'), b"%Y-%m-%d");
        assert_eq!(iso_format(Precision::Hours, b'T'), b"%Y-%m-%dT%H%:z");
        assert_eq!(
            iso_format(Precision::Seconds, b'T'),
            b"%Y-%m-%dT%H:%M:%S%:z"
        );
        assert_eq!(iso_format(Precision::Ns, b'T'), b"%Y-%m-%dT%H:%M:%S,%N%:z");
    }

    #[test]
    fn rfc_3339_uses_a_space_and_a_full_stop() {
        // The two spellings differ in exactly two characters, and both were
        // measured: `--rfc-3339=ns` gives `2001-09-09 01:46:40.000000000+00:00`
        // where `-Ins` gives a `T` and a comma.
        assert_eq!(
            iso_format(Precision::Seconds, b' '),
            b"%Y-%m-%d %H:%M:%S%:z"
        );
        assert_eq!(iso_format(Precision::Ns, b' '), b"%Y-%m-%d %H:%M:%S.%N%:z");
    }

    #[test]
    fn rfc_3339_refuses_hours_and_minutes_but_iso_accepts_them() {
        // Not symmetric, and not guessable: GNU's --rfc-3339 takes only date,
        // seconds and ns.
        assert!(Precision::parse(b"hours", true).is_some());
        assert!(Precision::parse(b"hours", false).is_none());
        assert!(Precision::parse(b"minutes", false).is_none());
        assert!(Precision::parse(b"seconds", false).is_some());
        assert!(Precision::parse(b"ns", false).is_some());
        assert!(Precision::parse(b"nosuch", true).is_none());
    }

    #[test]
    fn d_accepts_only_an_epoch() {
        assert_eq!(parse_when(os("@0")), Ok(When::Epoch(0)));
        assert_eq!(
            parse_when(os("@1000000000")),
            Ok(When::Epoch(1_000_000_000))
        );
        assert_eq!(parse_when(os("@-1")), Ok(When::Epoch(-1)));
        // Everything else is refused rather than guessed at, which is the
        // point: a plausible wrong date is the defect this file was rewritten
        // to remove.
        assert!(parse_when(os("yesterday")).is_err());
        assert!(parse_when(os("2021-03-04")).is_err());
        assert!(parse_when(os("@")).is_err());
        assert!(parse_when(os("@notanumber")).is_err());
        assert!(parse_when(os("")).is_err());
    }

    #[test]
    fn a_plus_operand_is_the_format_and_anything_else_is_an_error() {
        let cfg = parse_args(&[os("+%Y")]).unwrap();
        assert_eq!(cfg.shape, Shape::Custom(b"%Y".to_vec()));
        // A bare operand is not a format.
        assert!(parse_args(&[os("%Y")]).is_err());
        // A second one is an error even though the first was valid.
        assert!(parse_args(&[os("+%Y"), os("+%m")]).is_err());
    }

    #[test]
    fn utc_has_three_spellings_and_a_short_one() {
        for a in ["-u", "--utc", "--universal", "--uct"] {
            assert!(parse_args(&[os(a)]).unwrap().utc, "{a} should select UTC");
        }
        assert!(!parse_args(&[os("+%Y")]).unwrap().utc);
    }

    #[test]
    fn rfc_email_has_three_spellings() {
        for a in ["-R", "--rfc-email", "--rfc-822", "--rfc-2822"] {
            assert_eq!(parse_args(&[os(a)]).unwrap().shape, Shape::RfcEmail, "{a}");
        }
    }

    #[test]
    fn the_unimplemented_options_say_so_rather_than_doing_nothing() {
        for a in ["--set=x", "--file=x", "--debug", "--resolution"] {
            let e = parse_args(&[os(a)]).unwrap_err();
            assert!(e.contains("not implemented"), "{a} gave {e}");
        }
    }

    #[test]
    fn epoch_resolves_without_consulting_the_clock() {
        // The whole defect this file existed to fix: -d @0 must not be "now".
        assert_eq!(resolve(&When::Epoch(0)).unwrap(), (0, 0));
        assert_eq!(
            resolve(&When::Epoch(1_000_000_000)).unwrap(),
            (1_000_000_000, 0)
        );
    }
}
