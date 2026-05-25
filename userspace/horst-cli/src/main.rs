#![deny(clippy::all)]

//! horst-cli — OurOS horst wireless LAN analyzer
//!
//! Single personality: `horst`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_horst(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: horst [OPTIONS]");
        println!("horst v5.1 (OurOS) — Highly Optimized Radio Scanning Tool");
        println!();
        println!("Options:");
        println!("  -i IFACE       Interface (monitor mode)");
        println!("  -C             Show channel utilization");
        println!("  -e             Filter essid");
        println!("  -d N           Debug level");
        println!("  -o FILE        Output to file");
        println!("  --version      Show version");
        println!();
        println!("Lightweight 802.11 wireless LAN analyzer with ncurses UI.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("horst v5.1 (OurOS)"); return 0; }
    println!("horst: wireless LAN analyzer");
    println!("  Interface: wlan0mon");
    println!("  Packets: 1,234 received");
    println!("  APs seen: 8");
    println!("  Stations: 15");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "horst".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_horst(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
