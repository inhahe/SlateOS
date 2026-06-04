#![deny(clippy::all)]

//! reaper-cli — OurOS REAPER digital audio workstation
//!
//! Single personality: `reaper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_reaper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: reaper [OPTIONS] [FILE.rpp]");
        println!("REAPER v7.10 (OurOS) — Digital Audio Workstation");
        println!();
        println!("Options:");
        println!("  FILE.rpp          Open project file");
        println!("  -renderproject    Render project to audio");
        println!("  -batchconvert     Batch convert audio files");
        println!("  -splashlog        Show startup log");
        println!("  -cfgfile FILE     Use alternate config");
        println!("  -nosplash         Skip splash screen");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("REAPER v7.10 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-renderproject") {
        println!("Rendering project...");
        println!("  Format: WAV 48000Hz 24-bit");
        println!("  Master mix + stems");
        println!("  Rendering... Done.");
        return 0;
    }
    if args.iter().any(|a| a == "-batchconvert") {
        println!("Batch converter ready.");
        println!("  Drop files or specify input directory.");
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("Untitled.rpp");
    println!("REAPER v7.10 — Opening: {}", file);
    println!("  Tracks: 16");
    println!("  FX: ReaEQ, ReaComp, ReaVerb loaded");
    println!("  Audio: ASIO, 48000 Hz, 256 samples");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "reaper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_reaper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_reaper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/reaper"), "reaper");
        assert_eq!(basename(r"C:\bin\reaper.exe"), "reaper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("reaper.exe"), "reaper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_reaper(&["--help".to_string()], "reaper"), 0);
        assert_eq!(run_reaper(&["-h".to_string()], "reaper"), 0);
        let _ = run_reaper(&["--version".to_string()], "reaper");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_reaper(&[], "reaper");
    }
}
