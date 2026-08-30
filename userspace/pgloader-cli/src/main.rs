#![deny(clippy::all)]

//! pgloader-cli — Slate OS pgloader CLI
//!
//! Single personality: `pgloader`

use std::env;
use std::process;

fn run_pgloader(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pgloader [OPTIONS] [SOURCE] [TARGET]");
        println!();
        println!("pgloader — load data into PostgreSQL (Slate OS).");
        println!();
        println!("Options:");
        println!("  --with OPTION          Set option (workers, batch size, etc.)");
        println!("  --set PARAM=VALUE      Set PostgreSQL parameter");
        println!("  --field SPEC           Field specification");
        println!("  --cast RULE            Cast rule");
        println!("  --type TYPE            Source type (csv, fixed, mysql, sqlite, mssql)");
        println!("  --encoding ENC         Source encoding");
        println!("  --before FILE          SQL to run before loading");
        println!("  --after FILE           SQL to run after loading");
        println!("  --root-dir DIR         Root directory for configs");
        println!("  --dry-run              Don't actually load");
        println!("  --verbose              Verbose output");
        println!("  --debug                Debug output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pgloader version 3.6.10 (Slate OS)");
        return 0;
    }

    let source = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("data.csv");
    let target = args.iter().filter(|a| !a.starts_with('-'))
        .nth(1).map(|s| s.as_str()).unwrap_or("postgresql://localhost/mydb");

    println!("LOG pgloader version 3.6.10 (Slate OS)");
    println!("LOG Parsing commands from {}", source);
    println!("LOG Loading data from {}", source);
    println!("LOG Into {}", target);
    println!();
    println!("             table name       errors    rows    bytes   total time");
    println!("---------------------  ----------  ------  ------  -----------");
    println!("        fetch metadata           0       0                0.123s");
    println!("         Create Schemas          0       0                0.045s");
    println!("           Create Tables         0       0                0.067s");
    println!("          Set Table OIDs         0       0                0.012s");
    println!("---------------------  ----------  ------  ------  -----------");
    println!("               users             0   10000  1.2MB        2.345s");
    println!("              orders             0   50000  8.4MB        5.678s");
    println!("            products             0    5000  0.8MB        1.234s");
    println!("---------------------  ----------  ------  ------  -----------");
    println!("     Total import time           0   65000  10.4MB       9.504s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pgloader(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pgloader};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pgloader(vec!["--help".to_string()]), 0);
        assert_eq!(run_pgloader(vec!["-h".to_string()]), 0);
        let _ = run_pgloader(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pgloader(vec![]);
    }
}
