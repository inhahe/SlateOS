//! renice — alter the scheduling priority of running processes.
//!
//! Usage: renice [-n INCREMENT] [-p PID...] [-u USER]
//!        renice PRIORITY [-p] PID...
//!   -n INCREMENT  adjust priority by INCREMENT (relative)
//!   -p PID        apply to process with given PID (default mode)
//!   -u USER       apply to all processes owned by USER
//!
//! Note: uses the POSIX-layer getpriority()/setpriority() syscalls.
//! Exit codes:
//!   0  success
//!   1  error

use std::env;
use std::process;

// POSIX priority target types.
const PRIO_PROCESS: i32 = 0;
const PRIO_PGRP: i32 = 1;
const PRIO_USER: i32 = 2;

unsafe extern "C" {
    fn getpriority(which: i32, who: u32) -> i32;
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("renice: missing operand");
        eprintln!("Usage: renice [-n INCREMENT] [-p PID...] [-u USER]");
        process::exit(1);
    }

    let mut increment: Option<i32> = None;
    let mut absolute: Option<i32> = None;
    let mut pids: Vec<u32> = Vec::new();
    let mut user: Option<String> = None;
    let mut i = 0;

    // First argument may be an absolute priority (old-style syntax).
    if !args[0].starts_with('-') || args[0].parse::<i32>().is_ok() {
        if let Ok(prio) = args[0].parse::<i32>() {
            absolute = Some(prio);
            i = 1;
        }
    }

    while i < args.len() {
        match args[i].as_str() {
            "-n" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("renice: option -n requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<i32>() {
                    Ok(n) => increment = Some(n),
                    Err(_) => {
                        eprintln!("renice: invalid increment: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("renice: option -p requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<u32>() {
                    Ok(pid) => pids.push(pid),
                    Err(_) => {
                        eprintln!("renice: invalid PID: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-u" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("renice: option -u requires an argument");
                    process::exit(1);
                }
                user = Some(args[i].clone());
            }
            arg => {
                // Bare argument: treat as PID.
                match arg.parse::<u32>() {
                    Ok(pid) => pids.push(pid),
                    Err(_) => {
                        eprintln!("renice: invalid PID: {arg}");
                        process::exit(1);
                    }
                }
            }
        }
        i += 1;
    }

    if pids.is_empty() && user.is_none() {
        eprintln!("renice: no PID or user specified");
        process::exit(1);
    }

    let mut exit_code = 0;

    // Handle user-based renice.
    if let Some(ref _username) = user {
        // On a real system, we would look up the UID from /etc/passwd.
        // For now, try interpreting as a numeric UID.
        let uid: u32 = match _username.parse() {
            Ok(u) => u,
            Err(_) => {
                eprintln!("renice: unknown user: {_username}");
                eprintln!("  (numeric UID required until /etc/passwd lookup is implemented)");
                process::exit(1);
            }
        };

        if let Err(e) = renice_target(PRIO_USER, uid, increment, absolute) {
            eprintln!("renice: user {_username}: {e}");
            exit_code = 1;
        }
    }

    // Handle PID-based renice.
    for pid in &pids {
        if let Err(e) = renice_target(PRIO_PROCESS, *pid, increment, absolute) {
            eprintln!("renice: PID {pid}: {e}");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

fn renice_target(
    which: i32,
    who: u32,
    increment: Option<i32>,
    absolute: Option<i32>,
) -> Result<(), String> {
    let new_prio = if let Some(abs) = absolute {
        abs
    } else if let Some(incr) = increment {
        // SAFETY: getpriority() is provided by the POSIX layer. The `which` and
        // `who` parameters are plain integers specifying the target. The return
        // value is the current priority or -1 on error (but -1 is also a valid
        // priority, so we cannot distinguish errors here without checking errno).
        let current = unsafe { getpriority(which, who) };
        current + incr
    } else {
        return Err("no priority adjustment specified".to_string());
    };

    // Clamp to valid range [-20, 19].
    let clamped = new_prio.max(-20).min(19);

    // SAFETY: setpriority() is provided by the POSIX layer. The parameters are
    // plain integers: `which` selects process/pgroup/user, `who` is the ID,
    // and `clamped` is the desired priority within [-20, 19].
    let ret = unsafe { setpriority(which, who, clamped) };
    if ret != 0 {
        return Err("failed to set priority (permission denied or no such process)".to_string());
    }

    let label = match which {
        PRIO_PROCESS => format!("PID {who}"),
        PRIO_PGRP => format!("PGRP {who}"),
        PRIO_USER => format!("user {who}"),
        _ => format!("target {who}"),
    };

    println!("{label}: old priority -> new priority {clamped}");
    Ok(())
}
