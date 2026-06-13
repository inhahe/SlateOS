#![deny(clippy::all)]

//! jira-cli — SlateOS Jira CLI
//!
//! Single personality: `jira`

use std::env;
use std::process;

fn run_jira(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jira <COMMAND> [OPTIONS]");
        println!();
        println!("Jira project management CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  issue        Manage issues");
        println!("  sprint       Manage sprints");
        println!("  board        Manage boards");
        println!("  project      Manage projects");
        println!("  epic         Manage epics");
        println!("  search       Search issues (JQL)");
        println!("  me           Show current user");
        println!("  open         Open issue in browser");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "me" => {
            println!("User: alice.smith");
            println!("  Name:  Alice Smith");
            println!("  Email: alice@example.com");
            println!("  Role:  Developer");
            0
        }
        "issue" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Key         Type    Status        Priority  Assignee       Summary");
                    println!("PROJ-123    Bug     In Progress   High      alice.smith    Fix login timeout");
                    println!("PROJ-124    Story   To Do         Medium    bob.jones      Add search filters");
                    println!("PROJ-125    Task    Done          Low       charlie.b      Update docs");
                    println!("PROJ-126    Bug     To Do         Critical  alice.smith    Data loss on save");
                    println!("PROJ-127    Story   In Review     Medium    bob.jones      Dashboard widgets");
                }
                "create" => {
                    let summary = args.windows(2).find(|w| w[0] == "-s" || w[0] == "--summary").map(|w| w[1].as_str()).unwrap_or("New issue");
                    println!("✔ Created PROJ-128: {}", summary);
                }
                "view" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("PROJ-123");
                    println!("{}  Fix login timeout", key);
                    println!("  Type:       Bug");
                    println!("  Status:     In Progress");
                    println!("  Priority:   High");
                    println!("  Assignee:   alice.smith");
                    println!("  Reporter:   bob.jones");
                    println!("  Sprint:     Sprint 42");
                    println!("  Labels:     backend, auth");
                    println!("  Created:    2024-01-10");
                    println!("  Updated:    2024-01-15");
                }
                "assign" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("PROJ-123");
                    let user = args.get(3).map(|s| s.as_str()).unwrap_or("alice.smith");
                    println!("✔ {} assigned to {}", key, user);
                }
                "move" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("PROJ-123");
                    let status = args.get(3).map(|s| s.as_str()).unwrap_or("Done");
                    println!("✔ {} moved to '{}'", key, status);
                }
                _ => { println!("Issue operation: {}", sub); }
            }
            0
        }
        "sprint" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID    Name          State     Start          End");
                    println!("42    Sprint 42     active    2024-01-08     2024-01-22");
                    println!("41    Sprint 41     closed    2023-12-25     2024-01-07");
                    println!("43    Sprint 43     future    2024-01-22     2024-02-05");
                }
                "active" => {
                    println!("Sprint 42 (Active)");
                    println!("  Goal: Complete auth refactor");
                    println!("  Issues: 8 total (3 done, 3 in progress, 2 to do)");
                    println!("  Velocity: 21 story points");
                }
                _ => { println!("Sprint operation: {}", sub); }
            }
            0
        }
        "board" => {
            println!("Boards:");
            println!("  ID    Name              Type     Project");
            println!("  1     Engineering       scrum    PROJ");
            println!("  2     Support           kanban   SUP");
            0
        }
        "project" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Key     Name              Lead          Type");
                    println!("PROJ    My Project        alice.smith   Software");
                    println!("SUP     Support           bob.jones     Service Desk");
                    println!("INFRA   Infrastructure    charlie.b     Software");
                }
                _ => { println!("Project operation: {}", sub); }
            }
            0
        }
        "epic" => {
            println!("Epics:");
            println!("  Key         Status     Issues  Summary");
            println!("  PROJ-100    In Progress 5/12   Auth System Overhaul");
            println!("  PROJ-50     Done        8/8    API v2 Migration");
            println!("  PROJ-90     To Do       0/6    Dashboard Redesign");
            0
        }
        "search" => {
            let jql = args.get(1).map(|s| s.as_str()).unwrap_or("assignee = currentUser() AND status != Done");
            println!("JQL: {}", jql);
            println!("Results: 3 issues found");
            println!();
            println!("PROJ-123  Bug    In Progress  Fix login timeout");
            println!("PROJ-126  Bug    To Do        Data loss on save");
            println!("PROJ-128  Story  To Do        New issue");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: jira <command>. See --help.");
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
    let code = run_jira(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jira};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jira(vec!["--help".to_string()]), 0);
        assert_eq!(run_jira(vec!["-h".to_string()]), 0);
        let _ = run_jira(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jira(vec![]);
    }
}
