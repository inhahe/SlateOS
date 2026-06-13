#![deny(clippy::all)]

//! flyway-cli — SlateOS Flyway CLI
//!
//! Single personality: `flyway`

use std::env;
use std::process;

fn run_flyway(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flyway <COMMAND> [OPTIONS]");
        println!();
        println!("Flyway database migration tool (Slate OS).");
        println!();
        println!("Commands:");
        println!("  migrate      Apply pending migrations");
        println!("  clean        Drop all objects");
        println!("  info         Print migration status");
        println!("  validate     Validate applied migrations");
        println!("  undo         Undo last migration");
        println!("  baseline     Baseline existing database");
        println!("  repair       Repair metadata table");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Flyway Community Edition 10.6.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "migrate" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb (PostgreSQL 16.1)");
            println!("Schema history table: public.flyway_schema_history");
            println!();
            println!("  Current version: 3");
            println!("  Migrating schema to version 4 - Add products table");
            println!("  Migrating schema to version 5 - Add product categories");
            println!("  Migrating schema to version 6 - Add user preferences");
            println!();
            println!("Successfully applied 3 migration(s) (execution time 00:00.234s)");
            0
        }
        "clean" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!();
            println!("  WARNING: This will drop ALL objects in schema 'public'!");
            println!("  Successfully cleaned schema 'public'");
            println!("  Dropped 6 tables, 2 views, 8 indexes, 3 sequences");
            0
        }
        "info" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!("Schema version: 6");
            println!();
            println!("+-----------+---------+------------------------+--------+---------------------+---------+");
            println!("| Version   | State   | Description            | Type   | Installed On        | Time    |");
            println!("+-----------+---------+------------------------+--------+---------------------+---------+");
            println!("| 1         | Success | Create users table     | SQL    | 2024-01-10 10:00:00 | 0.045s  |");
            println!("| 2         | Success | Create orders table    | SQL    | 2024-01-10 10:00:01 | 0.032s  |");
            println!("| 3         | Success | Add email index        | SQL    | 2024-01-12 14:00:00 | 0.018s  |");
            println!("| 4         | Success | Add products table     | SQL    | 2024-01-15 14:00:00 | 0.041s  |");
            println!("| 5         | Success | Add product categories | SQL    | 2024-01-15 14:00:00 | 0.028s  |");
            println!("| 6         | Success | Add user preferences   | SQL    | 2024-01-15 14:00:00 | 0.035s  |");
            println!("| 7         | Pending | Add payment methods    | SQL    |                     |         |");
            println!("+-----------+---------+------------------------+--------+---------------------+---------+");
            0
        }
        "validate" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!();
            println!("Successfully validated 6 migration(s)");
            0
        }
        "undo" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!();
            println!("  Current version: 6");
            println!("  Undoing migration version 6 - Add user preferences");
            println!();
            println!("Successfully undid 1 migration (execution time 00:00.045s)");
            println!("  Current version: 5");
            0
        }
        "baseline" => {
            let version = args.windows(2).find(|w| w[0] == "-baselineVersion")
                .map(|w| w[1].as_str()).unwrap_or("1");
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!();
            println!("Successfully baselined schema at version {}", version);
            0
        }
        "repair" => {
            println!("Flyway Community Edition 10.6.0");
            println!("Database: jdbc:postgresql://localhost:5432/mydb");
            println!();
            println!("  Removing failed migration entry for version 7");
            println!("  Aligning checksums for applied migrations");
            println!("Successfully repaired schema history table");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: flyway <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flyway(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_flyway};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flyway(vec!["--help".to_string()]), 0);
        assert_eq!(run_flyway(vec!["-h".to_string()]), 0);
        let _ = run_flyway(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flyway(vec![]);
    }
}
