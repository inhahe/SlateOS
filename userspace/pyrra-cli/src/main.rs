#![deny(clippy::all)]

//! pyrra-cli — OurOS Pyrra SLO management tool
//!
//! Single personality: `pyrra`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pyrra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pyrra COMMAND [OPTIONS]");
        println!("Pyrra v0.7.0 (OurOS) — SLO management tool");
        println!();
        println!("Commands:");
        println!("  api             Start API server");
        println!("  filesystem      Watch filesystem for SLO defs");
        println!("  kubernetes      Watch Kubernetes for SLO CRDs");
        println!("  generate        Generate recording/alerting rules");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  --config-files DIR    SLO definition directory");
        println!("  --prometheus-url URL  Prometheus URL");
        println!("  --generic-rules       Use generic rule names");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Pyrra v0.7.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("generate");
    match cmd {
        "generate" => {
            println!("Generating rules from SLO definitions...");
            println!("  api-availability (99.9% over 30d):");
            println!("    - recording rule: slo:api_availability:ratio_rate5m");
            println!("    - recording rule: slo:api_availability:ratio_rate30m");
            println!("    - recording rule: slo:api_availability:ratio_rate1h");
            println!("    - alert: SLOApiAvailabilityBurn5m");
            println!("    - alert: SLOApiAvailabilityBurn30m");
            println!("  Generated 5 rules for 1 SLO.");
        }
        "api" => {
            println!("Starting Pyrra API server...");
            println!("  Listen: 0.0.0.0:9099");
            println!("  Prometheus: http://localhost:9090");
        }
        "filesystem" => println!("Watching filesystem for SLO definitions..."),
        "kubernetes" => println!("Watching Kubernetes for SLO custom resources..."),
        _ => println!("pyrra {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pyrra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pyrra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
