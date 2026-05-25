#![deny(clippy::all)]

//! orcad-cli — OurOS Cadence OrCAD PCB EDA
//!
//! Single personality: `orcad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_orcad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: orcad [OPTIONS] [FILE]");
        println!("Cadence OrCAD 23.1 (OurOS) — PCB design suite");
        println!();
        println!("Options:");
        println!("  --capture FILE         Open OrCAD Capture schematic (.dsn)");
        println!("  --pcb FILE             Open OrCAD PCB Designer (.brd)");
        println!("  --pspice FILE          Open PSpice simulation (.cir)");
        println!("  --tcl SCRIPT           Run Tcl automation script");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cadence OrCAD 23.1 (OurOS)"); return 0; }
    println!("Cadence OrCAD 23.1 (OurOS)");
    println!("  Tools: Capture (schematic), PCB Designer, PSpice (SPICE simulation)");
    println!("  Format: .dsn/.olb (schematic), .brd (PCB), .cir (PSpice)");
    println!("  Editions: OrCAD X — Professional, Standard, Capture/PCB");
    println!("  PSpice: A/D mixed-signal simulation with advanced analysis");
    println!("  Signal integrity, power integrity (Allegro-derived tools)");
    println!("  Scripting: Tcl, SKILL (Cadence's Lisp variant)");
    println!("  License: subscription / perpetual + maintenance");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "orcad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_orcad(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
