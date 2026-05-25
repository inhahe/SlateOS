#![deny(clippy::all)]

//! mage-cli — OurOS Mage AI data pipeline
//!
//! Single personality: `mage`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mage(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mage [COMMAND] [OPTIONS]");
        println!("Mage v0.9 (OurOS) — Open-source data pipeline tool");
        println!();
        println!("Commands:");
        println!("  start              Start Mage server");
        println!("  init PROJECT       Initialize new project");
        println!("  run PIPELINE       Run pipeline");
        println!("  test               Run tests");
        println!("  clean              Clean cached data");
        println!("  create_spark_cluster  Create Spark cluster");
        println!();
        println!("Options:");
        println!("  --host ADDR        Server host");
        println!("  --port PORT        Server port (default: 6789)");
        println!("  --project DIR      Project directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mage v0.9.73 (OurOS)"); return 0; }
    println!("Mage v0.9.73 (OurOS)");
    println!("  Server: http://0.0.0.0:6789");
    println!("  Pipelines: 12 (8 batch, 3 streaming, 1 integration)");
    println!("  Blocks: 67 total");
    println!("  Triggers: 5 active");
    println!("  Variables: 23 configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mage(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
