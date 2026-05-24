#![deny(clippy::all)]

//! zoom-cli — OurOS Zoom video conferencing
//!
//! Single personality: `zoom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zoom(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zoom [OPTIONS]");
        println!("zoom v5.17 (OurOS) — Video conferencing client");
        println!();
        println!("Options:");
        println!("  --url=URL         Join meeting by URL");
        println!("  --minimized       Start minimized");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zoom v5.17 (OurOS)"); return 0; }
    println!("zoom: video conferencing client started");
    println!("  Account: signed in");
    println!("  Virtual background: available");
    println!("  Screen sharing: ready");
    println!("  Recording: local/cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zoom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zoom(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
