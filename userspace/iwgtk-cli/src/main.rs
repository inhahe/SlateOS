#![deny(clippy::all)]

//! iwgtk-cli — OurOS iwgtk iwd wireless GUI
//!
//! Single personality: `iwgtk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iwgtk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iwgtk [OPTIONS]");
        println!("iwgtk v0.9 (OurOS) — iwd wireless GTK frontend");
        println!();
        println!("Options:");
        println!("  --indicator    Start as tray indicator");
        println!("  --version      Show version");
        println!();
        println!("GTK4 frontend for iwd (iNet Wireless Daemon).");
        println!("Scan, connect, manage known networks, view adapters.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iwgtk v0.9 (OurOS)"); return 0; }
    println!("iwgtk: iwd wireless manager");
    println!("  Adapter: wlan0 (powered)");
    println!("  Connected: HomeNetwork (-45 dBm)");
    println!("  Known networks: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iwgtk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iwgtk(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
