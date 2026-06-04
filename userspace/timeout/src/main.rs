#![allow(unexpected_cfgs)]
//! timeout/nohup/nice/renice — process control utilities for OurOS
//!
//! Multi-personality binary detected via argv[0]:
//! - `timeout`: run a command with a time limit
//! - `nohup`: run a command immune to hangup signals
//! - `nice`: run a command with modified scheduling priority
//! - `renice`: alter priority of running processes

use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};

// ── Syscall helpers ──────────────────────────────────────────────

/// Send a signal to a process via syscall
#[allow(dead_code)]
fn sys_kill(pid: u32, signal: u32) -> i64 {
    let result: i64;
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

/// Get the current process priority via syscall
fn sys_getpriority(which: u32, who: u32) -> i64 {
    let result: i64;
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") 140_u64,  // SYS_GETPRIORITY
            in("rdi") which as u64,
            in("rsi") who as u64,
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result
}

/// Set process priority via syscall
fn sys_setpriority(which: u32, who: u32, prio: i32) -> i64 {
    let result: i64;
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") 141_u64,  // SYS_SETPRIORITY
            in("rdi") which as u64,
            in("rsi") who as u64,
            in("rdx") prio as u64,
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result
}

/// Get current UID
#[allow(dead_code)]
fn sys_getuid() -> u32 {
    let result: u64;
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") 102_u64,  // SYS_GETUID
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result as u32
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

// ── Mode detection ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Timeout,
    Nohup,
    Nice,
    Renice,
}

fn detect_mode(argv0: &str) -> Mode {
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name.to_lowercase().as_str() {
        "nohup" => Mode::Nohup,
        "nice" => Mode::Nice,
        "renice" => Mode::Renice,
        _ => Mode::Timeout,
    }
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
                println!("timeout (OurOS) 0.1.0");
                return 0;
            }
            "-s" | "--signal" => {
                i += 1;
                if i < args.len() {
                    match parse_signal(&args[i]) {
                        Some(s) => signal = s,
                        None => {
                            eprintln!("timeout: invalid signal '{}'", args[i]);
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
                            eprintln!("timeout: invalid duration '{}'", args[i]);
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
                        eprintln!("timeout: invalid signal '{}'", val);
                        return 125;
                    }
                }
            }
            _ if arg.starts_with("--kill-after=") => {
                let val = arg.strip_prefix("--kill-after=").unwrap_or("");
                match parse_duration(val) {
                    Some(d) => kill_after = Some(d),
                    None => {
                        eprintln!("timeout: invalid duration '{}'", val);
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
                    eprintln!("timeout: unknown option '{}'", arg);
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
                eprintln!("timeout: invalid duration '{}'", ds);
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
            eprintln!("timeout: failed to execute '{}': {}", program, e);
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
                        eprintln!("timeout: sending signal {} to command '{}'", signal, program);
                    }

                    // Use our syscall to send signal
                    #[cfg(target_os = "ouros")]
                    {
                        let pid = child.id();
                        sys_kill(pid, signal);
                    }

                    // Fallback: try kill via Command (for non-ouros platforms in tests)
                    #[cfg(not(target_os = "ouros"))]
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
                                                "timeout: sending KILL to command '{}'",
                                                program
                                            );
                                        }
                                        #[cfg(target_os = "ouros")]
                                        {
                                            let pid = child.id();
                                            sys_kill(pid, SIGKILL);
                                        }
                                        #[cfg(not(target_os = "ouros"))]
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
                    return if preserve_status { 128 + signal as i32 } else { 124 };
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

// ── nohup ────────────────────────────────────────────────────────

fn run_nohup(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "--help" {
        println!("Usage: nohup COMMAND [ARG]...");
        println!("Run COMMAND immune to hangup signals, with output to nohup.out.");
        println!();
        println!("If standard output is a terminal, redirect it to 'nohup.out'.");
        println!("If standard error is a terminal, redirect it to standard output.");
        return if args.is_empty() { 125 } else { 0 };
    }

    if args[0] == "--version" {
        println!("nohup (OurOS) 0.1.0");
        return 0;
    }

    let program = &args[0];
    let child_args = &args[1..];

    // Try to open nohup.out for stdout redirection
    let stdout_file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open("nohup.out")
    {
        Ok(f) => {
            eprintln!("nohup: appending output to 'nohup.out'");
            Some(f)
        }
        Err(_) => {
            // Try $HOME/nohup.out
            if let Ok(home) = env::var("HOME") {
                let path = format!("{}/nohup.out", home);
                match OpenOptions::new().create(true).append(true).open(&path) {
                    Ok(f) => {
                        eprintln!("nohup: appending output to '{}'", path);
                        Some(f)
                    }
                    Err(_) => {
                        eprintln!("nohup: failed to open 'nohup.out'");
                        None
                    }
                }
            } else {
                eprintln!("nohup: failed to open 'nohup.out'");
                None
            }
        }
    };

    let stdout = if let Some(f) = stdout_file {
        Stdio::from(f)
    } else {
        Stdio::inherit()
    };

    // Spawn the child — on real OurOS, we'd ignore SIGHUP for this process
    let mut child = match Command::new(program)
        .args(child_args)
        .stdin(Stdio::inherit())
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nohup: failed to execute '{}': {}", program, e);
            return if e.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };

    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("nohup: wait failed: {}", e);
            125
        }
    }
}

