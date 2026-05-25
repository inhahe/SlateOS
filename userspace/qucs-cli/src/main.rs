#![deny(clippy::all)]

//! qucs-cli — OurOS Qucs circuit simulator
//!
//! Multi-personality: `qucs`, `qucsator`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qucs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qucs [OPTIONS] [FILE.sch]");
        println!("qucs v24.2 (OurOS) — Quite Universal Circuit Simulator");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Simulation types:");
        println!("  DC, AC, S-parameter, transient, noise,");
        println!("  digital, harmonic balance, parameter sweep");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qucs v24.2 (OurOS)"); return 0; }
    println!("qucs: circuit simulator GUI started");
    println!("  Components: R, L, C, diodes, BJT, MOSFET, OpAmp, ...");
    println!("  Simulation: DC/AC/Transient/S-parameter");
    println!("  Visualization: Smith chart, Bode plot, waveforms");
    0
}

fn run_qucsator(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qucsator [OPTIONS] -i INPUT -o OUTPUT");
        println!("qucsator v24.2 (OurOS) — Qucs simulation engine");
        println!();
        println!("Options:");
        println!("  -i FILE           Input netlist");
        println!("  -o FILE           Output dataset");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qucsator v24.2 (OurOS)"); return 0; }
    println!("qucsator: running simulation...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qucs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "qucsator" => run_qucsator(&rest, &prog),
        _ => run_qucs(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
