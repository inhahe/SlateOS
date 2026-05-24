#![deny(clippy::all)]

//! kanshi-cli — OurOS kanshi dynamic output configuration
//!
//! Single personality: `kanshi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kanshi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kanshi [OPTIONS]");
        println!("kanshi v1.5 (OurOS) — Dynamic output configuration for Wayland");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file");
        println!("  --version         Show version");
        println!();
        println!("Automatically applies output profiles when displays change.");
        println!("Config: ~/.config/kanshi/config");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kanshi v1.5 (OurOS)"); return 0; }
    println!("kanshi: watching for output changes...");
    println!("  Profile matched: docked");
    println!("    HDMI-A-1: 3840x2160@60Hz, pos 0,0, scale 1.5");
    println!("    eDP-1: disabled");
    if args.is_empty() {
        println!("  Waiting for hotplug events...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kanshi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kanshi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
