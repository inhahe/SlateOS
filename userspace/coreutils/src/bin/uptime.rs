//! uptime — tell how long the system has been running.
//!
//! ```text
//! uptime [-p|--pretty] [-s|--since]
//! ```
//!
//! procps-ng 4.0.4's `src/uptime.c`. Its default line is
//! `procps_uptime_sprint`, which `w` prints as its first line, so both
//! programs print it from one function: [`coreutils::procps::status_line`].
//! Checked by `scripts/uptime-diff.sh`.
//!
//! # It used to ignore its command line entirely
//!
//! This program read `/proc/uptime`, printed `up HH:MM`, and did that whatever
//! it was asked for. `uptime -s` asks when the machine booted and got how long
//! it had been up — not a worse format, a different question — and
//! `uptime --nosuchoption` exited 0. A program that does not look at `argv`
//! cannot honour an option and does not refuse one either, which is worse than
//! an unimplemented option and worse than the refusing stub §1006 forbids:
//! a refusal at least tells the caller it did not get what it asked for.
//!
//! Measured against procps-ng 4.0.4 before this was written:
//!
//! | | procps | here, before |
//! |---|---|---|
//! | `uptime -s` | `2026-09-11 19:27:28` | `up 00:26` |
//! | `uptime -p` | `up 1 hour, 21 minutes` | `up 00:26` |
//! | `uptime --nosuchoption` | usage, exit 1 | `up 00:26`, exit 0 |
//!
//! `scripts/check-argv-ignored.py` is the gate that now refuses the next one.
//!
//! # The default line
//!
//! ` 20:48:41 up  1:21,  1 user,  load average: 0.10, 0.10, 0.09` -- the clock,
//! the uptime, the sessions in utmp and the three load averages, each in
//! upstream's spelling, which is not the obvious one in three places: under an
//! hour the uptime is `N min` and not a clock, at an hour or more its hour is
//! padded with a space, and zero sessions is `0 user`, singular. The rules
//! live with the function now, in [`coreutils::procps`].
//!
//! Until 2026-10-02 this file had its own copy of that line, and the copy
//! differed from upstream's in what it was given rather than in how it printed
//! it:
//!
//! * **`/proc/uptime` was read with `str::parse`.** procps reads it with
//!   `fscanf ("%lf %lf")` and refuses a file without *two* numbers; a file
//!   holding only `120` printed `up 2 min` here and `Cannot get system uptime`
//!   there. `nan`, `inf` and negative values, which `fscanf` reads, were
//!   refusals here.
//! * **A load average that stopped short** -- `1.50 x` -- printed three zeros
//!   here, where procps prints the field it could read and zeros after it.
//! * **The session count included nameless `USER_PROCESS` entries**, which
//!   upstream's `count_users` skips (`ut_name[0] != '\0'`).
//!
//! # `-s`, to the second
//!
//! The boot time is `now - uptime` with both in fractions of a second, then
//! truncated, as upstream computes it: `(time_t) ((now_us / 1e6) -
//! uptime_secs)`. This used whole seconds of each, which is a second late
//! whenever the uptime's fraction exceeds the clock's -- every other run, on a
//! real `/proc/uptime` with its two decimals. The harness compares this clock
//! within a tolerance, so the unit tests pin it.
//!
//! # What is NOT compared, and why it took two experiments to find out
//!
//! On a systemd host the reference does not read `utmp` at all. `uptime` links
//! `libsystemd` and asks logind, so emptying `/run/utmp` inside the namespace
//! leaves the count at `1` while `who`, which does read `utmp`, drops to `0`.
//! A first pass concluded from that the field was unpinnable and could never
//! be compared.
//!
//! It can. Bind an empty directory over `/run/systemd` as well and procps
//! falls back to `utmp`, at which point the count is a fixture like every
//! other field. The first experiment answered a narrower question than the
//! one being asked.
//!
//! SlateOS has no logind, so `utmp` is the correct source there regardless.
//!
//! # Deliberate differences from procps-ng 4.0.4
//!
//! 1. **`-p` rolls over at a unit's boundary.** procps-ng 4.0.4 tests each
//!    unit with `>` rather than `>=`, so an uptime of exactly one minute prints
//!    `up ` and nothing else; this is upstream's evident intent instead. See
//!    `scripts/uptime-diff.sh`, where the cases are declared.
//! 2. **`--version`** says `uptime from SlateOS coreutils 0.1.0`.
//! 3. **A `/proc/uptime` holding something other than two numbers** is
//!    `uptime: Cannot get system uptime`, with no reason after it. Upstream
//!    appends `strerror (errno)` there too, and `errno` then is whatever an
//!    unrelated call left behind -- measured as nothing for an empty file and
//!    `No such file or directory` for a line of prose, which is no
//!    information about either.
//! 4. **An unreadable `/proc/loadavg` prints zeros**, where upstream prints
//!    whatever was on its stack.

