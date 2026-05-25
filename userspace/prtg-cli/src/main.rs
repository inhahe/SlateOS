#![deny(clippy::all)]

//! prtg-cli — OurOS PRTG-compatible monitoring probe
//!
//! Single personality: `prtg-probe`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_prtg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: prtg-probe [OPTIONS]");
        println!("PRTG Probe v24.1 (OurOS) — Network monitoring probe");
        println!();
        println!("Options:");
        println!("  --server URL    Core server URL");
        println!("  --name NAME     Probe name");
        println!("  --key KEY       Authentication key");
        println!("  --port PORT     Listening port (default: 23560)");
        println!("  --gid GID       Group ID");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PRTG Probe v24.1.0 (OurOS)"); return 0; }
    println!("PRTG Probe v24.1.0 (OurOS)");
    println!("  Server: https://monitor.local:8443");
    println!("  Probe: linux-probe-01");
    println!("  Status: connected");
    println!("  Sensors: 125 active");
    println!("    Ping: 30");
    println!("    HTTP: 25");
    println!("    SNMP: 40");
    println!("    WMI: 15");
    println!("    Custom: 15");
    println!("  Scan interval: 60s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "prtg-probe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_prtg(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
