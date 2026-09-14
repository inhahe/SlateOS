//! date — print a date and time.
//!
//! ```text
//! date [-u] [-d SPEC | -r FILE | -f FILE] [-R | -I[SPEC] | --rfc-3339=SPEC | +FORMAT]
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
//! %c %C %d %D %e %F %g %G %h %H %I %j %k %l %m %M %n %N %p %P %q %r %R %s %S %t
//! %T %u %U %V %w %W %x %X %y %Y %z %Z %%` — against a [`localtime::Tm`] that
//! knows its zone. So this file decides *which instant*, *which zone* and
//! *which format string*, and hands all three to code that already works.
//!
//! The previous version carried its own `unix_secs_to_datetime`, which is the
//! fourth copy of that arithmetic this tree has had to remove.
//!
//! # The `-d` language: measured, then implemented, and still bounded
//!
//! This entry used to read "`-d` with anything but `@EPOCH` is refused", on the
//! reasoning that guessing at a date language would reintroduce the defect
//! above in a subtler form — a date that is plausible and wrong. That reasoning
//! is right and is why the language was **measured** before any of it was
//! written: `scripts/probe-date-d-grammar.sh` against GNU 9.4. Three of its
//! results contradict what a careful guess would have produced:
//!
//! * **`epoch` is not a keyword.** `date -d epoch` is an error.
//! * **`@0 + 1 day` is an error.** `@SECONDS` does not combine with relative
//!   items at all.
//! * **`-d ''` is not an error** — an empty or all-blank operand means *today
//!   at midnight*, while a bare time like `05:06:07` means *today at that
//!   time*. Two different rules that look like one.
//!
//! What is implemented is exactly what that probe confirmed: `@SECONDS`;
//! `YYYY-MM-DD` with an optional `T`/space time and an optional `UTC`/`±HHMM`
//! zone; spelled-out months in GNU's three orders; slashed dates, including the
//! US `MM/DD/YYYY` reading of a bare one; bare times; and `now`, `today`,
//! `tomorrow`, `yesterday`. **Anything else is still refused rather than
//! approximated** — GNU's full language also has `2 weeks ago`, `next Friday`
//! and much more, and those remain outside rather than being guessed at.
//!
//! Out-of-range components are *refused*, not normalised: `date -d 2021-03-32`
//! is an error even though the `mktime` underneath would carry it into April.
//!
//! # What is refused rather than approximated
//!
//! * **`-s`/`--set`**, which sets the system clock.
//! * **`--debug`, `--resolution`.**
//!
//! Each says so. The table below still carries all sixteen of GNU's long
//! options, because the table is what decides whether an abbreviation is
//! ambiguous — `uname` paid for that lesson, where a missing name made every
//! abbreviation of it resolve to some *other* option.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote_os, quotef_os};
use coreutils::stdfd;
use localtime::{Civil, Tm, Zone, strftime};
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
const ALIASES: &[(&str, &str)] = &[
    ("uct", "utc"),
    ("universal", "utc"),
    // `--rfc-822` and `--rfc-2822` are the obsolete spellings of `--rfc-email`
    // and share its `val`, so `--rfc` has only TWO possibilities in GNU, not
    // four. Measured: `date --rfc` answers
    //     option '--rfc' is ambiguous; possibilities: '--rfc-email' '--rfc-3339'
    // where a name-only table lists all four and is wrong in the same way
    // `--u` was before the two rows above were added.
    ("rfc-822", "rfc-email"),
    ("rfc-2822", "rfc-email"),
];

/// GNU's default output, measured: `Sun Sep  9 01:46:40 UTC 2001`.
const DEFAULT_FORMAT: &[u8] = b"%a %b %e %H:%M:%S %Z %Y";
/// `-R`, measured: `Sun, 09 Sep 2001 01:46:40 +0000`.
const RFC_EMAIL_FORMAT: &[u8] = b"%a, %d %b %Y %H:%M:%S %z";

/// Which instant to print.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum When {
    Now,
    /// `-d SPEC`, held UNPARSED.
    ///
    /// The language `-d` accepts resolves bare dates in the LOCAL zone, and
    /// the zone is not settled until every option has been read, because `-u`
    /// changes it. Parsing here would also report a bad date before a bad
    /// option, where GNU reports the option first.
    Spec(OsString),
    /// `-r FILE`: the file's modification time, as `(secs, nanos)`.
    File(OsString),
    /// `-f FILE`: one `-d` spec per line. `-` is stdin.
    Lines(OsString),
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

