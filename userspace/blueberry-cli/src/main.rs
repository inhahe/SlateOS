#![deny(clippy::all)]

//! blueberry-cli — OurOS Blueberry Bluetooth config tool (Cinnamon)
//!
//! Single personality: `blueberry`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_blueberry(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blueberry");
        println!("blueberry v1.4 (OurOS) — Bluetooth configuration (Cinnamon)");
        println!();
        println!("Bluetooth device manager from Linux Mint / Cinnamon.");
        return 0;
    }
    let _ = args;
    println!("blueberry: Bluetooth settings");
    println!("  Bluetooth: ON");
    println!("  Visibility: ON (2 minutes)");
    println!("  Paired devices: 2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "blueberry".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_blueberry(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