use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps;
use coreutils::stdfd::{self, Stream};
use localtime::Zone;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

coreutils::guard_std_fds!();

/// procps exits 1 on a bad option; measured, `uptime -Q; echo $?` prints 1.
const UPTIME: Program = Program::new("uptime", 1);

/// procps-ng 4.0.4's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "phsV";

/// procps-ng 4.0.4's `longopts[]`, in its declaration order. Read from
/// `uptime --help` rather than guessed: it carries **four** options and none of
/// the `--raw`, `--json` or `-r` that the retired standalone invented.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("pretty", Takes::Nothing),
    ("help", Takes::Nothing),
    ("since", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// The default `up …` line.
    Plain,
    /// `-p`: `up 1 hour, 21 minutes`.
    Pretty,
    /// `-s`: the wall-clock time the machine booted.
    Since,
    Help,
    Version,
}

/// procps-ng 4.0.4's `--help`, byte for byte -- 241 bytes, on stdout for
/// `-h`, and on stderr after every command-line error.
///
/// The leading blank line and the trailing `For more details see uptime(1).`
/// are both procps'. Captured with `uptime --help | cat -A` rather than
/// retyped.
const HELP: &str = "
Usage:
 uptime [options]

Options:
 -p, --pretty   show uptime in pretty format
 -h, --help     display this help and exit
 -s, --since    system up since
 -V, --version  output version information and exit

For more details see uptime(1).
";

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

/// Parse `uptime`'s argv.
///
/// `-s` RETURNS HERE AND NOW. Only `-p` defers.
///
/// This was `req = …` per arm -- last-one-wins -- which agrees with procps on
/// `-ps` and is wrong on `-sp`, where it printed the pretty form.
/// `scripts/uptime-diff.sh` caught it, and only because `-ps` XPASSed: the
/// clustered pair had been declared an error without measuring it, and asking
/// why it was not one is what put `-sp` in the harness.
///
/// The first repair was wrong too, and in a way worth keeping a note about.
/// From `-ps`/`-sp` both printing the boot time the inference was "flags
/// resolved after the loop, `since` tested first" -- which fits both
/// observations and is still false. The discriminator is what `-s` does to
/// arguments AFTER it:
///
/// ```text
/// uptime -sV      -> the boot time        the -V never runs
/// uptime -sXYZ    -> the boot time, rc=0  no "invalid option -- 'X'"
/// uptime -s junk  -> the boot time, rc=0  an operand, ACCEPTED
/// uptime -Vs      -> the version          whichever fires first wins
/// ```
///
/// `-s` is not a flag with precedence, it is an early exit: procps prints and
/// leaves before the rest of argv is looked at.
///
/// An operand is refused only *after* the loop, as upstream's `if (optind !=
/// argc) usage (stderr)` is, so `uptime junk -s` prints the boot time too --
/// GNU's `getopt` has moved the operand past the options before upstream looks.
///
/// # Errors
///
/// getopt's sentence for an unknown, ambiguous or misused option, or `None`
/// for an operand, which upstream refuses with its usage alone. Either way the
/// whole usage follows on stderr.
fn parse_args(args: &[OsString]) -> Result<Request, Option<String>> {
    let mut pretty_flag = false;
    let mut operand = false;
    for item in UPTIME.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item.map_err(|e| Some(e.sentence))? {
            Opt::Long("pretty", _) | Opt::Short(b'p', _) => pretty_flag = true,
            Opt::Long("since", _) | Opt::Short(b's', _) => return Ok(Request::Since),
            Opt::Long("help", _) | Opt::Short(b'h', _) => return Ok(Request::Help),
            Opt::Long("version", _) | Opt::Short(b'V', _) => return Ok(Request::Version),
            Opt::Operand(_) => operand = true,
            // Unreachable: every entry of both tables is matched above.
            Opt::Long(other, _) => return Err(Some(format!("option '--{other}' is unhandled"))),
            Opt::Short(other, _) => return Err(Some(UPTIME.invalid_option(other).sentence)),
        }
    }
    if operand {
        return Err(None);
    }
    Ok(if pretty_flag {
        Request::Pretty
    } else {
        Request::Plain
    })
}

