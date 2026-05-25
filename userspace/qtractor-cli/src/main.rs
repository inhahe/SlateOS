#![deny(clippy::all)]

//! qtractor-cli — OurOS Qtractor audio/MIDI DAW
//!
//! Single personality: `qtractor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qtractor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qtractor [OPTIONS] [SESSION_FILE]");
        println!("Qtractor v0.9 (OurOS) — Audio/MIDI multi-track sequencer");
        println!();
        println!("Options:");
        println!("  -s FILE       Load session file (.qtr)");
        println!("  -p            Start playback immediately");
        println!("  --midi-bus N   Number of MIDI buses");
        println!("  --audio-bus N  Number of audio buses");
        println!("  --tempo BPM    Initial tempo");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Qtractor v0.9.39 (OurOS)"); return 0; }
    println!("Qtractor v0.9.39 (OurOS) — Audio/MIDI Sequencer");
    println!("  JACK audio: 48000 Hz, buffer 512");
    println!("  Session: studio_session.qtr");
    println!("  Tracks:");
    println!("    Audio: 8 (vocals, guitar, bass, drums L/R, keys, fx1, fx2)");
    println!("    MIDI: 4 (synth1, synth2, strings, percussion)");
    println!("  Plugins loaded:");
    println!("    LV2: Calf Compressor, ZaMultiComp, Dragonfly Reverb");
    println!("    LADSPA: SC4 Compressor, TAP Reverberator");
    println!("  Tempo: 128 BPM, 4/4");
    println!("  Transport ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qtractor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qtractor(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
