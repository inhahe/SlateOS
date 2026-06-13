#![deny(clippy::all)]

//! reason-cli — SlateOS Reason Studios Reason DAW/rack
//!
//! Single personality: `reason`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_reason(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: reason [OPTIONS] [SONG]");
        println!("Reason Studios Reason 12 (Slate OS) — Virtual rack DAW & instrument suite");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .reason song");
        println!("  --rack                 Show rack view (flip with TAB)");
        println!("  --combinator           Open Combinator builder");
        println!("  --rack-plugin          Run as Reason Rack Plugin");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Reason Studios Reason 12.7.5 (Slate OS)"); return 0; }
    println!("Reason Studios Reason 12.7.5 (Slate OS)");
    println!("  Mode: Standalone DAW or VST3/AU \"Reason Rack Plugin\"");
    println!("  Devices: Subtractor, Thor, Europa, Grain, Reason Drum Kits");
    println!("  Effects: 30+ classic & modern (Scream, RV-7, Pulveriser)");
    println!("  Rack Extensions: 100+ third-party modules (Propellerhead format)");
    println!("  Cable view: visualize/edit signal flow with virtual cables");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "reason".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_reason(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_reason};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/reason"), "reason");
        assert_eq!(basename(r"C:\bin\reason.exe"), "reason.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("reason.exe"), "reason");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_reason(&["--help".to_string()], "reason"), 0);
        assert_eq!(run_reason(&["-h".to_string()], "reason"), 0);
        let _ = run_reason(&["--version".to_string()], "reason");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_reason(&[], "reason");
    }
}
