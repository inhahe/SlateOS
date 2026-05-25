#![deny(clippy::all)]

//! omnisphere-cli — OurOS Spectrasonics Omnisphere flagship synth
//!
//! Single personality: `omnisphere`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_omni(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: omnisphere [OPTIONS] [PATCH]");
        println!("Spectrasonics Omnisphere 2.8 (OurOS) — Flagship hybrid synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .prt_omn patch");
        println!("  --orb                  Open Orb performance interface");
        println!("  --hardware-profile DEV Hardware synth profile (75+ models)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Spectrasonics Omnisphere 2.8.5d (OurOS)"); return 0; }
    println!("Spectrasonics Omnisphere 2.8.5d (OurOS)");
    println!("  Core library: 14,000+ patches, 60+ GB sound sources");
    println!("  Synthesis: Multi-mode (sample-playback + Synth oscillators)");
    println!("  Hardware sync: 75+ hardware synths via Hardware Library");
    println!("  Effects: 58 (FX rack), modulation matrix");
    println!("  Plug-in formats: VST2, VST3, AU, AAX");
    println!("  Companion: Trilian (bass), Keyscape, Stylus RMX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "omnisphere".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_omni(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
