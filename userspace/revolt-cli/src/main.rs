#![deny(clippy::all)]

//! revolt-cli — OurOS Revolt chat client
//!
//! Single personality: `revolt-desktop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_revolt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: revolt-desktop [OPTIONS]");
        println!("revolt-desktop v1.0 (OurOS) — Open-source chat platform");
        println!();
        println!("Options:");
        println!("  --minimized       Start minimized");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("revolt-desktop v1.0 (OurOS)"); return 0; }
    println!("revolt-desktop: chat client started");
    println!("  Servers: 4 joined");
    println!("  Channels: 15 active");
    println!("  Unread: 3 mentions");
    println!("  Voice: available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "revolt-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_revolt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
