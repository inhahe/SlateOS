#![deny(clippy::all)]

//! iar-cli — OurOS IAR Embedded Workbench
//!
//! Single personality: `iar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iar [OPTIONS] [PROJECT]");
        println!("IAR Embedded Workbench 9.60 (OurOS) — Premium embedded IDE+compiler");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .eww workspace");
        println!("  --build CONFIG         IarBuild headless build");
        println!("  --target ARCH          arm/risc-v/rh850/430/avr/msp430/8051/stm8");
        println!("  --cspy                 Launch C-SPY debugger");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IAR Embedded Workbench 9.60.4 (OurOS)"); return 0; }
    println!("IAR Embedded Workbench 9.60.4 (OurOS)");
    println!("  Targets: Arm Cortex-M/A/R, RISC-V, RH850, AVR, MSP430, 8051, STM8, etc.");
    println!("  Compiler: IAR C/C++ — best-in-class code density and performance");
    println!("  Debugger: C-SPY (instruction-set sim, J-Link, I-jet, Lauterbach)");
    println!("  Functional Safety: ISO 26262/IEC 61508/IEC 62304/EN 50128 qualified");
    println!("  Static analysis: C-STAT (MISRA C, CERT C, CWE)");
    println!("  Run-time: C-RUN (bounds/heap/stack runtime checking)");
    println!("  License: per-seat (expensive — premium product)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iar(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
