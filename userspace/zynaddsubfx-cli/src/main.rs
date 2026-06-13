#![deny(clippy::all)]

//! zynaddsubfx-cli — Slate OS ZynAddSubFX synthesizer
//!
//! Single personality: `zynaddsubfx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zynaddsubfx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zynaddsubfx [OPTIONS]");
        println!("ZynAddSubFX v3.0 (Slate OS) — Real-time software synthesizer");
        println!();
        println!("Options:");
        println!("  -r RATE        Sample rate (default: 48000)");
        println!("  -b SIZE        Buffer size (default: 256)");
        println!("  -o DRIVER      Audio output (jack, alsa, oss)");
        println!("  -I DRIVER      MIDI input (jack, alsa)");
        println!("  -l FILE        Load instrument/state file (.xiz/.xmz)");
        println!("  -L FILE        Load instrument to part 0");
        println!("  -p N           UDP OSC port");
        println!("  -N             Run without GUI");
        println!("  -U             Run with no UI (headless)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ZynAddSubFX v3.0.6 (Slate OS)"); return 0; }
    println!("ZynAddSubFX v3.0.6 (Slate OS)");
    println!("  Audio: JACK, 48000 Hz, buffer 256");
    println!("  MIDI: JACK");
    println!("  Engines:");
    println!("    ADsynth: additive synthesis (128 harmonics)");
    println!("    SUBsynth: subtractive synthesis");
    println!("    PADsynth: pad synthesis (bandwidth profiles)");
    println!("  Effects: Reverb, Echo, Chorus, Phaser, Alienwah, Distortion, EQ, DynFilter");
    println!("  Parts: 16 available");
    println!("  OSC port: 7777");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zynaddsubfx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zynaddsubfx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zynaddsubfx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zynaddsubfx"), "zynaddsubfx");
        assert_eq!(basename(r"C:\bin\zynaddsubfx.exe"), "zynaddsubfx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zynaddsubfx.exe"), "zynaddsubfx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zynaddsubfx(&["--help".to_string()], "zynaddsubfx"), 0);
        assert_eq!(run_zynaddsubfx(&["-h".to_string()], "zynaddsubfx"), 0);
        let _ = run_zynaddsubfx(&["--version".to_string()], "zynaddsubfx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zynaddsubfx(&[], "zynaddsubfx");
    }
}
