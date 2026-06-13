#![deny(clippy::all)]

//! diesel-cli — Slate OS Diesel ORM CLI
//!
//! Single personality: `diesel`

use std::env;
use std::process;

fn run_diesel(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: diesel <COMMAND> [OPTIONS]");
        println!();
        println!("Diesel ORM CLI — database management and code generation.");
        println!();
        println!("Commands:");
        println!("  setup                Set up database (create + run migrations)");
        println!("  database             Database management");
        println!("  migration            Migration management");
        println!("  print-schema         Print database schema as Rust code");
        println!("  completions          Generate shell completions");
        println!();
        println!("Options:");
        println!("  --database-url <URL>  Database URL (or $DATABASE_URL)");
        println!("  --config-file <FILE>  Config file (default: diesel.toml)");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("diesel 2.1.4 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, subcmd) {
        ("setup", _) => {
            println!("Creating database: myapp_dev");
            println!("Running pending migrations...");
            println!("  Running migration 2024-01-01-000001_create_users");
            println!("  Running migration 2024-01-02-000002_create_posts");
            println!("  Running migration 2024-01-10-000003_create_comments");
            println!("Writing schema to: src/schema.rs");
            0
        }
        ("database", "setup") => {
            println!("Creating database: myapp_dev");
            println!("  Database created.");
            0
        }
        ("database", "reset") => {
            println!("Dropping database: myapp_dev");
            println!("Creating database: myapp_dev");
            println!("Running migrations...");
            println!("  3 migrations applied.");
            0
        }
        ("database", "drop") => {
            println!("Dropping database: myapp_dev");
            println!("  Database dropped.");
            0
        }
        ("migration", "generate") | ("migration", "new") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("new_migration");
            println!("Creating migrations/2024-01-15-143000_{}/up.sql", name);
            println!("Creating migrations/2024-01-15-143000_{}/down.sql", name);
            0
        }
        ("migration", "run") => {
            println!("Running migration 2024-01-15-143000_add_user_roles");
            println!("Writing schema to: src/schema.rs");
            0
        }
        ("migration", "revert") => {
            println!("Rolling back migration 2024-01-15-143000_add_user_roles");
            println!("Writing schema to: src/schema.rs");
            0
        }
        ("migration", "redo") => {
            println!("Rolling back migration 2024-01-15-143000_add_user_roles");
            println!("Running migration 2024-01-15-143000_add_user_roles");
            println!("Writing schema to: src/schema.rs");
            0
        }
        ("migration", "list") | ("migration", "pending") => {
            println!("Migrations:");
            println!("  [X] 2024-01-01-000001_create_users");
            println!("  [X] 2024-01-02-000002_create_posts");
            println!("  [X] 2024-01-10-000003_create_comments");
            println!("  [ ] 2024-01-15-000004_add_user_roles");
            0
        }
        ("print-schema", _) => {
            println!("// @generated automatically by Diesel CLI.");
            println!();
            println!("diesel::table! {{");
            println!("    users (id) {{");
            println!("        id -> Int4,");
            println!("        name -> Varchar,");
            println!("        email -> Varchar,");
            println!("        created_at -> Timestamp,");
            println!("    }}");
            println!("}}");
            println!();
            println!("diesel::table! {{");
            println!("    posts (id) {{");
            println!("        id -> Int4,");
            println!("        user_id -> Int4,");
            println!("        title -> Varchar,");
            println!("        body -> Text,");
            println!("        published -> Bool,");
            println!("        created_at -> Timestamp,");
            println!("    }}");
            println!("}}");
            println!();
            println!("diesel::joinable!(posts -> users (user_id));");
            println!("diesel::allow_tables_to_appear_in_same_query!(users, posts);");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Error: command required. See --help.");
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
    let code = run_diesel(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_diesel};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_diesel(vec!["--help".to_string()]), 0);
        assert_eq!(run_diesel(vec!["-h".to_string()]), 0);
        let _ = run_diesel(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_diesel(vec![]);
    }
}
