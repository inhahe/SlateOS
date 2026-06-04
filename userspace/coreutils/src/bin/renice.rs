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
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const PRIO_PGRP: i32 = 1;
const PRIO_USER: i32 = 2;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn getpriority(which: i32, who: u32) -> i32;
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ReniceArgs {
    increment: Option<i32>,
    absolute: Option<i32>,
    pids: Vec<u32>,
    user: Option<String>,
}

/// Parse renice's argv.  Returns an error string suitable for
/// `eprintln!("renice: {e}")` if a flag's value is missing or invalid.
fn parse_args(args: &[String]) -> Result<ReniceArgs, String> {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }

    let mut out = ReniceArgs::default();
    let mut i: usize = 0;

    // First arg may be an absolute priority (BSD-style: `renice 5 PID...`).
    if let Some(first) = args.first()
        && let Ok(prio) = first.parse::<i32>()
    {
        out.absolute = Some(prio);
        i = 1;
    }

    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        match arg.as_str() {
            "-n" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -n requires an argument".to_string())?;
                out.increment = Some(
                    v.parse::<i32>()
                        .map_err(|_| format!("invalid increment: {v}"))?,
                );
            }
            "-p" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -p requires an argument".to_string())?;
                let pid = v.parse::<u32>().map_err(|_| format!("invalid PID: {v}"))?;
                out.pids.push(pid);
            }
            "-u" => {
                i = i.saturating_add(1);
                let v = args
                    .get(i)
                    .ok_or_else(|| "option -u requires an argument".to_string())?;
                out.user = Some(v.clone());
            }
            other => {
                let pid = other
                    .parse::<u32>()
                    .map_err(|_| format!("invalid PID: {other}"))?;
                out.pids.push(pid);
            }
        }
        i = i.saturating_add(1);
    }

    if out.pids.is_empty() && out.user.is_none() {
        return Err("no PID or user specified".to_string());
    }

    Ok(out)
}

/// Clamp a priority value to POSIX's standard range.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn clamp_priority(p: i32) -> i32 {
    p.clamp(-20, 19)
}

/// Format the "target" label printed when a renice succeeds.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn target_label(which: i32, who: u32) -> String {
    match which {
        PRIO_PROCESS => format!("PID {who}"),
        PRIO_PGRP => format!("PGRP {who}"),
        PRIO_USER => format!("user {who}"),
        _ => format!("target {who}"),
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("renice: {e}");
            if e == "missing operand" {
                eprintln!("Usage: renice [-n INCREMENT] [-p PID...] [-u USER]");
            }
            process::exit(1);
        }
    };

    let increment = parsed.increment;
    let absolute = parsed.absolute;
    let pids = parsed.pids;
    let user = parsed.user;

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

