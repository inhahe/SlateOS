//! date — print or set the system date and time.
//!
//! ```text
//! date [OPTION]... [+FORMAT]
//! date [-u|--utc|--universal] [MMDDhhmm[[CC]YY][.ss]]
//! ```
//!
//! A port of GNU coreutils 9.4's `date.c`: its `main`, `batch_convert` and
//! `show_date`, in that order below, with the same decisions made in the same
//! order — which matters, because several of them are errors, and which error
//! a bad command line gets is decided by which check runs first.
//!
//! # Where the instant comes from
//!
//! | source | how |
//! |---|---|
//! | nothing | the clock |
//! | `-d STRING`, `-s STRING` | [`coreutils::parse_datetime`], GNU's own parser |
//! | `-f FILE` | the same, once per line, printing as it goes |
//! | `-r FILE` | the file's modification time, nanoseconds included |
//! | `--resolution` | the clock's resolution, as an instant |
//! | an operand that is not `+FORMAT` | POSIX's `MMDDhhmm[[CC]YY][.ss]`, which also *sets* the clock |
//!
//! `-d` used to be a hand-written subset of GNU's date language, grown form by
//! form against `scripts/probe-date-d-grammar.sh` and refusing what it had not
//! measured. It was careful and it was still a different language: it could
//! not say `12:30 -5`, which GNU reads as half past twelve at UTC-5, because
//! the rule that decides that is a shift/reduce conflict in GNU's Bison
//! grammar and not a rule anyone wrote down. `parse_datetime` is that grammar,
//! so the question of which forms are implemented no longer arises.
//!
//! # `--debug`
//!
//! Upstream's parser explains itself under `--debug` — each part it
//! recognised, the zone it read the string in, every warning about
//! questionable input — and `date` adds the output format and a note when
//! repeated `-d` or `-s` options were discarded. All of it is here.
//!
//! # `-s`
//!
//! Sets the clock through `clock_settime`, falling back to `settimeofday` as
//! gnulib's `settime` does, and prints the date either way. Without the
//! privilege to set it, that is `cannot set date: Operation not permitted` and
//! status 1 — the same as GNU for anyone but root.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::parse_datetime::{Timespec, parse_datetime2};
use coreutils::posixtm::{self, Syntax};
use coreutils::quote::{os_bytes, quote, quote_os, quotef_os};
use coreutils::stdfd;
use localtime::{Zone, nstrftime_z};
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

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

/// The long names that are ONE option wearing several spellings.
///
/// Named `ALIASES` and not `LONG_ALIASES`: `scripts/getopt-ambiguity-check.py`
/// matches the former exactly, so a table declared under the other spelling is
/// invisible to it.
///
/// GNU's `struct option` carries a `val`, and `getopt_long` judges ambiguity by
/// that rather than by the name: two spellings sharing a `val` are one option,
/// so a prefix matching both of them RESOLVES instead of failing. `--uct`,
/// `--utc` and `--universal` share one, which is why `date --u` works in GNU,
/// and `--rfc-822` and `--rfc-2822` share `--rfc-email`'s, which is why `date
/// --rfc` lists only two possibilities.
const ALIASES: &[(&str, &str)] = &[
    ("uct", "utc"),
    ("universal", "utc"),
    ("rfc-822", "rfc-email"),
    ("rfc-2822", "rfc-email"),
];

/// `nl_langinfo (_DATE_FMT)` in the C locale, which is what GNU prints with no
/// format: `Sun Sep  9 01:46:40 UTC 2001`.
const DEFAULT_FORMAT: &[u8] = b"%a %b %e %H:%M:%S %Z %Y";
/// `-R`, measured: `Sun, 09 Sep 2001 01:46:40 +0000`.
const RFC_EMAIL_FORMAT: &[u8] = b"%a, %d %b %Y %H:%M:%S %z";
/// `--resolution`'s default format.
const RESOLUTION_FORMAT: &[u8] = b"%s.%N";

/// How much of the time `-I`/`--rfc-3339` prints: upstream's `Time_spec`.
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
/// "Valid arguments are:" list prints in — `hours` and `minutes` first, since
/// they are not valid for `--rfc-3339`, which takes the rest of the table.
const ISO_PRECISIONS: &[(&str, Precision)] = &[
    ("hours", Precision::Hours),
    ("minutes", Precision::Minutes),
    ("date", Precision::Date),
    ("seconds", Precision::Seconds),
    ("ns", Precision::Ns),
];

/// `--rfc-3339`'s argument table: `time_spec_string + 2`.
const RFC3339_PRECISIONS: &[(&str, Precision)] = &[
    ("date", Precision::Date),
    ("seconds", Precision::Seconds),
    ("ns", Precision::Ns),
];

/// Upstream's `iso_8601_format[]`: the date, then `T`, then the time to the
/// precision asked for, then the zone with a colon.
fn iso_8601_format(p: Precision) -> &'static [u8] {
    match p {
        Precision::Date => b"%Y-%m-%d",
        Precision::Seconds => b"%Y-%m-%dT%H:%M:%S%:z",
        Precision::Ns => b"%Y-%m-%dT%H:%M:%S,%N%:z",
        Precision::Hours => b"%Y-%m-%dT%H%:z",
        Precision::Minutes => b"%Y-%m-%dT%H:%M%:z",
    }
}

