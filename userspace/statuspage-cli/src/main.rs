#![deny(clippy::all)]

//! statuspage-cli — OurOS Atlassian Statuspage
//!
//! Single personality: `statuspage`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_statuspage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: statuspage [COMMAND] [OPTIONS]");
        println!("Statuspage v2.0 (OurOS) — Hosted status page service");
        println!();
        println!("Commands:");
        println!("  page list|get          List status pages");
        println!("  component list|update  Manage components");
        println!("  incident list|create|update  Manage incidents");
        println!("  metric list|submit     Manage metrics");
        println!("  subscriber list|add    Manage subscribers");
        println!("  maintenance create     Schedule maintenance");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --page-id ID       Page ID");
        println!("  --format json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Statuspage CLI v2.0.0 (OurOS)"); return 0; }
    println!("Statuspage v2.0.0 (OurOS)");
    println!("  Pages: 2");
    println!("  Components: 15 (12 operational, 2 degraded, 1 outage)");
    println!("  Open incidents: 1");
    println!("  Subscribers: 234");
    println!("  Uptime (30d): 99.95%");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "statuspage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_statuspage(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_statuspage};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/statuspage"), "statuspage");
        assert_eq!(basename(r"C:\bin\statuspage.exe"), "statuspage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("statuspage.exe"), "statuspage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_statuspage(&["--help".to_string()], "statuspage"), 0);
        assert_eq!(run_statuspage(&["-h".to_string()], "statuspage"), 0);
        let _ = run_statuspage(&["--version".to_string()], "statuspage");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_statuspage(&[], "statuspage");
    }
}
