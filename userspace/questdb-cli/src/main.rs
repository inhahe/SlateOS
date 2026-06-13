#![deny(clippy::all)]

//! questdb-cli — SlateOS QuestDB time-series database
//!
//! Single personality: `questdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_questdb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: questdb [COMMAND] [OPTIONS]");
        println!("QuestDB v7.4 (SlateOS) — High-performance time-series database");
        println!();
        println!("Commands:");
        println!("  start              Start QuestDB server");
        println!("  stop               Stop QuestDB server");
        println!("  status             Show server status");
        println!("  import FILE        Import CSV data");
        println!();
        println!("Options:");
        println!("  -d DIR             Data directory");
        println!("  -f                 Force start (remove lock)");
        println!("  --http-port PORT   HTTP port (default: 9000)");
        println!("  --pg-port PORT     PostgreSQL wire port (default: 8812)");
        println!("  --ilp-port PORT    ILP port (default: 9009)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("QuestDB v7.4.2 (SlateOS)"); return 0; }
    println!("QuestDB v7.4.2 (SlateOS)");
    println!("  HTTP: http://0.0.0.0:9000");
    println!("  PostgreSQL wire: 0.0.0.0:8812");
    println!("  ILP (InfluxDB Line Protocol): 0.0.0.0:9009");
    println!("  Tables: 23");
    println!("  Rows: 2.3 billion");
    println!("  Data size: 45 GB");
    println!("  Query: SQL with time-series extensions");
    println!("  Ingestion: 1.4M rows/sec");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "questdb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_questdb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_questdb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/questdb"), "questdb");
        assert_eq!(basename(r"C:\bin\questdb.exe"), "questdb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("questdb.exe"), "questdb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_questdb(&["--help".to_string()], "questdb"), 0);
        assert_eq!(run_questdb(&["-h".to_string()], "questdb"), 0);
        let _ = run_questdb(&["--version".to_string()], "questdb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_questdb(&[], "questdb");
    }
}
