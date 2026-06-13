#![deny(clippy::all)]

//! toad-cli — SlateOS Quest Toad database tools
//!
//! Single personality: `toad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_toad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: toad [OPTIONS]");
        println!("Quest Toad for Oracle 17.2 (SlateOS) — Database development & administration");
        println!();
        println!("Options:");
        println!("  --product PROD         oracle/sql-server/db2/mysql/postgres/dataPoint");
        println!("  --connect CONNSTR      Connect to database");
        println!("  --script FILE          Run automation script (TAS)");
        println!("  --tdm                  Toad Data Modeler");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Quest Toad for Oracle 17.2 (SlateOS)"); return 0; }
    println!("Quest Toad for Oracle 17.2 (SlateOS)");
    println!("  Editions: Toad for Oracle / SQL Server / IBM Db2 / MySQL / PostgreSQL");
    println!("  Toad Data Point (cross-platform query/reporting on 50+ data sources)");
    println!("  Features: SQL editor with autocomplete, schema browser, debugger, profiler");
    println!("  Code Analysis: Toad Code Analysis (Code Xpert, formatter, refactoring)");
    println!("  DBA Tools: Spotlight (monitoring), Foglight, Recovery Manager");
    println!("  Automation: Toad Automation Designer (TAS - drag-drop workflows)");
    println!("  License: per-seat perpetual + maintenance (commercial)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "toad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_toad(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_toad};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/toad"), "toad");
        assert_eq!(basename(r"C:\bin\toad.exe"), "toad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("toad.exe"), "toad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_toad(&["--help".to_string()], "toad"), 0);
        assert_eq!(run_toad(&["-h".to_string()], "toad"), 0);
        let _ = run_toad(&["--version".to_string()], "toad");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_toad(&[], "toad");
    }
}
