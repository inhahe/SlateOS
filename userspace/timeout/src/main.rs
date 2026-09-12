//! `timeout` — run a command with a time limit, for Slate OS
//!
//! This crate used to answer to `nohup`, `nice` and `renice` as well, via
//! argv[0]. Those three are gone: `userspace/coreutils` already had all
//! three as real binaries, and its versions are the ones with a
//! differential harness behind them (`scripts/nohup-diff.sh`,
//! `scripts/nice-diff.sh`). Measured against GNU with an unrecognised
//! option, the coreutils binaries matched it exactly and the personalities
//! here did not:
//!
//! | | coreutils (kept) | personality (removed) |
//! |---|---|---|
//! | `nohup --bogus`  | exit 125, `unrecognized option` | exit 127, and it created `nohup.out` |
//! | `nice --bogus`   | exit 125, `unrecognized option` | exit 127, `cannot set niceness (tried -20)` |
//! | `renice --bogus` | exit 1, `not enough arguments`  | exit 1, `invalid priority` |
//!
//! The `nice` line is the reason this was not left alone: an option it did
//! not recognise became a request to renice to -20. See design-decisions.md
//! §1005/§1006 — coreutils is the one home for a coreutils command.

use quoting::quoteaf_os;
use std::env;
use std::io;
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};

// ── Syscall helpers ──────────────────────────────────────────────
//
// Every raw `syscall` below is gated on `target_vendor = "slateos"`, which is
// true only when compiling for the real OS (see toolchain/x86_64-slateos.json).
//
// This gate is not a formality. A `syscall` instruction on a development host
// does not fail cleanly — it enters whatever kernel is actually running, with
// our SlateOS call number sitting in RAX, and those numbers mean *other
// things* elsewhere. This file is the worst offender in the tree: on Linux,
// RAX=62 is `kill(2)`, 140 is `getpriority(2)` and 141 is `setpriority(2)` —
// the very calls we intend, aimed at a real process on the developer's own
// machine. `cargo run -p timeout` on the Linux dev host would genuinely
// signal a process. The host arms below return `ENOSYS` instead.
//
// See known-issues.md
// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.

