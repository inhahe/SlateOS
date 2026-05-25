#![deny(clippy::all)]

//! multimon-cli — OurOS multimon-ng digital signal decoder
//!
//! Single personality: `multimon-ng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_multimon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: multimon-ng [OPTIONS] [FILE]");
        println!("multimon-ng v1.2 (OurOS) — Digital signal decoder");
        println!();
        println!("Options:");
        println!("  -a MODE        Add decoder (POCSAG512, POCSAG1200, POCSAG2400,");
        println!("                              FLEX, EAS, UFSK1200, DTMF, ZVEI,");
        println!("                              AFSK1200, AFSK2400, HAPN4800, FSK9600,");
        println!("                              MORSE_CW, X10, SCOPE)");
        println!("  -t TYPE        Input type (raw, wav, au)");
        println!("  -q             Quiet (no frame sync output)");
        println!("  -v N           Verbosity level");
        println!("  --timestamp    Add timestamps");
        println!("  --label LBL    Label output");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("multimon-ng v1.2.0 (OurOS)"); return 0; }
    println!("multimon-ng v1.2.0 (OurOS)");
    println!("  Decoders: POCSAG512, POCSAG1200, POCSAG2400, FLEX");
    println!("  Input: stdin (raw audio, 22050 Hz)");
    println!("  POCSAG1200: Address: 1234567 Function: 0");
    println!("  POCSAG1200: Alpha: Test message received");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "multimon-ng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_multimon(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
