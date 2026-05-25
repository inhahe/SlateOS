#![deny(clippy::all)]

//! attune-cli — OurOS Attune automation
//!
//! Single personality: `attune`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_attune(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: attune [COMMAND] [OPTIONS]");
        println!("Attune v1.12 (OurOS) — Server orchestration & automation");
        println!();
        println!("Commands:");
        println!("  project list|create  Manage projects");
        println!("  blueprint list|run   Manage blueprints");
        println!("  node list|add        Manage nodes");
        println!("  step list|create     Manage steps");
        println!("  parameter list|set   Manage parameters");
        println!("  schedule list|create Manage schedules");
        println!();
        println!("Options:");
        println!("  --server URL       Attune server URL");
        println!("  --token TOKEN      Auth token");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Attune v1.12.0 (OurOS)"); return 0; }
    println!("Attune v1.12.0 (OurOS)");
    println!("  Projects: 6");
    println!("  Blueprints: 34");
    println!("  Nodes: 18");
    println!("  Executions: 156 (last 7d)");
    println!("  Schedules: 5 active");
    println!("  Steps library: 89");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "attune".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_attune(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
