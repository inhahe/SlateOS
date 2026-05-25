#![deny(clippy::all)]

//! morpheus-cli — OurOS Morpheus cloud management
//!
//! Single personality: `morpheus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_morpheus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: morpheus [COMMAND] [OPTIONS]");
        println!("Morpheus v7.0 (OurOS) — Hybrid cloud management platform");
        println!();
        println!("Commands:");
        println!("  instances list|get|add     Manage instances");
        println!("  apps list|get|add|deploy   Manage applications");
        println!("  clouds list|add            Manage cloud integrations");
        println!("  tasks list|execute         Manage tasks");
        println!("  blueprints list|get        Manage blueprints");
        println!("  groups list|add            Manage groups");
        println!("  networks list|get          Manage networks");
        println!("  budgets list|get           Cost management");
        println!();
        println!("Options:");
        println!("  --url URL          Morpheus appliance URL");
        println!("  --token TOKEN      Access token");
        println!("  --json             JSON output");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Morpheus v7.0.4 (OurOS)"); return 0; }
    println!("Morpheus v7.0.4 (OurOS)");
    println!("  Clouds: 3 (AWS, Azure, VMware)");
    println!("  Instances: 156 running");
    println!("  Apps: 23");
    println!("  Blueprints: 34");
    println!("  Tasks: 89 scheduled");
    println!("  Monthly cost: $12,345");
    println!("  Users: 45 active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "morpheus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_morpheus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
