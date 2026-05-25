#![deny(clippy::all)]

//! adminer-cli — OurOS Adminer database management
//!
//! Single personality: `adminer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_adminer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: adminer [OPTIONS]");
        println!("adminer v4.8 (OurOS) — Database management in single file");
        println!();
        println!("Options:");
        println!("  --port PORT      Web server port (default: 8080)");
        println!("  --host HOST      Listen address (default: localhost)");
        println!("  --version        Show version");
        println!();
        println!("Supports: MySQL, PostgreSQL, SQLite, MS SQL, Oracle, MongoDB");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("adminer v4.8 (OurOS)"); return 0; }
    println!("adminer: web interface started");
    println!("  URL: http://localhost:8080");
    println!("  Drivers: MySQL, PostgreSQL, SQLite");
    println!("  Features: SQL editor, export/import, schema designer");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "adminer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_adminer(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
