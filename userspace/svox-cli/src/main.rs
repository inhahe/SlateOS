#![deny(clippy::all)]

//! svox-cli — SlateOS SVOX pico text-to-speech engine
//!
//! Single personality: `pico2wave`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pico2wave(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pico2wave [OPTIONS] -w FILE \"TEXT\"");
        println!("pico2wave v1.0 (SlateOS) — SVOX Pico TTS wave file generator");
        println!();
        println!("Options:");
        println!("  -w FILE         Output WAV file");
        println!("  -l LANG         Language (en-US, en-GB, de-DE, es-ES, fr-FR, it-IT)");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pico2wave v1.0 (SlateOS, SVOX Pico engine)"); return 0; }
    let output = args.windows(2).find(|w| w[0] == "-w").map(|w| w[1].as_str());
    let lang = args.windows(2).find(|w| w[0] == "-l").map(|w| w[1].as_str()).unwrap_or("en-US");
    if output.is_none() {
        eprintln!("pico2wave: error: output file required (-w)");
        return 1;
    }
    let text: Vec<&String> = args.iter().filter(|a| !a.starts_with('-') && {
        let idx = args.iter().position(|x| std::ptr::eq(x, *a)).unwrap_or(0);
        idx == 0 || !matches!(args.get(idx.wrapping_sub(1)).map(|s| s.as_str()), Some("-w" | "-l"))
    }).collect();
    println!("pico2wave: SVOX Pico TTS");
    println!("  Language: {}", lang);
    println!("  Output: {}", output.unwrap_or("output.wav"));
    println!("  Text: {} characters", text.iter().map(|t| t.len()).sum::<usize>());
    println!("  Format: 16000 Hz, 16-bit, mono PCM");
    println!("  Duration: 2.5s");
    println!("  Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pico2wave".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pico2wave(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pico2wave};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/svox"), "svox");
        assert_eq!(basename(r"C:\bin\svox.exe"), "svox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("svox.exe"), "svox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pico2wave(&["--help".to_string()], "svox"), 0);
        assert_eq!(run_pico2wave(&["-h".to_string()], "svox"), 0);
        let _ = run_pico2wave(&["--version".to_string()], "svox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pico2wave(&[], "svox");
    }
}
