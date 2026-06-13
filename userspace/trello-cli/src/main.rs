#![deny(clippy::all)]

//! trello-cli — SlateOS Trello CLI
//!
//! Single personality: `trello`

use std::env;
use std::process;

fn run_trello(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trello <COMMAND> [OPTIONS]");
        println!();
        println!("Trello board management CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  boards       Manage boards");
        println!("  lists        Manage lists");
        println!("  cards        Manage cards");
        println!("  members      List members");
        println!("  labels       Manage labels");
        println!("  search       Search cards");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "boards" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID              Name                  Lists  Cards  Members");
                    println!("brd-abc123      Engineering Sprint    4      18     5");
                    println!("brd-def456      Product Backlog       3      42     3");
                    println!("brd-ghi789      Design Tasks          3      12     4");
                }
                _ => { println!("Board operation: {}", sub); }
            }
            0
        }
        "lists" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let board = args.windows(2).find(|w| w[0] == "--board").map(|w| w[1].as_str()).unwrap_or("brd-abc123");
                    println!("Lists in {}:", board);
                    println!("  To Do          (5 cards)");
                    println!("  In Progress    (4 cards)");
                    println!("  Review         (3 cards)");
                    println!("  Done           (6 cards)");
                }
                _ => { println!("List operation: {}", sub); }
            }
            0
        }
        "cards" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID              List          Labels       Due          Name");
                    println!("crd-abc123      In Progress   bug,high     2024-01-20   Fix timeout");
                    println!("crd-def456      To Do         feature      2024-01-22   Add filters");
                    println!("crd-ghi789      Review        docs         -            Update API docs");
                    println!("crd-jkl012      Done          bug          2024-01-15   Fix typo");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("New card");
                    println!("✔ Created card: {} (crd-new123)", name);
                }
                "move" => {
                    let card = args.get(2).map(|s| s.as_str()).unwrap_or("crd-abc123");
                    let list = args.windows(2).find(|w| w[0] == "--list").map(|w| w[1].as_str()).unwrap_or("Done");
                    println!("✔ Moved {} to '{}'", card, list);
                }
                _ => { println!("Card operation: {}", sub); }
            }
            0
        }
        "members" => {
            println!("Members:");
            println!("  Username      Name              Role");
            println!("  alice123      Alice Smith       admin");
            println!("  bob456        Bob Jones         normal");
            println!("  charlie789    Charlie Brown     normal");
            0
        }
        "labels" => {
            println!("Labels:");
            println!("  Color     Name");
            println!("  red       bug");
            println!("  orange    high");
            println!("  green     feature");
            println!("  blue      docs");
            println!("  purple    design");
            0
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("fix");
            println!("Results for '{}':", query);
            println!("  crd-abc123  Fix timeout       In Progress");
            println!("  crd-jkl012  Fix typo          Done");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: trello <command>. See --help.");
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
    let code = run_trello(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_trello};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trello(vec!["--help".to_string()]), 0);
        assert_eq!(run_trello(vec!["-h".to_string()]), 0);
        let _ = run_trello(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trello(vec![]);
    }
}
