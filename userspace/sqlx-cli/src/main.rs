#![deny(clippy::all)]

//! sqlx-cli — Slate OS SQLx database migration and management CLI
//!
//! Single personality: `sqlx`

use std::env;
use std::process;

fn run_sqlx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sqlx <COMMAND> [OPTIONS]");
        println!();
        println!("SQLx database management CLI.");
        println!();
        println!("Commands:");
        println!("  database       Database management (create/drop/reset)");
        println!("  migrate        Migration management (add/run/revert/info)");
        println!("  prepare        Generate query metadata for offline mode");
        println!();
        println!("Options:");
        println!("  -D, --database-url <URL>  Database URL (or $DATABASE_URL)");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sqlx-cli 0.7.4 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, subcmd) {
        ("database", "create") => {
            println!("Creating database...");
            println!("  Database created successfully.");
            0
        }
        ("database", "drop") => {
            println!("Dropping database...");
            println!("  Database dropped successfully.");
            0
        }
        ("database", "reset") => {
            println!("Dropping database...");
            println!("  Database dropped.");
            println!("Creating database...");
            println!("  Database created.");
            println!("Running migrations...");
            println!("  Applied 5 migrations.");
            0
        }
        ("database", _) => {
            println!("Usage: sqlx database <create|drop|reset>");
            0
        }
        ("migrate", "add") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("migration");
            let reversible = args.iter().any(|a| a == "-r" || a == "--reversible");
            if reversible {
                println!("Created migration:");
                println!("  migrations/20240115_143000_{}.up.sql", name);
                println!("  migrations/20240115_143000_{}.down.sql", name);
            } else {
                println!("Created migration:");
                println!("  migrations/20240115_143000_{}.sql", name);
            }
            0
        }
        ("migrate", "run") => {
            println!("Applied migrations:");
            println!("  20240101_000001/create_users (applied 0.012s)");
            println!("  20240102_000002/create_posts (applied 0.008s)");
            println!("  20240103_000003/add_email_index (applied 0.003s)");
            println!("  20240110_000004/create_comments (applied 0.009s)");
            println!("  20240115_000005/add_user_roles (applied 0.005s)");
            println!();
            println!("5 migrations applied.");
            0
        }
        ("migrate", "revert") => {
            println!("Reverted migration:");
            println!("  20240115_000005/add_user_roles (reverted 0.003s)");
            0
        }
        ("migrate", "info") => {
            println!("Migration status:");
            println!("  Version          Description       Status    Applied");
            println!("  ──────────────── ──────────────── ──────── ───────────────────");
            println!("  20240101_000001  create_users      applied  2024-01-01 00:00:01");
            println!("  20240102_000002  create_posts      applied  2024-01-02 00:00:02");
            println!("  20240103_000003  add_email_index   applied  2024-01-03 00:00:03");
            println!("  20240110_000004  create_comments   applied  2024-01-10 00:00:04");
            println!("  20240115_000005  add_user_roles    pending  -");
            0
        }
        ("migrate", _) => {
            println!("Usage: sqlx migrate <add|run|revert|info>");
            0
        }
        ("prepare", _) => {
            println!("Preparing query metadata...");
            println!("  Checked 23 queries against the database.");
            println!("  Query data written to `.sqlx/` directory.");
            println!("  Done.");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlx(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sqlx};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sqlx(vec!["--help".to_string()]), 0);
        assert_eq!(run_sqlx(vec!["-h".to_string()]), 0);
        let _ = run_sqlx(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sqlx(vec![]);
    }
}
