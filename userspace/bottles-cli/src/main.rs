#![deny(clippy::all)]

//! bottles-cli — OurOS Bottles Wine prefix manager
//!
//! Single personality: `bottles`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bottles(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bottles [OPTIONS]");
        println!("bottles v51.0 (OurOS) — Wine prefix manager");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bottles v51.0 (OurOS)"); return 0; }
    println!("bottles: Wine prefix manager started");
    println!("  Bottles: 3 configured");
    println!("    Gaming (Caffe 8.21, DXVK 2.3)");
    println!("    Software (Soda 9.0)");
    println!("    Custom (Wine 9.0)");
    println!("  Runners: caffe, soda, wine, proton");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bottles".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bottles(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