/// Send a signal to a process via syscall.
#[allow(dead_code)]
fn sys_kill(pid: u32, signal: u32) -> i64 {
    #[cfg(target_vendor = "slateos")]
    {
        let result: i64;
        // SAFETY: SYS_KILL takes two scalars and touches no userspace memory.
        // rcx/r11 are clobbered by the SYSCALL instruction per the x86_64 ABI.
        unsafe {
            std::arch::asm!(
                "syscall",
                in("rax") 62_u64,  // SYS_KILL
                in("rdi") pid as u64,
                in("rsi") signal as u64,
                lateout("rax") result,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        result
    }
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (pid, signal);
        -38 // ENOSYS
    }
}

// ── Signal constants ─────────────────────────────────────────────

const SIGHUP: u32 = 1;
const SIGINT: u32 = 2;
const SIGQUIT: u32 = 3;
const SIGKILL: u32 = 9;
const SIGTERM: u32 = 15;
const SIGALRM: u32 = 14;
const SIGUSR1: u32 = 10;
const SIGUSR2: u32 = 12;

fn parse_signal(name: &str) -> Option<u32> {
    // Try numeric first
    if let Ok(n) = name.parse::<u32>() {
        if n <= 31 {
            return Some(n);
        }
        return None;
    }

    let upper = name.to_uppercase();
    let signame = upper.strip_prefix("SIG").unwrap_or(&upper);

    match signame {
        "HUP" => Some(SIGHUP),
        "INT" => Some(SIGINT),
        "QUIT" => Some(SIGQUIT),
        "KILL" => Some(SIGKILL),
        "TERM" => Some(SIGTERM),
        "ALRM" | "ALARM" => Some(SIGALRM),
        "USR1" => Some(SIGUSR1),
        "USR2" => Some(SIGUSR2),
        _ => None,
    }
}

// ── Duration parsing ─────────────────────────────────────────────

fn parse_duration(s: &str) -> Option<Duration> {
    if s.is_empty() {
        return None;
    }

    // Check for suffix
    let (num_part, multiplier) = if let Some(n) = s.strip_suffix('s') {
        (n, 1.0_f64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600.0)
    } else if let Some(n) = s.strip_suffix('d') {
        (n, 86400.0)
    } else {
        (s, 1.0) // default is seconds
    };

    let value: f64 = num_part.parse().ok()?;
    if value < 0.0 {
        return None;
    }

    let total_secs = value * multiplier;
    Some(Duration::from_secs_f64(total_secs))
}

// ── timeout ──────────────────────────────────────────────────────

fn run_timeout(args: &[String]) -> i32 {
    let mut signal = SIGTERM;
    let mut kill_after: Option<Duration> = None;
    let mut _foreground = false;
    let mut preserve_status = false;
    let mut verbose = false;
    let mut duration_str: Option<String> = None;
    let mut cmd_args: Vec<String> = Vec::new();

    let mut i = 0;
    let mut past_options = false;

    while i < args.len() {
        let arg = &args[i];

        if past_options {
            cmd_args.push(arg.clone());
            i += 1;
            continue;
        }

        match arg.as_str() {
            "--help" => {
                println!("Usage: timeout [OPTION] DURATION COMMAND [ARG]...");
                println!("Start COMMAND, and kill it if still running after DURATION.");
                println!();
                println!("Options:");
                println!("  -s, --signal=SIGNAL    specify signal to send (default: TERM)");
                println!("  -k, --kill-after=DUR   also send KILL signal after DUR");
                println!("  --foreground           don't create a new process group");
                println!("  --preserve-status      exit with the same status as COMMAND");
                println!("  -v, --verbose          diagnose signals sent on timeout");
                println!("  --help                 display this help and exit");
                println!("  --version              output version information");
                println!();
                println!("DURATION is a number with optional suffix: s (seconds, default),");
                println!("m (minutes), h (hours), d (days). Fractional values allowed.");
                return 0;
            }
            "--version" => {
                println!("timeout (Slate OS) 0.1.0");
                return 0;
            }
            "-s" | "--signal" => {
                i += 1;
                if i < args.len() {
                    match parse_signal(&args[i]) {
                        Some(s) => signal = s,
                        None => {
                            eprintln!("timeout: invalid signal {}", quoteaf_os(&args[i]));
                            return 125;
                        }
                    }
                }
            }
            "-k" | "--kill-after" => {
                i += 1;
                if i < args.len() {
                    match parse_duration(&args[i]) {
                        Some(d) => kill_after = Some(d),
                        None => {
                            eprintln!("timeout: invalid duration {}", quoteaf_os(&args[i]));
                            return 125;
                        }
                    }
                }
            }
            "--foreground" => _foreground = true,
            "--preserve-status" => preserve_status = true,
            "-v" | "--verbose" => verbose = true,
            "--" => {
                past_options = true;
            }
            _ if arg.starts_with("--signal=") => {
                let val = arg.strip_prefix("--signal=").unwrap_or("");
                match parse_signal(val) {
                    Some(s) => signal = s,
                    None => {
                        eprintln!("timeout: invalid signal {}", quoteaf_os(val));
                        return 125;
                    }
                }
            }
            _ if arg.starts_with("--kill-after=") => {
                let val = arg.strip_prefix("--kill-after=").unwrap_or("");
                match parse_duration(val) {
                    Some(d) => kill_after = Some(d),
                    None => {
                        eprintln!("timeout: invalid duration {}", quoteaf_os(val));
                        return 125;
                    }
                }
            }
            _ if arg.starts_with('-') && arg.len() == 2 => {
                // -N where N is a signal number
                if let Ok(n) = arg[1..].parse::<u32>() {
                    if n <= 31 {
                        signal = n;
                    }
                } else {
                    eprintln!("timeout: unknown option {}", quoteaf_os(arg));
                    return 125;
                }
            }
            _ => {
                if duration_str.is_none() {
                    duration_str = Some(arg.clone());
                } else {
                    cmd_args.push(arg.clone());
                    past_options = true; // Once we hit command, rest are args
                }
            }
        }
        i += 1;
    }

    let duration = match duration_str {
        Some(ref ds) => match parse_duration(ds) {
            Some(d) => d,
            None => {
                eprintln!("timeout: invalid duration {}", quoteaf_os(ds));
                return 125;
            }
        },
        None => {
            eprintln!("timeout: missing duration");
            eprintln!("Try 'timeout --help' for more information.");
            return 125;
        }
    };

    if cmd_args.is_empty() {
        eprintln!("timeout: missing command");
        eprintln!("Try 'timeout --help' for more information.");
        return 125;
    }

    // Spawn the child process
    let program = &cmd_args[0];
    let child_args = &cmd_args[1..];

    let mut child = match Command::new(program)
        .args(child_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("timeout: failed to execute {}: {}", quoteaf_os(program), e);
            // 126 = command found but not executable, 127 = not found
            return if e.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };

    let start = Instant::now();

    // Poll the child with timeout
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child finished
                if preserve_status {
                    return status.code().unwrap_or(1);
                }
                return status.code().unwrap_or(1);
            }
            Ok(None) => {
                // Still running — check timeout
                if start.elapsed() >= duration {
                    // Timeout expired — send signal
                    if verbose {
                        eprintln!(
                            "timeout: sending signal {} to command {}",
                            signal,
                            quoteaf_os(program)
                        );
                    }

                    // Use our syscall to send signal
                    #[cfg(target_vendor = "slateos")]
                    {
                        let pid = child.id();
                        sys_kill(pid, signal);
                    }

                    // Fallback: try kill via Command (for non-slateos platforms in tests)
                    #[cfg(not(target_vendor = "slateos"))]
                    {
                        let _ = child.kill();
                    }

                    // If kill-after is set, wait and then send KILL
                    if let Some(ka) = kill_after {
                        let kill_start = Instant::now();
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    if preserve_status {
                                        return status.code().unwrap_or(137);
                                    }
                                    return 124;
                                }
                                Ok(None) => {
                                    if kill_start.elapsed() >= ka {
                                        if verbose {
                                            eprintln!(
                                                "timeout: sending KILL to command {}",
                                                quoteaf_os(program)
                                            );
                                        }
                                        #[cfg(target_vendor = "slateos")]
                                        {
                                            let pid = child.id();
                                            sys_kill(pid, SIGKILL);
                                        }
                                        #[cfg(not(target_vendor = "slateos"))]
                                        {
                                            let _ = child.kill();
                                        }
                                        let _ = child.wait();
                                        return if preserve_status { 137 } else { 124 };
                                    }
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                                Err(_) => return 124,
                            }
                        }
                    }

                    // Wait for child to die
                    let _ = child.wait();
                    return if preserve_status {
                        128 + signal as i32
                    } else {
                        124
                    };
                }

                // Sleep briefly before polling again
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                return 125;
            }
        }
    }
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_timeout(&rest));
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Mode detection

    // Duration parsing
    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_duration("5s"), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
    }

    #[test]
    fn test_parse_duration_fractional() {
        let d = parse_duration("1.5s").unwrap();
        assert_eq!(d, Duration::from_millis(1500));
    }

    #[test]
    fn test_parse_duration_zero() {
        assert_eq!(parse_duration("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn test_parse_duration_empty() {
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn test_parse_duration_negative() {
        assert_eq!(parse_duration("-5"), None);
    }

    // Signal parsing
    #[test]
    fn test_parse_signal_name() {
        assert_eq!(parse_signal("TERM"), Some(SIGTERM));
        assert_eq!(parse_signal("KILL"), Some(SIGKILL));
        assert_eq!(parse_signal("HUP"), Some(SIGHUP));
        assert_eq!(parse_signal("INT"), Some(SIGINT));
    }

    #[test]
    fn test_parse_signal_with_sig_prefix() {
        assert_eq!(parse_signal("SIGTERM"), Some(SIGTERM));
        assert_eq!(parse_signal("SIGKILL"), Some(SIGKILL));
    }

    #[test]
    fn test_parse_signal_numeric() {
        assert_eq!(parse_signal("9"), Some(9));
        assert_eq!(parse_signal("15"), Some(15));
    }

    #[test]
    fn test_parse_signal_case_insensitive() {
        assert_eq!(parse_signal("term"), Some(SIGTERM));
        assert_eq!(parse_signal("kill"), Some(SIGKILL));
    }

    #[test]
    fn test_parse_signal_invalid() {
        assert_eq!(parse_signal("BOGUS"), None);
        assert_eq!(parse_signal("99"), None);
    }

    // UID lookup

    // Priority constants

    // Signal constants
    #[test]
    fn test_signal_constants() {
        assert_eq!(SIGHUP, 1);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGTERM, 15);
    }

    // Renice argument parsing

    // Nice without command prints current niceness

    // Duration edge cases
    #[test]
    fn test_parse_duration_large() {
        let d = parse_duration("365d").unwrap();
        assert_eq!(d, Duration::from_secs(365 * 86400));
    }

    #[test]
    fn test_parse_duration_small_fraction() {
        let d = parse_duration("0.1s").unwrap();
        assert_eq!(d, Duration::from_millis(100));
    }

    #[test]
    fn test_parse_duration_minutes_fraction() {
        let d = parse_duration("0.5m").unwrap();
        assert_eq!(d, Duration::from_secs(30));
    }
}