/// Upstream's `rfc_3339_format[]`: a space instead of the `T`, and a full stop
/// before the nanoseconds.
fn rfc_3339_format(p: Precision) -> &'static [u8] {
    match p {
        Precision::Seconds => b"%Y-%m-%d %H:%M:%S%:z",
        Precision::Ns => b"%Y-%m-%d %H:%M:%S.%N%:z",
        // `hours` and `minutes` are not in `--rfc-3339`'s table.
        _ => b"%Y-%m-%d",
    }
}

/// `date`'s command line, as upstream's `main` holds it once `getopt_long` is
/// done: which options were seen, and what they said.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[derive(Default)]
struct Config {
    /// `-d`: the last one.
    datestr: Option<OsString>,
    /// More than one `-d` was given.
    discarded_datestr: bool,
    /// `-s`: the last one.
    set_datestr: Option<OsString>,
    /// More than one `-s` was given.
    discarded_set_datestr: bool,
    /// `-f`.
    batch_file: Option<OsString>,
    /// `-r`.
    reference: Option<OsString>,
    /// `--resolution`.
    get_resolution: bool,
    /// `--debug`.
    debug: bool,
    /// `-u`, which upstream implements as `putenv ("TZ=UTC0")`.
    utc: bool,
    /// `-I`, `-R`, `--rfc-3339` or `+FORMAT`, of which there may be one.
    format: Option<Vec<u8>>,
    /// The operands, in order.
    operands: Vec<OsString>,
}

/// What the command line asks for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Config),
}

/// A command-line error: the message, printed after `date: `, and the exit.
/// Every one of `date`'s exits 1.
type Failure = String;

fn help_text() -> &'static str {
    "\
Usage: date [OPTION]... [+FORMAT]
  or:  date [-u|--utc|--universal] [MMDDhhmm[[CC]YY][.ss]]
Display date and time in the given FORMAT.
With -s, or with [MMDDhhmm[[CC]YY][.ss]], set the date and time.

Mandatory arguments to long options are mandatory for short options too.
  -d, --date=STRING          display time described by STRING, not 'now'
      --debug                annotate the parsed date,
                              and warn about questionable usage to stderr
  -f, --file=DATEFILE        like --date; once for each line of DATEFILE
  -I[FMT], --iso-8601[=FMT]  output date/time in ISO 8601 format.
                               FMT='date' for date only (the default),
                               'hours', 'minutes', 'seconds', or 'ns'
                               for date and time to the indicated precision.
                               Example: 2006-08-14T02:34:56-06:00
  --resolution               output the available resolution of timestamps
                               Example: 0.000000001
  -R, --rfc-email            output date and time in RFC 5322 format.
                               Example: Mon, 14 Aug 2006 02:34:56 -0600
      --rfc-3339=FMT         output date/time in RFC 3339 format.
                               FMT='date', 'seconds', or 'ns'
                               for date and time to the indicated precision.
                               Example: 2006-08-14 02:34:56-06:00
  -r, --reference=FILE       display the last modification time of FILE
  -s, --set=STRING           set time described by STRING
  -u, --utc, --universal     print or set Coordinated Universal Time (UTC)
      --help        display this help and exit
      --version     output version information and exit

All options that specify the date to display are mutually exclusive.
I.e.: --date, --file, --reference, --resolution.

FORMAT controls the output.  Interpreted sequences are:

  %%   a literal %
  %a   locale's abbreviated weekday name (e.g., Sun)
  %A   locale's full weekday name (e.g., Sunday)
  %b   locale's abbreviated month name (e.g., Jan)
  %B   locale's full month name (e.g., January)
  %c   locale's date and time (e.g., Thu Mar  3 23:05:25 2005)
  %C   century; like %Y, except omit last two digits (e.g., 20)
  %d   day of month (e.g., 01)
  %D   date; same as %m/%d/%y
  %e   day of month, space padded; same as %_d
  %F   full date; like %+4Y-%m-%d
  %g   last two digits of year of ISO week number (see %G)
  %G   year of ISO week number (see %V); normally useful only with %V
  %h   same as %b
  %H   hour (00..23)
  %I   hour (01..12)
  %j   day of year (001..366)
  %k   hour, space padded ( 0..23); same as %_H
  %l   hour, space padded ( 1..12); same as %_I
  %m   month (01..12)
  %M   minute (00..59)
  %n   a newline
  %N   nanoseconds (000000000..999999999)
  %p   locale's equivalent of either AM or PM; blank if not known
  %P   like %p, but lower case
  %q   quarter of year (1..4)
  %r   locale's 12-hour clock time (e.g., 11:11:04 PM)
  %R   24-hour hour and minute; same as %H:%M
  %s   seconds since the Epoch (1970-01-01 00:00 UTC)
  %S   second (00..60)
  %t   a tab
  %T   time; same as %H:%M:%S
  %u   day of week (1..7); 1 is Monday
  %U   week number of year, with Sunday as first day of week (00..53)
  %V   ISO week number, with Monday as first day of week (01..53)
  %w   day of week (0..6); 0 is Sunday
  %W   week number of year, with Monday as first day of week (00..53)
  %x   locale's date representation (e.g., 12/31/99)
  %X   locale's time representation (e.g., 23:13:48)
  %y   last two digits of year (00..99)
  %Y   year
  %z   +hhmm numeric time zone (e.g., -0400)
  %:z  +hh:mm numeric time zone (e.g., -04:00)
  %::z  +hh:mm:ss numeric time zone (e.g., -04:00:00)
  %:::z  numeric time zone with : to necessary precision (e.g., -04, +05:30)
  %Z   alphabetic time zone abbreviation (e.g., EDT)

