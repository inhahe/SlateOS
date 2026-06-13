#![deny(clippy::all)]

//! shortcut-cli — SlateOS Shortcut (formerly Clubhouse) CLI
//!
//! Single personality: `shortcut`

use std::env;
use std::process;

fn run_shortcut(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shortcut <COMMAND> [OPTIONS]");
        println!();
        println!("Shortcut project management CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  stories      Manage stories");
        println!("  epics        Manage epics");
        println!("  iterations   Manage iterations");
        println!("  projects     Manage projects");
        println!("  workflows    List workflows");
        println!("  members      List members");
        println!("  search       Search stories");
        println!("  labels       Manage labels");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "stories" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID      Type      State          Estimate  Owner    Name");
                    println!("sc-123  feature   In Development 3         alice    Add search filters");
                    println!("sc-124  bug       Ready for Dev  2         bob      Fix login timeout");
                    println!("sc-125  chore     In Review      1         charlie  Update CI config");
                    println!("sc-126  feature   Completed      5         alice    Dashboard redesign");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("New story");
                    let story_type = args.windows(2).find(|w| w[0] == "--type").map(|w| w[1].as_str()).unwrap_or("feature");
                    println!("✔ Created {} sc-127: {}", story_type, name);
                }
                "view" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("sc-123");
                    println!("{} — Add search filters", id);
                    println!("  Type:       feature");
                    println!("  State:      In Development");
                    println!("  Estimate:   3 points");
                    println!("  Owner:      alice");
                    println!("  Epic:       Search Improvements");
                    println!("  Iteration:  Sprint 12");
                    println!("  Labels:     frontend, ux");
                    println!("  Created:    2024-01-10");
                }
                "move" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("sc-123");
                    let state = args.windows(2).find(|w| w[0] == "--state").map(|w| w[1].as_str()).unwrap_or("In Review");
                    println!("✔ {} moved to '{}'", id, state);
                }
                _ => { println!("Story operation: {}", sub); }
            }
            0
        }
        "epics" => {
            println!("Epics:");
            println!("  ID       State        Stories  Name");
            println!("  ep-10    In Progress  5/12     Search Improvements");
            println!("  ep-11    To Do        0/8      Performance Optimization");
            println!("  ep-9     Done         10/10    Auth System v2");
            0
        }
        "iterations" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Name          Status      Start         End           Stories  Points");
                    println!("Sprint 12     started     2024-01-08    2024-01-22    8        21");
                    println!("Sprint 11     done        2023-12-25    2024-01-07    10       25");
                    println!("Sprint 13     unstarted   2024-01-22    2024-02-05    0        0");
                }
                _ => { println!("Iteration operation: {}", sub); }
            }
            0
        }
        "projects" => {
            println!("Projects:");
            println!("  Name              Stories  Epics  Team");
            println!("  Backend           25       3      Engineering");
            println!("  Frontend          18       2      Engineering");
            println!("  Mobile            12       2      Mobile");
            0
        }
        "workflows" => {
            println!("Workflows:");
            println!("  Default:");
            println!("    Unscheduled → Ready for Dev → In Development → In Review → Completed");
            0
        }
        "members" => {
            println!("Members:");
            println!("  Username    Name             Role      Groups");
            println!("  alice       Alice Smith      member    Engineering");
            println!("  bob         Bob Jones        member    Engineering");
            println!("  charlie     Charlie Brown    admin     Engineering, Ops");
            0
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("search");
            println!("Results for '{}':", query);
            println!("  sc-123  feature  In Development  Add search filters");
            println!("  sc-115  chore    Completed       Add search indexing");
            0
        }
        "labels" => {
            println!("Labels:");
            println!("  frontend    (blue)");
            println!("  backend     (green)");
            println!("  ux          (purple)");
            println!("  bug         (red)");
            println!("  tech-debt   (orange)");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: shortcut <command>. See --help.");
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
    let code = run_shortcut(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_shortcut};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shortcut(vec!["--help".to_string()]), 0);
        assert_eq!(run_shortcut(vec!["-h".to_string()]), 0);
        let _ = run_shortcut(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shortcut(vec![]);
    }
}
