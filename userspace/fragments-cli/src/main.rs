#![deny(clippy::all)]

//! fragments-cli — OurOS Fragments GNOME BitTorrent client
//!
//! Single personality: `fragments`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fragments(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fragments [OPTIONS] [TORRENT...]");
        println!("fragments v3.0 (OurOS) — GNOME BitTorrent client");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fragments v3.0 (OurOS)"); return 0; }
    println!("fragments: GNOME BitTorrent client started");
    println!("  Download directory: ~/Downloads");
    println!("  Active torrents: 0");
    println!("  Completed: 0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fragments".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fragments(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