#[cfg(target_os = "linux")]
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
        current.saturating_add(incr)
    } else {
        return Err("no priority adjustment specified".to_string());
    };

    let clamped = clamp_priority(new_prio);

    // SAFETY: setpriority() is provided by the POSIX layer. The parameters are
    // plain integers: `which` selects process/pgroup/user, `who` is the ID,
    // and `clamped` is the desired priority within [-20, 19].
    let ret = unsafe { setpriority(which, who, clamped) };
    if ret != 0 {
        return Err("failed to set priority (permission denied or no such process)".to_string());
    }

    println!("{}: old priority -> new priority {clamped}", target_label(which, who));
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn renice_target(
    _which: i32,
    _who: u32,
    _increment: Option<i32>,
    _absolute: Option<i32>,
) -> Result<(), String> {
    Err("renice not supported on this platform".to_string())
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
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_no_pid_no_user_errors() {
        // -n alone doesn't specify a target.
        let err = parse_args(&s(&["-n", "5"])).unwrap_err();
        assert!(err.contains("no PID or user specified"));
    }

    #[test]
    fn parse_absolute_then_pid() {
        // BSD style: `renice 5 1234`.
        let a = parse_args(&s(&["5", "1234"])).unwrap();
        assert_eq!(a.absolute, Some(5));
        assert_eq!(a.increment, None);
        assert_eq!(a.pids, vec![1234]);
    }

    #[test]
    fn parse_dash_n_increment() {
        let a = parse_args(&s(&["-n", "3", "-p", "1234"])).unwrap();
        assert_eq!(a.increment, Some(3));
        assert_eq!(a.absolute, None);
        assert_eq!(a.pids, vec![1234]);
    }

    #[test]
    fn parse_dash_n_negative() {
        let a = parse_args(&s(&["-n", "-5", "-p", "1234"])).unwrap();
        assert_eq!(a.increment, Some(-5));
    }

    #[test]
    fn parse_dash_n_invalid_errors() {
        let err = parse_args(&s(&["-n", "abc", "-p", "1"])).unwrap_err();
        assert!(err.contains("invalid increment"));
    }

    #[test]
    fn parse_dash_n_missing_value_errors() {
        let err = parse_args(&s(&["-n"])).unwrap_err();
        assert!(err.contains("-n"));
    }

    #[test]
    fn parse_dash_p_multiple() {
        let a = parse_args(&s(&["-n", "1", "-p", "100", "-p", "200"])).unwrap();
        assert_eq!(a.pids, vec![100, 200]);
    }

    #[test]
    fn parse_dash_p_invalid_errors() {
        let err = parse_args(&s(&["-n", "1", "-p", "notanumber"])).unwrap_err();
        assert!(err.contains("invalid PID"));
    }

    #[test]
    fn parse_dash_u_user() {
        let a = parse_args(&s(&["-n", "1", "-u", "alice"])).unwrap();
        assert_eq!(a.user.as_deref(), Some("alice"));
    }

    #[test]
    fn parse_bare_pid_treated_as_pid() {
        // No flag: bare arg is a PID.  `-n 1` provides the increment.
        let a = parse_args(&s(&["-n", "1", "5555"])).unwrap();
        assert_eq!(a.pids, vec![5555]);
    }

    #[test]
    fn parse_bare_nonnumeric_errors() {
        // Without an absolute first or a flag, a bare non-numeric is invalid.
        let err = parse_args(&s(&["-n", "1", "junk"])).unwrap_err();
        assert!(err.contains("invalid PID"));
    }

    #[test]
    fn parse_mixed_pids_and_user() {
        let a = parse_args(&s(&["-n", "2", "-p", "100", "-u", "bob", "200"])).unwrap();
        assert_eq!(a.increment, Some(2));
        assert_eq!(a.pids, vec![100, 200]);
        assert_eq!(a.user.as_deref(), Some("bob"));
    }

    // ---------------- clamp_priority ----------------

    #[test]
    fn clamp_within_range() {
        assert_eq!(clamp_priority(0), 0);
        assert_eq!(clamp_priority(-20), -20);
        assert_eq!(clamp_priority(19), 19);
        assert_eq!(clamp_priority(10), 10);
    }

    #[test]
    fn clamp_above_range() {
        assert_eq!(clamp_priority(20), 19);
        assert_eq!(clamp_priority(100), 19);
        assert_eq!(clamp_priority(i32::MAX), 19);
    }

    #[test]
    fn clamp_below_range() {
        assert_eq!(clamp_priority(-21), -20);
        assert_eq!(clamp_priority(-100), -20);
        assert_eq!(clamp_priority(i32::MIN), -20);
    }

    // ---------------- target_label ----------------

    #[test]
    fn label_process() {
        assert_eq!(target_label(PRIO_PROCESS, 42), "PID 42");
    }

    #[test]
    fn label_pgrp() {
        assert_eq!(target_label(PRIO_PGRP, 7), "PGRP 7");
    }

    #[test]
    fn label_user() {
        assert_eq!(target_label(PRIO_USER, 1000), "user 1000");
    }

    #[test]
    fn label_unknown() {
        assert_eq!(target_label(99, 5), "target 5");
    }
}
