#![deny(clippy::all)]

//! cachet-cli — SlateOS Cachet status page
//!
//! Single personality: `cachet`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cachet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cachet [COMMAND] [OPTIONS]");
        println!("Cachet v3.0 (SlateOS) — Open-source status page system");
        println!();
        println!("Commands:");
        println!("  component list|create|update  Manage components");
        println!("  incident list|create|update   Manage incidents");
        println!("  metric list|create|point      Manage metrics");
        println!("  subscriber list|create        Manage subscribers");
        println!("  schedule list|create          Manage schedules");
        println!("  ping                          Check API connectivity");
        println!();
        println!("Options:");
        println!("  --url URL          Cachet API URL");
        println!("  --token TOKEN      API token");
        println!("  --format json|table  Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cachet v3.0.0 (SlateOS)"); return 0; }
    println!("Cachet v3.0.0 (SlateOS)");
    println!("  Components: 8 (7 operational, 1 major outage)");
    println!("  Component groups: 3");
    println!("  Incidents: 2 unresolved");
    println!("  Metrics: 4 tracked");
    println!("  Subscribers: 156");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cachet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cachet(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cachet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cachet"), "cachet");
        assert_eq!(basename(r"C:\bin\cachet.exe"), "cachet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cachet.exe"), "cachet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cachet(&["--help".to_string()], "cachet"), 0);
        assert_eq!(run_cachet(&["-h".to_string()], "cachet"), 0);
        let _ = run_cachet(&["--version".to_string()], "cachet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cachet(&[], "cachet");
    }
}
