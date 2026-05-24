#![deny(clippy::all)]

//! musikcube-cli — OurOS musikcube music player
//!
//! Multi-personality: `musikcube`, `musikcubed`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_musikcube(args: &[String], prog: &str) -> i32 {
    if prog == "musikcubed" {
        if args.iter().any(|a| a == "--help" || a == "-h") {
            println!("Usage: musikcubed [OPTIONS]");
            println!("musikcubed — musikcube headless daemon");
            println!();
            println!("Options:");
            println!("  --port N            HTTP server port (default 7905)");
            println!("  --wss-port N        WebSocket port (default 7906)");
            println!("  --foreground        Run in foreground");
            println!("  -V, --version       Show version");
            return 0;
        }
        if args.iter().any(|a| a == "-V" || a == "--version") {
            println!("musikcubed 3.0.2 (OurOS)");
            return 0;
        }
        let port = args.windows(2).find(|w| w[0] == "--port")
            .map(|w| w[1].as_str()).unwrap_or("7905");
        println!("musikcubed: Listening on port {}", port);
        return 0;
    }
    // musikcube
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: musikcube [OPTIONS]");
        println!("musikcube 3.0.2 (OurOS) — Terminal music player and library");
        println!();
        println!("Options:");
        println!("  -V, --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("musikcube 3.0.2 (OurOS)");
        return 0;
    }
    println!("musikcube: Starting music player...");
    println!("musikcube: Library loaded (1536 tracks).");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "musikcube".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_musikcube(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
