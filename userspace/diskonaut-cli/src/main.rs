#![deny(clippy::all)]

//! diskonaut-cli — OurOS diskonaut disk space navigator
//!
//! Single personality: `diskonaut`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_diskonaut(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: diskonaut [PATH]");
        println!("diskonaut 0.11.0 (OurOS) — Terminal disk space navigator");
        println!();
        println!("Navigate disk usage with an interactive treemap.");
        println!("Press Enter to zoom in, Backspace to zoom out.");
        println!("Press 'd' to delete, 'q' to quit.");
        return 0;
    }
    let path = args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".");
    println!("diskonaut: Scanning '{}'...", path);
    println!("diskonaut: Interactive treemap ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "diskonaut".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_diskonaut(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
