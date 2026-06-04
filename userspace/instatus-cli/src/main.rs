#![deny(clippy::all)]

//! instatus-cli — OurOS Instatus status page
//!
//! Single personality: `instatus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_instatus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: instatus [COMMAND] [OPTIONS]");
        println!("Instatus v2.0 (OurOS) — Status page platform");
        println!();
        println!("Commands:");
        println!("  page list|create       Manage status pages");
        println!("  component list|update  Manage components");
        println!("  incident create|update Create/update incidents");
        println!("  metric add             Add metric data point");
        println!("  subscriber list|add    Manage subscribers");
        println!("  team list|invite       Manage team");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --page-id ID       Status page ID");
        println!("  --output json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Instatus v2.0.0 (OurOS)"); return 0; }
    println!("Instatus v2.0.0 (OurOS)");
    println!("  Pages: 1");
    println!("  Components: 10 (9 operational, 1 degraded)");
    println!("  Incidents: 0 open");
    println!("  Subscribers: 567");
    println!("  Uptime (30d): 99.98%");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "instatus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_instatus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_instatus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/instatus"), "instatus");
        assert_eq!(basename(r"C:\bin\instatus.exe"), "instatus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("instatus.exe"), "instatus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_instatus(&["--help".to_string()], "instatus"), 0);
        assert_eq!(run_instatus(&["-h".to_string()], "instatus"), 0);
        let _ = run_instatus(&["--version".to_string()], "instatus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_instatus(&[], "instatus");
    }
}
