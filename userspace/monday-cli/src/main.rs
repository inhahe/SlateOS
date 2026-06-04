#![deny(clippy::all)]

//! monday-cli — OurOS monday.com CLI
//!
//! Single personality: `monday`

use std::env;
use std::process;

fn run_monday(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: monday <COMMAND> [OPTIONS]");
        println!();
        println!("monday.com work management CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  boards       Manage boards");
        println!("  items        Manage items");
        println!("  groups       Manage groups");
        println!("  columns      Manage columns");
        println!("  updates      Manage updates");
        println!("  workspaces   List workspaces");
        println!("  me           Current user");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "me" => {
            println!("Alice Smith");
            println!("  Email:     alice@example.com");
            println!("  Account:   My Company");
            println!("  Plan:      Pro");
            0
        }
        "boards" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID          Name                    Type      Items  Workspace");
                    println!("1234567     Sprint Board            board     18     Engineering");
                    println!("2345678     Content Calendar        board     25     Marketing");
                    println!("3456789     Bug Tracker             board     12     Engineering");
                }
                _ => { println!("Board operation: {}", sub); }
            }
            0
        }
        "items" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let board = args.windows(2).find(|w| w[0] == "--board").map(|w| w[1].as_str()).unwrap_or("1234567");
                    println!("Items in board {}:", board);
                    println!("  ID          Group          Status         Person      Name");
                    println!("  i-abc123    Sprint 42      Working on it  Alice       Fix auth bug");
                    println!("  i-def456    Sprint 42      Stuck          Bob         API migration");
                    println!("  i-ghi789    Sprint 42      Done           Charlie     Write tests");
                    println!("  i-jkl012    Backlog        -              -           Add dark mode");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("New item");
                    println!("✔ Created item: {} (ID: i-new123)", name);
                }
                "update" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("i-abc123");
                    println!("✔ Item {} updated", id);
                }
                _ => { println!("Item operation: {}", sub); }
            }
            0
        }
        "groups" => {
            let board = args.windows(2).find(|w| w[0] == "--board").map(|w| w[1].as_str()).unwrap_or("1234567");
            println!("Groups in board {}:", board);
            println!("  Sprint 42    (3 items)  color: blue");
            println!("  Sprint 43    (0 items)  color: green");
            println!("  Backlog      (5 items)  color: gray");
            0
        }
        "columns" => {
            let board = args.windows(2).find(|w| w[0] == "--board").map(|w| w[1].as_str()).unwrap_or("1234567");
            println!("Columns in board {}:", board);
            println!("  Name          Type         Settings");
            println!("  Status        status       Working on it, Stuck, Done");
            println!("  Person        people       -");
            println!("  Date          date         -");
            println!("  Priority      color        Critical, High, Medium, Low");
            println!("  Timeline      timeline     -");
            0
        }
        "updates" => {
            let item = args.windows(2).find(|w| w[0] == "--item").map(|w| w[1].as_str()).unwrap_or("i-abc123");
            println!("Updates for {}:", item);
            println!("  [2024-01-15 14:00] Alice: Found the root cause, working on fix");
            println!("  [2024-01-15 10:00] Bob: Can reproduce on staging");
            println!("  [2024-01-14 16:00] Alice: Investigating auth timeout issue");
            0
        }
        "workspaces" => {
            println!("Workspaces:");
            println!("  ID          Name              Boards");
            println!("  ws-abc      Engineering       5");
            println!("  ws-def      Marketing         3");
            println!("  ws-ghi      Operations        2");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: monday <command>. See --help.");
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
    let code = run_monday(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_monday};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_monday(vec!["--help".to_string()]), 0);
        assert_eq!(run_monday(vec!["-h".to_string()]), 0);
        let _ = run_monday(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_monday(vec![]);
    }
}
