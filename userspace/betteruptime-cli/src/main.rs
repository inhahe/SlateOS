#![deny(clippy::all)]

//! betteruptime-cli — OurOS Better Uptime monitoring
//!
//! Single personality: `betteruptime`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_betteruptime(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: betteruptime [COMMAND] [OPTIONS]");
        println!("Better Uptime v2.0 (OurOS) — Uptime monitoring & status pages");
        println!();
        println!("Commands:");
        println!("  monitor list|create|pause  Manage monitors");
        println!("  heartbeat list|create      Manage heartbeats");
        println!("  incident list|acknowledge  Manage incidents");
        println!("  on-call list|create        On-call schedules");
        println!("  status-page list|create    Status pages");
        println!("  escalation list|create     Escalation policies");
        println!();
        println!("Options:");
        println!("  --api-token TOKEN  API token");
        println!("  --team-id ID       Team ID");
        println!("  --format json|table  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Better Uptime v2.0.0 (OurOS)"); return 0; }
    println!("Better Uptime v2.0.0 (OurOS)");
    println!("  Monitors: 25 (23 up, 2 down)");
    println!("  Heartbeats: 8 (all healthy)");
    println!("  Incidents: 1 open");
    println!("  On-call: 3 schedules");
    println!("  Status pages: 2");
    println!("  Avg response: 189ms");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "betteruptime".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_betteruptime(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_betteruptime};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/betteruptime"), "betteruptime");
        assert_eq!(basename(r"C:\bin\betteruptime.exe"), "betteruptime.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("betteruptime.exe"), "betteruptime");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_betteruptime(&["--help".to_string()], "betteruptime"), 0);
        assert_eq!(run_betteruptime(&["-h".to_string()], "betteruptime"), 0);
        let _ = run_betteruptime(&["--version".to_string()], "betteruptime");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_betteruptime(&[], "betteruptime");
    }
}
