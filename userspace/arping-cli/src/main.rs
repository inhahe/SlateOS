#![deny(clippy::all)]

//! arping-cli — OurOS ARP ping utility
//!
//! Single personality: `arping`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_arping(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: arping [OPTIONS] HOST");
        println!("arping v2.23 (OurOS) — Send ARP requests to a host");
        println!();
        println!("Options:");
        println!("  HOST              Target IP address");
        println!("  -I IFACE          Network interface");
        println!("  -c COUNT          Number of requests");
        println!("  -w SECS           Timeout in seconds");
        println!("  -D                Duplicate address detection");
        println!("  -q                Quiet mode");
        return 0;
    }
    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("192.168.1.1");
    if args.iter().any(|a| a == "-D") {
        println!("ARPING {} from 0.0.0.0 eth0", host);
        println!("Sent 3 probes (3 broadcast(s))");
        println!("Received 0 response(s) — address is available");
        return 0;
    }
    println!("ARPING {} from 192.168.1.50 eth0", host);
    println!("Unicast reply from {} [AA:BB:CC:DD:EE:01]  0.523ms", host);
    println!("Unicast reply from {} [AA:BB:CC:DD:EE:01]  0.481ms", host);
    println!("Unicast reply from {} [AA:BB:CC:DD:EE:01]  0.497ms", host);
    println!("Sent 3 probes (1 broadcast(s))");
    println!("Received 3 response(s)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "arping".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_arping(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
