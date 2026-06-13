#![deny(clippy::all)]

//! temporal-cli — SlateOS Temporal CLI
//!
//! Single personality: `temporal`

use std::env;
use std::process;

fn run_temporal(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: temporal <COMMAND> [OPTIONS]");
        println!();
        println!("Temporal workflow orchestration CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  server       Start dev server");
        println!("  workflow     Manage workflows");
        println!("  activity     Manage activities");
        println!("  task-queue   Manage task queues");
        println!("  schedule     Manage schedules");
        println!("  batch        Manage batch operations");
        println!("  operator     Manage cluster");
        println!("  env          Manage environments");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("temporal version 0.13.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("start-dev");
            match sub {
                "start-dev" => {
                    let port = args.windows(2).find(|w| w[0] == "--port")
                        .map(|w| w[1].as_str()).unwrap_or("7233");
                    let ui_port = args.windows(2).find(|w| w[0] == "--ui-port")
                        .map(|w| w[1].as_str()).unwrap_or("8233");
                    println!("Temporal development server starting...");
                    println!("  gRPC: localhost:{}", port);
                    println!("  UI:   http://localhost:{}", ui_port);
                    println!("  Namespace: default");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        "workflow" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Status     Workflow ID                Type               Start Time");
                    println!("  Running    order-process-abc123       OrderWorkflow      2024-01-15 14:00:00");
                    println!("  Completed  user-signup-def456         SignupWorkflow      2024-01-15 13:30:00");
                    println!("  Failed     report-gen-ghi789          ReportWorkflow      2024-01-15 12:00:00");
                    println!("  Running    data-pipeline-jkl012       PipelineWorkflow    2024-01-15 10:00:00");
                }
                "start" => {
                    let wf_type = args.windows(2).find(|w| w[0] == "--type")
                        .map(|w| w[1].as_str()).unwrap_or("MyWorkflow");
                    let wf_id = args.windows(2).find(|w| w[0] == "--workflow-id")
                        .map(|w| w[1].as_str()).unwrap_or("wf-new-001");
                    println!("Started workflow:");
                    println!("  Workflow ID: {}", wf_id);
                    println!("  Run ID:      run-abc123def456");
                    println!("  Type:        {}", wf_type);
                }
                "describe" => {
                    let wf_id = args.get(2).map(|s| s.as_str()).unwrap_or("order-process-abc123");
                    println!("Workflow ID:   {}", wf_id);
                    println!("Run ID:        run-abc123def456");
                    println!("Type:          OrderWorkflow");
                    println!("Status:        Running");
                    println!("Start Time:    2024-01-15 14:00:00 UTC");
                    println!("Task Queue:    orders-queue");
                    println!("History Events: 24");
                    println!("Pending Activities: 1");
                }
                "signal" => {
                    let wf_id = args.get(2).map(|s| s.as_str()).unwrap_or("order-process-abc123");
                    println!("Signal sent to workflow {}", wf_id);
                }
                "cancel" => {
                    let wf_id = args.get(2).map(|s| s.as_str()).unwrap_or("order-process-abc123");
                    println!("Cancelled workflow {}", wf_id);
                }
                _ => { println!("Workflow operation: {}", sub); }
            }
            0
        }
        "task-queue" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("describe");
            match sub {
                "describe" => {
                    let queue = args.get(2).map(|s| s.as_str()).unwrap_or("orders-queue");
                    println!("Task Queue: {}", queue);
                    println!("  Pollers:");
                    println!("    worker-1@host1 (last seen: 2s ago)");
                    println!("    worker-2@host2 (last seen: 1s ago)");
                    println!("  Backlog: 5 workflow tasks, 2 activity tasks");
                }
                _ => { println!("Task queue operation: {}", sub); }
            }
            0
        }
        "schedule" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Schedule ID       Workflow Type    Spec              Status");
                    println!("  daily-report      ReportWorkflow   every day 9am     Running");
                    println!("  hourly-sync       SyncWorkflow     every hour        Running");
                    println!("  weekly-cleanup    CleanupWorkflow  Sun 2am           Paused");
                }
                _ => { println!("Schedule operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: temporal <command>. See --help.");
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
    let code = run_temporal(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_temporal};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_temporal(vec!["--help".to_string()]), 0);
        assert_eq!(run_temporal(vec!["-h".to_string()]), 0);
        let _ = run_temporal(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_temporal(vec![]);
    }
}
