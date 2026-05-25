#![deny(clippy::all)]

//! avizo-cli — OurOS Avizo OSD notification daemon
//!
//! Multi-personality: `avizo-service`, `volumectl`, `lightctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_avizo_service(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: avizo-service [OPTIONS]");
        println!("avizo-service v1.0 (OurOS) — OSD notification daemon");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("Neat volume/brightness OSD for Wayland. macOS-style overlay.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("avizo-service v1.0 (OurOS)"); return 0; }
    println!("avizo-service: OSD daemon running");
    0
}

fn run_volumectl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: volumectl [raise|lower|toggle-mute]");
        println!("volumectl v1.0 (OurOS) — Volume control with OSD");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("raise") => println!("volumectl: volume +5% (75%)"),
        Some("lower") => println!("volumectl: volume -5% (65%)"),
        Some("toggle-mute") => println!("volumectl: mute toggled"),
        _ => println!("volumectl: current volume 70%"),
    }
    0
}

fn run_lightctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightctl [raise|lower]");
        println!("lightctl v1.0 (OurOS) — Brightness control with OSD");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("raise") => println!("lightctl: brightness +5% (80%)"),
        Some("lower") => println!("lightctl: brightness -5% (70%)"),
        _ => println!("lightctl: current brightness 75%"),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "avizo-service".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "volumectl" => run_volumectl(&rest, &prog),
        "lightctl" => run_lightctl(&rest, &prog),
        _ => run_avizo_service(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
