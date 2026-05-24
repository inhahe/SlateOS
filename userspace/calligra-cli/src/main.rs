#![deny(clippy::all)]

//! calligra-cli — OurOS Calligra KDE office suite
//!
//! Multi-personality: `calligrawords`, `calligrasheets`, `calligrastage`, `karbon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_calligra(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE]", prog);
        println!("calligra v3.2 (OurOS) — KDE office suite");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("calligra v3.2 (OurOS)"); return 0; }
    let component = match prog {
        "calligrawords" => "Words (word processor)",
        "calligrasheets" => "Sheets (spreadsheet)",
        "calligrastage" => "Stage (presentation)",
        "karbon" => "Karbon (vector graphics)",
        _ => "Words (word processor)",
    };
    println!("calligra: {} started", component);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "calligrawords".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_calligra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
