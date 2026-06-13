#![deny(clippy::all)]

//! linear-cli — SlateOS Linear CLI
//!
//! Single personality: `linear`

use std::env;
use std::process;

fn run_linear(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: linear <COMMAND> [OPTIONS]");
        println!();
        println!("Linear issue tracker CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  issue        Manage issues");
        println!("  cycle        Manage cycles");
        println!("  project      Manage projects");
        println!("  team         Manage teams");
        println!("  me           Show current user");
        println!("  search       Search issues");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "me" => {
            println!("alice (Alice Smith)");
            println!("  Email: alice@example.com");
            println!("  Team:  Engineering");
            0
        }
        "issue" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID          Status        Priority  Title");
                    println!("ENG-42      In Progress   Urgent    Fix auth flow");
                    println!("ENG-43      Todo          High      Add dark mode");
                    println!("ENG-44      In Review     Medium    Update API docs");
                    println!("ENG-45      Done          Low       Clean up logs");
                }
                "create" => {
                    let title = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--title").map(|w| w[1].as_str()).unwrap_or("New issue");
                    println!("✔ Created ENG-46: {}", title);
                }
                "view" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("ENG-42");
                    println!("{} — Fix auth flow", id);
                    println!("  Status:    In Progress");
                    println!("  Priority:  Urgent");
                    println!("  Assignee:  alice");
                    println!("  Cycle:     Cycle 12");
                    println!("  Project:   Auth Revamp");
                    println!("  Labels:    bug, backend");
                    println!("  Estimate:  3 points");
                }
                _ => { println!("Issue operation: {}", sub); }
            }
            0
        }
        "cycle" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Number  Name        Status    Progress    Dates");
                    println!("12      Cycle 12    active    60%         Jan 8 - Jan 22");
                    println!("11      Cycle 11    completed 100%        Dec 25 - Jan 7");
                    println!("13      Cycle 13    upcoming  0%          Jan 22 - Feb 5");
                }
                _ => { println!("Cycle operation: {}", sub); }
            }
            0
        }
        "project" => {
            println!("Projects:");
            println!("  Name              Status       Progress  Lead");
            println!("  Auth Revamp       In Progress  45%       alice");
            println!("  API v3            Planning     10%       bob");
            println!("  Dashboard         Completed    100%      charlie");
            0
        }
        "team" => {
            println!("Teams:");
            println!("  Name              Members  Issues");
            println!("  Engineering       8        42");
            println!("  Design            4        18");
            println!("  Product           3        12");
            0
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("auth");
            println!("Results for '{}':", query);
            println!("  ENG-42  In Progress  Fix auth flow");
            println!("  ENG-38  Done         Auth token refresh");
            println!("  ENG-30  Done         OAuth2 integration");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: linear <command>. See --help.");
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
    let code = run_linear(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_linear};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_linear(vec!["--help".to_string()]), 0);
        assert_eq!(run_linear(vec!["-h".to_string()]), 0);
        let _ = run_linear(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_linear(vec![]);
    }
}
