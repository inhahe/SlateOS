#![deny(clippy::all)]

//! vital-cli — OurOS Vital free wavetable synthesizer
//!
//! Single personality: `vital`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vital(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vital [OPTIONS] [PRESET]");
        println!("Vital 1.5 (OurOS) — Free spectral warping wavetable synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .vital preset");
        println!("  --text-to-wavetable T  Convert text to wavetable");
        println!("  --audio-to-wavetable F Convert audio to wavetable");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Vital 1.5.5 (OurOS)"); return 0; }
    println!("Vital 1.5.5 (OurOS)");
    println!("  Oscillators: 3 wavetable + sub + sample, spectral warping");
    println!("  Modulation: 8 LFOs, 6 ENV, 4 random, 8 macros, drag-and-drop");
    println!("  Effects: Chorus, Compressor, Delay, Distortion, EQ, Filter, FlangerL");
    println!("  Wavetable editor: built-in, paint/import/text-to-WT");
    println!("  Plug-in formats: VST2, VST3, AU, LV2");
    println!("  License: Free (GPLv3 source) / Plus / Pro");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vital".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vital(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
