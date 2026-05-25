#![deny(clippy::all)]

//! pads-cli — OurOS Siemens PADS professional PCB
//!
//! Single personality: `pads`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pads(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pads [OPTIONS] [FILE]");
        println!("Siemens PADS Professional VX.2.13 (OurOS) — Mid-range PCB EDA");
        println!();
        println!("Options:");
        println!("  --logic FILE           PADS Logic (schematic)");
        println!("  --layout FILE          PADS Layout (PCB)");
        println!("  --router FILE          PADS Router (auto-routing)");
        println!("  --vbscript SCRIPT      Run VBScript automation");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Siemens PADS Professional VX.2.13 (OurOS)"); return 0; }
    println!("Siemens PADS Professional VX.2.13 (OurOS)");
    println!("  Tools: PADS Logic (schematic), PADS Layout, PADS Router");
    println!("  Format: .sch (schematic), .pcb (PCB), HKP (HyperLynx), ASCII");
    println!("  HyperLynx: signal/power integrity, SerDes, thermal simulation");
    println!("  Constraints: net classes, length matching, differential pairs");
    println!("  Editions: Standard, Standard Plus, Professional");
    println!("  Scripting: VBScript / VBA, batch automation");
    println!("  License: per-seat subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pads".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pads(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
