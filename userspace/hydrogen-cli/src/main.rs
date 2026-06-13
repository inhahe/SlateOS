#![deny(clippy::all)]

//! hydrogen-cli — Slate OS Hydrogen drum machine
//!
//! Single personality: `hydrogen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hydrogen(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hydrogen [OPTIONS] [SONG_FILE]");
        println!("Hydrogen v1.2 (Slate OS) — Advanced drum machine");
        println!();
        println!("Options:");
        println!("  -d DRIVER     Audio driver (jack, alsa, oss)");
        println!("  -s SONG       Load song file (.h2song)");
        println!("  -k DRUMKIT    Load drumkit");
        println!("  -n             No GUI (headless)");
        println!("  -p PATTERN    Start with pattern N");
        println!("  --export FILE  Export song to WAV");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hydrogen v1.2.3 (Slate OS)"); return 0; }
    println!("Hydrogen v1.2.3 (Slate OS) — Drum Machine");
    println!("  Audio: JACK @ 44100 Hz");
    println!("  Drumkit: GMRockKit");
    println!("    Instruments: 18");
    println!("    Kick, Snare, HiHat(open/closed), Tom(hi/mid/lo),");
    println!("    Crash, Ride, Splash, China, Cowbell, Clap, Tambourine");
    println!("  Song: rock_beat.h2song");
    println!("    Patterns: 8");
    println!("    Tempo: 120 BPM");
    println!("    Resolution: 1/16");
    println!("  Playback started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hydrogen".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hydrogen(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hydrogen};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hydrogen"), "hydrogen");
        assert_eq!(basename(r"C:\bin\hydrogen.exe"), "hydrogen.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hydrogen.exe"), "hydrogen");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hydrogen(&["--help".to_string()], "hydrogen"), 0);
        assert_eq!(run_hydrogen(&["-h".to_string()], "hydrogen"), 0);
        let _ = run_hydrogen(&["--version".to_string()], "hydrogen");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hydrogen(&[], "hydrogen");
    }
}
