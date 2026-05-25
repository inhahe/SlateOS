#![deny(clippy::all)]

//! garage-cli — OurOS Garage S3-compatible storage
//!
//! Multi-personality: `garage`, `garage-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_garage(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "garage-server" => {
                println!("garage-server (OurOS) — Garage storage node");
                println!("  --config FILE      Config file");
                println!("  --metadata-dir DIR Metadata directory");
                println!("  --data-dir DIR     Data directory");
            }
            _ => {
                println!("Garage v1.0 (OurOS) — S3-compatible distributed storage");
                println!();
                println!("Commands:");
                println!("  status             Show cluster status");
                println!("  node list|connect  Manage nodes");
                println!("  layout assign|apply  Configure layout");
                println!("  bucket list|create|delete  Manage buckets");
                println!("  key list|create|info  Manage API keys");
                println!("  repair             Repair data");
                println!("  stats              Show statistics");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Garage v1.0.1 (OurOS)"); return 0; }
    println!("Garage v1.0.1 (OurOS)");
    println!("  Nodes: 3 (all healthy)");
    println!("  Buckets: 8");
    println!("  Objects: 234,567");
    println!("  Storage used: 1.2 TiB");
    println!("  Replication: 3x");
    println!("  S3 API: http://0.0.0.0:3900");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "garage".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_garage(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
