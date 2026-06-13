#![deny(clippy::all)]

//! adobepodcast-cli — SlateOS Adobe Podcast (formerly Project Shasta)
//!
//! Single personality: `adobepodcast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: adobepodcast [COMMAND] [OPTIONS]");
        println!("Adobe Podcast (Slate OS) — AI-powered audio enhancement & recording");
        println!();
        println!("Commands:");
        println!("  enhance FILE           Run Enhance Speech (AI) on file");
        println!("  mic-check              Mic Check (analyze recording space)");
        println!("  studio                 Open Adobe Podcast Studio");
        println!("  transcript FILE        Generate transcript");
        println!();
        println!("Options:");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe Podcast 1.0 (beta) (Slate OS)"); return 0; }
    println!("Adobe Podcast (Slate OS) — beta as of 2024");
    println!("  Enhance Speech: AI removes background noise + room echo");
    println!("  Mic Check: scores room/setup for podcast quality");
    println!("  Studio: browser-based multi-guest recording");
    println!("  Integration: Premiere Pro / Audition AI Enhance Speech filter");
    println!("  License: Free during beta / Creative Cloud subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "adobepodcast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ap(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/adobepodcast"), "adobepodcast");
        assert_eq!(basename(r"C:\bin\adobepodcast.exe"), "adobepodcast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("adobepodcast.exe"), "adobepodcast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ap(&["--help".to_string()], "adobepodcast"), 0);
        assert_eq!(run_ap(&["-h".to_string()], "adobepodcast"), 0);
        let _ = run_ap(&["--version".to_string()], "adobepodcast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ap(&[], "adobepodcast");
    }
}
