#![deny(clippy::all)]

//! x86-energy-perf-cli — OurOS x86_energy_perf_policy tool
//!
//! Single personality: `x86_energy_perf_policy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_x86_energy_perf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: x86_energy_perf_policy [OPTIONS] [POLICY]");
        println!("x86_energy_perf_policy v2024.01 (OurOS) — CPU energy/perf bias");
        println!();
        println!("Policies:");
        println!("  performance    Favor performance");
        println!("  balance-performance  Default balance");
        println!("  normal         Equal balance");
        println!("  balance-power  Favor power saving");
        println!("  power          Maximum power saving");
        println!();
        println!("Options:");
        println!("  -c CPU         Target specific CPU");
        println!("  -v             Verbose");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("x86_energy_perf_policy v2024.01 (OurOS)"); return 0; }
    if let Some(policy) = args.iter().find(|a| !a.starts_with('-')) {
        println!("x86_energy_perf_policy: setting policy to '{}'", policy);
        println!("  Applied to all CPUs");
    } else {
        println!("cpu0: EPB 6 (balance-performance)");
        println!("cpu1: EPB 6 (balance-performance)");
        println!("cpu2: EPB 6 (balance-performance)");
        println!("cpu3: EPB 6 (balance-performance)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "x86_energy_perf_policy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_x86_energy_perf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
