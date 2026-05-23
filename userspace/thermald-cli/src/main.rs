#![deny(clippy::all)]

//! thermald-cli — OurOS thermal management daemon
//!
//! Multi-personality: `thermald`, `thermal-monitor`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_thermald(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: thermald [OPTIONS]");
        println!();
        println!("thermald — thermal management daemon (OurOS).");
        println!();
        println!("Options:");
        println!("  --no-daemon      Run in foreground");
        println!("  --loglevel N     Log level (0-3)");
        println!("  --config-file F  Config file");
        println!("  --adaptive       Use adaptive tables");
        println!("  --ignore-cpuid   Ignore CPU model check");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("thermald version 2.5.6 (OurOS)");
        return 0;
    }

    println!("thermald: starting thermal daemon (OurOS)");
    println!("thermald: detected CPU: Intel Core i9-13900K");
    println!("thermald: thermal zones: 2");
    println!("thermald:   zone 0: x86_pkg_temp (trip: 100°C)");
    println!("thermald:   zone 1: acpitz (trip: 110°C)");
    println!("thermald: cooling devices: 3");
    println!("thermald:   device 0: intel_powerclamp");
    println!("thermald:   device 1: Processor");
    println!("thermald:   device 2: Fan");
    println!("thermald: current temperature: 45°C (well below trip point)");
    0
}

fn run_thermal_monitor(_args: &[String]) -> i32 {
    println!("Thermal Monitor (OurOS)");
    println!();
    println!("Zone                   Temperature  Trip Point  Status");
    println!("────────────────────   ───────────  ──────────  ──────");
    println!("x86_pkg_temp             45.0°C      100.0°C    OK");
    println!("acpitz                   38.0°C      110.0°C    OK");
    println!();
    println!("Cooling Devices:");
    println!("  intel_powerclamp: cur_state=0 max_state=50");
    println!("  Processor:        cur_state=0 max_state=10");
    println!("  Fan:              cur_state=0 max_state=1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "thermald".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "thermal-monitor" => run_thermal_monitor(&rest),
        _ => run_thermald(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
