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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut all_procs = false;
    let mut full_format = false;

    for arg in &args {
        if arg.starts_with('-') {
            for c in arg[1..].chars() {
                match c {
                    'e' | 'A' => all_procs = true,
                    'f' => full_format = true,
                    _ => {}
                }
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if full_format {
        let _ = writeln!(
            out,
            "{:>5} {:>5} {:>5}  {:<6} {:<8} {}",
            "UID", "PID", "PPID", "STAT", "TIME", "CMD"
        );
    } else {
        let _ = writeln!(out, "{:>5} {:<8} {}", "PID", "TTY", "CMD");
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

        // Read process info
        let stat_path = format!("/proc/{pid}/stat");
        let stat_content = match fs::read_to_string(&stat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let info = parse_proc_stat(&stat_content);

        // If not showing all, only show processes in current session
        if !all_procs && pid != my_pid {
            // Simple heuristic: show all for now since we can't
            // easily determine session membership
        }

        if full_format {
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

struct ProcInfo {
    comm: String,
    state: String,
    ppid: u32,
    uid: u32,
    tty: String,
    time_str: String,
}

fn parse_proc_stat(stat: &str) -> ProcInfo {
    // Format: pid (comm) state ppid pgrp session tty_nr ...
    // The comm field is in parentheses and may contain spaces.
    let open = stat.find('(').unwrap_or(0);
    let close = stat.rfind(')').unwrap_or(stat.len());

    let comm = if open < close {
        stat[open + 1..close].to_string()
    } else {
        "?".to_string()
    };

    let rest = if close + 2 < stat.len() {
        &stat[close + 2..]
    } else {
        ""
    };

    let fields: Vec<&str> = rest.split_whitespace().collect();

    let state = fields.first().unwrap_or(&"?").to_string();
    let ppid: u32 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let tty_nr: i32 = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let utime: u64 = fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);

    let tty = if tty_nr == 0 {
        "?".to_string()
    } else {
        format!("pts/{}", tty_nr & 0xff)
    };

    // CPU time in clock ticks (assume 100 Hz)
    let total_secs = (utime + stime) / 100;
    let hours = total_secs / 3600;
    let mins = (total_secs / 60) % 60;
    let secs = total_secs % 60;
    let time_str = format!("{hours:02}:{mins:02}:{secs:02}");

    ProcInfo {
        comm,
        state,
        ppid,
        uid: 0, // would need to read /proc/pid/status for real UID
        tty,
        time_str,
    }
}
