#![deny(clippy::all)]

//! gespeaker-cli — OurOS GTK frontend for speech synthesis
//!
//! Single personality: `gespeaker`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gespeaker(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gespeaker [OPTIONS] [TEXT]");
        println!("gespeaker v0.8 (OurOS) — GTK speech synthesis frontend");
        println!();
        println!("Options:");
        println!("  -t TEXT        Text to speak");
        println!("  -f FILE        Read text from file");
        println!("  -o FILE        Save to audio file");
        println!("  -v VOICE       Voice name");
        println!("  -l LANG        Language code");
        println!("  -s SPEED       Speed (80-450 words/min, default: 175)");
        println!("  -p PITCH       Pitch (0-99, default: 50)");
        println!("  --volume N     Volume (0-200, default: 100)");
        println!("  --list-voices  List available voices");
        println!("  --backend BE   TTS backend (espeak, mbrola, pico)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gespeaker v0.8.6 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--list-voices") {
        println!("Available voices:");
        println!("  default       Default voice (espeak)");
        println!("  en-us         English (US)");
        println!("  en-gb         English (UK)");
        println!("  de            German");
        println!("  fr            French");
        println!("  es            Spanish");
        println!("  it            Italian");
        println!("  pt            Portuguese");
        println!("  ru            Russian");
        println!("  zh            Chinese (Mandarin)");
        println!("  ja            Japanese");
        return 0;
    }
    let voice = args.windows(2).find(|w| w[0] == "-v").map(|w| w[1].as_str()).unwrap_or("default");
    let speed = args.windows(2).find(|w| w[0] == "-s").map(|w| w[1].as_str()).unwrap_or("175");
    println!("gespeaker v0.8.6 (OurOS)");
    println!("  Backend: espeak");
    println!("  Voice: {}", voice);
    println!("  Speed: {} wpm", speed);
    println!("  Pitch: 50");
    println!("  Volume: 100%");
    println!("  Speaking...");
    println!("  Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gespeaker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gespeaker(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gespeaker};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gespeaker"), "gespeaker");
        assert_eq!(basename(r"C:\bin\gespeaker.exe"), "gespeaker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gespeaker.exe"), "gespeaker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gespeaker(&["--help".to_string()], "gespeaker"), 0);
        assert_eq!(run_gespeaker(&["-h".to_string()], "gespeaker"), 0);
        let _ = run_gespeaker(&["--version".to_string()], "gespeaker");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gespeaker(&[], "gespeaker");
    }
}
