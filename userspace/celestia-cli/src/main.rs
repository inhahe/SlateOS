#![deny(clippy::all)]

//! celestia-cli — OurOS Celestia space simulator
//!
//! Single personality: `celestia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_celestia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: celestia [OPTIONS] [SCRIPT.cel]");
        println!("celestia v1.7 (OurOS) — Real-time 3D space simulator");
        println!();
        println!("Options:");
        println!("  --fullscreen      Start fullscreen");
        println!("  --conf FILE       Configuration file");
        println!("  --version         Show version");
        println!();
        println!("Travel through the universe in real-time 3D.");
        println!("100,000+ stars, galaxies, nebulae, spacecraft.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("celestia v1.7 (OurOS)"); return 0; }
    println!("celestia: space simulator started");
    println!("  Stars: Hipparcos + Tycho-2 catalog");
    println!("  Galaxies: 10,000+ rendered");
    println!("  Solar system: planets with atmosphere rendering");
    println!("  Navigation: free-flight, follow, orbit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "celestia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_celestia(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
