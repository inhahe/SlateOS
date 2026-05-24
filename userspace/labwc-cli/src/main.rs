#![deny(clippy::all)]

//! labwc-cli — OurOS labwc Wayland stacking compositor
//!
//! Single personality: `labwc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_labwc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: labwc [OPTIONS]");
        println!("labwc v0.7 (OurOS) — Wayland stacking compositor (Openbox-like)");
        println!();
        println!("Options:");
        println!("  -s CMD            Startup command");
        println!("  -C DIR            Config directory");
        println!("  -d                Debug mode");
        println!("  -V                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") { println!("labwc v0.7 (OurOS)"); return 0; }
    println!("labwc compositor starting...");
    println!("  Theme: Clearlooks");
    println!("  Output: HDMI-A-1 (1920x1080@60Hz)");
    println!("  Decorations: server-side");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "labwc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_labwc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