/// `-I`'s argument table, **in GNU's order**, which is the order the
/// "Valid arguments are:" list prints in — `hours` and `minutes` first, which
/// is gnulib's table order and not alphabetical or logical.
///
/// Measured: `date -Ibogus` lists hours, minutes, date, seconds, ns.
const ISO_PRECISIONS: &[(&str, Precision)] = &[
    ("hours", Precision::Hours),
    ("minutes", Precision::Minutes),
    ("date", Precision::Date),
    ("seconds", Precision::Seconds),
    ("ns", Precision::Ns),
];

/// `--rfc-3339`'s argument table. It does **not** accept `hours` or `minutes`;
/// `-I` does. Measured: `date --rfc-3339=bogus` lists only date, seconds, ns.
const RFC3339_PRECISIONS: &[(&str, Precision)] = &[
    ("date", Precision::Date),
    ("seconds", Precision::Seconds),
    ("ns", Precision::Ns),
];

/// Which option named the instant to print.
///
/// Tracked so a *second, different* one can be refused. Repeats of the same
/// option are fine and the last wins — measured, `date -d @0 -d @1` prints the
/// second — so this is about the kind, not the count.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DateSource {
    Date,
    Reference,
    Lines,
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
    let mut source: Option<DateSource> = None;

    for item in DATE.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item.map_err(|e| e.message())? {
            Opt::Long("utc" | "universal" | "uct", _) | Opt::Short(b'u', _) => cfg.utc = true,
            Opt::Long("rfc-email" | "rfc-822" | "rfc-2822", _) | Opt::Short(b'R', _) => {
                cfg.shape = Shape::RfcEmail;
            }
            Opt::Long("date", v) | Opt::Short(b'd', v) => {
                claim_source(&mut source, DateSource::Date)?;
                cfg.when = When::Spec(v.unwrap_or_default());
            }
            Opt::Long("reference", v) | Opt::Short(b'r', v) => {
                claim_source(&mut source, DateSource::Reference)?;
                cfg.when = When::File(v.unwrap_or_default());
            }
            Opt::Long("iso-8601", v) | Opt::Short(b'I', v) => {
                let spec = v.unwrap_or_else(|| OsString::from("date"));
                // `argmatch` rather than an exact match: GNU accepts any
                // unambiguous prefix here, so `-Isec` works, and it renders
                // the "Valid arguments are:" list from this same table. The
                // hand-rolled version did neither.
                let p = DATE
                    .argmatch(&os_bytes(&spec), "--iso-8601", ISO_PRECISIONS)
                    .map_err(|e| e.message())?;
                cfg.shape = Shape::Iso(p, b'T');
            }
            Opt::Long("rfc-3339", v) => {
                let spec = v.unwrap_or_default();
                let p = DATE
                    .argmatch(&os_bytes(&spec), "--rfc-3339", RFC3339_PRECISIONS)
                    .map_err(|e| e.message())?;
                cfg.shape = Shape::Iso(p, b' ');
            }
            Opt::Long("file", v) | Opt::Short(b'f', v) => {
                claim_source(&mut source, DateSource::Lines)?;
                cfg.when = When::Lines(v.unwrap_or_default());
            }
            // Refused rather than approximated -- see the module docs.
            Opt::Long(name @ ("set" | "debug" | "resolution"), _) => {
                return Err(DATE
                    .usage_referring(format!("option '--{name}' is not implemented"))
                    .message());
            }
            Opt::Short(b's', _) => {
                return Err(DATE
                    .usage_referring("option '-s' is not implemented".to_string())
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
                    // Three different messages, measured, and the difference
                    // is which of them the caller has already supplied:
                    //
                    //   date extra           -> invalid date ‘extra’
                    //   date +%F extra       -> extra operand ‘extra’
                    //   date -d @0 extra     -> the argument … lacks a leading '+'
                    //
                    // The middle one is "you have given me a format already";
                    // the last is "you have told me WHICH date, so a bare word
                    // cannot be one". With neither, a bare word is GNU's
                    // obsolete set-the-clock form and fails as a bad date.
                    if seen_format {
                        return Err(DATE
                            .usage_referring(format!("extra operand {}", quote_os(arg)))
                            .message());
                    }
                    if source.is_some() {
                        return Err(DATE
                            .usage_referring(format!(
                                "the argument {} lacks a leading '+';\n\
                                 when using an option to specify date(s), any non-option\n\
                                 argument must be a format string beginning with '+'",
                                quote_os(arg)
                            ))
                            .message());
                    }
                    // `usage`, not `usage_referring`: measured, this one
                    // carries no `Try 'date --help'` line.
                    return Err(DATE
                        .usage(format!("invalid date {}", quote_os(arg)))
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

/// Record which option named the instant, refusing a second *different* one.
///
/// `date -d @0 -r file` used to print the reference file's time and say
/// nothing, silently discarding one of two contradictory instructions — the
/// same "accepted and not honoured" shape as the flags surveyed in
/// `B-A-SURVEY-OF-FLAGS-WE-ACCEPT-AND-DO-NOT-HONOUR`. GNU refuses.
///
/// Repeats of the *same* option are not a conflict and the last wins,
/// measured: `date -d @0 -d @1` prints the second.
///
/// # Errors
///
/// A second date-source option of a different kind from the first.
fn claim_source(seen: &mut Option<DateSource>, now: DateSource) -> Result<(), String> {
    match seen {
        Some(prev) if *prev != now => Err(DATE
            .usage_referring(
                "the options to specify dates for printing are mutually exclusive".to_string(),
            )
            .message()),
        _ => {
            *seen = Some(now);
            Ok(())
        }
    }
}

/// `--help` and `--version` travel back through the error channel because they
/// are the only two non-error early exits, and a second success variant would
/// have to be threaded through every arm above to carry them.
const HELP_SENTINEL: &str = "\u{1}help";
const VERSION_SENTINEL: &str = "\u{1}version";

// ---------------------------------------------------------------------------
// The -d language
// ---------------------------------------------------------------------------
//
// Implemented against `scripts/probe-date-d-grammar.sh`, which measured GNU
// 9.4 rather than reading its documentation. Forms outside what that probe
// confirmed are REFUSED, not approximated — the module header explains why,
// and three measurements say the intuition to approximate from would have been
// wrong:
//
//   * `epoch` is NOT a keyword. `date -d epoch` is an error.
//   * `@0 + 1 day` is an error too: `@SECONDS` does not combine with relative
//     items at all.
//   * `-d ''` is not an error — an empty or all-blank string means TODAY AT
//     MIDNIGHT.

/// Month names, lowercased. GNU accepts the full name or a three-letter
/// prefix, so `Mar`, `mar` and `March` are one month and `Marc` is not.
const MONTH_NAMES: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn month_from_name(tok: &str) -> Option<i64> {
    let lower = tok.to_ascii_lowercase();
    MONTH_NAMES.iter().enumerate().find_map(|(i, name)| {
        (*name == lower || (lower.len() == 3 && name.starts_with(&lower)))
            .then(|| i64::try_from(i).unwrap_or(0).saturating_add(1))
    })
}

const fn is_leap(y: i64) -> bool {
    (y.rem_euclid(4) == 0 && y.rem_euclid(100) != 0) || y.rem_euclid(400) == 0
}

/// Length of a month, so an impossible day can be refused.
///
/// Needed because GNU *validates* rather than normalising here:
/// `date -d 2021-03-32` is an error, even though the `mktime` underneath would
/// happily carry it into April. `Zone::epoch` normalises, so without this the
/// 32nd of March would silently become the 1st of April.
const fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

fn num(tok: &str) -> Option<i64> {
    // No sign: a leading `-` here is a separator or an offset, never a number.
    tok.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| tok.parse().ok())
        .flatten()
}

/// `YYYY-MM-DD`, `YYYY/MM/DD`, or `MM/DD/YYYY`.
fn parse_ymd(tok: &str) -> Option<(i64, i64, i64)> {
    let parts: Vec<&str> = if tok.contains('-') {
        tok.split('-').collect()
    } else if tok.contains('/') {
        tok.split('/').collect()
    } else {
        return None;
    };
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    let (a, b, c) = (num(a)?, num(b)?, num(c)?);
    if tok.contains('-') {
        return Some((a, b, c));
    }
    // A slashed date is ambiguous and GNU resolves it by position: a bare
    // `03/04/2021` is MONTH/DAY/YEAR — US order, measured as 2021-03-04 — while
    // `2021/03/04` is year first. The four-digit component decides.
    if a > 31 {
        Some((a, b, c))
    } else {
        Some((c, a, b))
    }
}

/// `HH:MM` or `HH:MM:SS`, range-checked.
fn parse_hms(tok: &str) -> Option<(i64, i64, i64)> {
    let parts: Vec<&str> = tok.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [h, m] => (num(h)?, num(m)?, 0),
        [h, m, s] => (num(h)?, num(m)?, num(s)?),
        _ => return None,
    };
    // Measured: `date -d 25:00:00` is an error, so this is validation rather
    // than normalisation. 60 is allowed for a leap second, as C's tm_sec is.
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) || !(0..=60).contains(&s) {
        return None;
    }
    Some((h, m, s))
}

/// `UTC`, `GMT`, `Z`, or `±HHMM` / `±HH:MM`, as seconds east of Greenwich.
fn parse_zone_token(tok: &str) -> Option<i64> {
    let upper = tok.to_ascii_uppercase();
    if matches!(upper.as_str(), "UTC" | "GMT" | "Z") {
        return Some(0);
    }
    let (sign, rest) = match tok.as_bytes().first()? {
        b'+' => (1i64, tok.get(1..)?),
        b'-' => (-1i64, tok.get(1..)?),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if digits.len() != 4 {
        return None;
    }
    let hh = num(digits.get(..2)?)?;
    let mm = num(digits.get(2..)?)?;
    if hh > 23 || mm > 59 {
        return None;
    }
    Some(
        sign.saturating_mul(
            hh.saturating_mul(3600)
                .saturating_add(mm.saturating_mul(60)),
        ),
    )
}

/// `now`, `today`, `tomorrow`, `yesterday` — and nothing else.
///
/// `tomorrow` moves the CALENDAR day rather than adding 86 400 seconds. The
/// two differ across a daylight-saving change, and GNU does the calendar one
/// (it adds to `tm_mday` and calls `mktime`), so this does too.
fn keyword_instant(tok: &str, zone: &Zone, now: i64) -> Option<i64> {
    let shift = match tok.to_ascii_lowercase().as_str() {
        // Measured: `today` is NOT midnight, it is the current instant, the
        // same as `now`.
        "now" | "today" => return Some(now),
        "tomorrow" => 1,
        "yesterday" => -1,
        _ => return None,
    };
    let tm = zone.local(now, 0);
    Some(
        zone.epoch(&Civil {
            year: tm.year,
            month: i64::from(tm.month),
            day: i64::from(tm.day).saturating_add(shift),
            hour: i64::from(tm.hour),
            minute: i64::from(tm.minute),
            second: i64::from(tm.second),
        })
        .0,
    )
}

/// Resolve a `-d` operand to an instant, or `None` if it is not a date.
///
/// `now` is passed in rather than read here so the clock-relative forms are
/// testable: every keyword and the bare-time form depend on it.
fn parse_date_spec(raw: &[u8], zone: &Zone, now: i64) -> Option<i64> {
    let text = std::str::from_utf8(raw).ok()?;
    // A trailing comma is dropped so `March 4, 2021` tokenises like the others.
    let toks: Vec<&str> = text
        .split_whitespace()
        .map(|t| t.trim_end_matches(','))
        .filter(|t| !t.is_empty())
        .collect();

    // Empty or all blank: today at midnight, local.
    if toks.is_empty() {
        let tm = zone.local(now, 0);
        return Some(
            zone.epoch(&Civil {
                year: tm.year,
                month: i64::from(tm.month),
                day: i64::from(tm.day),
                ..Civil::default()
            })
            .0,
        );
    }

    if let [only] = toks.as_slice() {
        if let Some(t) = keyword_instant(only, zone, now) {
            return Some(t);
        }
        // `@SECONDS`, and only as the whole operand: `@0 + 1 day` is an error
        // in GNU, so this deliberately does not survive into the token loop.
        if let Some(rest) = only.strip_prefix('@') {
            return rest.parse::<i64>().ok();
        }
    }

    let mut ymd: Option<(i64, i64, i64)> = None;
    let mut hms: Option<(i64, i64, i64)> = None;
    let mut named_month: Option<i64> = None;
    let mut offset: Option<i64> = None;
    let mut bare: Vec<i64> = Vec::new();

    for tok in &toks {
        // `2021-03-04T05:06:07` carries both halves in one token.
        if let Some((d, t)) = tok.split_once('T')
            && !d.is_empty()
            && !t.is_empty()
            && let Some(date) = parse_ymd(d)
            && let Some(time) = parse_hms(t)
        {
            if ymd.is_some() || hms.is_some() {
                return None;
            }
            ymd = Some(date);
            hms = Some(time);
            continue;
        }
        if let Some(v) = parse_ymd(tok) {
            if ymd.replace(v).is_some() {
                return None;
            }
        } else if let Some(v) = parse_hms(tok) {
            if hms.replace(v).is_some() {
                return None;
            }
        } else if let Some(m) = month_from_name(tok) {
            if named_month.replace(m).is_some() {
                return None;
            }
        } else if let Some(z) = parse_zone_token(tok) {
            if offset.replace(z).is_some() {
                return None;
            }
        } else if let Some(n) = num(tok) {
            bare.push(n);
        } else {
            return None;
        }
    }

    let (year, month, day) = match (ymd, named_month) {
        // Both a numeric date and a month name is not a form GNU has.
        (Some(_), Some(_)) => return None,
        (Some(v), None) if bare.is_empty() => v,
        (Some(_), None) => return None,
        (None, Some(m)) => {
            // `Mar 4 2021` and `4 March 2021` differ only in order, and the
            // year is the component that cannot be a day.
            let [a, b] = bare.as_slice() else {
                return None;
            };
            if *a > 31 { (*a, m, *b) } else { (*b, m, *a) }
        }
        // A time with no date means today, local — measured.
        (None, None) if bare.is_empty() && hms.is_some() => {
            let tm = zone.local(now, 0);
            (tm.year, i64::from(tm.month), i64::from(tm.day))
        }
        (None, None) => return None,
    };

    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let (hour, minute, second) = hms.unwrap_or((0, 0, 0));

    match offset {
        // An explicit offset makes the wall clock absolute, so the local zone
        // must not be consulted at all: convert as if UTC, then step back by
        // the offset.
        Some(z) => Some(
            localtime::days_from_civil(year, month, day)
                .saturating_mul(86_400)
                .saturating_add(hour.saturating_mul(3_600))
                .saturating_add(minute.saturating_mul(60))
                .saturating_add(second)
                .saturating_sub(z),
        ),
        None => Some(
            zone.epoch(&Civil {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
            .0,
        ),
    }
}

/// The instant a [`When`] names, as `(seconds, nanoseconds)`.
fn resolve(when: &When, zone: &Zone) -> Result<(i64, u32), String> {
    match when {
        When::Now => {
            let d = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "date: the system clock is before the epoch".to_string())?;
            let secs = i64::try_from(d.as_secs())
                .map_err(|_| "date: the system clock is out of range".to_string())?;
            Ok((secs, d.subsec_nanos()))
        }
        When::Spec(raw) => {
            // `now` is read once and handed to the parser, so that every
            // clock-relative form in one operand agrees about what "now" is.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "date: the system clock is before the epoch".to_string())
                .and_then(|d| {
                    i64::try_from(d.as_secs())
                        .map_err(|_| "date: the system clock is out of range".to_string())
                })?;
            parse_date_spec(&os_bytes(raw), zone, now)
                .map(|s| (s, 0))
                // GNU's wording exactly, and with no `Try '… --help'` referral
                // -- measured.
                .ok_or_else(|| format!("date: invalid date {}", quote_os(raw)))
        }
        // `-f` produces MANY instants, so `main` handles it before reaching
        // here. This arm returns an error rather than panicking: an
        // unreachable branch that a later edit makes reachable should
        // misbehave visibly and recoverably, not abort the process.
        When::Lines(_) => Err("date: internal: -f is resolved per line".to_string()),
        When::File(path) => {
            let meta = fs::metadata(path)
                .map_err(|e| format!("date: {}: {}", quotef_os(path), strerror(&e)))?;
            let mtime = meta
                .modified()
                .map_err(|e| format!("date: {}: {}", quotef_os(path), strerror(&e)))?;
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
            // `date: `, because every error reaching here came from
            // `parse_args`/`parse_when` as a bare `Error::message()`, which is
            // the sentence *without* the program name — `Program::report` is
            // the thing that normally supplies it, and converting to `String`
            // early skips it. Five harness cases were a missing prefix alone:
            // `invalid option -- 'Q'` against GNU's `date: invalid option --
            // 'Q'`, and the same for `--nosuchoption`, `--set`, `-r` and `-f`.
            //
            // Not applied at the `resolve` site below: those messages build
            // their own prefix (`date: the system clock is before the epoch`),
            // and adding one here would double it.
            diag!("date: {}", m);
            return ExitCode::FAILURE;
        }
    };

    // The zone is settled first: `-d` resolves bare dates in it, so it is an
    // input to resolution and not merely to rendering.
    let zone = if cfg.utc {
        Zone::utc()
    } else {
        Zone::from_env()
    };

    // Written as bytes: a format string may contain any byte, and a `%` that
    // `strftime` does not recognise is passed through unchanged, so the result
    // is not necessarily UTF-8 even when the format was.
    use std::io::Write;
    let format = format_for(&cfg.shape);
    let mut stdout = std::io::stdout().lock();
    let emit = |secs: i64, nanos: u32, out: &mut std::io::StdoutLock<'_>| -> bool {
        let tm: Tm = zone.local(secs, nanos);
        out.write_all(&strftime(&format, &tm))
            .and_then(|()| out.write_all(b"\n"))
            .is_ok()
    };

    if let When::Lines(path) = &cfg.when {
        let text = match read_date_lines(path) {
            Ok(t) => t,
            Err(m) => {
                diag!("{}", m);
                return ExitCode::FAILURE;
            }
        };
        let now = match clock_now() {
            Ok(n) => n,
            Err(m) => {
                diag!("{}", m);
                return ExitCode::FAILURE;
            }
        };
        // A bad line is reported and the rest are still processed, with the
        // failure carried to the exit status. Measured: GNU prints the good
        // lines either side of a bad one and still exits 1.
        let mut worst = ExitCode::SUCCESS;
        // `split(b'\n')` rather than `lines()`, and the trailing empty piece
        // is dropped: a file ending in a newline must not gain a blank final
        // line, but a file NOT ending in one must still have its last line
        // read. Measured, both ways.
        let mut pieces: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
        if pieces.last().is_some_and(|p| p.is_empty()) {
            pieces.pop();
        }
        for line in pieces {
            match parse_date_spec(line, &zone, now) {
                Some(secs) => {
                    if !emit(secs, 0, &mut stdout) {
                        return ExitCode::FAILURE;
                    }
                }
                None => {
                    // `quote` on the raw BYTES, not a decoded string: a line of a
                    // file is arbitrary bytes, and it escapes control
                    // characters, so a newline inside one cannot forge a
                    // second diagnostic line.
                    diag!("date: invalid date {}", coreutils::quote::quote(line));
                    worst = ExitCode::FAILURE;
                }
            }
        }
        return worst;
    }

    let (secs, nanos) = match resolve(&cfg.when, &zone) {
        Ok(v) => v,
        Err(m) => {
            diag!("{}", m);
            return ExitCode::FAILURE;
        }
    };
    if !emit(secs, nanos, &mut stdout) {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// The bytes of `-f`'s file, or stdin when it is `-`.
///
/// # Errors
///
/// The file not being readable, reported the way GNU reports it.
fn read_date_lines(path: &OsString) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if os_bytes(path).as_ref() == b"-" {
        let mut buf = Vec::new();
        return std::io::stdin()
            .lock()
            .read_to_end(&mut buf)
            .map(|_| buf)
            .map_err(|e| format!("date: -: {}", strerror(&e)));
    }
    fs::read(path).map_err(|e| format!("date: {}: {}", quotef_os(path), strerror(&e)))
}

/// The current time as epoch seconds.
///
/// # Errors
///
/// A clock outside the representable range.
fn clock_now() -> Result<i64, String> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "date: the system clock is before the epoch".to_string())?;
    i64::try_from(d.as_secs()).map_err(|_| "date: the system clock is out of range".to_string())
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
        // seconds and ns. Asserted against the tables `argmatch` is driven by,
        // so the rule and the thing enforcing it cannot drift apart.
        let iso: Vec<&str> = ISO_PRECISIONS.iter().map(|&(n, _)| n).collect();
        let rfc: Vec<&str> = RFC3339_PRECISIONS.iter().map(|&(n, _)| n).collect();
        // The ORDER is not cosmetic: it is the order GNU prints in its
        // "Valid arguments are:" list, measured from `date -Ibogus`, and it is
        // neither alphabetical nor logical.
        assert_eq!(iso, ["hours", "minutes", "date", "seconds", "ns"]);
        assert_eq!(rfc, ["date", "seconds", "ns"]);

        assert!(parse_args(&[os("-Ihours")]).is_ok());
        assert!(parse_args(&[os("--rfc-3339=hours")]).is_err());
        assert!(parse_args(&[os("--rfc-3339=seconds")]).is_ok());
    }

    #[test]
    fn precision_arguments_accept_an_unambiguous_prefix() {
        // GNU's argmatch does prefix matching: `date -Isec` works. The
        // hand-rolled exact match this replaced refused it.
        assert!(parse_args(&[os("-Isec")]).is_ok());
        assert!(parse_args(&[os("--rfc-3339=sec")]).is_ok());
        // `s` is ambiguous between `seconds` and nothing else in the 3339
        // table, but in the ISO table it is unambiguous too; `n` vs `ns`
        // likewise. The interesting refusal is a word matching nothing.
        assert!(parse_args(&[os("-Ibogus")]).is_err());
    }

    #[test]
    fn date_sources_are_mutually_exclusive_but_repeats_are_not() {
        // Measured: `date -d @0 -r file` refuses, while `date -d @0 -d @1`
        // takes the second. Before this, the first case silently printed the
        // reference file's time and discarded the -d.
        let e = parse_args(&[os("-d"), os("@0"), os("-r"), os("f")]).unwrap_err();
        assert!(e.contains("mutually exclusive"), "{e}");
        let e = parse_args(&[os("-r"), os("f"), os("-d"), os("@0")]).unwrap_err();
        assert!(e.contains("mutually exclusive"), "{e}");

        let cfg = parse_args(&[os("-d"), os("@0"), os("-d"), os("@1")]).unwrap();
        assert_eq!(cfg.when, When::Spec(os("@1")));
    }

    #[test]
    fn a_bare_operand_gets_one_of_three_measured_messages() {
        // Which one depends on what the caller has already supplied.
        let e = parse_args(&[os("extra")]).unwrap_err();
        assert!(e.contains("invalid date"), "{e}");
        // ...and that one carries no `Try …` referral.
        assert!(!e.contains("--help"), "{e}");

        let e = parse_args(&[os("+%F"), os("extra")]).unwrap_err();
        assert!(e.contains("extra operand"), "{e}");

        let e = parse_args(&[os("-d"), os("@0"), os("extra")]).unwrap_err();
        assert!(e.contains("lacks a leading '+'"), "{e}");
        assert!(
            e.contains("must be a format string beginning with '+'"),
            "{e}"
        );
    }

    /// A pinned "now": 2021-06-15 12:00:00 UTC.
    ///
    /// Pinned rather than read from the clock so the keyword and bare-time
    /// forms are deterministic. `scripts/date-diff.sh` cannot test those at
    /// all -- both sides read their own clock and race -- so this is the only
    /// place `now`, `tomorrow` and `05:06:07` get checked against a known
    /// answer rather than against whatever the two processes happened to see.
    const TEST_NOW: i64 = 1_623_758_400;

    fn spec(s: &str) -> Option<i64> {
        parse_date_spec(s.as_bytes(), &Zone::utc(), TEST_NOW)
    }

    #[test]
    fn d_accepts_the_measured_absolute_forms() {
        // Every expected value here came from GNU 9.4 via
        // scripts/probe-date-d-grammar.sh, not from computing what it ought
        // to be with the same arithmetic the code under test uses.
        assert_eq!(spec("@0"), Some(0));
        assert_eq!(spec("@1000000000"), Some(1_000_000_000));
        assert_eq!(spec("@-1"), Some(-1));

        assert_eq!(spec("2021-03-04 05:06:07"), Some(1_614_834_367));
        assert_eq!(spec("2021-03-04T05:06:07"), Some(1_614_834_367));
        assert_eq!(spec("2021-03-04"), Some(1_614_816_000));
        assert_eq!(spec("1970-01-01 00:00:00 UTC"), Some(0));

        // Spelled-out months, in all three orders GNU accepts.
        assert_eq!(spec("Mar 4 2021"), Some(1_614_816_000));
        assert_eq!(spec("4 March 2021"), Some(1_614_816_000));
        assert_eq!(spec("March 4, 2021"), Some(1_614_816_000));

        // Slashes: year-first, and the US month/day order for a bare one.
        assert_eq!(spec("2021/03/04"), Some(1_614_816_000));
        assert_eq!(spec("03/04/2021"), Some(1_614_816_000));
    }

    #[test]
    fn d_applies_an_explicit_zone_offset() {
        // +0200 means the wall clock is two hours AHEAD, so the instant is two
        // hours EARLIER -- the sign that is easy to invert and that the
        // measured value pins down.
        assert_eq!(spec("2021-03-04 05:06:07 UTC"), Some(1_614_834_367));
        assert_eq!(spec("2021-03-04 05:06:07 +0200"), Some(1_614_827_167));
        assert_eq!(spec("2021-03-04 05:06:07 -0500"), Some(1_614_852_367));
        assert_eq!(
            spec("2021-03-04 05:06:07 +0200"),
            spec("2021-03-04 05:06:07 +02:00")
        );
    }

    #[test]
    fn d_clock_relative_forms() {
        assert_eq!(spec("now"), Some(TEST_NOW));
        // Measured: `today` is the current INSTANT, not midnight.
        assert_eq!(spec("today"), Some(TEST_NOW));
        assert_eq!(spec("tomorrow"), Some(TEST_NOW + 86_400));
        assert_eq!(spec("yesterday"), Some(TEST_NOW - 86_400));
        assert_eq!(spec("TOMORROW"), Some(TEST_NOW + 86_400));

        // A bare time means TODAY at that time; an empty operand means today
        // at MIDNIGHT. Both measured, and they are different rules.
        assert_eq!(spec("05:06:07"), Some(1_623_733_567));
        assert_eq!(spec(""), Some(1_623_715_200));
        assert_eq!(spec("   "), Some(1_623_715_200));
    }

    #[test]
    fn d_refuses_what_gnu_refuses() {
        // These three are the surprises, and each is a harness case:
        //   `epoch` is not a keyword, `@N` does not combine with relative
        //   items, and a plain sentence is not a date.
        assert_eq!(spec("epoch"), None);
        assert_eq!(spec("@0 + 1 day"), None);
        assert_eq!(spec("not a date at all"), None);

        // Out-of-range components are REFUSED, not normalised -- the reason
        // days_in_month exists, since the mktime underneath would carry them.
        assert_eq!(spec("2021-13-04"), None);
        assert_eq!(spec("2021-03-32"), None);
        assert_eq!(spec("2021-02-29"), None, "2021 is not a leap year");
        assert_eq!(spec("2020-02-29"), Some(1_582_934_400), "2020 is");
        assert_eq!(spec("25:00:00"), None);
        assert_eq!(spec("05:60:00"), None);

        assert_eq!(spec("@"), None);
        assert_eq!(spec("@notanumber"), None);
        assert_eq!(spec("2021-03-04 2022-03-04"), None, "two dates");
        assert_eq!(spec("05:06:07 08:09:10"), None, "two times");
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
        // `--file` left this list on 2026-09-14: it is implemented now, and
        // this test is what said so out loud rather than letting the old
        // refusal sit behind a working option.
        for a in ["--set=x", "--debug", "--resolution"] {
            let e = parse_args(&[os(a)]).unwrap_err();
            assert!(e.contains("not implemented"), "{a} gave {e}");
        }
        let cfg = parse_args(&[os("--file=x")]).unwrap();
        assert_eq!(cfg.when, When::Lines(os("x")));
    }

    #[test]
    fn f_is_a_date_source_and_collides_with_the_others() {
        // Measured: all three pairings refuse, in both orders.
        for args in [
            vec![os("-f"), os("a"), os("-d"), os("@0")],
            vec![os("-d"), os("@0"), os("-f"), os("a")],
            vec![os("-f"), os("a"), os("-r"), os("b")],
        ] {
            let e = parse_args(&args).unwrap_err();
            assert!(e.contains("mutually exclusive"), "{args:?} gave {e}");
        }
    }

    #[test]
    fn epoch_resolves_without_consulting_the_clock() {
        // The whole defect this file existed to fix: -d @0 must not be "now".
        let utc = Zone::utc();
        assert_eq!(resolve(&When::Spec(os("@0")), &utc).unwrap(), (0, 0));
        assert_eq!(
            resolve(&When::Spec(os("@1000000000")), &utc).unwrap(),
            (1_000_000_000, 0)
        );
    }
}
