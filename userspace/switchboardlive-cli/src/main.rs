#![deny(clippy::all)]

//! switchboardlive-cli — Slate OS Switchboard Live multi-streaming
//!
//! Single personality: `switchboardlive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sbl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: switchboardlive [COMMAND] [OPTIONS]");
        println!("Switchboard Live (Slate OS) — Enterprise multi-streaming distribution");
        println!();
        println!("Commands:");
        println!("  workflows              List streaming workflows");
        println!("  destinations           Manage destinations");
        println!("  schedule               Schedule a live event");
        println!("  start ID               Start workflow");
        println!("  stop ID                Stop workflow");
        println!("  geo                    Geo-restrict streams");
        println!();
        println!("Options:");
        println!("  --rtmp-pull             RTMP pull source");
        println!("  --srt                  SRT source");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Switchboard Live v3.4 (Slate OS)"); return 0; }
    println!("Switchboard Live (Slate OS)");
    println!("  Focus: Enterprise & broadcast-grade multi-distribution");
    println!("  Inputs: RTMP push/pull, SRT, NDI bridge");
    println!("  Destinations: 50+ platforms (YouTube, FB, Twitch, X, Trovo, etc.)");
    println!("  Features: Stream cloning, transcoding, mid-stream destination swap");
    println!("  License: Custom enterprise plans");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "switchboardlive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sbl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sbl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/switchboardlive"), "switchboardlive");
        assert_eq!(basename(r"C:\bin\switchboardlive.exe"), "switchboardlive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("switchboardlive.exe"), "switchboardlive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sbl(&["--help".to_string()], "switchboardlive"), 0);
        assert_eq!(run_sbl(&["-h".to_string()], "switchboardlive"), 0);
        let _ = run_sbl(&["--version".to_string()], "switchboardlive");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sbl(&[], "switchboardlive");
    }
}
