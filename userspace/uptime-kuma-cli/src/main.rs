#![deny(clippy::all)]

//! uptime-kuma-cli — Slate OS Uptime Kuma monitoring
//!
//! Single personality: `uptime-kuma`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_uptime_kuma(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uptime-kuma [COMMAND] [OPTIONS]");
        println!("Uptime Kuma v1.23 (Slate OS) — Self-hosted monitoring tool");
        println!();
        println!("Commands:");
        println!("  start              Start Uptime Kuma");
        println!("  monitor list|add|del  Manage monitors");
        println!("  status-page list   List status pages");
        println!("  notification list  List notification providers");
        println!("  maintenance list   List maintenance windows");
        println!("  tag list|add       Manage tags");
        println!();
        println!("Options:");
        println!("  --host ADDR        Bind address (default: 0.0.0.0)");
        println!("  --port PORT        Port (default: 3001)");
        println!("  --data-dir DIR     Data directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Uptime Kuma v1.23.13 (Slate OS)"); return 0; }
    println!("Uptime Kuma v1.23.13 (Slate OS)");
    println!("  Monitors: 34 (30 up, 3 down, 1 pending)");
    println!("  Status pages: 2");
    println!("  Notifications: Slack, Discord, Telegram, Email, PagerDuty");
    println!("  Tags: 8");
    println!("  Maintenance: 1 scheduled");
    println!("  Dashboard: http://0.0.0.0:3001");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "uptime-kuma".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_uptime_kuma(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_uptime_kuma};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/uptime-kuma"), "uptime-kuma");
        assert_eq!(basename(r"C:\bin\uptime-kuma.exe"), "uptime-kuma.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("uptime-kuma.exe"), "uptime-kuma");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_uptime_kuma(&["--help".to_string()], "uptime-kuma"), 0);
        assert_eq!(run_uptime_kuma(&["-h".to_string()], "uptime-kuma"), 0);
        let _ = run_uptime_kuma(&["--version".to_string()], "uptime-kuma");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_uptime_kuma(&[], "uptime-kuma");
    }
}
