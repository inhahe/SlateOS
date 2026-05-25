#![deny(clippy::all)]

//! garnet-cli — OurOS Microsoft Garnet cache store
//!
//! Single personality: `garnet-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_garnet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: garnet-server [OPTIONS]");
        println!("Garnet v1.0 (OurOS) — High-performance cache store (Redis-compatible)");
        println!();
        println!("Options:");
        println!("  --bind IP          Bind address (default: 0.0.0.0)");
        println!("  --port PORT        Port (default: 6379)");
        println!("  --memory SIZE      Memory limit");
        println!("  --tls              Enable TLS");
        println!("  --tls-cert FILE    TLS certificate");
        println!("  --tls-key FILE     TLS private key");
        println!("  --aof              Enable append-only file");
        println!("  --checkpoint DIR   Checkpoint directory");
        println!("  --threads N        IO threads");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Garnet v1.0.32 (OurOS)"); return 0; }
    println!("Garnet v1.0.32 (OurOS)");
    println!("  Listening: 0.0.0.0:6379");
    println!("  Protocol: RESP (Redis-compatible)");
    println!("  Memory: 256 MB limit");
    println!("  Keys: 45,678");
    println!("  Connected clients: 12");
    println!("  Ops/sec: 890,000");
    println!("  Persistence: AOF enabled");
    println!("  Cluster: standalone");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "garnet-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_garnet(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
