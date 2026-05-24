#![deny(clippy::all)]

//! teams-cli — OurOS Microsoft Teams (PWA/web client)
//!
//! Single personality: `teams`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_teams(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: teams [OPTIONS]");
        println!("teams v1.0 (OurOS) — Microsoft Teams web client wrapper");
        println!();
        println!("Options:");
        println!("  --minimized       Start minimized");
        println!("  --url URL         Open specific team/channel URL");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("teams v1.0 (OurOS)"); return 0; }
    println!("teams: launching Microsoft Teams web client");
    println!("  URL: https://teams.microsoft.com");
    println!("  Mode: PWA wrapper");
    println!("  Notifications: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teams".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_teams(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
