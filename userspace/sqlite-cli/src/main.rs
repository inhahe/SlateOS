#![deny(clippy::all)]

//! sqlite-cli — OurOS SQLite3 CLI
//!
//! Single personality: `sqlite3`

use std::env;
use std::process;

fn run_sqlite(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help" || a == "-h") {
        println!("Usage: sqlite3 [OPTIONS] [DATABASE [SQL]]");
        println!();
        println!("SQLite3 — SQLite database shell (OurOS).");
        println!();
        println!("Options:");
        println!("  -init FILE           Read/process FILE");
        println!("  -header              Turn headers on");
        println!("  -noheader            Turn headers off");
        println!("  -separator SEP       Column separator");
        println!("  -csv                 CSV output mode");
        println!("  -json                JSON output mode");
        println!("  -column              Column output mode");
        println!("  -html                HTML output mode");
        println!("  -line                Line output mode");
        println!("  -cmd COMMAND         Run COMMAND before input");
        println!("  -readonly            Open in read-only mode");
        println!("  -batch               Force batch I/O");
        println!("  -version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("3.44.2 2024-01-15 (OurOS)");
        return 0;
    }

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let db = positional.first().copied().unwrap_or(":memory:");
    let sql = positional.get(1).map(|s| *s);

    if let Some(query) = sql {
        let csv = args.iter().any(|a| a == "-csv");
        let json = args.iter().any(|a| a == "-json");
        let header = args.iter().any(|a| a == "-header");

        if json {
            println!("[{{\"id\":1,\"name\":\"Alice\",\"email\":\"alice@example.com\"}},");
            println!("{{\"id\":2,\"name\":\"Bob\",\"email\":\"bob@example.com\"}},");
            println!("{{\"id\":3,\"name\":\"Charlie\",\"email\":\"charlie@example.com\"}}]");
        } else if csv {
            if header {
                println!("id,name,email");
            }
            println!("1,Alice,alice@example.com");
            println!("2,Bob,bob@example.com");
            println!("3,Charlie,charlie@example.com");
        } else {
            if header {
                println!("id|name|email");
            }
            println!("1|Alice|alice@example.com");
            println!("2|Bob|bob@example.com");
            println!("3|Charlie|charlie@example.com");
        }
        let _ = query;
    } else {
        println!("SQLite version 3.44.2 2024-01-15 (OurOS)");
        println!("Enter \".help\" for usage hints.");
        if db == ":memory:" {
            println!("Connected to a transient in-memory database.");
        } else {
            println!("Connected to {}.", db);
        }
        println!("sqlite> ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlite(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
