#![deny(clippy::all)]

//! waveform-cli — SlateOS Tracktion Waveform DAW
//!
//! Single personality: `waveform`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: waveform [OPTIONS] [PROJECT]");
        println!("Tracktion Waveform Pro 13 (SlateOS) — Unlimited-track single-screen DAW");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .tracktionedit");
        println!("  --render FILE          Render to WAV/FLAC/OGG/MP3");
        println!("  --launcher             Open Launcher (clip-based session)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tracktion Waveform Pro 13.0.41 (SlateOS)"); return 0; }
    println!("Tracktion Waveform Pro 13.0.41 (SlateOS)");
    println!("  Editions: Free, Standard, Pro");
    println!("  Tracks: unlimited audio/MIDI on one timeline");
    println!("  Engine: JUCE-based (Tracktion Engine open source)");
    println!("  Features: Pattern Generator, Chord Track, Modulation matrix");
    println!("  Plug-in formats: VST2, VST3, AU, CLAP, LV2");
    println!("  License: perpetual + free upgrades within major version");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "waveform".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/waveform"), "waveform");
        assert_eq!(basename(r"C:\bin\waveform.exe"), "waveform.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("waveform.exe"), "waveform");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wf(&["--help".to_string()], "waveform"), 0);
        assert_eq!(run_wf(&["-h".to_string()], "waveform"), 0);
        let _ = run_wf(&["--version".to_string()], "waveform");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wf(&[], "waveform");
    }
}
