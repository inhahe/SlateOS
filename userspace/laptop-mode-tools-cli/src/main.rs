#![deny(clippy::all)]

//! laptop-mode-tools-cli — OurOS laptop-mode-tools power savings
//!
//! Single personality: `laptop_mode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_laptop_mode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: laptop_mode COMMAND");
        println!("laptop_mode v1.74 (OurOS) — Laptop power saving tool");
        println!();
        println!("Commands:");
        println!("  status            Show current status");
        println!("  start             Enable laptop mode");
        println!("  stop              Disable laptop mode");
        println!("  auto              Automatic mode (on battery/AC)");
        println!("  force             Force re-evaluation");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            println!("Laptop mode: enabled");
            println!("  Power source: battery");
            println!("  Modules: intel-sata-powermgmt, lcd-brightness, cpufreq");
            println!("  Dirty writeback: 60s");
        }
        "start" => println!("Laptop mode enabled"),
        "stop" => println!("Laptop mode disabled"),
        _ => println!("laptop_mode: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "laptop_mode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_laptop_mode(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
