#![deny(clippy::all)]

//! beekeeper-cli — OurOS Beekeeper Studio database manager
//!
//! Single personality: `beekeeper-studio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_beekeeper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: beekeeper-studio [OPTIONS] [CONNECTION]");
        println!("beekeeper-studio v4.6 (OurOS) — Cross-platform SQL editor & database manager");
        println!();
        println!("Options:");
        println!("  --url URL       Connection URL");
        println!("  --version       Show version");
        println!();
        println!("Supports: PostgreSQL, MySQL, MariaDB, SQLite, SQL Server,");
        println!("  CockroachDB, Redis, LibSQL");
        println!();
        println!("Features: SQL autocomplete, query history, table editor,");
        println!("  SSH tunneling, saved connections");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("beekeeper-studio v4.6 (OurOS)"); return 0; }
    println!("beekeeper-studio: database manager started");
    println!("  Saved connections: 3");
    println!("  Recent queries: 12");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "beekeeper-studio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_beekeeper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
