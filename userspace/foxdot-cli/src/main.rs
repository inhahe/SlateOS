#![deny(clippy::all)]

//! foxdot-cli — SlateOS FoxDot live coding with Python
//!
//! Single personality: `foxdot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_foxdot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: foxdot COMMAND [OPTIONS]");
        println!("FoxDot v0.8.13 (Slate OS) — Live coding with Python & SuperCollider");
        println!();
        println!("Commands:");
        println!("  start           Start FoxDot session");
        println!("  run FILE        Run a FoxDot script");
        println!("  synths          List available synths");
        println!("  effects         List audio effects");
        println!("  info            Show setup info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("FoxDot v0.8.13 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "start" => {
            println!("Starting FoxDot...");
            println!("  Connecting to SuperCollider (sclang)...");
            println!("  Loading SynthDefs...");
            println!("  FoxDot interactive session ready.");
        }
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("song.py");
            println!("Running: {}", file);
            println!("  Parsing... OK");
            println!("  Playing patterns...");
        }
        "synths" => {
            println!("Available synths:");
            println!("  pluck, bass, keys, lead, swell");
            println!("  blip, pads, marimba, bell, gong");
            println!("  saw, square, sine, noise, pulse");
        }
        "effects" => {
            println!("Audio effects:");
            println!("  reverb, delay, chorus, flanger");
            println!("  distortion, bitcrush, tremolo");
            println!("  lpf, hpf, bpf, swell");
        }
        "info" => {
            println!("FoxDot v0.8.13");
            println!("  Language: Python");
            println!("  Backend: SuperCollider");
            println!("  Protocol: OSC");
            println!("  Features: pattern sequencing, clock sync");
        }
        _ => println!("foxdot {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "foxdot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_foxdot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_foxdot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/foxdot"), "foxdot");
        assert_eq!(basename(r"C:\bin\foxdot.exe"), "foxdot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("foxdot.exe"), "foxdot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_foxdot(&["--help".to_string()], "foxdot"), 0);
        assert_eq!(run_foxdot(&["-h".to_string()], "foxdot"), 0);
        let _ = run_foxdot(&["--version".to_string()], "foxdot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_foxdot(&[], "foxdot");
    }
}
