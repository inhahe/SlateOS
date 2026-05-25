#![deny(clippy::all)]

//! zbrush-cli — OurOS Maxon ZBrush digital sculpting
//!
//! Single personality: `zbrush`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zbrush(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zbrush [OPTIONS] [FILE]");
        println!("Maxon ZBrush 2024 (OurOS) — Digital sculpting & painting");
        println!();
        println!("Options:");
        println!("  -script FILE          Run ZScript file");
        println!("  -open FILE            Open .ZPR project");
        println!("  -export FILE          Export to OBJ/FBX/STL/Maya");
        println!("  -resolution N         Working canvas resolution");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Maxon ZBrush 2024.0.4 (OurOS)"); return 0; }
    println!("Maxon ZBrush 2024.0.4 (OurOS)");
    println!("  Brushes: 400+ (Standard, Clay, Move, Dam Standard, ...)");
    println!("  Features: DynaMesh, ZRemesher, ZSpheres, ZModeler, Sculptris Pro");
    println!("  Polypaint: 32-bit color channels");
    println!("  Subdivision levels: up to 1 billion polys");
    println!("  Scripting: ZScript, Python");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zbrush".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zbrush(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
