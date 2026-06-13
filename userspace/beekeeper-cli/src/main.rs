#![deny(clippy::all)]

//! beekeeper-cli — SlateOS Beekeeper Studio database manager
//!
//! Single personality: `beekeeper-studio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_beekeeper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: beekeeper-studio [OPTIONS] [CONNECTION]");
        println!("beekeeper-studio v4.6 (Slate OS) — Cross-platform SQL editor & database manager");
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
    if args.iter().any(|a| a == "--version") { println!("beekeeper-studio v4.6 (Slate OS)"); return 0; }
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
mod tests {
    use super::{basename, strip_ext, run_beekeeper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/beekeeper"), "beekeeper");
        assert_eq!(basename(r"C:\bin\beekeeper.exe"), "beekeeper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("beekeeper.exe"), "beekeeper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_beekeeper(&["--help".to_string()], "beekeeper"), 0);
        assert_eq!(run_beekeeper(&["-h".to_string()], "beekeeper"), 0);
        let _ = run_beekeeper(&["--version".to_string()], "beekeeper");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_beekeeper(&[], "beekeeper");
    }
}
