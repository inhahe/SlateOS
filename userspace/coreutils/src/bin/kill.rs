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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("kill: missing operand");
        eprintln!("Usage: kill [-SIGNAL] PID...");
        process::exit(1);
    }

    // Handle -l (list signals)
    if args[0] == "-l" || args[0] == "-L" {
        for &(num, name) in SIGNALS {
            println!("{num:>2}) SIG{name}");
        }
        return;
    }

    let mut sig: i32 = 15; // SIGTERM
    let mut pids: Vec<&str> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            let sig_str = &arg[1..];
            // Try as number
            if let Ok(n) = sig_str.parse::<i32>() {
                sig = n;
                continue;
            }
            // Try as signal name (with or without SIG prefix)
            let name = sig_str
                .strip_prefix("SIG")
                .unwrap_or(sig_str)
                .to_uppercase();
            if let Some(&(num, _)) = SIGNALS.iter().find(|&&(_, n)| n == name) {
                sig = num;
                continue;
            }
            eprintln!("kill: unknown signal: {sig_str}");
            process::exit(1);
        } else {
            pids.push(arg);
        }
    }

    if pids.is_empty() {
        eprintln!("kill: missing PID");
        process::exit(1);
    }

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

        // SAFETY: kill() is provided by the POSIX layer, pid and sig are
        // plain integers.
        let ret = unsafe { kill(pid, sig) };
        if ret != 0 {
            eprintln!("kill: ({pid}) - No such process or permission denied");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
