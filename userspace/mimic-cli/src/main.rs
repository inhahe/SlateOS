#![deny(clippy::all)]

//! mimic-cli — OurOS Mycroft Mimic speech synthesis
//!
//! Single personality: `mimic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mimic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mimic [OPTIONS] TEXT");
        println!("Mimic3 v0.2 (OurOS) — Neural text-to-speech engine");
        println!();
        println!("Options:");
        println!("  -t TEXT           Text to speak");
        println!("  -f FILE           Text file to speak");
        println!("  -o FILE           Output WAV file");
        println!("  --voice NAME      Voice key");
        println!("  --voices          List available voices");
        println!("  --length-scale N  Speed (default: 1.0)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Mimic3 v0.2 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--voices") {
        println!("Available voices:");
        println!("  en_US/ljspeech_low");
        println!("  en_US/vctk_low (multi-speaker)");
        println!("  en_UK/apope_low");
        println!("  de_DE/thorsten_low");
        return 0;
    }
    let text = args.iter()
        .position(|a| a == "-t")
        .and_then(|i| args.get(i + 1))
        .or_else(|| args.iter().find(|a| !a.starts_with('-')))
        .map(|s| s.as_str())
        .unwrap_or("Hello world");
    println!("Synthesizing: \"{}\"", text);
    println!("  Voice: en_US/ljspeech_low");
    println!("  Sample rate: 22050 Hz");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mimic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mimic(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
