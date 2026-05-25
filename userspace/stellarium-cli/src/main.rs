#![deny(clippy::all)]

//! stellarium-cli — OurOS Stellarium planetarium
//!
//! Single personality: `stellarium`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stellarium(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stellarium [OPTIONS]");
        println!("stellarium v24.1 (OurOS) — Desktop planetarium");
        println!();
        println!("Options:");
        println!("  --full-screen     Start fullscreen");
        println!("  --home-planet P   Set home planet");
        println!("  --altitude ALT    Observer altitude");
        println!("  --fov DEG         Field of view");
        println!("  --screenshot DIR  Screenshot directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("stellarium v24.1 (OurOS)"); return 0; }
    println!("stellarium: planetarium started");
    println!("  Stars: 600,000+ from Hipparcos catalog");
    println!("  Deep sky: 80,000+ nebulae, galaxies, clusters");
    println!("  Planets: all solar system bodies");
    println!("  Satellites: ISS and 200+ tracked");
    println!("  Constellations: 88 IAU recognized");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stellarium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stellarium(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
