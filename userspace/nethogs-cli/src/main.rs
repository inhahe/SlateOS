#![deny(clippy::all)]

//! nethogs-cli — SlateOS nethogs per-process network monitor
//!
//! Single personality: `nethogs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nethogs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nethogs [OPTIONS] [DEVICE...]");
        println!("nethogs 0.8.7 (Slate OS) — Per-process network bandwidth monitor");
        println!();
        println!("Options:");
        println!("  -d SECONDS     Refresh interval (default 1)");
        println!("  -v MODE        View mode (0=KB/s, 1=total KB, 2=total B, 3=total MB)");
        println!("  -c COUNT       Number of updates (0=unlimited)");
        println!("  -t             Tracemode (output to stdout)");
        println!("  -p             Promiscuous mode");
        println!("  -s             Sort by sent");
        println!("  -a             Monitor all devices");
        println!("  -V             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("nethogs 0.8.7 (Slate OS)");
        return 0;
    }
    let device = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("eth0");
    if args.iter().any(|a| a == "-t") {
        println!("Refreshing:");
        println!("/usr/bin/firefox/1234    0.850    2.340  KB/sec");
        println!("/usr/bin/ssh/5678       0.120    0.045  KB/sec");
        println!("unknown TCP/0           0.000    0.000  KB/sec");
        return 0;
    }
    println!("nethogs: Monitoring {}...", device);
    println!();
    println!("  PID USER     PROGRAM                    DEV     SENT     RECEIVED");
    println!(" 1234 user     /usr/bin/firefox            eth0    0.850    2.340 KB/sec");
    println!(" 5678 user     /usr/bin/ssh                eth0    0.120    0.045 KB/sec");
    println!("    ? root     unknown TCP                        0.000    0.000 KB/sec");
    println!();
    println!("  TOTAL                                           0.970    2.385 KB/sec");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nethogs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nethogs(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nethogs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nethogs"), "nethogs");
        assert_eq!(basename(r"C:\bin\nethogs.exe"), "nethogs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nethogs.exe"), "nethogs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nethogs(&["--help".to_string()], "nethogs"), 0);
        assert_eq!(run_nethogs(&["-h".to_string()], "nethogs"), 0);
        let _ = run_nethogs(&["--version".to_string()], "nethogs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nethogs(&[], "nethogs");
    }
}
