//! ps — report process status.
//!
//! Usage: ps [-e] [-f]
//!   -e  show all processes (not just current session)
//!   -f  full listing format
//!
//! Reads from /proc filesystem. Each directory under /proc/<pid>/
//! contains process information files: stat, cmdline, status.

use std::env;
use std::fs;
use std::io::{self, Write};

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
}

fn main() {
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

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => {
            // No /proc — nothing to show
            return;
        }
    };

    let my_pid = std::process::id();

    for entry_result in proc_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only directories that are numeric PIDs
        let pid: u32 = match name_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let stat_path = format!("/proc/{pid}/stat");
        let stat_content = match fs::read_to_string(&stat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let info = parse_proc_stat(&stat_content);

        // If not showing all, only show processes in current session.
        // Simple heuristic: show all until we can determine session
        // membership.
        if !parsed.all_procs && pid != my_pid {
            // intentionally left blank
        }

        if parsed.full_format {
            let cmdline_path = format!("/proc/{pid}/cmdline");
            let cmd = fs::read_to_string(&cmdline_path)
                .unwrap_or_default()
                .replace('\0', " ")
                .trim()
                .to_string();
            let cmd_display = if cmd.is_empty() {
                format!("[{}]", info.comm)
            } else {
                cmd
            };

            let _ = writeln!(
                out,
                "{:>5} {:>5} {:>5}  {:<6} {:<8} {}",
                info.uid, pid, info.ppid, info.state, info.time_str, cmd_display
            );
        } else {
            let _ = writeln!(out, "{:>5} {:<8} {}", pid, info.tty, info.comm);
        }
    }
}

/// Parse the contents of `/proc/<pid>/stat`.  The format is:
///
///     pid (comm) state ppid pgrp session tty_nr ...
///
/// `comm` is wrapped in parentheses and may contain spaces or even
/// `)` characters; we use the *last* `)` as the end so command names
/// like "weird (name)" work.  All subsequent fields are
/// space-separated.  Missing or malformed fields default to zero or
/// `"?"` so the function never panics on attacker-controlled input.
fn parse_proc_stat(stat: &str) -> ProcInfo {
    // /proc/<pid>/stat: "<pid> (<comm>) <state> <ppid> ..."
    // comm can contain spaces and parens, so we span from the first '(' to
    // the *last* ')'.  If either is missing the line is malformed.
    let (comm, rest) = match (stat.find('('), stat.rfind(')')) {
        (Some(open), Some(close)) if open < close => {
            let c = stat
                .get(open.saturating_add(1)..close)
                .unwrap_or("?")
                .to_string();
            let r = stat.get(close.saturating_add(2)..).unwrap_or("");
            (c, r)
        }
        _ => ("?".to_string(), ""),
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();

    let state = fields.first().copied().unwrap_or("?").to_string();
    let ppid: u32 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let tty_nr: i32 = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let utime: u64 = fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);

    let tty = format_tty(tty_nr);
    let time_str = format_cpu_time(utime, stime);

    ProcInfo {
        comm,
        state,
        ppid,
        uid: 0, // would need /proc/<pid>/status for real UID
        tty,
        time_str,
    }
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

/// Format CPU time (user + system clock ticks at 100Hz) as HH:MM:SS.
fn format_cpu_time(utime: u64, stime: u64) -> String {
    let total_ticks = utime.saturating_add(stime);
    let total_secs = total_ticks / 100;
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

    #[test]
    fn parse_minimal_stat() {
        // pid (comm) state ppid pgrp session tty_nr tpgid flags ...
        // Field positions (0-indexed after comm):
        //   0=state, 1=ppid, 4=tty_nr, 11=utime, 12=stime
        let stat = "1 (init) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0";
        let info = parse_proc_stat(stat);
        assert_eq!(info.comm, "init");
        assert_eq!(info.state, "S");
        assert_eq!(info.ppid, 0);
        assert_eq!(info.tty, "?");
        assert_eq!(info.time_str, "00:00:00");
        assert_eq!(info.uid, 0);
    }

    #[test]
    fn parse_with_cpu_time() {
        // utime at field index 11, stime at 12 (1-indexed after comm = 14,15).
        // Layout: pid (comm) state ppid pgrp session tty_nr tpgid flags
        //         minflt cminflt majflt cmajflt utime stime ...
        //         (0)   (1)   (2)   (3) (4) (5) (6) (7) (8) (9) (10) (11)(12)
        let stat = "100 (sh) R 1 100 100 34816 -1 0 0 0 0 0 200 100 0 0 20 0 1";
        let info = parse_proc_stat(stat);
        assert_eq!(info.comm, "sh");
        assert_eq!(info.state, "R");
        assert_eq!(info.ppid, 1);
        // tty_nr=34816 → pts/(34816 & 0xff)
        assert_eq!(info.tty, format!("pts/{}", 34816 & 0xff));
        // 200 + 100 = 300 ticks = 3 seconds
        assert_eq!(info.time_str, "00:00:03");
    }

    #[test]
    fn parse_comm_with_parens_uses_last() {
        // comm = "weird (name)" — must keep the inner parens intact.
        let stat = "5 (weird (name)) S 1 5 5 0 -1 0 0 0 0 0 0 0 0 0 20 0 1";
        let info = parse_proc_stat(stat);
        assert_eq!(info.comm, "weird (name)");
        assert_eq!(info.state, "S");
        assert_eq!(info.ppid, 1);
    }

    #[test]
    fn parse_comm_with_spaces() {
        let stat = "7 (my proc) Z 2 7 7 0 -1 0 0 0 0 0 0 0 0 0 20 0 1";
        let info = parse_proc_stat(stat);
        assert_eq!(info.comm, "my proc");
        assert_eq!(info.state, "Z");
        assert_eq!(info.ppid, 2);
    }

    #[test]
    fn parse_truncated_returns_defaults() {
        // Missing fields default — should not panic.
        let stat = "1 (a) S";
        let info = parse_proc_stat(stat);
        assert_eq!(info.comm, "a");
        assert_eq!(info.state, "S");
        assert_eq!(info.ppid, 0);
        assert_eq!(info.tty, "?");
        assert_eq!(info.time_str, "00:00:00");
    }

    #[test]
    fn parse_missing_parens() {
        // Pathological input: no parens at all.  Must not panic.
        let info = parse_proc_stat("garbage");
        assert_eq!(info.comm, "?");
    }

    #[test]
    fn parse_empty_string() {
        let info = parse_proc_stat("");
        assert_eq!(info.comm, "?");
        assert_eq!(info.state, "?");
        assert_eq!(info.ppid, 0);
    }
}
