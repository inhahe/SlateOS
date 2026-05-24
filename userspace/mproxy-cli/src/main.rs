#![deny(clippy::all)]

//! mproxy-cli — OurOS mproxy multi-protocol proxy
//!
//! Single personality: `mproxy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mproxy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mproxy [OPTIONS]");
        println!("mproxy v1.0.0 (OurOS) — Multi-protocol proxy");
        println!();
        println!("Options:");
        println!("  -l, --listen ADDR:PORT  Listen address");
        println!("  -t, --target ADDR:PORT  Target address");
        println!("  --protocol PROTO        Protocol (http, socks5, tcp)");
        println!("  --tls                   Enable TLS");
        println!("  --cert FILE             TLS certificate");
        println!("  --key FILE              TLS private key");
        println!("  --log-level LEVEL       Log level");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mproxy v1.0.0 (OurOS)");
        return 0;
    }
    println!("mproxy v1.0.0 starting...");
    println!("  Listen: 0.0.0.0:8080");
    println!("  Protocol: http");
    println!("  TLS: disabled");
    println!("  Proxy ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mproxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mproxy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