/// The `-p` rendering, which is procps' own decomposition.
///
/// Transcribed from procps-ng's `uptime.c` rather than invented, because the
/// units are not the obvious ones: weeks are taken modulo 52 and days modulo 7,
/// so 400 days is "1 year, 5 weeks, 1 day" and never "400 days". Components
/// that are zero are omitted entirely, except that an uptime under a minute
/// still prints `up 0 minutes`. (Upstream's released code rolls over on `>`
/// rather than `>=`; see "Deliberate differences".)
fn pretty(total_secs: f64) -> String {
    let secs = total_secs.max(0.0) as u64;
    let decades = secs / (60 * 60 * 24 * 365 * 10);
    let years = (secs / (60 * 60 * 24 * 365)) % 10;
    let weeks = (secs / (60 * 60 * 24 * 7)) % 52;
    let days = (secs / (60 * 60 * 24)) % 7;
    let hours = (secs / (60 * 60)) % 24;
    let minutes = (secs / 60) % 60;

    let mut parts: Vec<String> = Vec::new();
    let mut push = |n: u64, unit: &str| {
        if n > 0 {
            let s = if n == 1 { "" } else { "s" };
            parts.push(format!("{n} {unit}{s}"));
        }
    };
    push(decades, "decade");
    push(years, "year");
    push(weeks, "week");
    push(days, "day");
    push(hours, "hour");
    if parts.is_empty() || minutes > 0 {
        let s = if minutes == 1 { "" } else { "s" };
        parts.push(format!("{minutes} minute{s}"));
    }
    format!("up {}", parts.join(", "))
}

/// C's `(time_t) x` for a double, as x86-64 performs it: truncation toward
/// zero in range, and `cvttsd2si`'s "integer indefinite", `LONG_MIN`, for NaN
/// or a value outside it -- which `localtime` then refuses.
// The `as` is reached only in range, checked first: there it truncates toward
// zero, which is C's conversion, and it cannot saturate.
#[allow(clippy::cast_possible_truncation)]
fn c_time_t(x: f64) -> i64 {
    // 2^63, which `i64` cannot hold; everything below it in magnitude truncates
    // to a value that fits.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if x.is_nan() || x >= LIMIT || x < -LIMIT {
        i64::MIN
    } else {
        x as i64
    }
}

/// `print_uptime_since`, given the clock (`now`, seconds and microseconds, as
/// `gettimeofday` gives them) and the uptime: the boot time as upstream
/// prints it, or `None` where its `localtime` fails -- a boot time whose year
/// does not fit in an `int`.
fn since_line(now: (i64, u32), uptime_secs: f64, zone: &Zone) -> Option<String> {
    // `now = (tim.tv_sec * 1000000.0) + tim.tv_usec`, in `double`, exactly as
    // upstream forms it: a second count of this size is exact in a double.
    #[allow(clippy::cast_precision_loss)]
    let now_us = (now.0 as f64) * 1_000_000.0 + f64::from(now.1);
    let up_since = c_time_t((now_us / 1_000_000.0) - uptime_secs);
    let tm = zone.localtime_r(up_since)?;
    // `tm_year + 1900` and `tm_mon + 1` in C's `int`.
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year.wrapping_add(1900),
        tm.tm_mon.wrapping_add(1),
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}

