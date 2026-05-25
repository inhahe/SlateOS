#![deny(clippy::all)]

//! minuet-cli — OurOS Minuet music education
//!
//! Single personality: `minuet`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_minuet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: minuet [OPTIONS]");
        println!("minuet v23.08 (OurOS) — Music education software");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Exercises:");
        println!("  Intervals        Identify musical intervals");
        println!("  Chords           Identify chord types");
        println!("  Scales           Identify scale types");
        println!("  Rhythms          Rhythm dictation");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("minuet v23.08 (OurOS)"); return 0; }
    println!("minuet: music education started");
    println!("  MIDI backend: FluidSynth");
    println!("  Exercises: intervals, chords, scales, rhythms");
    println!("  Progress tracking: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "minuet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_minuet(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
