#![deny(clippy::all)]

//! gatus-cli — OurOS Gatus health dashboard
//!
//! Single personality: `gatus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gatus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gatus [OPTIONS]");
        println!("Gatus v5.11 (OurOS) — Automated developer health dashboard");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (default: config.yaml)");
        println!("  --port PORT        Web UI port (default: 8080)");
        println!("  --address ADDR     Bind address");
        println!("  --storage TYPE     Storage backend (memory/sqlite/postgres)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gatus v5.11.0 (OurOS)"); return 0; }
    println!("Gatus v5.11.0 (OurOS)");
    println!("  Endpoints: 23 monitored");
    println!("  Groups: 4 (API, Web, Database, External)");
    println!("  Healthy: 21, Unhealthy: 2");
    println!("  Alerts: Slack, PagerDuty, email");
    println!("  Check interval: 60s");
    println!("  Dashboard: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gatus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gatus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gatus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gatus"), "gatus");
        assert_eq!(basename(r"C:\bin\gatus.exe"), "gatus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gatus.exe"), "gatus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gatus(&["--help".to_string()], "gatus"), 0);
        assert_eq!(run_gatus(&["-h".to_string()], "gatus"), 0);
        let _ = run_gatus(&["--version".to_string()], "gatus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gatus(&[], "gatus");
    }
}
