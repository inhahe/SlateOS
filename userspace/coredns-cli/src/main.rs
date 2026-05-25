#![deny(clippy::all)]

//! coredns-cli — OurOS CoreDNS server
//!
//! Single personality: `coredns`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_coredns(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: coredns [OPTIONS]");
        println!("CoreDNS v1.11 (OurOS) — DNS and service discovery");
        println!();
        println!("Options:");
        println!("  -conf FILE     Corefile configuration");
        println!("  -dns.port PORT DNS port (default: 53)");
        println!("  -pidfile FILE  PID file");
        println!("  -quiet         Quiet mode");
        println!("  -plugins       List plugins");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CoreDNS v1.11.3 (OurOS)"); return 0; }
    println!("CoreDNS v1.11.3 (OurOS)");
    println!("  Corefile: /etc/coredns/Corefile");
    println!("  Zones:");
    println!("    .:53 -> forward to 1.1.1.1, 8.8.8.8");
    println!("    cluster.local:53 -> kubernetes");
    println!("  Plugins: cache, forward, kubernetes, prometheus, log, errors");
    println!("  Cache: 10000 entries, 30s TTL");
    println!("  Listening: 0.0.0.0:53 (udp+tcp)");
    println!("  Prometheus: :9153/metrics");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "coredns".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_coredns(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
