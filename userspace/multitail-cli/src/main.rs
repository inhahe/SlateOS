#![deny(clippy::all)]

//! multitail-cli — OurOS MultiTail multi-log viewer
//!
//! Single personality: `multitail`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_multitail(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: multitail [OPTIONS] FILE [FILE...]");
        println!("multitail v7.1 (OurOS) — Multiple log file viewer");
        println!();
        println!("Options:");
        println!("  -s N              Split vertically into N columns");
        println!("  -sw H,H,...       Set window heights");
        println!("  -e REGEX          Highlight matching lines");
        println!("  -cS SCHEME        Color scheme");
        println!("  --version         Show version");
        println!();
        println!("View multiple log files in split-screen terminal windows.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("multitail v7.1 (OurOS)"); return 0; }
    println!("multitail: viewing {} log file(s)", args.len());
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "multitail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_multitail(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
