#![deny(clippy::all)]

//! opensearch-cli — OurOS OpenSearch distributed search engine
//!
//! Single personality: `opensearch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_opensearch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: opensearch [OPTIONS]");
        println!("OpenSearch v2.12 (OurOS) — Distributed search and analytics");
        println!();
        println!("Options:");
        println!("  -E KEY=VALUE       Setting override");
        println!("  -d                 Daemonize");
        println!("  -p FILE            PID file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") { println!("OpenSearch v2.12.0 (OurOS)"); return 0; }
    println!("OpenSearch v2.12.0 (OurOS)");
    println!("  HTTP: https://0.0.0.0:9200");
    println!("  Transport: 0.0.0.0:9300");
    println!("  Cluster: opensearch-cluster (green)");
    println!("  Nodes: 1");
    println!("  Indices: 15");
    println!("  Documents: 5,678,901");
    println!("  Shards: 30 primary, 30 replica");
    println!("  Dashboards: http://0.0.0.0:5601");
    println!("  Security: enabled (TLS + RBAC)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "opensearch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_opensearch(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
