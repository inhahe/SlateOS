//! logger — log messages to the system log.
//!
//! Usage: logger [-t TAG] [-p PRIORITY] [MESSAGE...]
//!   -t TAG       mark the message with the specified tag (default: "user")
//!   -p PRIORITY  specify priority in facility.level format (default: "user.notice")
//!
//! Writes a syslog-style text line to stdout (our OS uses text-based logs,
//! not binary syslog). If no MESSAGE arguments are given, reads from stdin.
//!
//! Output format:
//!   <priority> YYYY-MM-DDTHH:MM:SS TAG: MESSAGE

use coreutils::diag;
use coreutils::stdfd;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;
use std::time::SystemTime;

/// Map facility names to numeric codes (RFC 5424 compatible).
fn facility_code(name: &str) -> Option<u32> {
    match name {
        "kern" => Some(0),
        "user" => Some(1),
        "mail" => Some(2),
        "daemon" => Some(3),
        "auth" => Some(4),
        "syslog" => Some(5),
        "lpr" => Some(6),
        "news" => Some(7),
        "uucp" => Some(8),
        "cron" => Some(9),
        "local0" => Some(16),
        "local1" => Some(17),
        "local2" => Some(18),
        "local3" => Some(19),
        "local4" => Some(20),
        "local5" => Some(21),
        "local6" => Some(22),
        "local7" => Some(23),
        _ => None,
    }
}

/// Map severity names to numeric codes (RFC 5424 compatible).
fn severity_code(name: &str) -> Option<u32> {
    match name {
        "emerg" | "panic" => Some(0),
        "alert" => Some(1),
        "crit" => Some(2),
        "err" | "error" => Some(3),
        "warning" | "warn" => Some(4),
        "notice" => Some(5),
        "info" => Some(6),
        "debug" => Some(7),
        _ => None,
    }
}

/// Parse a "facility.severity" string into the PRI value.
fn parse_priority(s: &str) -> Option<u32> {
    if let Some((fac_str, sev_str)) = s.split_once('.') {
        let fac = facility_code(fac_str)?;
        let sev = severity_code(sev_str)?;
        Some(fac * 8 + sev)
    } else {
        // Could be severity alone (assume user facility).
        let sev = severity_code(s)?;
        Some(8 + sev)
    }
}

/// Format a timestamp from SystemTime as ISO 8601 (approximate, no TZ library).
fn format_timestamp() -> String {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            // Simple date/time computation (no leap seconds, approximate).
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;
            let seconds = time_of_day % 60;

            // Calculate year/month/day from days since epoch (1970-01-01).
            let (year, month, day) = days_to_date(days);

            format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}")
        }
        Err(_) => "1970-01-01T00:00:00".to_string(),
    }
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i as u64 + 1;
            break;
        }
        days -= md;
    }
    if month == 0 {
        month = 12;
    }

    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct LoggerArgs {
    tag: String,
    priority: String,
    message_parts: Vec<String>,
}

impl Default for LoggerArgs {
    fn default() -> Self {
        Self {
            tag: "user".to_string(),
            priority: "user.notice".to_string(),
            message_parts: Vec::new(),
        }
    }
}

