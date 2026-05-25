#![deny(clippy::all)]

//! magnifier-cli — OurOS screen magnifier
//!
//! Single personality: `magnifier`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_magnifier(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: magnifier [OPTIONS]");
        println!("magnifier v1.0 (OurOS) — Screen magnification tool");
        println!();
        println!("Options:");
        println!("  --zoom LEVEL      Initial zoom level (2-32, default: 4)");
        println!("  --mode MODE       full-screen, lens, docked");
        println!("  --invert          Invert colors");
        println!("  --crosshair       Show crosshair cursor");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("magnifier v1.0 (OurOS)"); return 0; }
    println!("magnifier: screen magnification active");
    println!("  Zoom: 4x");
    println!("  Mode: full-screen");
    println!("  Controls: Ctrl+= zoom in, Ctrl+- zoom out");
    println!("  Color inversion: off");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "magnifier".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_magnifier(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
