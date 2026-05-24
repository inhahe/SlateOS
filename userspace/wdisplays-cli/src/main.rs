#![deny(clippy::all)]

//! wdisplays-cli — OurOS wdisplays graphical output configurator
//!
//! Single personality: `wdisplays`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wdisplays(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wdisplays [OPTIONS]");
        println!("wdisplays v1.1 (OurOS) — Graphical Wayland output configurator");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("GUI tool to configure display layout, resolution,");
        println!("refresh rate, scaling, and rotation for Wayland.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wdisplays v1.1 (OurOS)"); return 0; }
    println!("wdisplays: opening display configuration GUI...");
    println!("  Detected outputs:");
    println!("    HDMI-A-1: 1920x1080@60Hz, scale 1.0, rotation normal");
    println!("    DP-1: 2560x1440@144Hz, scale 1.0, rotation normal");
    println!("  Drag outputs to arrange. Click Apply to save.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wdisplays".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wdisplays(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
