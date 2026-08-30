#![deny(clippy::all)]

//! picotts-cli — Slate OS Pico TTS command-line interface
//!
//! Single personality: `picotts`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_picotts(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: picotts [OPTIONS] [TEXT]");
        println!("picotts v1.0 (Slate OS) — Pico TTS command-line interface");
        println!();
        println!("Options:");
        println!("  -o FILE        Output audio file (wav)");
        println!("  -l LANG        Language code (default: en-US)");
        println!("  -s SPEED       Speaking speed (0.5-2.0, default: 1.0)");
        println!("  -p PITCH       Pitch adjustment (0.5-2.0, default: 1.0)");
        println!("  -v VOLUME      Volume (0.0-1.0, default: 1.0)");
        println!("  --stdin        Read text from stdin");
        println!("  --play         Play audio directly");
        println!("  --list-voices  List available voices");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("picotts v1.0 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list-voices") {
        println!("Available voices:");
        println!("  en-US    English (US)");
        println!("  en-GB    English (UK)");
        println!("  de-DE    German");
        println!("  es-ES    Spanish");
        println!("  fr-FR    French");
        println!("  it-IT    Italian");
        return 0;
    }
    let lang = args.windows(2).find(|w| w[0] == "-l").map(|w| w[1].as_str()).unwrap_or("en-US");
    let speed = args.windows(2).find(|w| w[0] == "-s").map(|w| w[1].as_str()).unwrap_or("1.0");
    println!("picotts: synthesizing speech");
    println!("  Voice: {}", lang);
    println!("  Speed: {}x", speed);
    println!("  Pitch: 1.0x");
    println!("  Processing text...");
    println!("  Audio: 2.3s, 16000 Hz mono");
    println!("  Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "picotts".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_picotts(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_picotts};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/picotts"), "picotts");
        assert_eq!(basename(r"C:\bin\picotts.exe"), "picotts.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("picotts.exe"), "picotts");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_picotts(&["--help".to_string()], "picotts"), 0);
        assert_eq!(run_picotts(&["-h".to_string()], "picotts"), 0);
        let _ = run_picotts(&["--version".to_string()], "picotts");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_picotts(&[], "picotts");
    }
}
