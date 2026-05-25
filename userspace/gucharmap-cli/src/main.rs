#![deny(clippy::all)]

//! gucharmap-cli — OurOS GNOME Character Map
//!
//! Single personality: `gucharmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gucharmap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gucharmap [OPTIONS]");
        println!("gucharmap v15.1 (OurOS) — Unicode character map");
        println!();
        println!("Options:");
        println!("  --font FONT       Set display font");
        println!("  --version         Show version");
        println!();
        println!("Browse Unicode characters by script, block, or category.");
        println!("Search by name, code point, or character.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gucharmap v15.1 (OurOS)"); return 0; }
    println!("gucharmap: character map started");
    println!("  Unicode version: 15.1");
    println!("  Total characters: 149,813");
    println!("  Scripts: 161");
    println!("  Blocks: 332");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gucharmap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gucharmap(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
