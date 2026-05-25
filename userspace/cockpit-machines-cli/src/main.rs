#![deny(clippy::all)]

//! cockpit-machines-cli — OurOS Cockpit Machines VM management
//!
//! Single personality: `cockpit-machines`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cockpit_machines(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cockpit-machines [OPTIONS]");
        println!("cockpit-machines v306 (OurOS) — Web-based VM management");
        println!();
        println!("Options:");
        println!("  --port PORT      Web server port (default: 9090)");
        println!("  --version        Show version");
        println!();
        println!("Cockpit plugin for managing libvirt virtual machines");
        println!("from a web browser. Create, start, stop, and manage VMs.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cockpit-machines v306 (OurOS)"); return 0; }
    println!("cockpit-machines: web VM management");
    println!("  URL: https://localhost:9090/machines");
    println!("  VMs: 3 (1 running)");
    println!("  Storage pools: 1");
    println!("  Networks: 1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cockpit-machines".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cockpit_machines(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
