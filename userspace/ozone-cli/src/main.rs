#![deny(clippy::all)]

//! ozone-cli — OurOS iZotope Ozone mastering suite
//!
//! Single personality: `ozone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ozone(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ozone [OPTIONS] [TRACK]");
        println!("iZotope Ozone 11 Advanced (OurOS) — AI-powered mastering suite");
        println!();
        println!("Options:");
        println!("  --assistant            Run Master Assistant (AI)");
        println!("  --reference FILE       Use reference track");
        println!("  --target STREAM        Target stream loudness (spotify/youtube/tidal/apple)");
        println!("  --analyze FILE         Analyze track");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iZotope Ozone 11.1.0 Advanced (OurOS)"); return 0; }
    println!("iZotope Ozone 11.1.0 Advanced (OurOS)");
    println!("  Modules: Master Assistant, Maximizer, Imager, Stabilizer, Match EQ");
    println!("  AI: Assistant View 2.0 (instrument-aware mastering)");
    println!("  Loudness: ITU-R BS.1770-4, integrated/short/momentary LUFS");
    println!("  Streaming targets: Spotify, YouTube, Tidal, Apple Music, Amazon, Deezer");
    println!("  Plug-in formats: VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ozone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ozone(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ozone};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ozone"), "ozone");
        assert_eq!(basename(r"C:\bin\ozone.exe"), "ozone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ozone.exe"), "ozone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ozone(&["--help".to_string()], "ozone"), 0);
        assert_eq!(run_ozone(&["-h".to_string()], "ozone"), 0);
        let _ = run_ozone(&["--version".to_string()], "ozone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ozone(&[], "ozone");
    }
}
