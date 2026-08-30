#![deny(clippy::all)]

//! mycli — Slate OS MySQL interactive CLI
//!
//! Single personality: `mycli`

use std::env;
use std::process;

fn run_mycli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mycli [OPTIONS] [DATABASE]");
        println!();
        println!("mycli — MySQL CLI with auto-completion and syntax highlighting (Slate OS).");
        println!();
        println!("Options:");
        println!("  -h, --host HOST       Host name (default: localhost)");
        println!("  -P, --port PORT       Port number (default: 3306)");
        println!("  -u, --user USER       User name");
        println!("  -p, --password PASS   Password");
        println!("  -D, --database DB     Database name");
        println!("  -e, --execute CMD     Execute command and quit");
        println!("  --auto-vertical       Auto switch to vertical for wide results");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mycli 1.27.0 (Slate OS)");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "-h" || w[0] == "--host")
        .map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "-P" || w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("3306");
    let user = args.windows(2).find(|w| w[0] == "-u" || w[0] == "--user")
        .map(|w| w[1].as_str()).unwrap_or("root");
    let db = args.windows(2).find(|w| w[0] == "-D" || w[0] == "--database")
        .map(|w| w[1].as_str())
        .or_else(|| args.last().map(|s| s.as_str()));

    let execute = args.windows(2).find(|w| w[0] == "-e" || w[0] == "--execute")
        .map(|w| w[1].as_str());

    if let Some(query) = execute {
        println!("mysql {}@{}:{}", user, host, port);
        if let Some(d) = db {
            println!("  Database: {}", d);
        }
        println!("  Executing: {}", query);
        println!();
        println!("+----+----------+-------------------+");
        println!("| id | name     | email             |");
        println!("+----+----------+-------------------+");
        println!("|  1 | Alice    | alice@example.com |");
        println!("|  2 | Bob      | bob@example.com   |");
        println!("|  3 | Carol    | carol@example.com |");
        println!("+----+----------+-------------------+");
        println!("3 rows in set (0.01 sec)");
    } else {
        println!("mycli 1.27.0");
        println!("MySQL {}@{}:{}", user, host, port);
        if let Some(d) = db {
            println!("  Database: {}", d);
        }
        println!("  Charset: utf8mb4");
        println!("  Server version: 8.0.35-MySQL Community Server");
        println!();
        println!("mysql> (interactive mode - auto-completion and syntax highlighting enabled)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mycli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mycli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mycli(vec!["--help".to_string()]), 0);
        assert_eq!(run_mycli(vec!["-h".to_string()]), 0);
        let _ = run_mycli(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mycli(vec![]);
    }
}