By default, date pads numeric fields with zeroes.
The following optional flags may follow '%':

  -  (hyphen) do not pad the field
  _  (underscore) pad with spaces
  0  (zero) pad with zeros
  +  pad with zeros, and put '+' before future years with >4 digits
  ^  use upper case if possible
  #  use opposite case if possible

After any flags comes an optional field width, as a decimal number;
then an optional modifier, which is either
E to use the locale's alternate representations if available, or
O to use the locale's alternate numeric symbols if available.

Examples:
Convert seconds since the Epoch (1970-01-01 UTC) to a date
  $ date --date='@2147483647'

Show the time on the west coast of the US (use tzselect(1) to find TZ)
  $ TZ='America/Los_Angeles' date

Show the local time for 9AM next Friday on the west coast of the US
  $ date --date='TZ=\"America/Los_Angeles\" 09:00 next Fri'
"
}

/// Upstream's `getopt_long` loop.
///
/// Only what upstream decides *inside* the loop is decided here: getopt's own
/// errors, an `-I`/`--rfc-3339` argument that is not in its table, and a
/// second output format. Everything else — conflicting options, operands —
/// is [`check_options`], which runs after the loop as upstream's does, so
/// that `date -d x -r y --bogus` reports `--bogus`.
///
/// # Errors
///
/// Those three, as the message to print after `date: `.
fn parse_args(args: &[OsString]) -> Result<Request, Failure> {
    let mut cfg = Config::default();
    for item in DATE.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        let mut new_format: Option<Vec<u8>> = None;
        match item.map_err(|e| e.message())? {
            Opt::Long("date", v) | Opt::Short(b'd', v) => {
                cfg.discarded_datestr |= cfg.datestr.is_some();
                cfg.datestr = Some(v.unwrap_or_default());
            }
            Opt::Long("debug", _) => cfg.debug = true,
            Opt::Long("file", v) | Opt::Short(b'f', v) => {
                cfg.batch_file = Some(v.unwrap_or_default());
            }
            Opt::Long("resolution", _) => cfg.get_resolution = true,
            Opt::Long("rfc-3339", v) => {
                let spec = v.unwrap_or_default();
                let p = DATE
                    .argmatch(&os_bytes(&spec), "--rfc-3339", RFC3339_PRECISIONS)
                    .map_err(|e| e.message())?;
                new_format = Some(rfc_3339_format(p).to_vec());
            }
            Opt::Long("iso-8601", v) | Opt::Short(b'I', v) => {
                let p = match v {
                    Some(spec) => DATE
                        .argmatch(&os_bytes(&spec), "--iso-8601", ISO_PRECISIONS)
                        .map_err(|e| e.message())?,
                    None => Precision::Date,
                };
                new_format = Some(iso_8601_format(p).to_vec());
            }
            Opt::Long("reference", v) | Opt::Short(b'r', v) => {
                cfg.reference = Some(v.unwrap_or_default());
            }
            Opt::Long("rfc-email" | "rfc-822" | "rfc-2822", _) | Opt::Short(b'R', _) => {
                new_format = Some(RFC_EMAIL_FORMAT.to_vec());
            }
            Opt::Long("set", v) | Opt::Short(b's', v) => {
                cfg.discarded_set_datestr |= cfg.set_datestr.is_some();
                cfg.set_datestr = Some(v.unwrap_or_default());
            }
            Opt::Long("utc" | "universal" | "uct", _) | Opt::Short(b'u', _) => cfg.utc = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long(other, _) => {
                return Err(DATE
                    .usage_referring(format!("option '--{other}' is unhandled"))
                    .message());
            }
            Opt::Short(other, _) => return Err(DATE.invalid_option(other).message()),
            Opt::Operand(arg) => cfg.operands.push(arg.clone()),
        }
        if let Some(f) = new_format {
            if cfg.format.is_some() {
                // `error (EXIT_FAILURE, …)`: no referral.
                return Err(DATE
                    .usage("multiple output formats specified".to_string())
                    .message());
            }
            cfg.format = Some(f);
        }
    }
    Ok(Request::Run(cfg))
}

