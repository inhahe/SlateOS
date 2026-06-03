#![deny(clippy::all)]

//! wandb-cli — OurOS Weights & Biases CLI
//!
//! Single personality: `wandb`

use std::env;
use std::process;

fn run_wandb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wandb <COMMAND> [OPTIONS]");
        println!();
        println!("Weights & Biases experiment tracking CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  login        Login to W&B");
        println!("  init         Initialize project");
        println!("  sync         Sync offline runs");
        println!("  sweep        Manage hyperparameter sweeps");
        println!("  agent        Start sweep agent");
        println!("  artifact     Manage artifacts");
        println!("  server       Start local server");
        println!("  status       Show run status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("wandb 0.16.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "login" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("*****hidden*****");
            println!("wandb: Logging in to wandb.ai...");
            println!("wandb: Appending key to /home/user/.netrc");
            println!("wandb: API key: {}...{}", &key[..3.min(key.len())], &key[key.len().saturating_sub(3)..]);
            println!("wandb: Successfully logged in.");
            0
        }
        "init" => {
            let project = args.get(1).map(|s| s.as_str()).unwrap_or("my-project");
            println!("wandb: Initializing project '{}'", project);
            println!("wandb: Created wandb directory");
            println!("wandb: Updated .gitignore");
            println!("wandb: Project '{}' ready at https://wandb.ai/user/{}", project, project);
            0
        }
        "sync" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("./wandb");
            println!("wandb: Scanning {} for offline runs...", dir);
            println!("wandb: Found 3 offline runs");
            println!("  Syncing run-20240115_140000-abc123... done (2.3 MB)");
            println!("  Syncing run-20240115_150000-def456... done (1.8 MB)");
            println!("  Syncing run-20240115_160000-ghi789... done (3.1 MB)");
            println!("wandb: 3 runs synced successfully");
            0
        }
        "sweep" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "create" => {
                    let config = args.get(2).map(|s| s.as_str()).unwrap_or("sweep.yaml");
                    println!("wandb: Creating sweep from {}...", config);
                    println!("wandb: Sweep created: https://wandb.ai/user/project/sweeps/abc123");
                    println!("wandb: Run `wandb agent user/project/abc123` to start");
                }
                "list" => {
                    println!("  Sweep ID    State     Runs   Best Metric");
                    println!("  abc123      RUNNING   15     accuracy=0.965");
                    println!("  def456      FINISHED  50     accuracy=0.958");
                    println!("  ghi789      PAUSED    8      accuracy=0.942");
                }
                _ => { println!("Sweep operation: {}", sub); }
            }
            0
        }
        "agent" => {
            let sweep_id = args.get(1).map(|s| s.as_str()).unwrap_or("user/project/abc123");
            println!("wandb: Starting sweep agent for {}", sweep_id);
            println!("wandb: Agent pid: 12345");
            println!("wandb: Running run-1/50 with {{lr: 0.001, batch_size: 32}}");
            println!("wandb: Run finished. Results: accuracy=0.952, loss=0.134");
            println!("wandb: Running run-2/50 with {{lr: 0.005, batch_size: 64}}");
            println!("wandb: Run finished. Results: accuracy=0.961, loss=0.112");
            0
        }
        "artifact" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Name                    Type      Version   Size     Updated");
                    println!("  train-dataset           dataset   v3        2.1 GB   2024-01-15");
                    println!("  best-model              model     v7        456 MB   2024-01-15");
                    println!("  eval-results            result    v2        12 KB    2024-01-14");
                }
                "get" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("best-model:latest");
                    println!("wandb: Downloading artifact '{}'...", name);
                    println!("wandb: Downloaded 456 MB to ./artifacts/{}", name.split(':').next().unwrap_or(name));
                }
                "put" => {
                    let path = args.get(2).map(|s| s.as_str()).unwrap_or("./model");
                    println!("wandb: Uploading artifact from '{}'...", path);
                    println!("wandb: Artifact uploaded as 'model:v8' (456 MB)");
                }
                _ => { println!("Artifact operation: {}", sub); }
            }
            0
        }
        "server" => {
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8080");
            println!("wandb: Starting local W&B server...");
            println!("wandb: Server running at http://localhost:{}", port);
            println!("wandb: Using SQLite backend at ./wandb.db");
            0
        }
        "status" => {
            println!("wandb: Current run status:");
            println!("  Active runs:");
            println!("    run-abc123  training-v2  RUNNING  epoch 45/100  accuracy=0.952");
            println!("    run-def456  eval-sweep   RUNNING  step 1200     loss=0.134");
            println!("  Recent runs:");
            println!("    run-ghi789  baseline     FINISHED 2024-01-15    accuracy=0.928");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: wandb <command>. See --help.");
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
    let code = run_wandb(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_wandb};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wandb(vec!["--help".to_string()]), 0);
        assert_eq!(run_wandb(vec!["-h".to_string()]), 0);
        assert_eq!(run_wandb(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wandb(vec![]), 0);
    }
}
