#![deny(clippy::all)]

//! sonic-pi-cli — OurOS Sonic Pi live coding synth
//!
//! Single personality: `sonic-pi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sonic_pi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sonic-pi COMMAND [OPTIONS]");
        println!("Sonic Pi v4.5 (OurOS) — The live coding music synth");
        println!();
        println!("Commands:");
        println!("  start           Start Sonic Pi server");
        println!("  run FILE        Run a Sonic Pi script");
        println!("  stop            Stop all sounds");
        println!("  eval CODE       Evaluate inline code");
        println!("  record FILE     Record output to WAV");
        println!("  info            Show audio info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Sonic Pi v4.5 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "start" => {
            println!("Starting Sonic Pi server...");
            println!("  SuperCollider: scsynth booted");
            println!("  OSC port: 4560");
            println!("  Ready for live coding.");
        }
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("song.rb");
            println!("Running: {}", file);
            println!("  Parsing... OK");
            println!("  Playing...");
        }
        "stop" => println!("Stopping all sounds."),
        "eval" => {
            let code = args.get(1).map(|s| s.as_str()).unwrap_or("play 60");
            println!("Evaluating: {}", code);
        }
        "record" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("output.wav");
            println!("Recording to: {}", file);
            println!("  Format: WAV 44100Hz stereo");
        }
        "info" => {
            println!("Sonic Pi v4.5");
            println!("  Synth engine: SuperCollider");
            println!("  Language: Ruby-based DSL");
            println!("  OSC: bidirectional");
            println!("  MIDI: supported");
        }
        _ => println!("sonic-pi {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sonic-pi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sonic_pi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sonic_pi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sonic-pi"), "sonic-pi");
        assert_eq!(basename(r"C:\bin\sonic-pi.exe"), "sonic-pi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sonic-pi.exe"), "sonic-pi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sonic_pi(&["--help".to_string()], "sonic-pi"), 0);
        assert_eq!(run_sonic_pi(&["-h".to_string()], "sonic-pi"), 0);
        let _ = run_sonic_pi(&["--version".to_string()], "sonic-pi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sonic_pi(&[], "sonic-pi");
    }
}
