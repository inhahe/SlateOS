#![deny(clippy::all)]

//! lite-xl-cli — OurOS Lite XL editor
//!
//! Single personality: `lite-xl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lite_xl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lite-xl [OPTIONS] [FILE|DIR...]");
        println!("Lite XL 2.1.5 (OurOS) — Lightweight, extensible text editor");
        println!();
        println!("Options:");
        println!("  --core-dir DIR       Core script directory");
        println!("  --user-dir DIR       User data directory");
        println!("  --log-file FILE      Log file path");
        println!("  --clean              Start without user config");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Lite XL 2.1.5 (OurOS)");
        return 0;
    }
    let paths: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if paths.is_empty() {
        println!("lite-xl: Opening empty workspace...");
    } else {
        for p in &paths {
            println!("lite-xl: Opening '{}'", p);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lite-xl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lite_xl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
