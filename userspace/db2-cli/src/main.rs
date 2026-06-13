#![deny(clippy::all)]

//! db2-cli — SlateOS IBM Db2 database
//!
//! Single personality: `db2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_db2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: db2 [OPTIONS] [SQL]");
        println!("IBM Db2 12.1 (SlateOS) — Enterprise database (LUW)");
        println!();
        println!("Options:");
        println!("  -tf FILE               Run SQL script file");
        println!("  connect to DB user U using P  Establish connection");
        println!("  --datastudio           Launch Db2 Data Studio (Eclipse)");
        println!("  --pureScale            pureScale clustered config");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IBM Db2 v12.1.0.0 LUW (SlateOS)"); return 0; }
    println!("IBM Db2 v12.1.0.0 LUW (SlateOS)");
    println!("  Editions: Community (free), Standard, Advanced; mainframe Db2 z/OS");
    println!("  Db2 LUW: Linux/Unix/Windows; Db2 z/OS: IBM Z mainframe");
    println!("  Languages: SQL PL, PL/SQL compatibility (Oracle migration), Java, Python");
    println!("  Features: BLU Acceleration (in-memory columnar), pureScale clusters");
    println!("  HADR (high availability disaster recovery), Q Replication");
    println!("  AI: native vector search, watsonx.data lakehouse integration");
    println!("  License: Free (Community 16-core/128GB cap); enterprise per-VPC");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "db2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_db2(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_db2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/db2"), "db2");
        assert_eq!(basename(r"C:\bin\db2.exe"), "db2.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("db2.exe"), "db2");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_db2(&["--help".to_string()], "db2"), 0);
        assert_eq!(run_db2(&["-h".to_string()], "db2"), 0);
        let _ = run_db2(&["--version".to_string()], "db2");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_db2(&[], "db2");
    }
}