/// Parse logger's argv into a `LoggerArgs`.  Returns an error string suitable
/// for `eprintln!("logger: {e}")` if a `-t` / `-p` flag is missing its value.
fn parse_args(args: &[String]) -> Result<LoggerArgs, String> {
    let mut out = LoggerArgs::default();
    let mut i: usize = 0;
    let mut options_ended = false;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        i = i.saturating_add(1);

        // `--` ends the options. Everything after it is message text, which is
        // the only way to log a message that begins with a dash.
        if !options_ended && arg == "--" {
            options_ended = true;
            continue;
        }
        // A bare `-` is a message, not an option, and always has been.
        if options_ended || !arg.starts_with('-') || arg == "-" {
            out.message_parts.push(arg.clone());
            continue;
        }

        // An option's value may be glued on (`-tX`) or be the next argument
        // (`-t X`). util-linux accepts both.
        let take_value = |letter: char, rest: &str, i: &mut usize| -> Result<String, String> {
            if !rest.is_empty() {
                return Ok(rest.to_string());
            }
            let Some(v) = args.get(*i) else {
                return Err(format!("option requires an argument -- '{letter}'"));
            };
            *i = i.saturating_add(1);
            Ok(v.clone())
        };

        let body = arg.strip_prefix('-').unwrap_or(arg);
        if let Some(long) = body.strip_prefix('-') {
            // Long options. None is implemented; refusing is still right,
            // because the alternative -- which this did until 2026-09-12 --
            // is to LOG the option text as the message.
            let name = long.split('=').next().unwrap_or(long);
            let _ = name;
            return Err(format!("unrecognized option '{arg}'"));
        }
        // NOT a loop over the cluster, deliberately. Both options this
        // implements take a value, so the first letter either consumes the
        // rest of the argument (`-tX`) or the next one (`-t X`) -- there is
        // never a second letter to read. Clippy pointed out that the `while`
        // this started as could not iterate twice; writing it as a loop would
        // have implied a clustering rule that does not exist here.
        let mut chars = body.chars();
        let Some(c) = chars.next() else {
            // A lone `-` is handled above as a message, so an empty body here
            // cannot happen; treat it as a message rather than panicking.
            out.message_parts.push(arg.clone());
            continue;
        };
        let rest: String = chars.collect();
        match c {
            't' => out.tag = take_value('t', &rest, &mut i)?,
            'p' => out.priority = take_value('p', &rest, &mut i)?,
            other => return Err(format!("invalid option -- '{other}'")),
        }
    }
    Ok(out)
}

