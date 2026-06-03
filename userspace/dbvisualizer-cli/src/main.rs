#![deny(clippy::all)]

//! dbvisualizer-cli — OurOS DbVisualizer database tool
//!
//! Single personality: `dbvis`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dbvis(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dbvis [OPTIONS]");
        println!("DbVisualizer v24.1 (OurOS) — Universal database tool");
        println!();
        println!("Options:");
        println!("  --connection NAME  Use saved connection");
        println!("  --sql FILE         Execute SQL file");
        println!("  --database DB      Target database");
        println!("  --export TABLE FMT Export table (csv/json/xml/sql)");
        println!("  --er-diagram DB    Generate ER diagram");
        println!("  --explain SQL      Explain query plan");
        println!("  --batch            Run in batch mode");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DbVisualizer v24.1.3 (OurOS)"); return 0; }
    println!("DbVisualizer v24.1.3 (OurOS)");
    println!("  Connections: 9 saved");
    println!("  Supported: 50+ databases via JDBC");
    println!("  SQL history: 234 queries");
    println!("  Bookmarks: 12");
    println!("  ER diagrams: 3 saved");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dbvis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dbvis(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dbvis};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dbvisualizer"), "dbvisualizer");
        assert_eq!(basename(r"C:\bin\dbvisualizer.exe"), "dbvisualizer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dbvisualizer.exe"), "dbvisualizer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dbvis(&["--help".to_string()], "dbvisualizer"), 0);
        assert_eq!(run_dbvis(&["-h".to_string()], "dbvisualizer"), 0);
        assert_eq!(run_dbvis(&["--version".to_string()], "dbvisualizer"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dbvis(&[], "dbvisualizer"), 0);
    }
}
