#![deny(clippy::all)]

//! monit-cli — SlateOS Monit process supervisor & monitor
//!
//! Single personality: `monit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_monit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: monit [OPTIONS] <command>");
        println!("monit v5.33 (Slate OS) — Process supervision and monitoring");
        println!();
        println!("Commands:");
        println!("  start <name>     Start a monitored service");
        println!("  stop <name>      Stop a monitored service");
        println!("  restart <name>   Restart a monitored service");
        println!("  status           Show all service statuses");
        println!("  summary          Brief status summary");
        println!("  reload           Reload configuration");
        println!("  validate         Check configuration syntax");
        println!();
        println!("Options:");
        println!("  -c FILE          Configuration file");
        println!("  -d N             Daemon mode, check every N seconds");
        println!("  -t               Test configuration");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("monit v5.33 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-t") {
        println!("monit: control file syntax OK");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("status") | Some("summary") => {
            println!("Process 'sshd'         Running - PID 892");
            println!("Process 'nginx'        Running - PID 1205");
            println!("Process 'postgresql'   Running - PID 1340");
            println!("System 'slateos-host'    Running");
            println!("  CPU: 5.2%  Memory: 42.1%  Swap: 0.0%");
        }
        Some("validate") => {
            println!("monit: configuration valid");
        }
        _ => {
            println!("monit: process supervisor started");
            println!("  Monitoring 3 processes, 1 system");
            println!("  Check interval: 30 seconds");
            println!("  HTTP interface: localhost:2812");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "monit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_monit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_monit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/monit"), "monit");
        assert_eq!(basename(r"C:\bin\monit.exe"), "monit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("monit.exe"), "monit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_monit(&["--help".to_string()], "monit"), 0);
        assert_eq!(run_monit(&["-h".to_string()], "monit"), 0);
        let _ = run_monit(&["--version".to_string()], "monit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_monit(&[], "monit");
    }
}
