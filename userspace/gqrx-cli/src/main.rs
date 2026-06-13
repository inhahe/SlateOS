#![deny(clippy::all)]

//! gqrx-cli — Slate OS Gqrx SDR receiver
//!
//! Single personality: `gqrx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gqrx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gqrx [OPTIONS]");
        println!("gqrx v2.17 (Slate OS) — SDR receiver powered by GNU Radio");
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
    if args.iter().any(|a| a == "--version") { println!("Gqrx v2.17.5 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Available SDR devices:");
        println!("  0: RTL-SDR (RTL2832U), Serial: 00000001");
        println!("  1: Airspy Mini, Serial: 1234ABCD");
        println!("  2: HackRF One, Serial: 0000000000000000");
        return 0;
    }
    let freq = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("100000000");
    println!("Gqrx v2.17.5 (Slate OS) — SDR Receiver");
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
mod tests {
    use super::{basename, strip_ext, run_gqrx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gqrx"), "gqrx");
        assert_eq!(basename(r"C:\bin\gqrx.exe"), "gqrx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gqrx.exe"), "gqrx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gqrx(&["--help".to_string()], "gqrx"), 0);
        assert_eq!(run_gqrx(&["-h".to_string()], "gqrx"), 0);
        let _ = run_gqrx(&["--version".to_string()], "gqrx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gqrx(&[], "gqrx");
    }
}