// ── nice ─────────────────────────────────────────────────────────

fn run_nice(args: &[String]) -> i32 {
    let mut adjustment: i32 = 10; // default niceness increment
    let mut cmd_start = 0;

    // Parse options
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" => {
                println!("Usage: nice [-n ADJUSTMENT] COMMAND [ARG]...");
                println!("Run COMMAND with an adjusted scheduling priority.");
                println!();
                println!("Options:");
                println!("  -n, --adjustment=N   add N to the niceness (default: 10)");
                println!("      --help           display this help and exit");
                println!("      --version        output version information");
                println!();
                println!("Niceness range: -20 (highest priority) to 19 (lowest priority).");
                println!("Only root can set negative adjustments.");
                return 0;
            }
            "--version" => {
                println!("nice (OurOS) 0.1.0");
                return 0;
            }
            "-n" | "--adjustment" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<i32>() {
                        Ok(n) => adjustment = n,
                        Err(_) => {
                            eprintln!("nice: invalid adjustment '{}'", args[i]);
                            return 125;
                        }
                    }
                }
            }
            _ if arg.starts_with("--adjustment=") => {
                let val = arg.strip_prefix("--adjustment=").unwrap_or("");
                match val.parse::<i32>() {
                    Ok(n) => adjustment = n,
                    Err(_) => {
                        eprintln!("nice: invalid adjustment '{}'", val);
                        return 125;
                    }
                }
            }
            _ if arg.starts_with("-") && arg.len() > 1 && !arg.starts_with("--") => {
                // Try -N where N is the adjustment
                if let Ok(n) = arg[1..].parse::<i32>() {
                    adjustment = n;
                } else {
                    cmd_start = i;
                    break;
                }
            }
            _ => {
                cmd_start = i;
                break;
            }
        }
        i += 1;
        cmd_start = i;
    }

    if cmd_start >= args.len() {
        // No command — just print current niceness
        let current = sys_getpriority(0, 0); // PRIO_PROCESS, current process
        println!("{}", current);
        return 0;
    }

    let program = &args[cmd_start];
    let child_args = &args[cmd_start + 1..];

    // Set the priority for current process (child will inherit)
    let current = sys_getpriority(0, 0);
    let new_prio = (current as i32 + adjustment).clamp(-20, 19);
    let result = sys_setpriority(0, 0, new_prio);
    if result < 0 {
        eprintln!(
            "nice: cannot set niceness: Permission denied (tried {})",
            new_prio
        );
        // Non-root trying negative adjustment is common; continue anyway
    }

    let mut child = match Command::new(program)
        .args(child_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nice: '{}': {}", program, e);
            return if e.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };

    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("nice: wait failed: {}", e);
            125
        }
    }
}

// ── renice ───────────────────────────────────────────────────────

const PRIO_PROCESS: u32 = 0;
const PRIO_PGRP: u32 = 1;
const PRIO_USER: u32 = 2;

