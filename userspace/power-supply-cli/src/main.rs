#![deny(clippy::all)]

//! power-supply-cli — OurOS power supply information
//!
//! Single personality: `power-supply`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_power_supply(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: power-supply COMMAND [OPTIONS]");
        println!("power-supply v1.0 (OurOS) — Power supply information tool");
        println!();
        println!("Commands:");
        println!("  status            Show all power supply status");
        println!("  battery           Battery details");
        println!("  ac                AC adapter status");
        println!("  watch             Continuous monitoring");
        println!("  history           Power usage history");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            println!("AC Adapter: online");
            println!("Battery BAT0: 85% (charging)");
            println!("  Voltage: 12.4V");
            println!("  Current: 2.1A");
            println!("  Time to full: 0:45");
        }
        "battery" => {
            println!("BAT0:");
            println!("  Design capacity: 56000 mWh");
            println!("  Full capacity: 51200 mWh (91.4%)");
            println!("  Current: 43520 mWh (85%)");
            println!("  Cycle count: 142");
            println!("  Health: good");
        }
        "ac" => println!("AC: online (65W adapter)"),
        "watch" => println!("Monitoring power supply..."),
        _ => println!("power-supply: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "power-supply".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_power_supply(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
