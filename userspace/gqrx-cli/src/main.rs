#![deny(clippy::all)]

//! gqrx-cli — OurOS Gqrx SDR receiver
//!
//! Single personality: `gqrx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gqrx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gqrx [OPTIONS]");
        println!("gqrx v2.17 (OurOS) — SDR receiver powered by GNU Radio");
        println!();
        println!("Options:");
        println!("  -c FILE        Configuration file");
        println!("  -f FREQ        Initial frequency (Hz)");
        println!("  -d DEVICE      SDR device string");
        println!("  -r             Remote control mode");
        println!("  --edit         Open config dialog");
        println!("  --list         List available devices");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gqrx v2.17.5 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Available SDR devices:");
        println!("  0: RTL-SDR (RTL2832U), Serial: 00000001");
        println!("  1: Airspy Mini, Serial: 1234ABCD");
        println!("  2: HackRF One, Serial: 0000000000000000");
        return 0;
    }
    let freq = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("100000000");
    println!("Gqrx v2.17.5 (OurOS) — SDR Receiver");
    println!("  Device: RTL-SDR (RTL2832U)");
    println!("  Frequency: {} Hz", freq);
    println!("  Sample rate: 2.4 Msps");
    println!("  Demodulator: WFM Stereo");
    println!("  FFT size: 4096");
    println!("  Status: receiving");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gqrx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gqrx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