/// `procps_uptime`'s answer: the seconds since boot, or why there are none --
/// the file could not be read (with the reason), or did not hold two numbers.
fn read_uptime() -> Result<f64, Option<io::Error>> {
    let text = std::fs::read(procps::UPTIME_FILE).map_err(Some)?;
    procps::uptime_secs(&text).ok_or(None)
}

/// Upstream's `xerr (EXIT_FAILURE, "Cannot get system uptime")`.
fn cannot_get_uptime(why: Option<&io::Error>) -> String {
    match why {
        Some(e) => format!("uptime: Cannot get system uptime: {}", strerror(e)),
        None => "uptime: Cannot get system uptime".to_string(),
    }
}

/// The sessions upstream's `count_users` counts, from the live utmp: zero
/// when it is missing or cannot be read, as `getutent` reports both.
fn user_count() -> i32 {
    std::fs::read(coreutils::utmp::UTMP_FILE)
        .map_or(0, |data| procps::count_users(&utmpfile::parse(&data)))
}

/// The three load averages, as `procps_loadavg` leaves them -- or zeros when
/// the file cannot be read, where upstream prints uninitialised memory.
fn load_averages() -> (f64, f64, f64) {
    std::fs::read(procps::LOADAVG_FILE).map_or((0.0, 0.0, 0.0), |text| procps::load_averages(&text))
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = Stream::stdout();
    let status = match parse_args(&args) {
        Err(sentence) => {
            let mut err = Stream::stderr();
            // Each write is deliberately unread: a failed write is `Stream`'s
            // to remember and `close_stderr`'s to act on.
            if let Some(sentence) = sentence {
                let _ = err.write_all(format!("uptime: {sentence}\n").as_bytes());
            }
            let _ = err.write_all(HELP.as_bytes());
            1
        }
        Ok(Request::Help) => {
            let _ = out.write_all(HELP.as_bytes());
            0
        }
        Ok(Request::Version) => {
            let _ = out.write_all(b"uptime from SlateOS coreutils 0.1.0\n");
            0
        }
        Ok(request) => show(&request, &mut out),
    };
    stdfd::close_stdout("uptime", out, ExitCode::from(status))
}

