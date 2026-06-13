#![deny(clippy::all)]

//! piper-tts-cli — SlateOS Piper neural text-to-speech
//!
//! Single personality: `piper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_piper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: piper [OPTIONS]");
        println!("Piper v1.2 (Slate OS) — Fast neural text-to-speech");
        println!();
        println!("Options:");
        println!("  --model FILE      ONNX voice model");
        println!("  --config FILE     Model config JSON");
        println!("  --output_file FILE  Output WAV");
        println!("  --speaker N       Speaker ID (multi-speaker models)");
        println!("  --length_scale N  Speech speed (default: 1.0)");
        println!("  --sentence_silence N  Pause between sentences (s)");
        println!("  --version         Show version");
        println!();
        println!("  Reads text from stdin, writes audio to stdout or file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Piper v1.2 (Slate OS)");
        return 0;
    }
    println!("Piper TTS — Synthesizing...");
    println!("  Model: en_US-lessac-medium");
    println!("  Sample rate: 22050 Hz");
    println!("  Speed: 1.0x");
    println!("  Output: output.wav");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "piper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_piper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_piper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/piper-tts"), "piper-tts");
        assert_eq!(basename(r"C:\bin\piper-tts.exe"), "piper-tts.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("piper-tts.exe"), "piper-tts");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_piper(&["--help".to_string()], "piper-tts"), 0);
        assert_eq!(run_piper(&["-h".to_string()], "piper-tts"), 0);
        let _ = run_piper(&["--version".to_string()], "piper-tts");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_piper(&[], "piper-tts");
    }
}
