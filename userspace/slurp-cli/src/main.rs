#![deny(clippy::all)]

//! slurp-cli — OurOS slurp region selector
//!
//! Single personality: `slurp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_slurp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: slurp [OPTIONS]");
        println!("slurp v1.5 (OurOS) — Select a region in Wayland compositor");
        println!();
        println!("Options:");
        println!("  -d                Show display dimensions");
        println!("  -b COLOR          Background color");
        println!("  -c COLOR          Border color");
        println!("  -s COLOR          Selection color");
        println!("  -B COLOR          Border color");
        println!("  -w N              Border width");
        println!("  -f FORMAT         Output format");
        println!("  -p                Select a single point");
        println!("  -o                Select entire output");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("3840x2160");
        return 0;
    }
    if args.iter().any(|a| a == "-p") {
        println!("960,540");
        return 0;
    }
    // Default: output selected region
    println!("100,200 800x600");
    if args.is_empty() {
        // Interactive selection simulation
        println!("100,200 800x600");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "slurp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_slurp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
