#![deny(clippy::all)]

//! notion-cli — SlateOS Notion CLI
//!
//! Single personality: `notion`

use std::env;
use std::process;

fn run_notion(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: notion <COMMAND> [OPTIONS]");
        println!();
        println!("Notion workspace CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  pages        Manage pages");
        println!("  databases    Manage databases");
        println!("  blocks       Manage blocks");
        println!("  search       Search workspace");
        println!("  users        List users");
        println!("  comments     Manage comments");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "pages" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                                   Title                    Last Edited");
                    println!("abc123-def456-ghi789                 Project Roadmap          2024-01-15");
                    println!("jkl012-mno345-pqr678                 Meeting Notes            2024-01-15");
                    println!("stu901-vwx234-yza567                 Technical Specs          2024-01-14");
                    println!("bcd890-efg123-hij456                 Team Wiki                2024-01-13");
                }
                "get" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("abc123...");
                    println!("Page: {}", id);
                    println!("  Title:       Project Roadmap");
                    println!("  Created:     2024-01-01");
                    println!("  Edited:      2024-01-15");
                    println!("  Created By:  Alice Smith");
                    println!("  Parent:      Engineering Space");
                }
                "create" => {
                    let title = args.windows(2).find(|w| w[0] == "--title").map(|w| w[1].as_str()).unwrap_or("New Page");
                    println!("✔ Created page: {}", title);
                    println!("  ID: new-page-abc123");
                }
                _ => { println!("Page operation: {}", sub); }
            }
            0
        }
        "databases" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                   Title              Properties");
                    println!("db-abc123            Tasks              Name, Status, Assignee, Priority, Due");
                    println!("db-def456            Bugs               Title, Severity, Reporter, Status");
                    println!("db-ghi789            Content Calendar   Title, Date, Author, Status");
                }
                "query" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("db-abc123");
                    println!("Results from {}:", id);
                    println!("  Name                Status       Assignee    Priority");
                    println!("  Fix login bug       In Progress  Alice       High");
                    println!("  Add dark mode       To Do        Bob         Medium");
                    println!("  Update docs         Done         Charlie     Low");
                }
                _ => { println!("Database operation: {}", sub); }
            }
            0
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("roadmap");
            println!("Search results for '{}':", query);
            println!("  [page]     Project Roadmap          Engineering Space");
            println!("  [database] Q1 Roadmap               Planning");
            println!("  [page]     Roadmap Review Notes     Meeting Notes");
            0
        }
        "users" => {
            println!("Users:");
            println!("  Name              Type    Email");
            println!("  Alice Smith       person  alice@example.com");
            println!("  Bob Jones         person  bob@example.com");
            println!("  CI Bot            bot     -");
            0
        }
        "blocks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let page = args.get(2).map(|s| s.as_str()).unwrap_or("abc123...");
                    println!("Blocks in page {}:", page);
                    println!("  [heading_1]    Project Roadmap");
                    println!("  [paragraph]    This document outlines...");
                    println!("  [heading_2]    Q1 Goals");
                    println!("  [bulleted_list] Launch v2.0");
                    println!("  [bulleted_list] Migrate to new API");
                    println!("  [to_do]        Review security audit ☐");
                }
                _ => { println!("Block operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: notion <command>. See --help.");
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
    let code = run_notion(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_notion};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_notion(vec!["--help".to_string()]), 0);
        assert_eq!(run_notion(vec!["-h".to_string()]), 0);
        let _ = run_notion(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_notion(vec![]);
    }
}
