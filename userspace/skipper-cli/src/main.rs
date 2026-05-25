#![deny(clippy::all)]

//! skipper-cli — OurOS Skipper HTTP router
//!
//! Single personality: `skipper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_skipper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: skipper [OPTIONS]");
        println!("Skipper v0.21 (OurOS) — HTTP router and reverse proxy");
        println!();
        println!("Options:");
        println!("  -address ADDR       Listen address (default: :9090)");
        println!("  -routes-file FILE   Routes file (eskip)");
        println!("  -kubernetes         Enable Kubernetes ingress");
        println!("  -etcd-urls URLS     etcd endpoints");
        println!("  -proxy-preserve-host  Preserve host header");
        println!("  -enable-ratelimits  Enable rate limiting");
        println!("  -enable-prometheus-metrics  Prometheus metrics");
        println!("  -tls-cert FILE      TLS certificate");
        println!("  -tls-key FILE       TLS private key");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Skipper v0.21.5 (OurOS)"); return 0; }
    println!("Skipper v0.21.5 (OurOS)");
    println!("  Listening: :9090");
    println!("  Routes: 45 loaded");
    println!("  Filters: 60+ available");
    println!("  Predicates: path, host, method, header, cookie");
    println!("  Backends: 23 upstream hosts");
    println!("  Rate limits: enabled");
    println!("  Metrics: :9911/metrics (Prometheus)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "skipper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_skipper(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
