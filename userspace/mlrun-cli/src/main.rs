#![deny(clippy::all)]

//! mlrun-cli — SlateOS MLRun CLI
//!
//! Multi-personality: `mlrun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mlrun(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mlrun COMMAND [OPTIONS]");
        println!("MLRun CLI 1.6.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  project        Manage projects");
        println!("  run            Run a function");
        println!("  build          Build a function");
        println!("  deploy         Deploy a function");
        println!("  get            Get resources");
        println!("  logs           Get run logs");
        println!("  watch          Watch a run");
        println!("  config         Manage config");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("mlrun 1.6.0"),
        "project" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("NAME              STATE     FUNCTIONS   RUNS");
                println!("ml-project        active    5           42");
                println!("data-pipeline     active    3           18");
            } else {
                println!("mlrun project: '{}' completed", sub);
            }
        }
        "run" => {
            let func = args.get(1).map(|s| s.as_str()).unwrap_or("training");
            println!("Running function '{}'...", func);
            println!("  Run ID: abc123");
            println!("  Status: completed");
            println!("  Duration: 5m 23s");
            println!("  Results:");
            println!("    accuracy: 0.95");
            println!("    loss: 0.12");
        }
        "deploy" => {
            let func = args.get(1).map(|s| s.as_str()).unwrap_or("serving");
            println!("Deploying function '{}'...", func);
            println!("  Building container...");
            println!("  Deploying to cluster...");
            println!("  Endpoint: http://serving.default.svc.cluster.local:8080");
        }
        "logs" => {
            let run_id = args.get(1).map(|s| s.as_str()).unwrap_or("abc123");
            println!("Logs for run '{}':", run_id);
            println!("  [INFO] Starting training...");
            println!("  [INFO] Epoch 1/10: loss=0.45, acc=0.82");
            println!("  [INFO] Epoch 10/10: loss=0.12, acc=0.95");
            println!("  [INFO] Model saved.");
        }
        _ => println!("mlrun: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mlrun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mlrun(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mlrun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mlrun"), "mlrun");
        assert_eq!(basename(r"C:\bin\mlrun.exe"), "mlrun.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mlrun.exe"), "mlrun");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mlrun(&["--help".to_string()]), 0);
        assert_eq!(run_mlrun(&["-h".to_string()]), 0);
        let _ = run_mlrun(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mlrun(&[]);
    }
}
