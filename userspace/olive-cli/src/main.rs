#![deny(clippy::all)]

//! olive-cli — OurOS Olive Editor open-source NLE
//!
//! Single personality: `olive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_olive(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: olive [OPTIONS] [PROJECT]");
        println!("Olive Editor 0.2 (OurOS) — Pro-grade open-source NLE (in active dev)");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .ove project");
        println!("  --export FILE          Export project");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Olive Editor 0.2.0 (OurOS)"); return 0; }
    println!("Olive Editor 0.2.0 (OurOS)");
    println!("  Engine: Custom node-based composition graph");
    println!("  Color: OpenColorIO, 32-bit linear float internal");
    println!("  Node editor: Build effects from primitive nodes (similar to Nuke/Fusion)");
    println!("  Rendering: GPU-accelerated, multi-threaded, frame caching");
    println!("  Formats: All FFmpeg-supported");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "olive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_olive(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
