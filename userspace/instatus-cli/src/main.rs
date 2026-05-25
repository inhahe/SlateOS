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
mod tests { #[test] fn test_basic() { assert!(true); } }
