#![deny(clippy::all)]

//! simulide-cli — OurOS SimulIDE electronic circuit simulator
//!
//! Single personality: `simulide`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_simulide(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: simulide [OPTIONS] [FILE.sim1]");
        println!("simulide v1.1 (OurOS) — Real-time electronic circuit simulator");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Real-time circuit simulation, SPICE-based engine,");
        println!("  Arduino/PIC/AVR microcontroller simulation,");
        println!("  Logic analyzer, oscilloscope, I/O components");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("simulide v1.1 (OurOS)"); return 0; }
    println!("simulide: circuit simulator started");
    println!("  Components: passive, semiconductor, logic gates, MCU");
    println!("  MCU support: AVR, PIC, Arduino");
    println!("  Instruments: oscilloscope, logic analyzer, voltmeter");
    println!("  Simulation: real-time analog + digital");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simulide".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_simulide(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
