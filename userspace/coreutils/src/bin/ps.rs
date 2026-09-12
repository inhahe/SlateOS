//! ps — report process status.
//!
//! Usage: ps [-e] [-f]
//!   -e  show all processes (not just current session)
//!   -f  full listing format
//!
//! Reads from /proc filesystem. Each directory under /proc/<pid>/
//! contains process information files: stat, cmdline, status.

use coreutils::diag;
use coreutils::stdfd;
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct PsArgs {
    all_procs: bool,
    full_format: bool,
}

/// Parse ps's argv.  BSD-style and POSIX-style flags are accepted via
/// the same clustered short-flag syntax used by the rest of these
/// utilities.  Unknown short flags are silently ignored, matching the
/// previous behaviour.
/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// Print the table.
    List(PsArgs),
    /// `--help`.
    Help,
}

/// Parse `ps`'s argv.
///
/// # Errors
///
/// An unknown option, short or long. procps distinguishes the two and so does
/// this; the messages are its own, measured.
///
/// # What this used to do, and why a test was protecting it
///
/// The body was one loop with `_ => {}` at the bottom, and a test named
/// `parse_unknown_silently_ignored` asserting that it stayed that way --
/// "Preserves previous behaviour: no error, no panic." It preserved a defect.
/// Measured against procps-ng:
///
/// | | procps | here, before |
/// |---|---|---|
/// | `ps -Q` | `error: unsupported SysV option`, exit 1 | the default table, exit 0 |
/// | `ps --nosuchoption` | `error: unknown gnu long option`, exit 1 | the default table, exit 0 |
/// | `ps --help` | usage, exit 0 | **the process table**, exit 0 |
///
/// The `--help` row is the one that shows how bad the shape was. There was no
/// long-option branch at all: `--help` had its first `-` stripped and the rest
/// was walked a character at a time, so it was read as `-h -e -l -p`, the `e`
/// matched, and `ps --help` turned on "show all processes". Any long option
/// containing `e`, `A` or `f` silently set that flag -- `--full` would have
/// set `-f` by accident and `--version` would have set `-e`.
///
/// `scripts/check-argv-ignored.py` did not catch it and is not wrong to have
/// missed it: that gate finds a program ignoring its command line ENTIRELY,
/// which is the defect `uptime` had. This one read `argv`, honoured `-e` and
/// `-f`, and discarded the rest -- the same hole one notch finer.
///
/// Found by `scripts/ps-diff.sh`.
fn parse_args(args: &[String]) -> Result<Request, String> {
    let mut out = PsArgs::default();
    for arg in args {
        // Long options are matched WHOLE. Splitting them into characters is
        // what made `--help` mean `-e`.
        if let Some(long) = arg.strip_prefix("--") {
            if long == "help" {
                return Ok(Request::Help);
            }
            return Err("error: unknown gnu long option".to_string());
        }
        if let Some(flags) = arg.strip_prefix('-') {
            for c in flags.chars() {
                match c {
                    'e' | 'A' => out.all_procs = true,
                    'f' => out.full_format = true,
                    _ => return Err("error: unsupported SysV option".to_string()),
                }
            }
        }
        // A bare operand is still ignored: procps takes PID lists in that
        // position and this build does not implement them, so refusing here
        // would reject a command line procps accepts. Unchanged, and still
        // covered by `parse_bare_args_ignored`.
    }
    Ok(Request::List(out))
}

/// procps-ng's `ps --help` with no topic, byte for byte.
///
/// Captured with `ps --help | cat -A`. The leading blank line is procps' and
/// so is the trailing `For more details see ps(1).` -- `uptime`'s help text
/// was missing exactly those two things for exactly the same reason, which is
/// that they are invisible when you retype a help message instead of
/// measuring it.
fn help_text() -> &'static str {
    "
Usage:
 ps [options]

 Try 'ps --help <simple|list|output|threads|misc|all>'
  or 'ps --help <s|l|o|t|m|a>'
 for additional help text.

