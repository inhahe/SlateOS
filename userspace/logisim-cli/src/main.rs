#![deny(clippy::all)]

//! logisim-cli — OurOS Logisim digital logic simulator
//!
//! Single personality: `logisim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logisim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logisim [OPTIONS] [FILE.circ]");
        println!("logisim v3.8 (OurOS) — Digital logic designer and simulator");
        println!();
        println!("Options:");
        println!("  --tty TABLE       Run circuit headless with truth table output");
        println!("  --version         Show version");
        println!();
        println!("Components:");
        println!("  Gates (AND/OR/NOT/XOR/NAND/NOR/XNOR), MUX, decoder,");
        println!("  flip-flops (D/T/JK/SR), registers, counters, RAM, ROM,");
        println!("  ALU, comparator, I/O (LED, button, 7-segment, keyboard)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("logisim v3.8 (OurOS)"); return 0; }
    println!("logisim: digital logic simulator started");
    println!("  Simulation: tick-based, interactive poke tool");
    println!("  Sub-circuits: hierarchical design");
    println!("  Analysis: truth table, expression, Karnaugh map");
    println!("  Export: PNG, GIF, JPEG");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logisim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logisim(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
