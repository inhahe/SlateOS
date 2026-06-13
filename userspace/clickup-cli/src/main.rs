#![deny(clippy::all)]

//! clickup-cli — SlateOS ClickUp CLI
//!
//! Single personality: `clickup`

use std::env;
use std::process;

fn run_clickup(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clickup <COMMAND> [OPTIONS]");
        println!();
        println!("ClickUp project management CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  tasks        Manage tasks");
        println!("  spaces       Manage spaces");
        println!("  folders      Manage folders");
        println!("  lists        Manage lists");
        println!("  goals        Manage goals");
        println!("  time         Time tracking");
        println!("  me           Current user");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "me" => {
            println!("Alice Smith");
            println!("  Email:     alice@example.com");
            println!("  Role:      admin");
            println!("  Workspace: My Workspace");
            0
        }
        "tasks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID          Status        Priority  Assignee    Name");
                    println!("abc123      in progress   urgent    alice       Fix auth bug");
                    println!("def456      to do         high      bob         Add dashboard");
                    println!("ghi789      review        normal    charlie     Update docs");
                    println!("jkl012      complete      low       alice       Refactor tests");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("New task");
                    println!("✔ Created task: {} (ID: mno345)", name);
                }
                "update" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("abc123");
                    println!("✔ Task {} updated", id);
                }
                _ => { println!("Task operation: {}", sub); }
            }
            0
        }
        "spaces" => {
            println!("Spaces:");
            println!("  ID          Name              Folders  Lists");
            println!("  sp-abc      Engineering       3        8");
            println!("  sp-def      Marketing         2        5");
            println!("  sp-ghi      Operations        1        3");
            0
        }
        "folders" => {
            let space = args.windows(2).find(|w| w[0] == "--space").map(|w| w[1].as_str()).unwrap_or("sp-abc");
            println!("Folders in {}:", space);
            println!("  Backend      (3 lists, 15 tasks)");
            println!("  Frontend     (2 lists, 10 tasks)");
            println!("  DevOps       (3 lists, 8 tasks)");
            0
        }
        "goals" => {
            println!("Goals:");
            println!("  Name                    Progress  Due");
            println!("  Launch v2.0             65%       2024-03-01");
            println!("  Reduce bug count        80%       2024-02-01");
            println!("  Improve test coverage   45%       2024-04-01");
            0
        }
        "time" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Time entries (this week):");
                    println!("  Task              Duration    Date");
                    println!("  Fix auth bug      2h 30m      2024-01-15");
                    println!("  Code review       1h 15m      2024-01-15");
                    println!("  Add dashboard     3h 00m      2024-01-14");
                }
                "start" => {
                    let task = args.get(2).map(|s| s.as_str()).unwrap_or("abc123");
                    println!("✔ Timer started for task {}", task);
                }
                "stop" => {
                    println!("✔ Timer stopped. Duration: 1h 23m");
                }
                _ => { println!("Time operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: clickup <command>. See --help.");
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
    let code = run_clickup(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_clickup};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_clickup(vec!["--help".to_string()]), 0);
        assert_eq!(run_clickup(vec!["-h".to_string()]), 0);
        let _ = run_clickup(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_clickup(vec![]);
    }
}
