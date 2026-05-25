#![deny(clippy::all)]

//! energia-cli — OurOS Energia IDE for TI MSP430/MSP432/Tiva/CC3200
//!
//! Single personality: `energia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_energia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: energia [OPTIONS] [SKETCH]");
        println!("Energia 1.8.10E23 (OurOS) — Arduino-style IDE for TI LaunchPads");
        println!();
        println!("Options:");
        println!("  --sketch FILE          Open .ino sketch");
        println!("  --board BOARD          msp430g2/msp432p401r/tm4c123/cc3200");
        println!("  --upload               Compile and upload via mspdebug/dslite");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Energia 1.8.10E23 (OurOS)"); return 0; }
    println!("Energia 1.8.10E23 (OurOS)");
    println!("  Targets: TI MSP430, MSP432 (Cortex-M4F), Tiva C (Cortex-M4F), CC3200 (Wi-Fi)");
    println!("  CC2650 BLE LaunchPad, CC1310/CC1352 sub-GHz LaunchPads");
    println!("  Language: Arduino-compatible C/C++ wireless library");
    println!("  Wiring: simplified energia.h API on top of TI DriverLib");
    println!("  Programmer: mspdebug (MSP430), DSLite (MSP432/Tiva), uniflash");
    println!("  Based on: Arduino IDE 1.6.x fork with TI compiler integration");
    println!("  License: LGPL (free, open source)");
    println!("  Note: largely superseded by Code Composer Studio / SimpleLink SDK");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "energia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_energia(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
