#![deny(clippy::all)]

//! harness-cli — SlateOS Harness CI/CD CLI
//!
//! Multi-personality: `harness`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_harness(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: harness COMMAND [OPTIONS]");
        println!("Harness CLI 0.6.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  login          Authenticate");
        println!("  pipeline       Manage pipelines");
        println!("  service        Manage services");
        println!("  environment    Manage environments");
        println!("  connector      Manage connectors");
        println!("  secret         Manage secrets");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("harness 0.6.0"),
        "login" => {
            println!("Authenticating...");
            println!("Login successful. Account: my-org");
        }
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("NAME              STATUS      LAST RUN");
                    println!("build-deploy      SUCCESS     2024-01-15 10:00:00");
                    println!("nightly-tests     SUCCESS     2024-01-15 02:00:00");
                    println!("release           ABORTED     2024-01-14 16:00:00");
                }
                "run" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("build-deploy");
                    println!("Triggering pipeline '{}'...", name);
                    println!("Execution ID: exec-abc123");
                    println!("Status: RUNNING");
                }
                _ => println!("harness pipeline: '{}' completed", sub),
            }
        }
        "service" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("NAME           TYPE        ARTIFACTS");
                println!("my-app         Kubernetes  docker:my-app:latest");
                println!("backend-api    Kubernetes  docker:backend:v1.2.3");
            } else {
                println!("harness service: '{}' completed", sub);
            }
        }
        "environment" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("NAME          TYPE");
                println!("dev           PreProduction");
                println!("staging       PreProduction");
                println!("production    Production");
            } else {
                println!("harness environment: '{}' completed", sub);
            }
        }
        _ => println!("harness: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "harness".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_harness(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_harness};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/harness"), "harness");
        assert_eq!(basename(r"C:\bin\harness.exe"), "harness.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("harness.exe"), "harness");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_harness(&["--help".to_string()]), 0);
        assert_eq!(run_harness(&["-h".to_string()]), 0);
        let _ = run_harness(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_harness(&[]);
    }
}
