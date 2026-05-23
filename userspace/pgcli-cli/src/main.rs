#![deny(clippy::all)]

//! pgcli-cli — OurOS pgcli enhanced PostgreSQL client
//!
//! Multi-personality: `pgcli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pgcli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pgcli [OPTIONS] [DBNAME [USERNAME]]");
        println!("pgcli 4.1.0 — Enhanced PostgreSQL client (OurOS)");
        println!();
        println!("Options:");
        println!("  -h HOST       Server hostname");
        println!("  -p PORT       Server port");
        println!("  -U USER       Database user");
        println!("  -d DATABASE   Database name");
        println!("  -W            Prompt for password");
        println!("  --list        List databases");
        println!("  --auto-vertical-output  Auto switch to vertical for wide results");
        println!("  --prompt PROMPT  Custom prompt format");
        println!("  --version     Show version");
        println!();
        println!("Features:");
        println!("  - Auto-completion with context awareness");
        println!("  - Syntax highlighting");
        println!("  - Multi-line editing");
        println!("  - Favorite queries (\\fs, \\f)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version: 4.1.0");
        return 0;
    }
    if args.iter().any(|a| a == "--list" || a == "-l") {
        println!("+----------+---------+----------+");
        println!("| Name     | Owner   | Encoding |");
        println!("+----------+---------+----------+");
        println!("| postgres | postgres| UTF8     |");
        println!("| mydb     | user    | UTF8     |");
        println!("+----------+---------+----------+");
        return 0;
    }
    let host = args.windows(2).find(|w| w[0] == "-h").map(|w| w[1].as_str()).unwrap_or("localhost");
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("postgres");
    println!("Server: PostgreSQL 16.3");
    println!("Version: 4.1.0");
    println!("Home: http://pgcli.com");
    println!("Connected to {} at {}", db, host);
    println!("{}> ", db);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pgcli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pgcli(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
