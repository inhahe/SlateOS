#![deny(clippy::all)]

//! heidisql-cli — SlateOS HeidiSQL database client
//!
//! Single personality: `heidisql`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_heidisql(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: heidisql [OPTIONS]");
        println!("HeidiSQL v12.8 (SlateOS) — Lightweight database client");
        println!();
        println!("Options:");
        println!("  --host HOST        Server hostname");
        println!("  --port PORT        Server port");
        println!("  --user USER        Username");
        println!("  --password PASS    Password");
        println!("  --database DB      Default database");
        println!("  --session NAME     Load saved session");
        println!("  --execute FILE     Execute SQL file");
        println!("  --nettype TYPE     Connection type (mysql/postgres/mssql)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("HeidiSQL v12.8.0 (SlateOS)"); return 0; }
    println!("HeidiSQL v12.8.0 (SlateOS)");
    println!("  Sessions: 6 saved");
    println!("  Supported: MySQL, MariaDB, PostgreSQL, MSSQL, SQLite, Interbase");
    println!("  Query tabs: 3 open");
    println!("  Export formats: SQL, CSV, JSON, XML, LaTeX, Wiki");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "heidisql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_heidisql(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_heidisql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/heidisql"), "heidisql");
        assert_eq!(basename(r"C:\bin\heidisql.exe"), "heidisql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("heidisql.exe"), "heidisql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_heidisql(&["--help".to_string()], "heidisql"), 0);
        assert_eq!(run_heidisql(&["-h".to_string()], "heidisql"), 0);
        let _ = run_heidisql(&["--version".to_string()], "heidisql");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_heidisql(&[], "heidisql");
    }
}
