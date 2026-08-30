#![deny(clippy::all)]

//! masscan-cli — Slate OS masscan CLI
//!
//! Single personality: `masscan`

use std::env;
use std::process;

fn run_masscan(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: masscan [OPTIONS] IP_RANGE");
        println!();
        println!("masscan — mass IP port scanner (Slate OS).");
        println!();
        println!("Options:");
        println!("  -p PORTS               Port range (e.g., 80, 1-1000)");
        println!("  --rate N               Packets per second");
        println!("  --banners              Grab banners");
        println!("  --open                 Only show open ports");
        println!("  -oX FILE               XML output");
        println!("  -oG FILE               Grepable output");
        println!("  -oJ FILE               JSON output");
        println!("  -oL FILE               List output");
        println!("  --echo                 Print current settings");
        println!("  --adapter NAME         Network adapter");
        println!("  --adapter-ip IP        Source IP");
        println!("  --router-mac MAC       Router MAC address");
        println!("  --exclude IP           Exclude IP range");
        println!("  --excludefile FILE     Exclude file");
        println!("  --wait N               Seconds to wait after sending");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Masscan version 1.3.2 (Slate OS)");
        return 0;
    }

    let rate = args.windows(2).find(|w| w[0] == "--rate")
        .map(|w| w[1].as_str()).unwrap_or("100");
    let ports = args.windows(2).find(|w| w[0] == "-p")
        .map(|w| w[1].as_str()).unwrap_or("80");
    let target = args.iter()
        .find(|a| !a.starts_with('-') && (a.contains('.') || a.contains(':')))
        .map(|s| s.as_str())
        .unwrap_or("10.0.0.0/24");

    println!("Starting masscan 1.3.2 (Slate OS)");
    println!("Initiating SYN Stealth Scan");
    println!("Scanning {} ports on {} -- rate: {} pps", ports, target, rate);
    println!();
    println!("Discovered open port 22/tcp on 10.0.0.1");
    println!("Discovered open port 80/tcp on 10.0.0.1");
    println!("Discovered open port 443/tcp on 10.0.0.1");
    println!("Discovered open port 22/tcp on 10.0.0.5");
    println!("Discovered open port 80/tcp on 10.0.0.10");
    println!("Discovered open port 8080/tcp on 10.0.0.10");
    println!();
    println!("rate: {rate:>8} pps, 256 total hosts, 6 open ports");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_masscan(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_masscan};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_masscan(vec!["--help".to_string()]), 0);
        assert_eq!(run_masscan(vec!["-h".to_string()]), 0);
        let _ = run_masscan(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_masscan(vec![]);
    }
}