/// Render one log line in the syslog-style text format the rest of the OS
/// expects: `<PRI> TIMESTAMP TAG: MESSAGE`.
fn format_log_line(pri: u32, timestamp: &str, tag: &str, msg: &str) -> String {
    format!("<{pri}> {timestamp} {tag}: {msg}")
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            diag!("logger: {e}");
            return ExitCode::from(1);
        }
    };

    let pri = match parse_priority(&parsed.priority) {
        Some(p) => p,
        None => {
            diag!("logger: unknown priority: {}", parsed.priority);
            return ExitCode::from(1);
        }
    };

    let timestamp = format_timestamp();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if parsed.message_parts.is_empty() {
        // Read messages from stdin, one per line.
        let reader = BufReader::new(io::stdin());
        for line in reader.lines() {
            match line {
                Ok(msg) => {
                    let _ = writeln!(
                        out,
                        "{}",
                        format_log_line(pri, &timestamp, &parsed.tag, &msg)
                    );
                }
                Err(e) => {
                    diag!("logger: {e}");
                    return ExitCode::from(1);
                }
            }
        }
    } else {
        let msg = parsed.message_parts.join(" ");
        let _ = writeln!(
            out,
            "{}",
            format_log_line(pri, &timestamp, &parsed.tag, &msg)
        );
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// AN UNKNOWN OPTION USED TO BECOME THE MESSAGE.
    ///
    /// `logger -Q` logged the string `-Q` at exit 0 -- so a script that
    /// mistyped an option recorded the typo in the system log and reported
    /// success. util-linux refuses, and the wording here is its own, measured:
    ///
    ///     logger: invalid option -- 'Q'
    ///     logger: unrecognized option '--no-such-option-xyzzy'
    ///     logger: option requires an argument -- 't'
    ///
    /// Found by sweeping all 83 coreutils bins with an unknown long option and
    /// looking at which exited 0. Six did; five of those are correct -- `echo`,
    /// `printf`, `expr`, `test` and `true` all treat it as text or an operand.
    /// This one was the finding.
    #[test]
    fn an_unknown_option_is_refused_not_logged() {
        assert_eq!(
            parse_args(&s(&["-Q"])).unwrap_err(),
            "invalid option -- 'Q'"
        );
        assert_eq!(
            parse_args(&s(&["--no-such-option-xyzzy"])).unwrap_err(),
            "unrecognized option '--no-such-option-xyzzy'"
        );
        // Clustered behind a valid flag, which is where a character-at-a-time
        // loop is likeliest to let one through.
        assert!(parse_args(&s(&["-t", "x", "-Q"])).is_err());
        // A missing argument is its own message, not "invalid option".
        assert_eq!(
            parse_args(&s(&["-t"])).unwrap_err(),
            "option requires an argument -- 't'"
        );
        assert_eq!(
            parse_args(&s(&["-p"])).unwrap_err(),
            "option requires an argument -- 'p'"
        );
    }

    /// A message may still start with a dash -- after `--`, which is the only
    /// way to say so once unknown options are refused.
    #[test]
    fn a_message_can_still_begin_with_a_dash() {
        let a = parse_args(&s(&["--", "-dashmsg"])).expect("-- ends the options");
        assert_eq!(a.message_parts, vec!["-dashmsg".to_string()]);
        // A bare `-` was never an option and still is not.
        let bare = parse_args(&s(&["-"])).expect("a bare dash is a message");
        assert_eq!(bare.message_parts, vec!["-".to_string()]);
        // Everything after `--` is message, including things that look like
        // options we DO implement.
        let after = parse_args(&s(&["--", "-t", "x"])).expect("all message");
        assert_eq!(after.message_parts, vec!["-t".to_string(), "x".to_string()]);
    }

    /// An option's value may be glued on, which util-linux accepts and this
    /// did not: `logger -tX hello` tags the line `X`.
    #[test]
    fn an_option_value_may_be_glued_or_separate() {
        assert_eq!(parse_args(&s(&["-tX", "hi"])).expect("glued").tag, "X");
        assert_eq!(
            parse_args(&s(&["-t", "X", "hi"])).expect("separate").tag,
            "X"
        );
        assert_eq!(
            parse_args(&s(&["-pkern.err"])).expect("glued").priority,
            "kern.err"
        );
        let both = parse_args(&s(&["-tX", "-pkern.err", "hi"])).expect("both");
        assert_eq!(
            (both.tag.as_str(), both.priority.as_str()),
            ("X", "kern.err")
        );
        assert_eq!(both.message_parts, vec!["hi".to_string()]);
    }

    // ---------------- facility_code ----------------

    #[test]
    fn facility_kern_is_0() {
        assert_eq!(facility_code("kern"), Some(0));
    }

    #[test]
    fn facility_user_is_1() {
        assert_eq!(facility_code("user"), Some(1));
    }

    #[test]
    fn facility_local7_is_23() {
        assert_eq!(facility_code("local7"), Some(23));
    }

    #[test]
    fn facility_unknown_is_none() {
        assert_eq!(facility_code("bogus"), None);
    }

    // ---------------- severity_code ----------------

    #[test]
    fn severity_emerg_panic_aliases() {
        assert_eq!(severity_code("emerg"), Some(0));
        assert_eq!(severity_code("panic"), Some(0));
    }

    #[test]
    fn severity_err_error_aliases() {
        assert_eq!(severity_code("err"), Some(3));
        assert_eq!(severity_code("error"), Some(3));
    }

    #[test]
    fn severity_warning_warn_aliases() {
        assert_eq!(severity_code("warning"), Some(4));
        assert_eq!(severity_code("warn"), Some(4));
    }

    #[test]
    fn severity_debug_is_7() {
        assert_eq!(severity_code("debug"), Some(7));
    }

    #[test]
    fn severity_unknown_is_none() {
        assert_eq!(severity_code("loud"), None);
    }

    // ---------------- parse_priority ----------------

    #[test]
    fn priority_user_notice_is_13() {
        // user(1) * 8 + notice(5) = 13.
        assert_eq!(parse_priority("user.notice"), Some(13));
    }

    #[test]
    fn priority_kern_emerg_is_0() {
        assert_eq!(parse_priority("kern.emerg"), Some(0));
    }

    #[test]
    fn priority_local7_debug_is_191() {
        // local7(23) * 8 + debug(7) = 191.
        assert_eq!(parse_priority("local7.debug"), Some(191));
    }

    #[test]
    fn priority_severity_only_defaults_to_user() {
        // Severity-only is treated as user facility: 8 + sev.
        assert_eq!(parse_priority("warning"), Some(8 + 4));
    }

    #[test]
    fn priority_unknown_facility_is_none() {
        assert_eq!(parse_priority("bogus.warning"), None);
    }

    #[test]
    fn priority_unknown_severity_is_none() {
        assert_eq!(parse_priority("user.bogus"), None);
    }

    // ---------------- is_leap ----------------

    #[test]
    fn leap_basics() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(!is_leap(2100));
    }

    // ---------------- days_to_date ----------------

    #[test]
    fn date_epoch_day_zero() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn date_epoch_day_one() {
        assert_eq!(days_to_date(1), (1970, 1, 2));
    }

    #[test]
    fn date_end_of_1970() {
        // Day 364 of 1970 = 1970-12-31.
        assert_eq!(days_to_date(364), (1970, 12, 31));
    }

    #[test]
    fn date_start_of_1971() {
        assert_eq!(days_to_date(365), (1971, 1, 1));
    }

    #[test]
    fn date_leap_day_2000() {
        // 2000-02-29 corresponds to day 11016 since epoch.
        // (1970..2000 has 7 leap years: 72,76,80,84,88,92,96.  Days =
        // 30*365 + 7 = 10957.  Then Jan 2000 = 31 days -> +31 = 10988.
        // Feb 28 + 1 = +28 -> day 10988+28 = 11016 -> 2000-02-29.)
        assert_eq!(days_to_date(11016), (2000, 2, 29));
    }

    // ---------------- parse_args ----------------

    #[test]
    fn args_defaults() {
        let a = parse_args(&s(&[])).unwrap();
        assert_eq!(a, LoggerArgs::default());
    }

    #[test]
    fn args_dash_t_sets_tag() {
        let a = parse_args(&s(&["-t", "myapp"])).unwrap();
        assert_eq!(a.tag, "myapp");
    }

    #[test]
    fn args_dash_p_sets_priority() {
        let a = parse_args(&s(&["-p", "kern.err"])).unwrap();
        assert_eq!(a.priority, "kern.err");
    }

    /// These two asserted only that the message CONTAINED `-t` / `-p`, which
    /// was true of the old wording ("option -t requires an argument") and is
    /// false of util-linux's ("option requires an argument -- 't'"). A
    /// substring check on an unmeasured string passes for any message that
    /// happens to mention the option, so it could not have caught the wording
    /// being wrong in the first place. Pinned to the measured text instead.
    #[test]
    fn args_dash_t_missing_value_errors() {
        let err = parse_args(&s(&["-t"])).unwrap_err();
        assert_eq!(err, "option requires an argument -- 't'");
    }

    #[test]
    fn args_dash_p_missing_value_errors() {
        let err = parse_args(&s(&["-p"])).unwrap_err();
        assert_eq!(err, "option requires an argument -- 'p'");
    }

    #[test]
    fn args_collects_message_parts() {
        let a = parse_args(&s(&["hello", "world"])).unwrap();
        assert_eq!(a.message_parts, vec!["hello", "world"]);
    }

    #[test]
    fn args_mixed_flags_and_message() {
        let a = parse_args(&s(&["-t", "tag1", "first", "-p", "user.info", "second"])).unwrap();
        assert_eq!(a.tag, "tag1");
        assert_eq!(a.priority, "user.info");
        assert_eq!(a.message_parts, vec!["first", "second"]);
    }

    // ---------------- format_log_line ----------------

    #[test]
    fn format_log_line_basic() {
        let line = format_log_line(13, "2024-01-01T00:00:00", "myapp", "hello");
        assert_eq!(line, "<13> 2024-01-01T00:00:00 myapp: hello");
    }

    #[test]
    fn format_log_line_empty_message() {
        let line = format_log_line(0, "1970-01-01T00:00:00", "kern", "");
        assert_eq!(line, "<0> 1970-01-01T00:00:00 kern: ");
    }
}