/// What upstream decides between the option loop and the work: which options
/// conflict, what the operands are, and the `--debug` notes about discarded
/// options. Returns the `-s`-style operand, if the one operand is that.
///
/// # Errors
///
/// A conflict or a bad operand, as the message to print after `date: `.
fn check_options(cfg: &mut Config) -> Result<Option<OsString>, Failure> {
    let option_specified_date = [
        cfg.datestr.is_some(),
        cfg.batch_file.is_some(),
        cfg.reference.is_some(),
        cfg.get_resolution,
    ]
    .iter()
    .filter(|&&given| given)
    .count();
    if option_specified_date > 1 {
        return Err(DATE
            .usage_referring(
                "the options to specify dates for printing are mutually exclusive".to_string(),
            )
            .message());
    }
    let set_date = cfg.set_datestr.is_some();
    if set_date && option_specified_date > 0 {
        return Err(DATE
            .usage_referring(
                "the options to print and set the time may not be used together".to_string(),
            )
            .message());
    }

    if cfg.discarded_datestr && cfg.debug {
        diag!("date: only using last of multiple -d options");
    }
    if cfg.discarded_set_datestr && cfg.debug {
        diag!("date: only using last of multiple -s options");
    }

    let mut operands = cfg.operands.iter();
    let Some(first) = operands.next() else {
        return Ok(None);
    };
    if let Some(second) = operands.next() {
        return Err(DATE
            .usage_referring(format!("extra operand {}", quote_os(second)))
            .message());
    }
    let bytes = os_bytes(first);
    if let Some(fmt) = bytes.strip_prefix(b"+") {
        if cfg.format.is_some() {
            return Err(DATE
                .usage("multiple output formats specified".to_string())
                .message());
        }
        cfg.format = Some(fmt.to_vec());
        return Ok(None);
    }
    if set_date || option_specified_date > 0 {
        return Err(DATE
            .usage_referring(format!(
                "the argument {} lacks a leading '+';\n\
                 when using an option to specify date(s), any non-option\n\
                 argument must be a format string beginning with '+'",
                quote_os(first)
            ))
            .message());
    }
    // `MMDDhhmm[[CC]YY][.ss]`: POSIX's way of setting the clock.
    Ok(Some(first.clone()))
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut cfg = match parse_args(&args) {
        Ok(Request::Run(cfg)) => cfg,
        Ok(Request::Help) => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Ok(Request::Version) => {
            println!("date (SlateOS coreutils)");
            return ExitCode::SUCCESS;
        }
        Err(m) => {
            diag!("date: {}", m);
            return ExitCode::FAILURE;
        }
    };
    let posix_operand = match check_options(&mut cfg) {
        Ok(operand) => operand,
        Err(m) => {
            diag!("date: {}", m);
            return ExitCode::FAILURE;
        }
    };

    let format = cfg.format.clone().unwrap_or_else(|| {
        if cfg.get_resolution {
            RESOLUTION_FORMAT.to_vec()
        } else {
            DEFAULT_FORMAT.to_vec()
        }
    });
    let format = adjust_resolution(&format).unwrap_or(format);

    // `-u` is `putenv ("TZ=UTC0")`, so it is also what `--debug` reports as
    // the zone's source.
    let tzstring: Option<Vec<u8>> = if cfg.utc {
        Some(b"UTC0".to_vec())
    } else {
        std::env::var_os("TZ").map(|v| os_bytes(&v).into_owned())
    };
    let tz = Zone::from_tz(tzstring.as_deref());
    let session = Session {
        format: &format,
        tz: &tz,
        tzstring: tzstring.as_deref(),
        debug: cfg.debug,
    };

    let ok = if let Some(batch) = &cfg.batch_file {
        match session.batch_convert(batch) {
            Ok(ok) => ok,
            Err(m) => {
                diag!("date: {}", m);
                return ExitCode::FAILURE;
            }
        }
    } else {
        match session.single(&cfg, posix_operand) {
            Ok(ok) => ok,
            Err(m) => {
                diag!("date: {}", m);
                return ExitCode::FAILURE;
            }
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// What every instant is printed with.
struct Session<'a> {
    format: &'a [u8],
    tz: &'a Zone,
    tzstring: Option<&'a [u8]>,
    debug: bool,
}

impl Session<'_> {
    /// `parse_datetime2` with this session's zone and `--debug` setting.
    fn parse(&self, text: &[u8]) -> Option<Timespec> {
        if self.debug {
            let mut err = io::stderr().lock();
            parse_datetime2(text, None, Some(&mut err), self.tz, self.tzstring)
        } else {
            parse_datetime2(text, None, None, self.tz, self.tzstring)
        }
    }

    /// Everything upstream's `main` does when there is no `-f`: find the one
    /// instant, set the clock if asked, and print it. `Ok(false)` is "printed
    /// what it could, exit 1".
    ///
    /// # Errors
    ///
    /// A fatal error — an unreadable `-r` file or a date that does not parse —
    /// as the message to print after `date: `.
    fn single(&self, cfg: &Config, posix_operand: Option<OsString>) -> Result<bool, Failure> {
        let mut ok = true;
        let mut set_date = cfg.set_datestr.is_some();
        let (when, datestr) = if let Some(operand) = posix_operand {
            // Setting the clock from POSIX's digit string.
            set_date = true;
            let syntax = Syntax::TRAILING_YEAR
                .with(Syntax::CENTURY)
                .with(Syntax::SECONDS);
            let now = Timespec::now().tv_sec;
            let when =
                posixtm::posixtime(&os_bytes(&operand), syntax, self.tz, now).map(|t| Timespec {
                    tv_sec: t,
                    tv_nsec: 0,
                });
            (when, Some(operand))
        } else if let Some(reference) = &cfg.reference {
            let meta = std::fs::metadata(reference)
                .map_err(|e| format!("{}: {}", quotef_os(reference), strerror(&e)))?;
            let mtime = meta
                .modified()
                .map_err(|e| format!("{}: {}", quotef_os(reference), strerror(&e)))?;
            (Some(Timespec::from_system_time(mtime)), None)
        } else if cfg.get_resolution {
            let res = gettime_res();
            let when = Timespec {
                tv_sec: res / 1_000_000_000,
                tv_nsec: i32::try_from(res % 1_000_000_000).unwrap_or(0),
            };
            (Some(when), None)
        } else if let Some(text) = cfg.set_datestr.as_ref().or(cfg.datestr.as_ref()) {
            (self.parse(&os_bytes(text)), Some(text.clone()))
        } else {
            (Some(Timespec::now()), None)
        };

        let Some(when) = when else {
            let text = datestr.unwrap_or_default();
            return Err(format!("invalid date {}", quote_os(&text)));
        };

        if set_date {
            // Set the clock, then print the date whether or not that worked.
            if let Err(e) = settime(when) {
                diag!("date: cannot set date: {}", strerror(&e));
                ok = false;
            }
        }
        Ok(self.show_date(when) && ok)
    }

    /// Upstream's `batch_convert`: every line of `input` as a date, printed
    /// as it is read. A line that is not a date is reported and the rest are
    /// still converted; the result is `false` if any failed.
    ///
    /// # Errors
    ///
    /// The file not opening, or a read error, as the message to print after
    /// `date: `.
    fn batch_convert(&self, input: &OsString) -> Result<bool, Failure> {
        let bytes = os_bytes(input);
        let stdin;
        let file;
        let (mut reader, name): (Box<dyn BufRead>, String) = if bytes.as_ref() == b"-" {
            stdin = io::stdin();
            (Box::new(stdin.lock()), "standard input".to_string())
        } else {
            file = std::fs::File::open(input)
                .and_then(stdfd::fd_safer)
                .map_err(|e| format!("{}: {}", quotef_os(input), strerror(&e)))?;
            (Box::new(io::BufReader::new(file)), quotef_os(input))
        };

        let mut ok = true;
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    let what = if bytes.as_ref() == b"-" {
                        "standard input".to_string()
                    } else {
                        name.clone()
                    };
                    return Err(format!("{what}: read error: {}", strerror(&e)));
                }
            }
            // The line is a C string to upstream: it ends at a NUL.
            let text = line.split(|&b| b == 0).next().unwrap_or(&[]);
            match self.parse(text) {
                Some(when) => ok &= self.show_date(when),
                None => {
                    let shown = text.strip_suffix(b"\n").unwrap_or(text);
                    diag!("date: invalid date {}", quote(shown));
                    ok = false;
                }
            }
        }
        Ok(ok)
    }

    /// Upstream's `show_date`: print `when` in the format, or say that it is
    /// out of range.
    fn show_date(&self, when: Timespec) -> bool {
        if self.debug {
            diag!("date: output format: {}", quote(self.format));
        }
        if self.tz.localtime_r(when.tv_sec).is_none() {
            diag!(
                "date: time {} is out of range",
                quote(when.tv_sec.to_string().as_bytes())
            );
            return false;
        }
        let nanos = u32::try_from(when.tv_nsec).unwrap_or(0);
        let tm = self.tz.local(when.tv_sec, nanos);
        let mut out = nstrftime_z(self.format, &tm, self.tz);
        out.push(b'\n');
        let mut stdout = io::stdout().lock();
        // A failed write is caught when stdout is closed at exit; upstream's
        // `close_stdout` reports it there, and so does `stdfd::close_stderr`.
        stdout.write_all(&out).is_ok()
    }
}

