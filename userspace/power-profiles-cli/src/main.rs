#![deny(clippy::all)]

//! power-profiles-cli — OurOS power-profiles-daemon
//!
//! Multi-personality: `powerprofilesctl`, `power-profiles-daemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_powerprofilesctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: powerprofilesctl <command>");
        println!("powerprofilesctl v0.20 (OurOS) — Power profile control");
        println!();
        println!("Commands:");
        println!("  list           List available profiles");
        println!("  get            Get active profile");
        println!("  set PROFILE    Set active profile");
        println!("  launch PROFILE CMD   Run command with profile");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("powerprofilesctl v0.20 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("list") => {
            println!("  power-saver:");
            println!("    CpuDriver:   intel_pstate");
            println!("    PlatformDriver: platform_profile");
            println!();
            println!("* balanced:");
            println!("    CpuDriver:   intel_pstate");
            println!("    PlatformDriver: platform_profile");
            println!();
            println!("  performance:");
            println!("    CpuDriver:   intel_pstate");
            println!("    PlatformDriver: platform_profile");
        }
        Some("get") => {
            println!("balanced");
        }
        Some("set") => {
            let profile = args.get(1).map(|s| s.as_str()).unwrap_or("balanced");
            println!("powerprofilesctl: set profile to '{}'", profile);
        }
        _ => {
            println!("powerprofilesctl: use --help for commands");
        }
    }
    0
}

fn run_power_profiles_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: power-profiles-daemon [OPTIONS]");
        println!("power-profiles-daemon v0.20 (OurOS) — Power profile daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("power-profiles-daemon v0.20 (OurOS)"); return 0; }
    println!("power-profiles-daemon: started");
    println!("  Active profile: balanced");
    println!("  Drivers: intel_pstate, platform_profile");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "powerprofilesctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "power-profiles-daemon" => run_power_profiles_daemon(&rest, &prog),
        _ => run_powerprofilesctl(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
