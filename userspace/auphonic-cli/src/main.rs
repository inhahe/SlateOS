#![deny(clippy::all)]

//! auphonic-cli — SlateOS Auphonic audio post-production
//!
//! Single personality: `auphonic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_au(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: auphonic [COMMAND] [OPTIONS]");
        println!("Auphonic (SlateOS) — Automated audio post-production for podcasts");
        println!();
        println!("Commands:");
        println!("  upload FILE            Upload file for processing");
        println!("  preset NAME            Use named preset");
        println!("  status ID              Get production status");
        println!("  download ID            Download processed result");
        println!("  publish DESTINATION    Publish to outlet (SoundCloud/PodBean/FTP/S3)");
        println!();
        println!("Options:");
        println!("  --leveler              Apply leveler (preserves dynamics)");
        println!("  --filter               Noise/hum reduction");
        println!("  --loudness TARGET      Match target LUFS (-16 podcast, -23 broadcast)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Auphonic v2024.05 (SlateOS)"); return 0; }
    println!("Auphonic (SlateOS)");
    println!("  Mode: Web service / desktop client / API");
    println!("  Processing: Intelligent leveler, noise/hum reduction, loudness norm");
    println!("  Multi-track: Per-track processing with crosstalk removal");
    println!("  Speech recognition: Whisper integration, automatic chapters");
    println!("  Publishing: Direct to 20+ outlets (RSS, FTP, S3, podcast hosts)");
    println!("  License: Free (2h/month) / Production hours subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "auphonic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_au(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_au};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/auphonic"), "auphonic");
        assert_eq!(basename(r"C:\bin\auphonic.exe"), "auphonic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("auphonic.exe"), "auphonic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_au(&["--help".to_string()], "auphonic"), 0);
        assert_eq!(run_au(&["-h".to_string()], "auphonic"), 0);
        let _ = run_au(&["--version".to_string()], "auphonic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_au(&[], "auphonic");
    }
}
