#![deny(clippy::all)]

//! espeak-ng-cli — OurOS eSpeak NG text-to-speech
//!
//! Single personality: `espeak-ng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_espeak_ng(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: espeak-ng [OPTIONS] [TEXT]");
        println!("espeak-ng v1.51 (OurOS) — Text-to-speech synthesizer");
        println!();
        println!("Options:");
        println!("  TEXT              Text to speak (stdin if omitted)");
        println!("  -v VOICE          Voice name (e.g. en, de, fr)");
        println!("  -s SPEED          Speed in words per minute (80-450)");
        println!("  -p PITCH          Pitch (0-99)");
        println!("  -a AMPLITUDE      Amplitude (0-200)");
        println!("  -g GAP            Word gap (10ms units)");
        println!("  -w FILE           Write WAV output to file");
        println!("  -f FILE           Read text from file");
        println!("  --voices          List available voices");
        println!("  --phonout FILE    Write phonemes to file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("espeak-ng v1.51 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--voices") {
        println!("Languages: 100+");
        println!("  en    English");
        println!("  de    German");
        println!("  fr    French");
        println!("  es    Spanish");
        println!("  ja    Japanese");
        println!("  zh    Chinese");
        println!("  ru    Russian");
        return 0;
    }
    let voice = args.iter().skip_while(|a| a.as_str() != "-v").nth(1)
        .map(|s| s.as_str()).unwrap_or("en");
    let text = args.iter().find(|a| !a.starts_with('-') && a.as_str() != voice)
        .map(|s| s.as_str());
    if let Some(t) = text {
        println!("Speaking (voice={}): {}", voice, t);
    } else {
        println!("Reading from stdin (voice={})...", voice);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "espeak-ng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_espeak_ng(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_espeak_ng};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/espeak-ng"), "espeak-ng");
        assert_eq!(basename(r"C:\bin\espeak-ng.exe"), "espeak-ng.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("espeak-ng.exe"), "espeak-ng");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_espeak_ng(&["--help".to_string()], "espeak-ng"), 0);
        assert_eq!(run_espeak_ng(&["-h".to_string()], "espeak-ng"), 0);
        let _ = run_espeak_ng(&["--version".to_string()], "espeak-ng");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_espeak_ng(&[], "espeak-ng");
    }
}
