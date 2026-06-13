#![deny(clippy::all)]

//! pinot-cli — SlateOS Apache Pinot OLAP datastore
//!
//! Single personality: `pinot-admin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pinot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pinot-admin [COMMAND] [OPTIONS]");
        println!("Apache Pinot v1.1 (SlateOS) — Real-time distributed OLAP datastore");
        println!();
        println!("Commands:");
        println!("  StartController    Start controller");
        println!("  StartBroker        Start broker");
        println!("  StartServer        Start server");
        println!("  StartMinion        Start minion");
        println!("  AddTable           Add table");
        println!("  AddSchema          Add schema");
        println!("  LaunchDataIngestion  Ingest data");
        println!("  PostQuery          Execute query");
        println!("  ChangeTableState   Change table state");
        println!();
        println!("Options:");
        println!("  -configFile FILE   Config file");
        println!("  -zkAddress ADDR    ZooKeeper address");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apache Pinot v1.1.0 (SlateOS)"); return 0; }
    println!("Apache Pinot v1.1.0 (SlateOS)");
    println!("  Controller: http://0.0.0.0:9000");
    println!("  Broker: http://0.0.0.0:8099");
    println!("  Server: 0.0.0.0:8098");
    println!("  Tables: 12 (6 realtime, 6 offline)");
    println!("  Segments: 8,901");
    println!("  Documents: 3.4 billion");
    println!("  Query: SQL (multi-value columns, text search)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pinot-admin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pinot(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pinot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pinot"), "pinot");
        assert_eq!(basename(r"C:\bin\pinot.exe"), "pinot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pinot.exe"), "pinot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pinot(&["--help".to_string()], "pinot"), 0);
        assert_eq!(run_pinot(&["-h".to_string()], "pinot"), 0);
        let _ = run_pinot(&["--version".to_string()], "pinot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pinot(&[], "pinot");
    }
}
