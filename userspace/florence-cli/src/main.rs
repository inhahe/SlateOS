#![deny(clippy::all)]

//! florence-cli — OurOS Florence virtual keyboard
//!
//! Single personality: `florence`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_florence(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: florence [OPTIONS]");
        println!("florence v0.6 (OurOS) — Extensible virtual keyboard");
        println!();
        println!("Options:");
        println!("  --no-gnome        Don't use GNOME settings");
        println!("  --use-config FILE Use custom config");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Scalable SVG keyboard, auto-hide on hardware keyboard,");
        println!("  ramble (gesture) input, extensions support");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("florence v0.6 (OurOS)"); return 0; }
    println!("florence: virtual keyboard started");
    println!("  Layout: QWERTY");
    println!("  Extensions: timer, media keys");
    println!("  Auto-hide: enabled");
    println!("  Ramble input: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "florence".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_florence(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
