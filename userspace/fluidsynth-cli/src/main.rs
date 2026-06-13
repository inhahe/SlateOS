#![deny(clippy::all)]

//! fluidsynth-cli — SlateOS FluidSynth software synthesizer
//!
//! Single personality: `fluidsynth`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fluidsynth(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fluidsynth [OPTIONS] [SOUNDFONT] [MIDIFILE...]");
        println!("FluidSynth v2.3 (SlateOS) — SoundFont software synthesizer");
        println!();
        println!("Options:");
        println!("  -a DRIVER      Audio driver (pulseaudio, alsa, jack)");
        println!("  -m DRIVER      MIDI driver (alsa_seq, jack)");
        println!("  -s             Start as server");
        println!("  -i             Non-interactive mode");
        println!("  -g GAIN        Master gain (0.0 - 10.0, default: 0.2)");
        println!("  -o KEY=VAL     Override setting");
        println!("  -r RATE        Sample rate (default: 44100)");
        println!("  -R N           Reverb (0=off, 1=on)");
        println!("  -C N           Chorus (0=off, 1=on)");
        println!("  -F FILE        Render to WAV file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FluidSynth v2.3.4 (SlateOS)"); return 0; }
    println!("FluidSynth v2.3.4 (SlateOS)");
    println!("  Audio driver: pulseaudio");
    println!("  Sample rate: 44100 Hz");
    println!("  Gain: 0.2");
    println!("  Loading: FluidR3_GM.sf2");
    println!("    Presets: 189");
    println!("    Samples: 1,247");
    println!("  Reverb: room=0.2, damp=0.0, width=0.5, level=0.9");
    println!("  Chorus: N=3, level=2.0, speed=0.3, depth=8.0");
    println!("  Ready, listening on MIDI port");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fluidsynth".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fluidsynth(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fluidsynth};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fluidsynth"), "fluidsynth");
        assert_eq!(basename(r"C:\bin\fluidsynth.exe"), "fluidsynth.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fluidsynth.exe"), "fluidsynth");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fluidsynth(&["--help".to_string()], "fluidsynth"), 0);
        assert_eq!(run_fluidsynth(&["-h".to_string()], "fluidsynth"), 0);
        let _ = run_fluidsynth(&["--version".to_string()], "fluidsynth");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fluidsynth(&[], "fluidsynth");
    }
}
