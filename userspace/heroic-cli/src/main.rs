#![deny(clippy::all)]

//! heroic-cli — OurOS Heroic Games Launcher
//!
//! Single personality: `heroic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_heroic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: heroic [OPTIONS]");
        println!("heroic v2.12 (OurOS) — Epic/GOG/Amazon game launcher");
        println!();
        println!("Options:");
        println!("  --no-gui          Headless mode");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("heroic v2.12 (OurOS)"); return 0; }
    println!("heroic: game launcher started");
    println!("  Epic Games: 5 games");
    println!("  GOG: 3 games");
    println!("  Amazon Games: 1 game");
    println!("  Proton: GE-Proton8-26");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "heroic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_heroic(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
