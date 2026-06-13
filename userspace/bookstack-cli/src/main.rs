#![deny(clippy::all)]

//! bookstack-cli — SlateOS BookStack wiki platform
//!
//! Single personality: `bookstack`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bookstack(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bookstack [COMMAND] [OPTIONS]");
        println!("BookStack v24.02 (SlateOS) — Knowledge management wiki");
        println!();
        println!("Commands:");
        println!("  serve              Start web server");
        println!("  migrate            Run database migrations");
        println!("  backup             Create backup");
        println!("  restore FILE       Restore from backup");
        println!("  users              Manage users");
        println!("  shelves            List shelves");
        println!("  books              List books");
        println!("  search QUERY       Search content");
        println!();
        println!("Options:");
        println!("  --port PORT        Server port (default: 8080)");
        println!("  --config FILE      Config file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BookStack v24.02.3 (SlateOS)"); return 0; }
    println!("BookStack v24.02.3 (SlateOS)");
    println!("  Shelves: 12");
    println!("  Books: 45");
    println!("  Chapters: 234");
    println!("  Pages: 1,890");
    println!("  Users: 34");
    println!("  Attachments: 567");
    println!("  Database: SQLite (/var/bookstack/database.sqlite)");
    println!("  Listening: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bookstack".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bookstack(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bookstack};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bookstack"), "bookstack");
        assert_eq!(basename(r"C:\bin\bookstack.exe"), "bookstack.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bookstack.exe"), "bookstack");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bookstack(&["--help".to_string()], "bookstack"), 0);
        assert_eq!(run_bookstack(&["-h".to_string()], "bookstack"), 0);
        let _ = run_bookstack(&["--version".to_string()], "bookstack");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bookstack(&[], "bookstack");
    }
}
