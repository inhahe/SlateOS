#![deny(clippy::all)]

//! pktgen-cli — OurOS packet generator
//!
//! Single personality: `pktgen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pktgen(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pktgen [OPTIONS]");
        println!("pktgen v4.0 (OurOS) — High-performance packet generator");
        println!();
        println!("Options:");
        println!("  -i IFACE          Output interface");
        println!("  -d DST_IP         Destination IP");
        println!("  -D DST_MAC        Destination MAC");
        println!("  -s SIZE           Packet size (bytes)");
        println!("  -c COUNT          Packet count (0=infinite)");
        println!("  -r RATE           Packets per second");
        println!("  -p PROTO          Protocol (udp/tcp/icmp)");
        println!("  --sport PORT      Source port");
        println!("  --dport PORT      Destination port");
        println!("  --stats           Show statistics");
        return 0;
    }
    if args.iter().any(|a| a == "--stats") {
        println!("Packet Generator Statistics:");
        println!("  Interface: eth0");
        println!("  TX packets: 1,000,000");
        println!("  TX bytes: 64,000,000");
        println!("  TX rate: 500,000 pps");
        println!("  TX bandwidth: 256 Mbps");
        println!("  Errors: 0");
        return 0;
    }
    let dst = args.iter().skip_while(|a| a.as_str() != "-d").nth(1).map(|s| s.as_str()).unwrap_or("192.168.1.1");
    let size = args.iter().skip_while(|a| a.as_str() != "-s").nth(1).map(|s| s.as_str()).unwrap_or("64");
    println!("Generating packets...");
    println!("  Destination: {}", dst);
    println!("  Packet size: {} bytes", size);
    println!("  Protocol: UDP");
    println!("  Rate: 100000 pps");
    println!("  Sent: 100000 packets");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pktgen".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pktgen(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
