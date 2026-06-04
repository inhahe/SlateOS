#![deny(clippy::all)]

//! sqlpad-cli — OurOS SQLPad web SQL editor
//!
//! Single personality: `sqlpad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sqlpad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sqlpad [OPTIONS]");
        println!("SQLPad v7.4 (OurOS) — Web-based SQL editor & visualization");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 3000)");
        println!("  --ip ADDR          Bind address");
        println!("  --db-path DIR      SQLite database path");
        println!("  --base-url PATH    Base URL path prefix");
        println!("  --admin EMAIL      Admin email");
        println!("  --passphrase PASS  Encryption passphrase");
        println!("  --seed-data-path DIR  Seed data directory");
        println!("  --query-result-max-rows N  Max result rows");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SQLPad v7.4.2 (OurOS)"); return 0; }
    println!("SQLPad v7.4.2 (OurOS)");
    println!("  Server: http://0.0.0.0:3000");
    println!("  Connections: 4 configured");
    println!("  Queries: 56 saved");
    println!("  Users: 8");
    println!("  Drivers: postgres, mysql, sqlserver, sqlite, presto, trino");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sqlpad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlpad(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sqlpad};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sqlpad"), "sqlpad");
        assert_eq!(basename(r"C:\bin\sqlpad.exe"), "sqlpad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sqlpad.exe"), "sqlpad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sqlpad(&["--help".to_string()], "sqlpad"), 0);
        assert_eq!(run_sqlpad(&["-h".to_string()], "sqlpad"), 0);
        let _ = run_sqlpad(&["--version".to_string()], "sqlpad");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sqlpad(&[], "sqlpad");
    }
}
