#![deny(clippy::all)]

//! mlflow-cli — Slate OS MLflow CLI
//!
//! Single personality: `mlflow`

use std::env;
use std::process;

fn run_mlflow(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mlflow <COMMAND> [OPTIONS]");
        println!();
        println!("MLflow ML lifecycle management CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  server       Start tracking server");
        println!("  ui           Start MLflow UI");
        println!("  experiments  Manage experiments");
        println!("  runs         Manage runs");
        println!("  models       Manage models");
        println!("  deployments  Manage deployments");
        println!("  artifacts    Manage artifacts");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "server" | "ui" => {
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("5000");
            println!("[MLflow] Starting {} at http://localhost:{}", cmd, port);
            println!("[MLflow] Serving on http://0.0.0.0:{}", port);
            0
        }
        "experiments" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Experiment ID  Name                  Artifact Location    Lifecycle");
                    println!("  0              Default               mlruns/0             active");
                    println!("  1              text-classification   mlruns/1             active");
                    println!("  2              image-segmentation    mlruns/2             active");
                    println!("  3              recommender           mlruns/3             deleted");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("my-experiment");
                    println!("Created experiment '{}' with ID 4", name);
                }
                _ => { println!("Experiment operation: {}", sub); }
            }
            0
        }
        "runs" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Run ID                              Status     Start Time           Metrics");
                    println!("  abc123def456ghi789jkl012mno345pqr   FINISHED   2024-01-15 14:00:00  accuracy=0.95, loss=0.12");
                    println!("  stu678vwx901yza234bcd567efg890hij   FINISHED   2024-01-15 12:00:00  accuracy=0.92, loss=0.18");
                    println!("  klm123nop456qrs789tuv012wxy345zab   RUNNING    2024-01-15 15:00:00  -");
                }
                _ => { println!("Run operation: {}", sub); }
            }
            0
        }
        "models" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Name                    Latest Version  Stage        Updated");
                    println!("  text-classifier         3               Production   2024-01-15");
                    println!("  image-segmenter         2               Staging      2024-01-14");
                    println!("  recommender-v2          1               None         2024-01-13");
                }
                "serve" => {
                    let model = args.get(2).map(|s| s.as_str()).unwrap_or("text-classifier");
                    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("5001");
                    println!("Serving model '{}' at http://localhost:{}/invocations", model, port);
                }
                _ => { println!("Model operation: {}", sub); }
            }
            0
        }
        "artifacts" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Artifacts:");
                    println!("  model/             (directory)");
                    println!("  model/MLmodel      1.2 KB");
                    println!("  model/model.pkl    45.6 MB");
                    println!("  metrics.json       256 B");
                    println!("  confusion_matrix.png  89 KB");
                }
                _ => { println!("Artifact operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: mlflow <command>. See --help.");
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
    let code = run_mlflow(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mlflow};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mlflow(vec!["--help".to_string()]), 0);
        assert_eq!(run_mlflow(vec!["-h".to_string()]), 0);
        let _ = run_mlflow(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mlflow(vec![]);
    }
}
