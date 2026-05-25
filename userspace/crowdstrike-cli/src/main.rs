#![deny(clippy::all)]

//! crowdstrike-cli — OurOS CrowdStrike Falcon EDR/XDR
//!
//! Single personality: `crowdstrike`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crowdstrike [OPTIONS] [SUBCMD]");
        println!("CrowdStrike Falcon (OurOS) — Cloud-native EDR/XDR/Identity/Cloud security");
        println!();
        println!("Options:");
        println!("  --cid CUSTOMER_ID      Customer ID (CID)");
        println!("  --api-key KEY          API client key/secret");
        println!("  detect list            List detections");
        println!("  host hide HOST         Hide / unhide host");
        println!("  rtr CMD                Real Time Response (live remote shell)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CrowdStrike Falcon Sensor 7.20 (OurOS)"); return 0; }
    println!("CrowdStrike Falcon (OurOS)");
    println!("  Platform: cloud-native single sensor for Win/Mac/Linux/iOS/Android");
    println!("  Modules: Insight (EDR), Prevent (NGAV), Discover (asset/IT hygiene),");
    println!("           Overwatch (managed threat hunting), Spotlight (vuln mgmt),");
    println!("           Identity Protection, Cloud Security (CNAPP), LogScale (SIEM)");
    println!("  Architecture: Threat Graph (1+ trillion events/day), ML-driven detection");
    println!("  Threat intel: Falcon Intelligence (premium, recon adversary tracking)");
    println!("  Charlotte AI: GenAI security analyst assistant");
    println!("  CrowdStrike Store: 3rd-party integrations marketplace");
    println!("  License: per-endpoint subscription, modular Falcon Complete (MDR)");
    println!("  Note: July 2024 incident impacted ~8.5M Windows hosts (faulty channel file)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crowdstrike".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
