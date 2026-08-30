#![deny(clippy::all)]

//! ableton-cli — Slate OS Ableton Live DAW
//!
//! Single personality: `ableton`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_live(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ableton [OPTIONS] [PROJECT]");
        println!("Ableton Live 12 Suite (Slate OS) — Performance-oriented DAW");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .als project");
        println!("  --export FILE          Export to WAV/AIFF/MP3/FLAC");
        println!("  --tempo BPM            Set tempo");
        println!("  --max                  Enable Max for Live");
        println!("  --push                 Connect to Push controller");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ableton Live 12.0.10 Suite (Slate OS)"); return 0; }
    println!("Ableton Live 12.0.10 Suite (Slate OS)");
    println!("  Editions: Intro, Standard, Suite");
    println!("  Views: Session (clip launch), Arrangement (timeline)");
    println!("  Instruments: Wavetable, Operator, Drum Rack, Sampler, etc.");
    println!("  Max for Live: built-in patcher (Max/MSP) for custom devices");
    println!("  Audio-to-MIDI, Warping (timestretch), Comping, Tuning systems");
    println!("  License: perpetual + version upgrades");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ableton".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_live(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_live};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ableton"), "ableton");
        assert_eq!(basename(r"C:\bin\ableton.exe"), "ableton.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ableton.exe"), "ableton");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_live(&["--help".to_string()], "ableton"), 0);
        assert_eq!(run_live(&["-h".to_string()], "ableton"), 0);
        let _ = run_live(&["--version".to_string()], "ableton");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_live(&[], "ableton");
    }
}
