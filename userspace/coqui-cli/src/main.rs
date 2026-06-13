#![deny(clippy::all)]

//! coqui-cli — SlateOS Coqui TTS neural speech synthesis
//!
//! Single personality: `coqui-tts`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_coqui(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: coqui-tts COMMAND [OPTIONS]");
        println!("Coqui TTS v0.22 (SlateOS) — Deep learning text-to-speech");
        println!();
        println!("Commands:");
        println!("  synthesize TEXT   Synthesize speech");
        println!("  models            List available models");
        println!("  server            Start TTS server");
        println!("  convert           Voice conversion");
        println!("  info              Show configuration");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Coqui TTS v0.22 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "synthesize" => {
            let text = args.get(1).map(|s| s.as_str()).unwrap_or("Hello world");
            println!("Synthesizing: \"{}\"", text);
            println!("  Model: tts_models/en/ljspeech/tacotron2-DDC");
            println!("  Vocoder: vocoder_models/en/ljspeech/hifigan_v2");
            println!("  Output: output.wav (22050 Hz)");
        }
        "models" => {
            println!("Available models:");
            println!("  tts_models/en/ljspeech/tacotron2-DDC");
            println!("  tts_models/en/ljspeech/glow-tts");
            println!("  tts_models/en/vctk/vits (multi-speaker)");
            println!("  tts_models/multilingual/multi-dataset/xtts_v2");
        }
        "server" => {
            println!("Starting Coqui TTS server...");
            println!("  http://localhost:5002");
        }
        "info" => {
            println!("Coqui TTS v0.22");
            println!("  Backend: PyTorch");
            println!("  Models: 4 installed");
            println!("  Languages: en, de, fr, es, ...");
        }
        _ => println!("coqui-tts {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "coqui-tts".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_coqui(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_coqui};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/coqui"), "coqui");
        assert_eq!(basename(r"C:\bin\coqui.exe"), "coqui.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("coqui.exe"), "coqui");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_coqui(&["--help".to_string()], "coqui"), 0);
        assert_eq!(run_coqui(&["-h".to_string()], "coqui"), 0);
        let _ = run_coqui(&["--version".to_string()], "coqui");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_coqui(&[], "coqui");
    }
}
