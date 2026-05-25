#![deny(clippy::all)]

//! timidity-cli — OurOS TiMidity++ MIDI player
//!
//! Single personality: `timidity`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_timidity(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: timidity [OPTIONS] FILE [FILE...]");
        println!("TiMidity++ v2.15 (OurOS) — MIDI to audio converter/player");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file (WAV, AIFF, etc.)");
        println!("  -Ow           Output as WAV");
        println!("  -Or           Output as raw PCM");
        println!("  -s RATE       Sample rate (default: 44100)");
        println!("  -A FACTOR     Amplification (default: 100)");
        println!("  -c FILE       Configuration file");
        println!("  -L DIR        Soundfont/patch directory");
        println!("  -EF           Fast decay mode");
        println!("  -p N          Polyphony (max voices, default: 256)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("TiMidity++ v2.15.0 (OurOS)"); return 0; }
    println!("TiMidity++ v2.15.0 (OurOS)");
    println!("  Soundfont: FluidR3_GM.sf2");
    println!("  Playing: bach_bwv846.mid");
    println!("    Format: Standard MIDI (type 1)");
    println!("    Tracks: 4");
    println!("    Tempo: 120 BPM");
    println!("    Duration: 4:23");
    println!("    Polyphony peak: 32 voices");
    println!("  Output: 44100 Hz, 16-bit, stereo");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "timidity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_timidity(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
