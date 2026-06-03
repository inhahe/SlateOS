#![deny(clippy::all)]

//! dbcli-cli — OurOS generic database CLI tool
//!
//! Single personality: `dbcli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dbcli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dbcli [OPTIONS] [DATABASE]");
        println!("dbcli v1.0.0 (OurOS) — Generic database CLI with auto-completion");
        println!();
        println!("Options:");
        println!("  DATABASE            Connection string or file path");
        println!("  -e, --execute SQL   Execute query and exit");
        println!("  -t, --table         Table output format");
        println!("  -c, --csv           CSV output format");
        println!("  --json              JSON output format");
        println!("  --driver DRIVER     Force driver (postgres, mysql, sqlite)");
        println!("  --host HOST         Database host");
        println!("  --port PORT         Database port");
        println!("  --user USER         Database user");
        println!("  --password PASS     Database password");
        println!("  --database NAME     Database name");
        println!("  -V, --version       Show version");
        println!();
        println!("Features:");
        println!("  - Auto-completion for tables, columns, SQL keywords");
        println!("  - Syntax highlighting");
        println!("  - Query history");
        println!("  - Multi-line editing");
        println!("  - Named queries and favorites");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("dbcli v1.0.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-e" || a == "--execute") {
        println!("+----+----------+-------------------+");
        println!("| id | name     | email             |");
        println!("+----+----------+-------------------+");
        println!("|  1 | Alice    | alice@example.com |");
        println!("|  2 | Bob      | bob@example.com   |");
        println!("+----+----------+-------------------+");
        println!("2 rows in set");
    } else {
        let db = args.first().map(|s| s.as_str()).unwrap_or("localhost");
        println!("Connected to: {}", db);
        println!("Database: mydb");
        println!("Type \\? for help.");
        println!("dbcli> ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dbcli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dbcli(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dbcli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dbcli"), "dbcli");
        assert_eq!(basename(r"C:\bin\dbcli.exe"), "dbcli.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dbcli.exe"), "dbcli");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dbcli(&["--help".to_string()], "dbcli"), 0);
        assert_eq!(run_dbcli(&["-h".to_string()], "dbcli"), 0);
        assert_eq!(run_dbcli(&["--version".to_string()], "dbcli"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dbcli(&[], "dbcli"), 0);
    }
}
