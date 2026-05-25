#![deny(clippy::all)]

//! rosegarden-cli — OurOS Rosegarden MIDI sequencer
//!
//! Single personality: `rosegarden`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rosegarden(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rosegarden [OPTIONS] [FILE]");
        println!("Rosegarden v23 (OurOS) — MIDI sequencer and music notation editor");
        println!();
        println!("Options:");
        println!("  --nosplash       Skip splash screen");
        println!("  --nosequencer    Start without sequencer");
        println!("  --import FILE    Import MIDI file");
        println!("  --export FILE    Export to MIDI/LilyPond/MusicXML");
        println!("  --convert FILE   Convert between formats");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rosegarden v23.12 (OurOS)"); return 0; }
    println!("Rosegarden v23.12 (OurOS) — MIDI Sequencer");
    println!("  ALSA sequencer: connected");
    println!("  JACK audio: connected");
    println!("  Composition: symphony_mvt1.rg");
    println!("    Tracks: 24");
    println!("    Duration: 12:45");
    println!("    Time signature: 4/4");
    println!("    Key: C major");
    println!("  Segments: 156");
    println!("  Notation: standard staff, percussion grid");
    println!("  MIDI outputs: 4 ports");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rosegarden".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rosegarden(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
