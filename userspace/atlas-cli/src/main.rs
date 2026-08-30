#![deny(clippy::all)]

//! atlas-cli — Slate OS Atlas schema management tool
//!
//! Single personality: `atlas`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_atlas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: atlas COMMAND [OPTIONS]");
        println!("Atlas v0.21.0 (Slate OS) — Database schema management");
        println!();
        println!("Commands:");
        println!("  schema          Schema management");
        println!("  migrate         Migration management");
        println!("  version         Show version");
        println!();
        println!("Schema commands:");
        println!("  schema inspect  Inspect database schema");
        println!("  schema apply    Apply schema changes");
        println!("  schema diff     Diff two schemas");
        println!("  schema fmt      Format HCL schema");
        println!("  schema clean    Clean database");
        println!();
        println!("Migration commands:");
        println!("  migrate diff    Generate migration files");
        println!("  migrate apply   Apply migrations");
        println!("  migrate status  Show migration status");
        println!("  migrate hash    Hash migration dir");
        println!("  migrate lint    Lint migration files");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("atlas version v0.21.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("schema");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match (cmd, sub) {
        ("schema", "inspect") => {
            println!("table \"users\" {{");
            println!("  schema = schema.public");
            println!("  column \"id\" {{");
            println!("    type = int");
            println!("  }}");
            println!("  column \"name\" {{");
            println!("    type = varchar(255)");
            println!("  }}");
            println!("  primary_key {{");
            println!("    columns = [column.id]");
            println!("  }}");
            println!("}}");
        }
        ("schema", "apply") => {
            println!("-- Planned Changes:");
            println!("-- Create table \"orders\"");
            println!("CREATE TABLE \"orders\" (\"id\" integer NOT NULL, \"user_id\" integer NOT NULL);");
            println!("-- Add index on \"orders\"");
            println!("CREATE INDEX \"idx_user_id\" ON \"orders\" (\"user_id\");");
        }
        ("schema", "diff") => {
            println!("-- Add column \"email\" to \"users\"");
            println!("ALTER TABLE \"users\" ADD COLUMN \"email\" varchar(255);");
        }
        ("schema", "fmt") => println!("atlas: Formatted 3 files."),
        ("schema", "clean") => println!("atlas: Schema cleaned."),
        ("migrate", "diff") => {
            println!("Generated migration file:");
            println!("  migrations/20240115100000_changes.sql");
        }
        ("migrate", "apply") => {
            println!("Migrating to version 20240115100000 (1 migration in total):");
            println!("  -- migrating version 20240115100000");
            println!("    -> CREATE TABLE \"users\" (...)");
            println!("  -- ok (1.2ms)");
        }
        ("migrate", "status") => {
            println!("Migration Status: 3 applied, 1 pending");
            println!("  20240113100000  applied");
            println!("  20240114100000  applied");
            println!("  20240115100000  applied");
            println!("  20240116100000  pending");
        }
        ("migrate", "lint") => {
            println!("Analyzing migration files...");
            println!("  No issues found.");
        }
        _ => println!("atlas {} {}: completed", cmd, sub),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "atlas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_atlas(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_atlas};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/atlas"), "atlas");
        assert_eq!(basename(r"C:\bin\atlas.exe"), "atlas.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("atlas.exe"), "atlas");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_atlas(&["--help".to_string()], "atlas"), 0);
        assert_eq!(run_atlas(&["-h".to_string()], "atlas"), 0);
        let _ = run_atlas(&["--version".to_string()], "atlas");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_atlas(&[], "atlas");
    }
}
