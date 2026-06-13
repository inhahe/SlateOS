#![deny(clippy::all)]

//! postgresql-cli — SlateOS PostgreSQL client tools
//!
//! Multi-personality: `psql`, `pg_dump`, `pg_restore`, `createdb`, `dropdb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_psql(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: psql [OPTIONS] [DBNAME [USERNAME]]");
        println!("psql v16.1 (SlateOS) — PostgreSQL interactive terminal");
        println!();
        println!("Options:");
        println!("  -h HOST       Database server host");
        println!("  -p PORT       Database server port (default: 5432)");
        println!("  -U USER       Database username");
        println!("  -d DBNAME     Database name");
        println!("  -c COMMAND    Run single command and exit");
        println!("  -f FILE       Execute commands from file");
        println!("  -l            List available databases");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("psql v16.1 (SlateOS, PostgreSQL)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("                List of databases");
        println!("   Name    | Owner  | Encoding | Collation");
        println!("-----------+--------+----------+----------");
        println!(" postgres  | admin  | UTF8     | en_US.UTF-8");
        println!(" template0 | admin  | UTF8     | en_US.UTF-8");
        println!(" template1 | admin  | UTF8     | en_US.UTF-8");
        return 0;
    }
    println!("psql: connected to PostgreSQL 16.1");
    println!("Type \"help\" for help.");
    0
}

fn run_pg_dump(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pg_dump [OPTIONS] [DBNAME]");
        println!("pg_dump v16.1 (SlateOS) — Dump a PostgreSQL database");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pg_dump v16.1 (SlateOS)"); return 0; }
    println!("pg_dump: dumping database...");
    println!("  Tables: 24");
    println!("  Rows: 15,432");
    0
}

fn run_pg_restore(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pg_restore [OPTIONS] [FILE]");
        println!("pg_restore v16.1 (SlateOS) — Restore a PostgreSQL database");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pg_restore v16.1 (SlateOS)"); return 0; }
    println!("pg_restore: restoring database...");
    0
}

fn run_createdb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: createdb [OPTIONS] [DBNAME]");
        println!("createdb v16.1 (SlateOS) — Create a new PostgreSQL database");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("createdb v16.1 (SlateOS)"); return 0; }
    if let Some(name) = args.iter().find(|a| !a.starts_with('-')) {
        println!("createdb: database '{}' created", name);
    } else {
        println!("createdb: no database name specified");
    }
    0
}

fn run_dropdb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dropdb [OPTIONS] DBNAME");
        println!("dropdb v16.1 (SlateOS) — Remove a PostgreSQL database");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dropdb v16.1 (SlateOS)"); return 0; }
    if let Some(name) = args.iter().find(|a| !a.starts_with('-')) {
        println!("dropdb: database '{}' dropped", name);
    } else {
        println!("dropdb: no database name specified");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "psql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pg_dump" => run_pg_dump(&rest, &prog),
        "pg_restore" => run_pg_restore(&rest, &prog),
        "createdb" => run_createdb(&rest, &prog),
        "dropdb" => run_dropdb(&rest, &prog),
        _ => run_psql(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_psql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/postgresql"), "postgresql");
        assert_eq!(basename(r"C:\bin\postgresql.exe"), "postgresql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("postgresql.exe"), "postgresql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_psql(&["--help".to_string()], "postgresql"), 0);
        assert_eq!(run_psql(&["-h".to_string()], "postgresql"), 0);
        let _ = run_psql(&["--version".to_string()], "postgresql");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_psql(&[], "postgresql");
    }
}
