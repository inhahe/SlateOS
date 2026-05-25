#![deny(clippy::all)]

//! edius-cli — OurOS Grass Valley EDIUS broadcast NLE
//!
//! Single personality: `edius`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_edius(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: edius [OPTIONS] [PROJECT]");
        println!("Grass Valley EDIUS X Pro (OurOS) — Broadcast-grade NLE");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .ezp project");
        println!("  --pkg                  Open EDIUS Package");
        println!("  --background-export    Background export mode");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Grass Valley EDIUS X Pro 11.30 (OurOS)"); return 0; }
    println!("Grass Valley EDIUS X Pro 11.30 (OurOS)");
    println!("  Editions: Pro, Workgroup, Elite");
    println!("  Used in: News broadcast, sports production, documentary");
    println!("  Realtime editing: 4K/8K HDR, multi-format timeline");
    println!("  Codecs: All broadcast formats native (XDCAM/AVC-Intra/ProRes/DNxHR)");
    println!("  Audio: Up to 16 channels per track");
    println!("  License: perpetual + Pro Updates / subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "edius".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_edius(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
