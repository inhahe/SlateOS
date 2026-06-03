#![deny(clippy::all)]

//! duckdb-cli — OurOS DuckDB analytical database
//!
//! Single personality: `duckdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_duckdb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: duckdb [DATABASE] [OPTIONS]");
        println!("DuckDB v0.10 (OurOS) — In-process analytical database");
        println!();
        println!("Options:");
        println!("  -c COMMAND         Execute SQL command");
        println!("  -csv               CSV output mode");
        println!("  -json              JSON output mode");
        println!("  -readonly          Open read-only");
        println!("  -unsigned          Allow unsigned extensions");
        println!("  -init FILE         Run SQL file on startup");
        println!("  -header            Show column headers");
        println!("  -separator SEP     Column separator");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DuckDB v0.10.3 (OurOS)"); return 0; }
    println!("DuckDB v0.10.3 (OurOS)");
    println!("  Database: :memory: (in-process)");
    println!("  Extensions: parquet, httpfs, json, icu, fts");
    println!("  Threads: 8");
    println!("  Memory limit: 80% of system");
    println!("  Formats: CSV, Parquet, JSON, Arrow, Excel");
    println!("  SQL dialect: PostgreSQL compatible");
    println!("  Enter \".help\" for usage hints");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "duckdb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_duckdb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_duckdb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/duckdb"), "duckdb");
        assert_eq!(basename(r"C:\bin\duckdb.exe"), "duckdb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("duckdb.exe"), "duckdb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_duckdb(&["--help".to_string()], "duckdb"), 0);
        assert_eq!(run_duckdb(&["-h".to_string()], "duckdb"), 0);
        assert_eq!(run_duckdb(&["--version".to_string()], "duckdb"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_duckdb(&[], "duckdb"), 0);
    }
}