/// Upstream's `adjust_resolution`: a copy of `format` with each `%-N` made
/// `%9N`, `%6N` or whatever the clock's resolution supports, or `None` if
/// there is no `%-N`.
fn adjust_resolution(format: &[u8]) -> Option<Vec<u8>> {
    let mut copy: Option<Vec<u8>> = None;
    let mut i = 0usize;
    while i < format.len() {
        if format.get(i) == Some(&b'%') {
            match (
                format.get(i.saturating_add(1)),
                format.get(i.saturating_add(2)),
            ) {
                (Some(b'-'), Some(b'N')) => {
                    let c = copy.get_or_insert_with(|| format.to_vec());
                    if let Some(slot) = c.get_mut(i.saturating_add(1)) {
                        // `res_width` is 0-9, so this is a digit.
                        *slot = b'0'.saturating_add(res_width(gettime_res()));
                    }
                    i = i.saturating_add(2);
                }
                (Some(b'%'), _) => i = i.saturating_add(1),
                _ => {}
            }
        }
        i = i.saturating_add(1);
    }
    copy
}

/// Upstream's `res_width`: the digits a nanosecond count needs to show a
/// resolution of `res` nanoseconds without losing information.
fn res_width(res: i64) -> u8 {
    let mut digits = 9u8;
    let mut r: i64 = 1;
    loop {
        r = r.saturating_mul(10);
        if r > res {
            break;
        }
        digits = digits.saturating_sub(1);
    }
    digits
}

/// Euclid's algorithm, for `gettime_res`.
fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = a.checked_rem(b).unwrap_or(0);
        a = b;
        b = t;
    }
    a
}

/// gnulib's `gettime_res`: the clock's resolution in nanoseconds — what
/// `clock_getres` claims, refined by sampling the clock, since some systems
/// report the timer interval instead.
fn gettime_res() -> i64 {
    const HZ: i64 = 1_000_000_000;
    let (sec, nsec) = clock_getres_realtime().unwrap_or((1, 0));
    let mut r = if nsec <= 0 { HZ } else { nsec };
    r = if sec < i64::MAX.saturating_sub(r) / HZ {
        r.saturating_add(sec.saturating_mul(HZ))
    } else {
        i64::MAX
    };
    for _ in 0..32 {
        if r <= 1 {
            break;
        }
        let now = Timespec::now();
        r = gcd(
            r,
            if now.tv_nsec != 0 {
                i64::from(now.tv_nsec)
            } else {
                HZ
            },
        );
    }
    r
}

