//! ps — report process status.
//!
//! Usage: ps [-e] [-f]
//!   -e  show all processes (not just current session)
//!   -f  full listing format
//!
//! Reads from /proc filesystem. Each directory under /proc/<pid>/
//! contains process information files: stat, cmdline, status.

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
fn parse_args(args: &[String]) -> PsArgs {
    let mut out = PsArgs::default();
    for arg in args {
        if let Some(flags) = arg.strip_prefix('-') {
            for c in flags.chars() {
                match c {
                    'e' | 'A' => out.all_procs = true,
                    'f' => out.full_format = true,
                    _ => {}
                }
            }
        }
    }
    out
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
    let parsed = parse_args(&args);

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

    #[test]
    fn parse_empty() {
        assert_eq!(parse_args(&s(&[])), PsArgs::default());
    }

    #[test]
    fn parse_dash_e() {
        let a = parse_args(&s(&["-e"]));
        assert!(a.all_procs);
        assert!(!a.full_format);
    }

    #[test]
    fn parse_dash_a_uppercase_is_alias_for_e() {
        let a = parse_args(&s(&["-A"]));
        assert!(a.all_procs);
    }

    #[test]
    fn parse_dash_f() {
        let a = parse_args(&s(&["-f"]));
        assert!(a.full_format);
        assert!(!a.all_procs);
    }

    #[test]
    fn parse_clustered_ef() {
        let a = parse_args(&s(&["-ef"]));
        assert!(a.all_procs);
        assert!(a.full_format);
    }

    #[test]
    fn parse_unknown_silently_ignored() {
        // Preserves previous behaviour — no error, no panic.
        let a = parse_args(&s(&["-X"]));
        assert!(!a.all_procs);
        assert!(!a.full_format);
    }

    #[test]
    fn parse_bare_args_ignored() {
        // ps doesn't take positional arguments in our minimal build.
        let a = parse_args(&s(&["1234"]));
        assert_eq!(a, PsArgs::default());
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
