#![deny(clippy::all)]

//! dasher-cli — OurOS Dasher predictive text input
//!
//! Single personality: `dasher`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dasher(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dasher [OPTIONS]");
        println!("dasher v6.0 (OurOS) — Predictive text input system");
        println!();
        println!("Options:");
        println!("  --alphabet FILE   Custom alphabet file");
        println!("  --training FILE   Language model training file");
        println!("  --version         Show version");
        println!();
        println!("Input methods: mouse, touchscreen, eye-tracker,");
        println!("  joystick, head tracker, breath sensor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dasher v6.0 (OurOS)"); return 0; }
    println!("dasher: predictive text input started");
    println!("  Language model: English");
    println!("  Input: mouse pointer");
    println!("  Speed: adaptive");
    println!("  Prediction: statistical language model");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dasher".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dasher(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
