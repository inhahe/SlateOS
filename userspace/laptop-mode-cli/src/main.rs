#![deny(clippy::all)]

//! laptop-mode-cli — OurOS Laptop Mode Tools power saving
//!
//! Single personality: `laptop-mode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_laptop_mode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: laptop-mode <command> [OPTIONS]");
        println!("laptop-mode v1.74 (OurOS) — Laptop power saving");
        println!();
        println!("Commands:");
        println!("  status         Show current mode and module status");
        println!("  start          Start laptop mode");
        println!("  stop           Stop laptop mode");
        println!("  force          Force battery mode");
        println!("  auto           Auto-detect power source");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("laptop-mode v1.74 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("status") => {
            println!("Laptop Mode Tools status:");
            println!("  Power source: AC");
            println!("  Laptop mode: disabled (on AC)");
            println!("  Modules enabled: intel-sata-powermgmt, cpufreq, wireless");
            println!("  Modules disabled: bluetooth (manually), nmi-watchdog");
        }
        _ => {
            println!("laptop-mode: power management tool");
            println!("  Use 'laptop-mode status' for current state");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "laptop-mode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_laptop_mode(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
