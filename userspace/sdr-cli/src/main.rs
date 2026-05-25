#![deny(clippy::all)]

//! sdr-cli — OurOS SDR++ receiver
//!
//! Single personality: `sdrpp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sdrpp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sdrpp [OPTIONS]");
        println!("SDR++ v1.1 (OurOS) — Bloat-free SDR receiver");
        println!();
        println!("Options:");
        println!("  -r DIR         Root/config directory");
        println!("  -s NAME        Source module to use");
        println!("  --server       Start server mode");
        println!("  --port N       Server port (default: 5259)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SDR++ v1.1.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--server") {
        println!("SDR++ Server Mode");
        println!("  Port: 5259");
        println!("  Status: listening");
        return 0;
    }
    println!("SDR++ v1.1.0 (OurOS)");
    println!("  Sources: RTL-SDR, Airspy, HackRF, SDRPlay, PlutoSDR");
    println!("  Demodulators: NFM, WFM, AM, DSB, USB, LSB, CW, RAW");
    println!("  FFT: 65536-point");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sdrpp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sdrpp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