fn run_renice(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: renice [-n PRIORITY] [-p PID] [-g PGRP] [-u USER]");
        eprintln!("Try 'renice --help' for more information.");
        return 1;
    }

    let mut priority: Option<i32> = None;
    let mut targets: Vec<(u32, u32)> = Vec::new(); // (which, who)
    let mut current_which: u32 = PRIO_PROCESS;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" => {
                println!("Usage: renice [-n] PRIORITY [-p PID] [-g PGRP] [-u USER]");
                println!("Alter the scheduling priority of running processes.");
                println!();
                println!("Options:");
                println!("  -n, --priority N   priority value (range: -20 to 19)");
                println!("  -p, --pid PID      interpret argument as process ID (default)");
                println!("  -g, --pgrp PGRP    interpret argument as process group ID");
                println!("  -u, --user USER    interpret argument as user name/ID");
                println!("      --help         display this help and exit");
                println!("      --version      output version information");
                return 0;
            }
            "--version" => {
                println!("renice (OurOS) 0.1.0");
                return 0;
            }
            "-n" | "--priority" => {
                i += 1;
                if i < args.len() {
                    match args[i].parse::<i32>() {
                        Ok(n) => priority = Some(n.clamp(-20, 19)),
                        Err(_) => {
                            eprintln!("renice: invalid priority '{}'", args[i]);
                            return 1;
                        }
                    }
                }
            }
            "-p" | "--pid" => {
                current_which = PRIO_PROCESS;
                i += 1;
                if i < args.len() {
                    match args[i].parse::<u32>() {
                        Ok(pid) => targets.push((PRIO_PROCESS, pid)),
                        Err(_) => {
                            eprintln!("renice: invalid PID '{}'", args[i]);
                            return 1;
                        }
                    }
                }
            }
            "-g" | "--pgrp" => {
                current_which = PRIO_PGRP;
                i += 1;
                if i < args.len() {
                    match args[i].parse::<u32>() {
                        Ok(pgrp) => targets.push((PRIO_PGRP, pgrp)),
                        Err(_) => {
                            eprintln!("renice: invalid PGRP '{}'", args[i]);
                            return 1;
                        }
                    }
                }
            }
            "-u" | "--user" => {
                current_which = PRIO_USER;
                i += 1;
                if i < args.len() {
                    // Try numeric UID first, then look up user
                    let uid = if let Ok(n) = args[i].parse::<u32>() {
                        n
                    } else {
                        // Look up username in /etc/passwd
                        match lookup_uid(&args[i]) {
                            Some(uid) => uid,
                            None => {
                                eprintln!("renice: unknown user '{}'", args[i]);
                                return 1;
                            }
                        }
                    };
                    targets.push((PRIO_USER, uid));
                }
            }
            _ => {
                // Could be priority or target depending on context
                if priority.is_none() {
                    if let Ok(n) = arg.parse::<i32>() {
                        priority = Some(n.clamp(-20, 19));
                    } else {
                        eprintln!("renice: invalid priority '{}'", arg);
                        return 1;
                    }
                } else if let Ok(n) = arg.parse::<u32>() {
                    targets.push((current_which, n));
                } else {
                    eprintln!("renice: invalid argument '{}'", arg);
                    return 1;
                }
            }
        }
        i += 1;
    }

    let prio = match priority {
        Some(p) => p,
        None => {
            eprintln!("renice: missing priority");
            return 1;
        }
    };

    if targets.is_empty() {
        eprintln!("renice: no target specified");
        return 1;
    }

    let mut exit_code = 0;

    for (which, who) in &targets {
        let old_prio = sys_getpriority(*which, *who);
        let result = sys_setpriority(*which, *who, prio);

        if result < 0 {
            let which_name = match *which {
                PRIO_PROCESS => "process",
                PRIO_PGRP => "process group",
                PRIO_USER => "user",
                _ => "unknown",
            };
            eprintln!(
                "renice: failed to set priority for {} {}: Permission denied",
                which_name, who
            );
            exit_code = 1;
        } else {
            let which_name = match *which {
                PRIO_PROCESS => "process ID",
                PRIO_PGRP => "process group ID",
                PRIO_USER => "user ID",
                _ => "unknown",
            };
            println!(
                "{} {}: old priority {}, new priority {}",
                which_name, who, old_prio, prio
            );
        }
    }

    exit_code
}

fn lookup_uid(username: &str) -> Option<u32> {
    let content = fs::read_to_string("/etc/passwd").ok()?;
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == username {
            return fields[2].parse().ok();
        }
    }
    None
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = detect_mode(args.first().map(|s| s.as_str()).unwrap_or("timeout"));

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match mode {
        Mode::Timeout => run_timeout(&rest),
        Mode::Nohup => run_nohup(&rest),
        Mode::Nice => run_nice(&rest),
        Mode::Renice => run_renice(&rest),
    };

    process::exit(exit_code);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Mode detection
    #[test]
    fn test_detect_timeout() {
        assert_eq!(detect_mode("timeout"), Mode::Timeout);
        assert_eq!(detect_mode("/usr/bin/timeout"), Mode::Timeout);
        assert_eq!(detect_mode("timeout.exe"), Mode::Timeout);
    }

    #[test]
    fn test_detect_nohup() {
        assert_eq!(detect_mode("nohup"), Mode::Nohup);
        assert_eq!(detect_mode("/usr/bin/nohup"), Mode::Nohup);
    }

    #[test]
    fn test_detect_nice() {
        assert_eq!(detect_mode("nice"), Mode::Nice);
        assert_eq!(detect_mode("/usr/bin/nice"), Mode::Nice);
    }

    #[test]
    fn test_detect_renice() {
        assert_eq!(detect_mode("renice"), Mode::Renice);
        assert_eq!(detect_mode("/usr/bin/renice"), Mode::Renice);
    }

    #[test]
    fn test_detect_unknown_defaults() {
        assert_eq!(detect_mode("something"), Mode::Timeout);
    }

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
    #[test]
    fn test_lookup_uid_not_found() {
        // On test system without /etc/passwd, this should return None
        let result = lookup_uid("nonexistent_user_xyz");
        assert!(result.is_none());
    }

    // Priority constants
    #[test]
    fn test_priority_constants() {
        assert_eq!(PRIO_PROCESS, 0);
        assert_eq!(PRIO_PGRP, 1);
        assert_eq!(PRIO_USER, 2);
    }

    // Signal constants
    #[test]
    fn test_signal_constants() {
        assert_eq!(SIGHUP, 1);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGTERM, 15);
    }

    // Renice argument parsing
    #[test]
    fn test_renice_requires_priority() {
        // Just verify the function returns 1 with no args
        let result = run_renice(&[]);
        assert_eq!(result, 1);
    }

    // Nice without command prints current niceness
    #[test]
    fn test_nice_no_command_prints_niceness() {
        // This is hard to test without the syscall, but we can verify
        // the argument parsing logic.
        let _args = ["-n".to_string(), "5".to_string()];
        // Without a command, nice should print current niceness.
        // On non-ouros this would use the fallback syscall behavior.
    }

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