For more details see ps(1).
"
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ProcInfo {
    comm: String,
    state: String,
    ppid: u32,
    uid: u32,
    tty: String,
    time_str: String,
    /// The `-f` command line, already rendered. Empty when `-f` was not asked
    /// for, which is also when it was never read.
    cmd: String,
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
        Ok(Request::List(p)) => p,
        Ok(Request::Help) => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            // procps prints the error AND its whole usage, both on stderr,
            // separated by a blank line -- which `help_text`'s leading newline
            // supplies. Measured: `ps --nosuchoption 2>&1` is the message, an
            // empty line, then the same 170 bytes `--help` prints.
            //
            // This is the opposite of the call made for `uptime`, whose
            // upstream is also procps: there the shared `coreutils::getopt`
            // formatter prints GNU's `Try '… --help'` hint for all 86 bins and
            // changing it for one would make the other 85 wrong. `ps` does not
            // go through that formatter, so matching costs nothing here.
            diag!("{}", message);
            eprint!("{}", help_text());
            return ExitCode::FAILURE;
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if parsed.full_format {
        let _ = writeln!(
            out,
            "{:>5} {:>5} {:>5}  {:<6} {:<8} CMD",
            "UID", "PID", "PPID", "STAT", "TIME"
        );
    } else {
        let _ = writeln!(out, "{:>5} {:<8} CMD", "PID", "TTY");
    }

    let procfs = procinfo::ProcFs::new();
    let Ok(pids) = procfs.process_ids() else {
        // No /proc — nothing to show.
        return ExitCode::SUCCESS;
    };

    for pid in pids {
        let Ok(Some(info)) = read_one(&procfs, pid, parsed.full_format) else {
            // A process that exits between the listing and the read is the
            // normal case for anything walking /proc, not a failure.
            continue;
        };
        let pid32 = u32::try_from(pid).unwrap_or(0);

        if parsed.full_format {
            let _ = writeln!(
                out,
                "{:>5} {:>5} {:>5}  {:<6} {:<8} {}",
                info.uid, pid32, info.ppid, info.state, info.time_str, info.cmd
            );
        } else {
            let _ = writeln!(out, "{:>5} {:<8} {}", pid32, info.tty, info.comm);
        }
    }

    ExitCode::SUCCESS
}

/// One process, read through [`procinfo`].
///
/// The `/proc/<pid>/stat` parsing this used to do itself now lives in the
/// crate, shared with `userspace/ps`, `userspace/htop` and
/// `apps/procexplorer`. Two things it could not do on its own:
///
/// * **the real UID.** This was `uid: 0` with the comment "would need
///   `/proc/<pid>/status` for real UID" — so the `-f` listing showed every
///   process as root. The crate reads `status`, so it is a field now.
/// * **a name that is not UTF-8.** `read_to_string` fails on one, and the
///   process was skipped entirely by `continue`. A `ps` that omits exactly
///   the processes with unusual names is the worst way to be wrong.
///
/// # Errors
///
/// Any read error other than "no such file", which is `Ok(None)`.
fn read_one(procfs: &procinfo::ProcFs, pid: u64, full: bool) -> std::io::Result<Option<ProcInfo>> {
    let Some(stat) = procfs.process_stat(pid)? else {
        return Ok(None);
    };
    let comm = procinfo::display_bytes(&stat.comm);
    let uid = procfs
        .process_status(pid)?
        .and_then(|st| st.uid)
        .unwrap_or(0);
    // Only `-f` prints the command line, and reading it costs a second open
    // per process.
    let cmd = if full {
        let args = procfs.process_cmdline(pid)?.unwrap_or_default();
        if args.is_empty() {
            format!("[{comm}]")
        } else {
            args.iter()
                .map(|a| procinfo::display_bytes(a))
                .collect::<Vec<_>>()
                .join(" ")
        }
    } else {
        String::new()
    };
    Ok(Some(ProcInfo {
        comm,
        state: char::from(stat.state).to_string(),
        ppid: u32::try_from(stat.ppid).unwrap_or(0),
        uid,
        tty: format_tty(i32::try_from(stat.tty_nr).unwrap_or(0)),
        time_str: format_cpu_time(stat.utime_ticks, stat.stime_ticks),
        cmd,
    }))
}

