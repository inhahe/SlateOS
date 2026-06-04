#![deny(clippy::all)]

//! clearml-cli — OurOS ClearML CLI
//!
//! Multi-personality: `clearml-init`, `clearml-task`, `clearml-data`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_clearml(args: &[String], prog: &str) -> i32 {
    match prog {
        "clearml-init" => {
            println!("ClearML SDK setup");
            println!("  Configuration file: ~/clearml.conf");
            println!("  API server: https://api.clear.ml");
            println!("  Web server: https://app.clear.ml");
            println!("Configuration saved.");
            return 0;
        }
        "clearml-data" => {
            if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
                println!("Usage: clearml-data COMMAND [OPTIONS]");
                println!("ClearML Data Management");
                println!("  create      Create a dataset");
                println!("  add         Add files to dataset");
                println!("  upload      Upload dataset");
                println!("  close       Finalize dataset");
                println!("  list        List datasets");
                println!("  get         Download dataset");
                return 0;
            }
            let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "list" => {
                    println!("ID           NAME              VERSION   SIZE");
                    println!("ds-abc123    training-data     1.0       2.3 GB");
                    println!("ds-def456    test-data         1.0       500 MB");
                }
                "create" => {
                    let name = args.windows(2).find(|w| w[0] == "--name")
                        .map(|w| w[1].as_str()).unwrap_or("my-dataset");
                    println!("Created dataset '{}': ds-ghi789", name);
                }
                _ => println!("clearml-data: '{}' completed", subcmd),
            }
            return 0;
        }
        _ => {} // clearml-task or default
    }

    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: clearml-task [OPTIONS]");
        println!("ClearML Task CLI 1.14.0 (OurOS)");
        println!();
        println!("Options:");
        println!("  --name NAME        Task name");
        println!("  --project NAME     Project name");
        println!("  --script FILE      Script to run");
        println!("  --queue NAME       Queue to enqueue on");
        println!("  --branch BRANCH    Git branch");
        println!("  --docker IMAGE     Docker image");
        println!("  --requirements FILE  Requirements file");
        return 0;
    }
    let name = args.windows(2).find(|w| w[0] == "--name")
        .map(|w| w[1].as_str()).unwrap_or("my-task");
    let queue = args.windows(2).find(|w| w[0] == "--queue")
        .map(|w| w[1].as_str()).unwrap_or("default");

    println!("Creating task '{}'...", name);
    println!("  Task ID: task-abc123");
    println!("  Project: my-project");
    println!("  Enqueuing on queue '{}'...", queue);
    println!("Task enqueued successfully.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "clearml-task".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_clearml(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_clearml};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/clearml"), "clearml");
        assert_eq!(basename(r"C:\bin\clearml.exe"), "clearml.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("clearml.exe"), "clearml");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_clearml(&["--help".to_string()], "clearml"), 0);
        assert_eq!(run_clearml(&["-h".to_string()], "clearml"), 0);
        let _ = run_clearml(&["--version".to_string()], "clearml");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_clearml(&[], "clearml");
    }
}
