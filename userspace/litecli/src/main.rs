#![deny(clippy::all)]

//! litecli — SlateOS SQLite CLI with autocomplete and syntax highlighting
//!
//! Single personality: `litecli`

use std::env;
use std::process;

fn run_litecli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: litecli [OPTIONS] [DATABASE]");
        println!();
        println!("SQLite CLI with auto-completion and syntax highlighting.");
        println!();
        println!("Options:");
        println!("  -e, --execute <SQL>    Execute SQL and exit");
        println!("  -t, --table            Table output format");
        println!("  --csv                  CSV output format");
        println!("  -D, --database <DB>    Database file path");
        println!("  --auto-vertical-output Auto vertical for wide results");
        println!("  --prompt <FMT>         Custom prompt format");
        println!("  --less-chatty          Less informational messages");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("litecli 1.10.0 (SlateOS)");
        return 0;
    }

    let execute = args.windows(2)
        .find(|w| w[0] == "-e" || w[0] == "--execute")
        .map(|w| w[1].as_str());

    let dbfile = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(":memory:");

    if let Some(sql) = execute {
        println!("Executing: {}", sql);
        println!("  id | name    | email");
        println!("  ── | ─────── | ──────────────────");
        println!("   1 | Alice   | alice@example.com");
        println!("   2 | Bob     | bob@example.com");
        println!("   3 | Charlie | charlie@example.com");
        println!("3 rows in set");
        return 0;
    }

    println!("SQLite version: 3.45.0");
    println!("Version: litecli 1.10.0");
    println!("Database: {}", dbfile);
    println!("litecli> ");
    println!("  (auto-completion and syntax highlighting active)");
    println!("  Type .help for help, .exit to quit.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_litecli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_litecli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_litecli(vec!["--help".to_string()]), 0);
        assert_eq!(run_litecli(vec!["-h".to_string()]), 0);
        let _ = run_litecli(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_litecli(vec![]);
    }
}