/// Format the `tty_nr` field from /proc/<pid>/stat.  Zero is "?" (no
/// controlling terminal); otherwise we render it as `pts/<minor>`,
/// taking the low 8 bits as the minor number.
fn format_tty(tty_nr: i32) -> String {
    if tty_nr == 0 {
        "?".to_string()
    } else {
        format!("pts/{}", tty_nr & 0xff)
    }
}

/// Format CPU time (user + system clock ticks) as HH:MM:SS.
///
/// The tick rate is `procinfo::TICKS_PER_SEC` rather than a literal 100:
/// it is a fact about the kernel and belongs in one place.
fn format_cpu_time(utime: u64, stime: u64) -> String {
    let total_ticks = utime.saturating_add(stime);
    let total_secs = total_ticks / procinfo::TICKS_PER_SEC;
    let hours = total_secs / 3600;
    let mins = (total_secs / 60) % 60;
    let secs = total_secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    /// The listing options, unwrapped. Every one of these was valid before and
    /// still is; only the return type moved.
    fn listed(args: &[&str]) -> PsArgs {
        match parse_args(&s(args)) {
            Ok(Request::List(p)) => p,
            other => panic!("expected a listing request, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty() {
        assert_eq!(listed(&[]), PsArgs::default());
    }

    #[test]
    fn parse_dash_e() {
        let a = listed(&["-e"]);
        assert!(a.all_procs);
        assert!(!a.full_format);
    }

    #[test]
    fn parse_dash_a_uppercase_is_alias_for_e() {
        assert!(listed(&["-A"]).all_procs);
    }

    #[test]
    fn parse_dash_f() {
        let a = listed(&["-f"]);
        assert!(a.full_format);
        assert!(!a.all_procs);
    }

    #[test]
    fn parse_clustered_ef() {
        let a = listed(&["-ef"]);
        assert!(a.all_procs);
        assert!(a.full_format);
    }

    /// THIS TEST USED TO ASSERT THE BUG.
    ///
    /// It was called `parse_unknown_silently_ignored` and its comment read
    /// "Preserves previous behaviour — no error, no panic." What it preserved
    /// was `ps -Q` printing the whole process table and exiting 0 where procps
    /// prints `error: unsupported SysV option` and exits 1. A test can hold a
    /// defect in place as firmly as it holds a feature, and the only thing
    /// distinguishing the two is whether anyone measured.
    #[test]
    fn parse_unknown_short_option_is_refused() {
        let err = parse_args(&s(&["-Q"])).expect_err("-Q is not an option here");
        assert_eq!(err, "error: unsupported SysV option");
        // Still refused when clustered behind valid flags, which is where a
        // character-at-a-time parser is most likely to let one through.
        assert!(parse_args(&s(&["-efQ"])).is_err());
    }

    /// `--help` USED TO PRINT THE PROCESS TABLE.
    ///
    /// There was no long-option branch: `--help` had one `-` stripped and the
    /// rest was walked character by character, so it was read as `-h -e -l -p`
    /// and the `e` matched. The bug is not that `--help` was unimplemented, it
    /// is that a long option silently became whichever short flags its letters
    /// happened to spell.
    #[test]
    fn long_options_are_matched_whole_not_letter_by_letter() {
        assert_eq!(parse_args(&s(&["--help"])), Ok(Request::Help));
        // The three that would have set a flag by accident.
        for arg in ["--nosuchoption", "--full", "--version"] {
            let err = parse_args(&s(&[arg])).expect_err(arg);
            assert_eq!(err, "error: unknown gnu long option", "for {arg}");
        }
    }

    #[test]
    fn parse_bare_args_ignored() {
        // ps doesn't take positional arguments in our minimal build. procps
        // reads a PID list here, so refusing would reject a command line the
        // reference accepts -- deliberately unchanged.
        assert_eq!(listed(&["1234"]), PsArgs::default());
    }

    // ---------------- format_tty ----------------

    #[test]
    fn tty_zero_is_question_mark() {
        assert_eq!(format_tty(0), "?");
    }

    #[test]
    fn tty_nonzero_masked_to_byte() {
        assert_eq!(format_tty(34816), format!("pts/{}", 34816 & 0xff));
        assert_eq!(format_tty(1), "pts/1");
        assert_eq!(format_tty(255), "pts/255");
        assert_eq!(format_tty(256), "pts/0");
    }

    // ---------------- format_cpu_time ----------------

    #[test]
    fn cpu_time_zero() {
        assert_eq!(format_cpu_time(0, 0), "00:00:00");
    }

    #[test]
    fn cpu_time_one_second() {
        // 100 ticks at 100Hz = 1 second.
        assert_eq!(format_cpu_time(100, 0), "00:00:01");
    }

    #[test]
    fn cpu_time_user_plus_system() {
        // 50 + 50 = 100 ticks = 1 second.
        assert_eq!(format_cpu_time(50, 50), "00:00:01");
    }

    #[test]
    fn cpu_time_one_minute() {
        assert_eq!(format_cpu_time(60 * 100, 0), "00:01:00");
    }

    #[test]
    fn cpu_time_one_hour() {
        assert_eq!(format_cpu_time(3600 * 100, 0), "01:00:00");
    }

    #[test]
    fn cpu_time_hms_combined() {
        // 1h 23m 45s = 3600 + 1380 + 45 = 5025 sec = 502500 ticks.
        assert_eq!(format_cpu_time(502500, 0), "01:23:45");
    }

    #[test]
    fn cpu_time_overflow_saturates() {
        // utime + stime overflowing saturates to u64::MAX / 100.
        let s = format_cpu_time(u64::MAX, u64::MAX);
        // Should not panic; just produce a very large hour value.
        assert!(s.contains(':'));
    }

    // ---------------- parse_proc_stat ----------------

    // The `/proc/<pid>/stat` parsing tests that used to live here have moved to
    // `procinfo`, with the parser. They covered the minimal line, a `comm`
    // containing spaces, a `comm` containing parentheses, and pathological
    // input; `procinfo/src/tests.rs` covers all four and adds one this file
    // could not — a `comm` that is not valid UTF-8, which this program used to
    // drop the process for.
    //
    // **One behavioural difference is deliberate and worth naming.** The old
    // parser returned a defaulted `ProcInfo` for a truncated line such as
    // `"1 (a) S"`, so `ps` printed a row of zeros for it. `procinfo` returns
    // `None` for a line with fewer than 22 fields, so the process is skipped.
    // A three-field `stat` is not a process the kernel is describing; printing
    // a confident row of zeros for it is the same class of mistake as the CPU
    // bars' `max(1)`.
    //
    // What stays here is what is still this program's: the two formatters.

    #[test]
    fn no_controlling_terminal_is_a_question_mark() {
        assert_eq!(format_tty(0), "?");
    }

    /// The low eight bits are the minor number, which is why 34816 and 256
    /// both render as `pts/0` — the major is not shown.
    #[test]
    fn a_tty_renders_as_its_minor_number() {
        assert_eq!(format_tty(34816), "pts/0");
        assert_eq!(format_tty(34817), "pts/1");
        assert_eq!(format_tty(0x8_00_ff), "pts/255");
    }

    /// Ticks, not seconds. `procinfo::TICKS_PER_SEC` is the divisor, so this
    /// test also pins that the two agree.
    #[test]
    fn cpu_time_is_ticks_rendered_as_hms() {
        assert_eq!(format_cpu_time(0, 0), "00:00:00");
        assert_eq!(format_cpu_time(200, 100), "00:00:03");
        assert_eq!(procinfo::TICKS_PER_SEC, 100);
        // One hour, one minute, one second.
        let ticks = (3600 + 60 + 1) * procinfo::TICKS_PER_SEC;
        assert_eq!(format_cpu_time(ticks, 0), "01:01:01");
    }

    /// Hours are not wrapped at 24: a process can run for days, and `01:00:00`
    /// after 25 hours would be a lie.
    #[test]
    fn hours_accumulate_past_a_day() {
        let ticks = 25 * 3600 * procinfo::TICKS_PER_SEC;
        assert_eq!(format_cpu_time(ticks, 0), "25:00:00");
    }
}
