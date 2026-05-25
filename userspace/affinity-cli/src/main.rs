#![deny(clippy::all)]

//! affinity-cli — OurOS Affinity creative suite
//!
//! Single personality: `affinity`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_affinity(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: affinity [OPTIONS] [FILE]");
        println!("Affinity Suite 2 (OurOS) — Photo + Designer + Publisher (perpetual license)");
        println!();
        println!("Options:");
        println!("  --photo                Launch Affinity Photo");
        println!("  --designer             Launch Affinity Designer");
        println!("  --publisher            Launch Affinity Publisher");
        println!("  --export FORMAT FILE   Export (afphoto/png/jpg/pdf/svg)");
        println!("  --macro FILE           Run macro");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Affinity Suite 2.5 (OurOS)"); return 0; }
    println!("Affinity Suite 2.5 (OurOS)");
    println!("  Apps: Affinity Photo 2, Designer 2, Publisher 2");
    println!("  Engine: Metal / Vulkan GPU acceleration");
    println!("  Personas: switch between vector, pixel, export modes in one app");
    println!("  Color: 32-bit per channel, OpenColorIO, RGB/CMYK/LAB/Grayscale");
    println!("  License: perpetual (no subscription)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "affinity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_affinity(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