/// `struct timespec` as the C library lays it out on every target we build.
#[cfg(unix)]
#[repr(C)]
struct CTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// `clock_getres (CLOCK_REALTIME)`.
#[cfg(unix)]
fn clock_getres_realtime() -> Option<(i64, i64)> {
    unsafe extern "C" {
        fn clock_getres(clk: i32, res: *mut CTimespec) -> i32;
    }
    const CLOCK_REALTIME: i32 = 0;
    let mut res = CTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `res` is a valid, writable `struct timespec` for the duration
    // of the call, which retains no pointer to it.
    let rc = unsafe { clock_getres(CLOCK_REALTIME, &mut res) };
    (rc == 0).then_some((res.tv_sec, res.tv_nsec))
}

/// The development host has no `clock_getres`; `SystemTime` there counts in
/// 100 ns units.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the unix arm can fail, and the two must share a signature"
)]
fn clock_getres_realtime() -> Option<(i64, i64)> {
    Some((0, 100))
}

/// gnulib's `settime`: `clock_settime`, and `settimeofday` if that failed for
/// any reason but a lack of permission.
#[cfg(unix)]
fn settime(ts: Timespec) -> io::Result<()> {
    #[repr(C)]
    struct CTimeval {
        tv_sec: i64,
        tv_usec: i64,
    }
    unsafe extern "C" {
        fn clock_settime(clk: i32, tp: *const CTimespec) -> i32;
        fn settimeofday(tv: *const CTimeval, tz: *const core::ffi::c_void) -> i32;
    }
    const CLOCK_REALTIME: i32 = 0;
    const EPERM: i32 = 1;

    let spec = CTimespec {
        tv_sec: ts.tv_sec,
        tv_nsec: i64::from(ts.tv_nsec),
    };
    // SAFETY: `spec` is a valid `struct timespec` for the duration of the
    // call, which only reads it.
    if unsafe { clock_settime(CLOCK_REALTIME, &spec) } == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(EPERM) {
        return Err(err);
    }
    let tv = CTimeval {
        tv_sec: ts.tv_sec,
        tv_usec: i64::from(ts.tv_nsec / 1000),
    };
    // SAFETY: `tv` is a valid `struct timeval` for the duration of the call,
    // and a null zone pointer is the documented "no zone" argument.
    if unsafe { settimeofday(&tv, std::ptr::null()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The development host cannot set its clock from here.
#[cfg(not(unix))]
fn settime(_ts: Timespec) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn config(args: &[&str]) -> Config {
        let args: Vec<OsString> = args.iter().map(|a| os(a)).collect();
        match parse_args(&args) {
            Ok(Request::Run(cfg)) => cfg,
            other => panic!("{args:?} gave {other:?}"),
        }
    }

    /// The error `check_options` gives, for a command line `parse_args`
    /// accepts.
    fn conflict(args: &[&str]) -> String {
        let mut cfg = config(args);
        check_options(&mut cfg).unwrap_err()
    }

    #[test]
    fn iso_formats_match_the_measured_gnu_strings() {
        assert_eq!(config(&["-I"]).format.unwrap(), b"%Y-%m-%d");
        assert_eq!(
            config(&["-Iseconds"]).format.unwrap(),
            b"%Y-%m-%dT%H:%M:%S%:z"
        );
        assert_eq!(
            config(&["-Ins"]).format.unwrap(),
            b"%Y-%m-%dT%H:%M:%S,%N%:z"
        );
        assert_eq!(config(&["-Ihours"]).format.unwrap(), b"%Y-%m-%dT%H%:z");
        assert_eq!(
            config(&["--rfc-3339=ns"]).format.unwrap(),
            b"%Y-%m-%d %H:%M:%S.%N%:z"
        );
    }

    #[test]
    fn rfc_3339_refuses_hours_and_minutes_but_iso_accepts_them() {
        assert!(parse_args(&[os("--rfc-3339=hours")]).is_err());
        assert!(parse_args(&[os("-Iminutes")]).is_ok());
    }

    #[test]
    fn precision_arguments_accept_an_unambiguous_prefix() {
        assert_eq!(config(&["-Isec"]).format.unwrap(), b"%Y-%m-%dT%H:%M:%S%:z");
    }

    #[test]
    fn a_second_output_format_is_an_error_even_the_same_one() {
        for args in [
            vec!["-R", "-I"],
            vec!["-R", "-R"],
            vec!["-Iseconds", "--rfc-3339=date"],
        ] {
            let args: Vec<OsString> = args.iter().map(|a| os(a)).collect();
            let e = parse_args(&args).unwrap_err();
            assert_eq!(e, "multiple output formats specified", "{args:?}");
        }
        // A +FORMAT operand after an option format, too -- checked after the
        // loop.
        assert_eq!(
            conflict(&["-R", "+%s"]),
            "multiple output formats specified"
        );
    }

    #[test]
    fn date_sources_are_mutually_exclusive_after_the_loop() {
        for args in [
            vec!["-f", "a", "-d", "@0"],
            vec!["-d", "@0", "-f", "a"],
            vec!["-f", "a", "-r", "b"],
            vec!["-d", "@0", "--resolution"],
        ] {
            let e = conflict(&args);
            assert!(e.contains("mutually exclusive"), "{args:?} gave {e}");
        }
        // Repeats of one source are not a conflict: the last wins.
        let mut cfg = config(&["-d", "@0", "-d", "@1"]);
        assert!(check_options(&mut cfg).is_ok());
        assert_eq!(cfg.datestr, Some(os("@1")));
        assert!(cfg.discarded_datestr);
        // Setting and printing do not mix.
        let e = conflict(&["-s", "@0", "-d", "@1"]);
        assert!(e.contains("may not be used together"), "{e}");
    }

    #[test]
    fn a_getopt_error_comes_before_a_conflict() {
        let e = parse_args(&[os("-d"), os("@0"), os("-r"), os("x"), os("-Q")]).unwrap_err();
        assert!(e.contains("invalid option"), "{e}");
    }

    #[test]
    fn the_operands_get_one_of_four_measured_answers() {
        // A +FORMAT.
        let mut cfg = config(&["+%Y"]);
        assert_eq!(check_options(&mut cfg).unwrap(), None);
        assert_eq!(cfg.format.unwrap(), b"%Y");
        // Two operands: the SECOND is named.
        let e = conflict(&["+%F", "extra"]);
        assert!(e.starts_with("extra operand"), "{e}");
        assert!(e.contains("extra"), "{e}");
        // An operand beside a date option must be a format.
        let e = conflict(&["-d", "@0", "extra"]);
        assert!(e.contains("lacks a leading '+'"), "{e}");
        // Alone, it is POSIX's clock-setting form.
        let mut cfg = config(&["0101000070"]);
        assert_eq!(check_options(&mut cfg).unwrap(), Some(os("0101000070")));
    }

    #[test]
    fn utc_has_three_spellings_and_a_short_one() {
        for a in ["-u", "--utc", "--universal", "--uct", "--u"] {
            assert!(config(&[a]).utc, "{a} should select UTC");
        }
    }

    #[test]
    fn rfc_email_has_three_spellings() {
        for a in ["-R", "--rfc-email", "--rfc-822", "--rfc-2822"] {
            assert_eq!(config(&[a]).format.unwrap(), RFC_EMAIL_FORMAT, "{a}");
        }
    }

    #[test]
    fn every_option_is_implemented() {
        let cfg = config(&["--debug", "--resolution"]);
        assert!(cfg.debug && cfg.get_resolution);
        let cfg = config(&["--set=@0"]);
        assert_eq!(cfg.set_datestr, Some(os("@0")));
        let cfg = config(&["--file=x"]);
        assert_eq!(cfg.batch_file, Some(os("x")));
    }

    #[test]
    fn a_dash_n_takes_the_clocks_resolution() {
        // `%-N` becomes a width; `%%-N` is a literal and is left alone.
        let adjusted = adjust_resolution(b"%s.%-N %%-N").unwrap();
        assert!(adjusted.starts_with(b"%s.%"));
        assert_eq!(adjusted.get(5), Some(&b'N'));
        assert!(adjusted.get(4).is_some_and(u8::is_ascii_digit));
        assert!(adjusted.ends_with(b" %%-N"));
        assert_eq!(adjust_resolution(b"%s.%N"), None);
    }

    #[test]
    fn res_width_counts_the_digits_a_resolution_needs() {
        assert_eq!(res_width(1), 9);
        assert_eq!(res_width(100), 7);
        assert_eq!(res_width(1_000), 6);
        assert_eq!(res_width(999), 7);
        assert_eq!(res_width(1_000_000_000), 0);
    }

    /// 2021-06-15 12:00:00 UTC, a Tuesday.
    ///
    /// These are the measurements the hand-written `-d` parser was built
    /// against, read off GNU 9.4 (`date -u -d '2021-06-15 12:00:00 <form>'
    /// +%s`, or `scripts/probe-date-d-grammar.sh`). They are kept as a check
    /// that the port answers what GNU answered.
    const TEST_NOW: i64 = 1_623_758_400;

    fn spec(s: &str) -> Option<i64> {
        let now = Timespec {
            tv_sec: TEST_NOW,
            tv_nsec: 0,
        };
        parse_datetime2(s.as_bytes(), Some(now), None, &Zone::utc(), Some(b"UTC0"))
            .map(|t| t.tv_sec)
    }

    #[test]
    fn d_accepts_the_measured_absolute_forms() {
        assert_eq!(spec("@0"), Some(0));
        assert_eq!(spec("@1000000000"), Some(1_000_000_000));
        assert_eq!(spec("@-1"), Some(-1));
        assert_eq!(spec("2021-03-04 05:06:07"), Some(1_614_834_367));
        assert_eq!(spec("2021-03-04T05:06:07"), Some(1_614_834_367));
        assert_eq!(spec("2021-03-04"), Some(1_614_816_000));
        assert_eq!(spec("1970-01-01 00:00:00 UTC"), Some(0));
        assert_eq!(spec("Mar 4 2021"), Some(1_614_816_000));
        assert_eq!(spec("4 March 2021"), Some(1_614_816_000));
        assert_eq!(spec("March 4, 2021"), Some(1_614_816_000));
        assert_eq!(spec("2021/03/04"), Some(1_614_816_000));
        assert_eq!(spec("03/04/2021"), Some(1_614_816_000));
    }

    #[test]
    fn d_applies_an_explicit_zone_offset() {
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
        assert_eq!(spec("today"), Some(TEST_NOW));
        assert_eq!(spec("tomorrow"), Some(TEST_NOW + 86_400));
        assert_eq!(spec("yesterday"), Some(TEST_NOW - 86_400));
        assert_eq!(spec("TOMORROW"), Some(TEST_NOW + 86_400));
    }

    #[test]
    fn d_accepts_the_measured_relative_forms() {
        assert_eq!(spec("1 day"), Some(1_623_844_800));
        assert_eq!(spec("2 days ago"), Some(1_623_585_600));
        assert_eq!(spec("3 weeks ago"), Some(1_621_944_000));
        assert_eq!(spec("1 month ago"), Some(1_621_080_000));
        assert_eq!(spec("2 years ago"), Some(1_560_600_000));
        assert_eq!(spec("2 hours"), Some(1_623_765_600));
        assert_eq!(spec("30 minutes"), Some(1_623_760_200));
        assert_eq!(spec("1 fortnight"), Some(1_624_968_000));
        assert_eq!(spec("next day"), Some(1_623_844_800));
        assert_eq!(spec("last week"), Some(1_623_153_600));
        assert_eq!(spec("next month"), Some(1_626_350_400));
        assert_eq!(spec("last year"), Some(1_592_222_400));
        assert_eq!(spec("1 sec"), spec("1 second"));
        assert_eq!(spec("2 mins"), spec("2 minutes"));
        assert_eq!(spec("1 day 2 hours ago"), Some(1_623_837_600));
        assert_eq!(spec("1 day ago 2 hours"), Some(1_623_679_200));
        assert_eq!(spec("2021-06-15 12:00:00 1 day"), Some(1_623_844_800));
        assert_eq!(spec("2021-01-31 1 month"), Some(1_614_729_600));
        assert_eq!(spec("2021-03-31 1 month ago"), Some(1_614_729_600));
        // A signed number straight after a bare time is a ZONE OFFSET: noon
        // at UTC+1, a day later. The hand-written parser refused this; GNU's
        // grammar decides it by a shift/reduce conflict, which the port
        // inherits.
        assert_eq!(spec("2021-06-15 12:00:00 +1 day"), Some(1_623_841_200));
        // -90 is not a valid offset (more than 24 hours), so GNU refuses.
        assert_eq!(spec("2021-06-15 12:00:00 -90 seconds"), None);
        assert_eq!(spec("2021-06-15 12:00:00 +0100"), Some(1_623_754_800));
        assert_eq!(spec("2021-06-15 +1 day"), Some(1_623_801_600));
        assert_eq!(spec("@0 1 day"), None);
        assert_eq!(spec("1 banana"), None);
        assert_eq!(spec("next banana"), None);
    }

    #[test]
    fn d_accepts_the_measured_weekday_forms() {
        assert_eq!(spec("Tuesday"), Some(1_623_715_200));
        assert_eq!(spec("next Tuesday"), Some(1_624_320_000));
        assert_eq!(spec("last Tuesday"), Some(1_623_110_400));
        assert_eq!(spec("Wednesday"), Some(1_623_801_600));
        assert_eq!(spec("Monday"), Some(1_624_233_600));
        assert_eq!(spec("last Monday"), Some(1_623_628_800));
        assert_eq!(spec("Friday"), Some(1_623_974_400));
        assert_eq!(spec("last Friday"), Some(1_623_369_600));
        assert_eq!(spec("next Wednesday"), spec("Wednesday"));
        assert_eq!(spec("tue"), spec("Tuesday"));
        assert_eq!(spec("TUESDAY"), spec("Tuesday"));
        assert_eq!(spec("tues"), spec("Tuesday"));
        assert_eq!(spec("thurs"), spec("Thursday"));
        assert_eq!(spec("thur"), spec("Thursday"));
        assert_eq!(spec("mon"), spec("Monday"));
        assert_eq!(
            spec("2021-06-15 12:00:00 Monday"),
            spec("2021-06-15 12:00:00")
        );
        assert_eq!(spec("Blursday"), None);
        assert_eq!(spec("Monday Tuesday"), None);
    }

    #[test]
    fn d_accepts_a_day_keyword_with_a_time_and_a_twelve_hour_clock() {
        assert_eq!(spec("yesterday 09:00"), Some(1_623_661_200));
        assert_eq!(spec("tomorrow 17:30"), Some(1_623_864_600));
        assert_eq!(spec("today 09:00"), Some(1_623_747_600));
        assert_eq!(spec("12 am"), Some(1_623_715_200));
        assert_eq!(spec("12 pm"), Some(1_623_758_400));
        assert_eq!(spec("1 pm"), Some(1_623_762_000));
        assert_eq!(spec("11 am"), Some(1_623_754_800));
        assert_eq!(spec("12:30 pm"), Some(1_623_760_200));
        assert_eq!(spec("2021-06-15 3 pm"), Some(1_623_769_200));
        assert_eq!(spec("13 pm"), None);
        assert_eq!(spec("0 am"), None);
        assert_eq!(spec("noon"), None);
        assert_eq!(spec("midnight"), None);
        assert_eq!(spec("05:06:07"), Some(1_623_733_567));
        assert_eq!(spec(""), Some(1_623_715_200));
        assert_eq!(spec("   "), Some(1_623_715_200));
    }

    #[test]
    fn d_refuses_what_gnu_refuses() {
        assert_eq!(spec("epoch"), None);
        assert_eq!(spec("@0 + 1 day"), None);
        assert_eq!(spec("not a date at all"), None);
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
}
