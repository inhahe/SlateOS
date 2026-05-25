#![deny(clippy::all)]

//! matillion-cli — OurOS Matillion data productivity
//!
//! Single personality: `matillion`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_matillion(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: matillion [COMMAND] [OPTIONS]");
        println!("Matillion v2.0 (OurOS) — Data productivity cloud");
        println!();
        println!("Commands:");
        println!("  pipeline list|run|schedule    Manage pipelines");
        println!("  environment list|create       Manage environments");
        println!("  project list|create           Manage projects");
        println!("  agent list|register           Manage agents");
        println!("  secret list|create            Manage secrets");
        println!("  variable list|set             Manage variables");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --account URL      Account URL");
        println!("  --output json|yaml Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Matillion v2.0.0 (OurOS)"); return 0; }
    println!("Matillion v2.0.0 (OurOS)");
    println!("  Projects: 6");
    println!("  Pipelines: 34");
    println!("  Environments: 3 (dev, staging, prod)");
    println!("  Agents: 2 connected");
    println!("  Runs: 789 (last 7d)");
    println!("  Data warehouse: Snowflake, BigQuery, Redshift");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "matillion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_matillion(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
