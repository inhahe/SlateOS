#![deny(clippy::all)]

//! wirecast-cli — SlateOS Telestream Wirecast live production
//!
//! Single personality: `wirecast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wirecast [OPTIONS] [DOCUMENT]");
        println!("Telestream Wirecast 16 Pro (SlateOS) — Cross-platform live video production");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .wcst document");
        println!("  --rendezvous           Open Rendezvous (remote guests)");
        println!("  --replay               Show Replay (instant replay)");
        println!("  --start                Start broadcasting");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Telestream Wirecast 16.2.0 Pro (SlateOS)"); return 0; }
    println!("Telestream Wirecast 16.2.0 Pro (SlateOS)");
    println!("  Editions: Studio, Pro");
    println!("  Inputs: Camera, NDI, SRT, screen, IP camera, Rendezvous (browser)");
    println!("  Outputs: YouTube/Facebook/Twitch/Vimeo/Custom, multi-streaming");
    println!("  Built-in: PTZ control, ISO recording, virtual sets, chroma key");
    println!("  Captioning: Live closed captions (AI or manual)");
    println!("  License: perpetual + premium support, cross-platform (Win/Mac)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wirecast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wirecast"), "wirecast");
        assert_eq!(basename(r"C:\bin\wirecast.exe"), "wirecast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wirecast.exe"), "wirecast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wc(&["--help".to_string()], "wirecast"), 0);
        assert_eq!(run_wc(&["-h".to_string()], "wirecast"), 0);
        let _ = run_wc(&["--version".to_string()], "wirecast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wc(&[], "wirecast");
    }
}
