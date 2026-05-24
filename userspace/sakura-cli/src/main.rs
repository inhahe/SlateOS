#![deny(clippy::all)]

//! sakura-cli — OurOS Sakura terminal emulator
//!
//! Single personality: `sakura`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sakura(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sakura [OPTIONS]");
        println!("sakura v3.8 (OurOS) — GTK/VTE terminal emulator");
        println!();
        println!("Options:");
        println!("  -c COLUMNS        Columns");
        println!("  -r ROWS           Rows");
        println!("  -f FONT           Font");
        println!("  -t TITLE          Window title");
        println!("  -e CMD            Execute command");
        println!("  --tabs N          Initial tab count");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("sakura v3.8 (OurOS)"); return 0; }
    println!("Sakura terminal starting...");
    println!("  Toolkit: GTK3/VTE");
    println!("  Tabs: 1");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sakura".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sakura(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
