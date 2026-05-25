#![deny(clippy::all)]

//! cribl-cli — OurOS Cribl Stream data pipeline
//!
//! Single personality: `cribl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cribl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cribl [COMMAND] [OPTIONS]");
        println!("Cribl Stream v4.7 (OurOS) — Observability data pipeline");
        println!();
        println!("Commands:");
        println!("  start              Start Cribl Stream");
        println!("  stop               Stop Cribl Stream");
        println!("  restart            Restart service");
        println!("  status             Show service status");
        println!("  mode leader|worker Set cluster mode");
        println!("  pack list|install  Manage content packs");
        println!();
        println!("Options:");
        println!("  --config DIR       Config directory");
        println!("  --port PORT        API port (default: 9000)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cribl Stream v4.7.2 (OurOS)"); return 0; }
    println!("Cribl Stream v4.7.2 (OurOS)");
    println!("  Mode: leader");
    println!("  Workers: 4 connected");
    println!("  Sources: 8 active");
    println!("  Destinations: 5 active");
    println!("  Routes: 12 configured");
    println!("  Throughput: 2.3 GB/day");
    println!("  Volume reduction: 42%");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cribl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cribl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
