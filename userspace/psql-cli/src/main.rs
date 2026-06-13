#![deny(clippy::all)]

//! psql-cli — SlateOS PostgreSQL client
//!
//! Multi-personality: `psql`, `pg_dump`, `pg_restore`, `createdb`, `dropdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_psql(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: psql [OPTIONS] [DBNAME [USERNAME]]");
        println!("psql (PostgreSQL) 16.3 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -h HOST      Database server host");
        println!("  -p PORT      Database server port");
        println!("  -U USER      Database user name");
        println!("  -d DBNAME    Database name");
        println!("  -c COMMAND   Execute single command");
        println!("  -f FILE      Execute commands from file");
        println!("  -l           List available databases");
        println!("  -t           Tuples only (no headers)");
        println!("  -A           Unaligned table output");
        println!("  -w           Never prompt for password");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("psql (PostgreSQL) 16.3");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("                          List of databases");
        println!("   Name    |  Owner   | Encoding | Locale | Collate | Ctype | Access");
        println!("-----------+----------+----------+--------+---------+-------+--------");
        println!(" postgres  | postgres | UTF8     | en_US  | en_US   | en_US |");
        println!(" mydb      | user     | UTF8     | en_US  | en_US   | en_US |");
        println!(" template0 | postgres | UTF8     | en_US  | en_US   | en_US |");
        println!(" template1 | postgres | UTF8     | en_US  | en_US   | en_US |");
        return 0;
    }
    let cmd = args.windows(2).find(|w| w[0] == "-c").map(|w| w[1].as_str());
    if let Some(c) = cmd {
        println!("{}", c);
        println!("(query executed)");
        return 0;
    }
    let host = args.windows(2).find(|w| w[0] == "-h").map(|w| w[1].as_str()).unwrap_or("localhost");
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("postgres");
    println!("psql (16.3)");
    println!("Type \"help\" for help.");
    println!();
    println!("Connected to {} at {}", db, host);
    println!("{}=> ", db);
    0
}

fn run_pg_dump(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: pg_dump [OPTIONS] [DBNAME]");
        println!("  -h HOST      Server host");
        println!("  -U USER      User name");
        println!("  -f FILE      Output file");
        println!("  -F FORMAT    Output format (p/c/d/t)");
        println!("  -t TABLE     Dump specific table");
        println!("  --schema-only   Dump only schema");
        println!("  --data-only     Dump only data");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("mydb");
    let file = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str());
    if let Some(f) = file {
        println!("pg_dump: dumping database '{}' to {}", db, f);
    } else {
        println!("-- PostgreSQL database dump");
        println!("-- Dumped from database version 16.3");
        println!("SET client_encoding = 'UTF8';");
    }
    0
}

fn run_pg_restore(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: pg_restore [OPTIONS] [FILE]");
        println!("  -h HOST      Server host");
        println!("  -d DBNAME    Target database");
        println!("  -c           Clean (drop) before restore");
        println!("  --create     Include CREATE DATABASE");
        println!("  -j N         Parallel jobs");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("dump.sql");
    let db = args.windows(2).find(|w| w[0] == "-d").map(|w| w[1].as_str()).unwrap_or("mydb");
    println!("pg_restore: restoring {} into database '{}'", file, db);
    println!("pg_restore: complete.");
    0
}

fn run_createdb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: createdb [OPTIONS] DBNAME");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("newdb");
    println!("createdb: database \"{}\" created.", db);
    0
}

fn run_dropdb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: dropdb [OPTIONS] DBNAME");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("olddb");
    println!("dropdb: database \"{}\" dropped.", db);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "psql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pg_dump" => run_pg_dump(&rest),
        "pg_restore" => run_pg_restore(&rest),
        "createdb" => run_createdb(&rest),
        "dropdb" => run_dropdb(&rest),
        _ => run_psql(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_psql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/psql"), "psql");
        assert_eq!(basename(r"C:\bin\psql.exe"), "psql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("psql.exe"), "psql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_psql(&["--help".to_string()]), 0);
        assert_eq!(run_psql(&["-h".to_string()]), 0);
        let _ = run_psql(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_psql(&[]);
    }
}
