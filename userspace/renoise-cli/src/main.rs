#![deny(clippy::all)]

//! renoise-cli — SlateOS Renoise tracker DAW
//!
//! Single personality: `renoise`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_renoise(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: renoise [OPTIONS] [FILE.xrns]");
        println!("Renoise v3.4.3 (Slate OS) — Digital audio workstation / tracker");
        println!();
        println!("Options:");
        println!("  FILE.xrns         Open song file");
        println!("  --render FILE     Render song to WAV");
        println!("  --scripting       Enable scripting terminal");
        println!("  --fullscreen      Start in fullscreen");
        println!("  --samplerate N    Set sample rate");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Renoise v3.4.3 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--render") {
        let file = args.iter()
            .position(|a| a == "--render")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("output.wav");
        println!("Rendering to: {}", file);
        println!("  Sample rate: 44100 Hz");
        println!("  Bit depth: 24-bit");
        println!("  Rendering... Done.");
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("Untitled.xrns");
    println!("Renoise v3.4.3 — Opening: {}", file);
    println!("  Tracks: 12");
    println!("  Patterns: 32");
    println!("  BPM: 120, LPB: 4");
    println!("  Audio: 44100 Hz / 16-bit stereo");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "renoise".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_renoise(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_renoise};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/renoise"), "renoise");
        assert_eq!(basename(r"C:\bin\renoise.exe"), "renoise.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("renoise.exe"), "renoise");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_renoise(&["--help".to_string()], "renoise"), 0);
        assert_eq!(run_renoise(&["-h".to_string()], "renoise"), 0);
        let _ = run_renoise(&["--version".to_string()], "renoise");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_renoise(&[], "renoise");
    }
}
