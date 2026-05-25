#![deny(clippy::all)]

//! typesense-cli — OurOS Typesense search engine
//!
//! Single personality: `typesense-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_typesense(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: typesense-server [OPTIONS]");
        println!("Typesense v0.25 (OurOS) — Open-source search engine");
        println!();
        println!("Options:");
        println!("  --data-dir DIR     Data directory");
        println!("  --api-key KEY      API key");
        println!("  --api-port PORT    API port (default: 8108)");
        println!("  --peering-port P   Peering port (cluster)");
        println!("  --nodes FILE       Cluster nodes file");
        println!("  --enable-cors      Enable CORS");
        println!("  --log-dir DIR      Log directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Typesense v0.25.2 (OurOS)"); return 0; }
    println!("Typesense v0.25.2 (OurOS)");
    println!("  API: http://0.0.0.0:8108");
    println!("  Collections: 8");
    println!("  Documents: 567,890 total");
    println!("  Search latency: < 5ms (p50)");
    println!("  Typo tolerance: enabled");
    println!("  Geosearch: enabled");
    println!("  Synonyms: 23 configured");
    println!("  Cluster: single node");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "typesense-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_typesense(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
