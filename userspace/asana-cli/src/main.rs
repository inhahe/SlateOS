#![deny(clippy::all)]

//! asana-cli — OurOS Asana CLI
//!
//! Single personality: `asana`

use std::env;
use std::process;

fn run_asana(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: asana <COMMAND> [OPTIONS]");
        println!();
        println!("Asana project management CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  tasks        Manage tasks");
        println!("  projects     Manage projects");
        println!("  sections     Manage sections");
        println!("  workspaces   List workspaces");
        println!("  teams        List teams");
        println!("  me           Show current user");
        println!("  search       Search tasks");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "me" => {
            println!("Alice Smith (alice@example.com)");
            println!("  Workspace: My Company");
            println!("  Team: Engineering");
            0
        }
        "tasks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("GID           Completed  Due         Assignee     Name");
                    println!("1234567890    ☐          2024-01-20  alice        Fix login timeout");
                    println!("2345678901    ☐          2024-01-18  bob          Add search filters");
                    println!("3456789012    ☑          2024-01-15  charlie      Update docs");
                    println!("4567890123    ☐          2024-01-22  alice        Refactor API layer");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("New task");
                    println!("✔ Created task: {} (GID: 5678901234)", name);
                }
                "complete" => {
                    let gid = args.get(2).map(|s| s.as_str()).unwrap_or("1234567890");
                    println!("✔ Task {} marked complete", gid);
                }
                "view" => {
                    let gid = args.get(2).map(|s| s.as_str()).unwrap_or("1234567890");
                    println!("Task {}", gid);
                    println!("  Name:       Fix login timeout");
                    println!("  Completed:  false");
                    println!("  Due:        2024-01-20");
                    println!("  Assignee:   alice");
                    println!("  Project:    Backend Tasks");
                    println!("  Section:    In Progress");
                    println!("  Tags:       bug, backend");
                }
                _ => { println!("Task operation: {}", sub); }
            }
            0
        }
        "projects" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("GID           Name              Team           Tasks");
                    println!("p-abc123      Backend Tasks     Engineering    12");
                    println!("p-def456      Frontend          Engineering    8");
                    println!("p-ghi789      Design System     Design         15");
                }
                _ => { println!("Project operation: {}", sub); }
            }
            0
        }
        "sections" => {
            let project = args.windows(2).find(|w| w[0] == "--project").map(|w| w[1].as_str()).unwrap_or("p-abc123");
            println!("Sections in {}:", project);
            println!("  To Do         (4 tasks)");
            println!("  In Progress   (3 tasks)");
            println!("  In Review     (2 tasks)");
            println!("  Done          (3 tasks)");
            0
        }
        "workspaces" => {
            println!("GID           Name");
            println!("w-abc123      My Company");
            println!("w-def456      Side Project");
            0
        }
        "teams" => {
            println!("GID           Name              Members");
            println!("t-abc123      Engineering       8");
            println!("t-def456      Design            4");
            println!("t-ghi789      Product           3");
            0
        }
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("login");
            println!("Results for '{}':", query);
            println!("  1234567890  Fix login timeout     In Progress");
            println!("  0987654321  Login page redesign   Done");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: asana <command>. See --help.");
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
    let code = run_asana(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_asana};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_asana(vec!["--help".to_string()]), 0);
        assert_eq!(run_asana(vec!["-h".to_string()]), 0);
        assert_eq!(run_asana(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_asana(vec![]), 0);
    }
}
