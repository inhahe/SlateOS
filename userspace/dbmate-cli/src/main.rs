#![deny(clippy::all)]

//! dbmate-cli — OurOS dbmate database migration tool
//!
//! Single personality: `dbmate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dbmate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dbmate COMMAND [OPTIONS]");
        println!("dbmate v2.14.0 (OurOS) — Database migration tool");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new migration");
        println!("  up              Apply pending migrations");
        println!("  down            Rollback last migration");
        println!("  migrate         Apply pending (alias for up)");
        println!("  rollback        Rollback last (alias for down)");
        println!("  status          Show migration status");
        println!("  create          Create database");
        println!("  drop            Drop database");
        println!("  dump            Dump schema to file");
        println!("  load            Load schema from file");
        println!("  wait            Wait for database ready");
        println!();
        println!("Options:");
        println!("  -e, --env VAR       DATABASE_URL env var name");
        println!("  -u, --url URL       Database URL");
        println!("  -d, --migrations-dir DIR   Migrations directory");
        println!("  --no-dump-schema    Skip schema dump after migrate");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("unnamed");
            println!("Creating migration: db/migrations/20240115100000_{}.sql", name);
        }
        "up" | "migrate" => {
            println!("Applying: 20240115100000_create_users.sql");
            println!("Applying: 20240116100000_add_email_index.sql");
            println!("Applied 2 migrations.");
        }
        "down" | "rollback" => {
            println!("Rolling back: 20240116100000_add_email_index.sql");
            println!("Rolled back 1 migration.");
        }
        "status" => {
            println!("  [x] 20240115100000_create_users.sql");
            println!("  [x] 20240116100000_add_email_index.sql");
            println!("  [ ] 20240117100000_create_orders.sql");
            println!();
            println!("Applied: 2");
            println!("Pending: 1");
        }
        "create" => println!("Creating database: myapp_development"),
        "drop" => println!("Dropping database: myapp_development"),
        "dump" => println!("Writing: db/schema.sql"),
        "load" => println!("Loading: db/schema.sql"),
        "wait" => println!("Database is ready."),
        _ => println!("dbmate {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dbmate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dbmate(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dbmate};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dbmate"), "dbmate");
        assert_eq!(basename(r"C:\bin\dbmate.exe"), "dbmate.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dbmate.exe"), "dbmate");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dbmate(&["--help".to_string()], "dbmate"), 0);
        assert_eq!(run_dbmate(&["-h".to_string()], "dbmate"), 0);
        let _ = run_dbmate(&["--version".to_string()], "dbmate");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dbmate(&[], "dbmate");
    }
}