/// The three readings, `-p` and `-s` and the default, in upstream's order of
/// reads: the clock, then `/proc/uptime`.
fn show(request: &Request, out: &mut Stream) -> u8 {
    let zone = Zone::from_env();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or((0, 0), |d| {
            (
                i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                d.subsec_micros(),
            )
        });
    let up = match read_uptime() {
        Ok(up) => up,
        Err(why) => {
            coreutils::diag!("{}", cannot_get_uptime(why.as_ref()));
            return 1;
        }
    };
    let line = match request {
        Request::Pretty => pretty(up),
        Request::Since => match since_line(now, up, &zone) {
            Some(line) => line,
            None => {
                coreutils::diag!("uptime: localtime");
                return 1;
            }
        },
        _ => procps::status_line(&zone.local(now.0, 0), up, user_count(), load_averages()),
    };
    let _ = out.write_all(format!("{line}\n").as_bytes());
    0
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Result<Request, Option<String>> {
        let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse_args(&owned)
    }

    /// `-s` beats `-p` in either order, and `-h`/`-V` do not defer at all.
    ///
    /// Measured, because "the last option wins" is the rule almost every
    /// utility here follows and is wrong for exactly this pair:
    ///
    /// ```text
    /// uptime -ps   -> 2026-09-12 04:50:23      uptime -sV -> 2026-09-12 …
    /// uptime -sp   -> 2026-09-12 04:50:23      uptime -Vs -> uptime from …
    /// ```
    #[test]
    fn since_beats_pretty_in_either_order() {
        assert_eq!(argv(&["-p", "-s"]), Ok(Request::Since));
        assert_eq!(
            argv(&["-s", "-p"]),
            Ok(Request::Since),
            "-sp must not be Pretty"
        );
        assert_eq!(argv(&["-ps"]), Ok(Request::Since));
        assert_eq!(argv(&["-sp"]), Ok(Request::Since));
        assert_eq!(argv(&["--since", "--pretty"]), Ok(Request::Since));
        assert_eq!(argv(&["--pretty", "--since"]), Ok(Request::Since));
        // Neither alone changes the other's answer.
        assert_eq!(argv(&["-p"]), Ok(Request::Pretty));
        assert_eq!(argv(&["-s"]), Ok(Request::Since));
        assert_eq!(argv(&[]), Ok(Request::Plain));
        // `-h` and `-V` act where they are seen rather than deferring.
        assert_eq!(argv(&["-V", "-s"]), Ok(Request::Version));
        // `-s` first means the `-V` is never reached -- the assertion that
        // disproved the "deferred flags" model.
        assert_eq!(argv(&["-s", "-V"]), Ok(Request::Since));
        // And the discriminator itself: procps accepts these at rc=0 because
        // `-s` leaves before argv is examined any further.
        assert_eq!(argv(&["-s", "junk"]), Ok(Request::Since));
        assert_eq!(argv(&["-sZ"]), Ok(Request::Since));
        assert_eq!(argv(&["-h", "-V"]), Ok(Request::Help));
        assert_eq!(argv(&["-V", "-h"]), Ok(Request::Version));
    }

    /// An operand is refused after the options, not where it stands.
    #[test]
    fn an_operand_is_refused_only_after_the_options() {
        assert_eq!(argv(&["junk"]), Err(None));
        assert_eq!(argv(&["junk", "-p"]), Err(None));
        // GNU's getopt moves the operand past `-s`, which then leaves first.
        assert_eq!(argv(&["junk", "-s"]), Ok(Request::Since));
        assert_eq!(argv(&["junk", "-V"]), Ok(Request::Version));
        // An option error is getopt's own, found first.
        assert_eq!(
            argv(&["junk", "-Z"]),
            Err(Some("invalid option -- 'Z'".to_string()))
        );
    }

    #[test]
    fn option_errors_are_getopts_sentences() {
        assert_eq!(
            argv(&["-Z"]),
            Err(Some("invalid option -- 'Z'".to_string()))
        );
        assert_eq!(
            argv(&["--nosuchoption"]),
            Err(Some("unrecognized option '--nosuchoption'".to_string()))
        );
        assert_eq!(
            argv(&["--pretty=value"]),
            Err(Some(
                "option '--pretty' doesn't allow an argument".to_string()
            ))
        );
    }

    #[test]
    fn the_help_is_upstreams() {
        assert_eq!(HELP.len(), 241);
        assert!(HELP.starts_with("\nUsage:\n uptime [options]\n"));
    }

    // --- `-p`, whose units are procps' and not the obvious ones ---------

    #[test]
    fn pretty_under_a_minute_still_says_minutes() {
        // The one case with no non-zero component. procps prints `up 0
        // minutes` rather than a bare `up`, so the string is never truncated
        // to something that reads as an error.
        assert_eq!(pretty(0.0), "up 0 minutes");
        assert_eq!(pretty(59.0), "up 0 minutes");
    }

    #[test]
    fn pretty_singular_and_plural() {
        assert_eq!(pretty(60.0), "up 1 minute");
        assert_eq!(pretty(120.0), "up 2 minutes");
        assert_eq!(pretty(3600.0), "up 1 hour");
        assert_eq!(pretty(7200.0), "up 2 hours");
    }

    #[test]
    fn pretty_omits_zero_components() {
        assert_eq!(pretty(3600.0), "up 1 hour");
        assert_eq!(pretty(3660.0), "up 1 hour, 1 minute");
    }

    #[test]
    fn pretty_matches_the_live_reading() {
        // 4993 s is what /proc/uptime said while this was being written, and
        // procps printed exactly this for it.
        assert_eq!(pretty(4993.0), "up 1 hour, 23 minutes");
    }

    #[test]
    fn pretty_weeks_are_modulo_52_and_days_modulo_7() {
        // EACH COMPONENT IS COMPUTED INDEPENDENTLY FROM THE TOTAL with its own
        // modulo, so they do not sum back to it:
        //
        //     years = (total / 365d) % 10  = 1
        //     weeks = (total /   7d) % 52  = (400/7) % 52 = 57 % 52 = 5
        //     days  = (total /   1d) %  7  = 400 % 7       = 1
        //
        // 1 year + 5 weeks + 1 day is 401 days, not 400. That is upstream's
        // arithmetic and this reproduces it; the test is here because the
        // reasonable-looking answer is the wrong one.
        let four_hundred_days = 400.0 * 86400.0;
        assert_eq!(pretty(four_hundred_days), "up 1 year, 5 weeks, 1 day");
        // 8 days is one week and one day, never "8 days".
        assert_eq!(pretty(8.0 * 86400.0), "up 1 week, 1 day");
    }

    #[test]
    fn pretty_clamps_a_negative_rather_than_wrapping() {
        assert_eq!(pretty(-1.0), "up 0 minutes");
    }

    // --- `-s` --------------------------------------------------------------

    /// The fractions count. 1000.460864 s less 100.5 s is 899.96..., which
    /// truncates to 899 -- where whole seconds of each gave 900.
    #[test]
    fn since_subtracts_fractions_before_truncating() {
        let utc = Zone::utc();
        assert_eq!(
            since_line((1000, 460_864), 100.5, &utc).as_deref(),
            Some("1970-01-01 00:14:59")
        );
        assert_eq!(
            since_line((1000, 600_000), 100.5, &utc).as_deref(),
            Some("1970-01-01 00:15:00")
        );
        // A real one, measured: procps printed 18:07:53 for 16000.50 at
        // 22:34:34.46.
        let now = 1_790_980_474;
        assert_eq!(
            since_line((now, 460_864), 16000.5, &utc).as_deref(),
            Some("2026-10-02 18:07:53")
        );
    }

    #[test]
    fn since_truncates_toward_zero_before_the_epoch() {
        let utc = Zone::utc();
        // 10 - 10.5 = -0.5, which C truncates to 0, not -1.
        assert_eq!(
            since_line((10, 0), 10.5, &utc).as_deref(),
            Some("1970-01-01 00:00:00")
        );
        assert_eq!(
            since_line((10, 0), 11.5, &utc).as_deref(),
            Some("1969-12-31 23:59:59")
        );
    }

    #[test]
    fn since_refuses_a_year_localtime_cannot_hold() {
        let utc = Zone::utc();
        assert_eq!(since_line((0, 0), f64::NAN, &utc), None);
        assert_eq!(since_line((0, 0), f64::INFINITY, &utc), None);
        assert_eq!(since_line((0, 0), 1e300, &utc), None);
    }

    #[test]
    fn c_time_t_is_cvttsd2si() {
        assert_eq!(c_time_t(899.96), 899);
        assert_eq!(c_time_t(-0.5), 0);
        assert_eq!(c_time_t(-1.5), -1);
        assert_eq!(c_time_t(f64::NAN), i64::MIN);
        assert_eq!(c_time_t(1e19), i64::MIN);
        assert_eq!(c_time_t(-1e19), i64::MIN);
    }

    #[test]
    fn the_refusal_names_the_reason_only_when_there_is_one() {
        assert_eq!(cannot_get_uptime(None), "uptime: Cannot get system uptime");
        let e = io::Error::from(io::ErrorKind::NotFound);
        assert!(
            cannot_get_uptime(Some(&e)).starts_with("uptime: Cannot get system uptime: "),
            "{}",
            cannot_get_uptime(Some(&e))
        );
    }
}
