//! kill — send a signal to a process.
//!
//! Usage: kill [-SIGNAL] PID...
//!        kill -l
//!   Default signal is TERM (15).
//!   -l  list signal names.
//!
//! Note: our OS uses IPC messages rather than Unix signals for process
//! control, but we provide kill for POSIX compatibility. It calls the
//! POSIX-layer kill() which translates to the appropriate IPC message.

use std::env;
use std::process;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

const SIGNALS: &[(i32, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (6, "ABRT"),
    (9, "KILL"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
];

/// What action the parsed argv requests.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum KillAction {
    /// `-l` / `-L`: list signals and exit 0.
    ListSignals,
    /// Send `signal` to each pid.
    Send { signal: i32, pids: Vec<String> },
}

/// Resolve a signal token (the bit after `-`).  Accepts:
/// * a decimal signal number (e.g. `9`),
/// * a name with or without `SIG` prefix (case-insensitive, e.g. `KILL`, `sigterm`).
fn resolve_signal(token: &str) -> Option<i32> {
    if let Ok(n) = token.parse::<i32>() {
        return Some(n);
    }
    let name = token.strip_prefix("SIG").unwrap_or(token).to_uppercase();
    let name_alt = token.to_uppercase();
    let name_alt = name_alt.strip_prefix("SIG").unwrap_or(&name_alt).to_string();
    SIGNALS
        .iter()
        .find(|&&(_, n)| n == name || n == name_alt)
        .map(|&(num, _)| num)
}

/// Parse kill's argv into a `KillAction`.  Returns an error string suitable
/// for `eprintln!("kill: {e}")`.
fn parse_args(args: &[String]) -> Result<KillAction, String> {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }

    // -l / -L only if it's the very first argument.
    if let Some(first) = args.first()
        && (first == "-l" || first == "-L")
    {
        return Ok(KillAction::ListSignals);
    }

    let mut signal: i32 = 15;
    let mut pids: Vec<String> = Vec::new();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            let sig_str = arg.get(1..).unwrap_or("");
            match resolve_signal(sig_str) {
                Some(n) => signal = n,
                None => return Err(format!("unknown signal: {sig_str}")),
            }
        } else {
            pids.push(arg.clone());
        }
    }

    if pids.is_empty() {
        return Err("missing PID".to_string());
    }

    Ok(KillAction::Send { signal, pids })
}

/// Format the `-l` listing as a single string (one line per signal).
fn format_signal_list() -> String {
    let mut s = String::new();
    for &(num, name) in SIGNALS {
        s.push_str(&format!("{num:>2}) SIG{name}\n"));
    }
    s
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let action = match parse_args(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("kill: {e}");
            if e == "missing operand" {
                eprintln!("Usage: kill [-SIGNAL] PID...");
            }
            process::exit(1);
        }
    };

    match action {
        KillAction::ListSignals => {
            print!("{}", format_signal_list());
        }
        KillAction::Send { signal, pids } => {
            let mut exit_code = 0;
            for pid_str in &pids {
                let pid: i32 = match pid_str.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("kill: invalid PID: {pid_str}");
                        exit_code = 1;
                        continue;
                    }
                };
                #[cfg(target_os = "linux")]
                {
                    // SAFETY: kill() is provided by the POSIX layer, pid and sig
                    // are plain integers.
                    let ret = unsafe { kill(pid, signal) };
                    if ret != 0 {
                        eprintln!("kill: ({pid}) - No such process or permission denied");
                        exit_code = 1;
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (pid, signal);
                    eprintln!("kill: not supported on this platform");
                    exit_code = 1;
                }
            }
            process::exit(exit_code);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- resolve_signal ----------------

    #[test]
    fn resolve_number() {
        assert_eq!(resolve_signal("9"), Some(9));
        assert_eq!(resolve_signal("15"), Some(15));
    }

    #[test]
    fn resolve_name_bare() {
        assert_eq!(resolve_signal("KILL"), Some(9));
        assert_eq!(resolve_signal("TERM"), Some(15));
        assert_eq!(resolve_signal("HUP"), Some(1));
    }

    #[test]
    fn resolve_name_with_sig_prefix() {
        assert_eq!(resolve_signal("SIGKILL"), Some(9));
        assert_eq!(resolve_signal("SIGTERM"), Some(15));
    }

    #[test]
    fn resolve_name_lowercase() {
        assert_eq!(resolve_signal("kill"), Some(9));
        assert_eq!(resolve_signal("sigterm"), Some(15));
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert_eq!(resolve_signal("NOPE"), None);
        assert_eq!(resolve_signal("SIGNOPE"), None);
    }

    #[test]
    fn resolve_empty_returns_none() {
        assert_eq!(resolve_signal(""), None);
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_list_lower_l() {
        assert_eq!(parse_args(&s(&["-l"])).unwrap(), KillAction::ListSignals);
    }

    #[test]
    fn parse_list_upper_l() {
        assert_eq!(parse_args(&s(&["-L"])).unwrap(), KillAction::ListSignals);
    }

    #[test]
    fn parse_default_signal_is_term() {
        let act = parse_args(&s(&["1234"])).unwrap();
        match act {
            KillAction::Send { signal, pids } => {
                assert_eq!(signal, 15);
                assert_eq!(pids, vec!["1234"]);
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_numeric_signal() {
        let act = parse_args(&s(&["-9", "1234"])).unwrap();
        match act {
            KillAction::Send { signal, pids } => {
                assert_eq!(signal, 9);
                assert_eq!(pids, vec!["1234"]);
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_name_signal() {
        let act = parse_args(&s(&["-KILL", "1234"])).unwrap();
        match act {
            KillAction::Send { signal, .. } => assert_eq!(signal, 9),
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_sig_prefixed_name() {
        let act = parse_args(&s(&["-SIGTERM", "1"])).unwrap();
        match act {
            KillAction::Send { signal, .. } => assert_eq!(signal, 15),
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_signal_errors() {
        let err = parse_args(&s(&["-NOPE", "1"])).unwrap_err();
        assert!(err.contains("unknown signal"));
    }

    #[test]
    fn parse_missing_pid_errors() {
        let err = parse_args(&s(&["-9"])).unwrap_err();
        assert!(err.contains("missing PID"));
    }

    #[test]
    fn parse_multiple_pids() {
        let act = parse_args(&s(&["-INT", "100", "200", "300"])).unwrap();
        match act {
            KillAction::Send { signal, pids } => {
                assert_eq!(signal, 2);
                assert_eq!(pids, vec!["100", "200", "300"]);
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn parse_signal_after_pids_still_applies() {
        // Implementation scans all args; order doesn't matter for collecting.
        let act = parse_args(&s(&["100", "-9", "200"])).unwrap();
        match act {
            KillAction::Send { signal, pids } => {
                assert_eq!(signal, 9);
                assert_eq!(pids, vec!["100", "200"]);
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    // ---------------- format_signal_list ----------------

    #[test]
    fn format_signal_list_includes_all() {
        let listing = format_signal_list();
        for &(_, name) in SIGNALS {
            assert!(listing.contains(&format!("SIG{name}")));
        }
    }

    #[test]
    fn format_signal_list_one_line_each() {
        let listing = format_signal_list();
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(lines.len(), SIGNALS.len());
    }

    #[test]
    fn format_signal_list_includes_numbers() {
        let listing = format_signal_list();
        assert!(listing.contains(" 9) SIGKILL"));
        assert!(listing.contains("15) SIGTERM"));
    }
}
