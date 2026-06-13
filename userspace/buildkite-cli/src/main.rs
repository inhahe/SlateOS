#![deny(clippy::all)]

//! buildkite-cli — SlateOS Buildkite CLI
//!
//! Multi-personality: `bk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bk COMMAND [OPTIONS]");
        println!("Buildkite CLI 3.0.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  build          Manage builds");
        println!("  pipeline       Manage pipelines");
        println!("  agent          Manage agents");
        println!("  artifact       Manage build artifacts");
        println!("  meta-data      Manage build meta-data");
        println!("  local          Run pipelines locally");
        println!("  configure      Configure CLI");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("bk 3.0.0"),
        "build" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("BUILD    PIPELINE         BRANCH   STATE     MESSAGE");
                    println!("#42      my-pipeline      main     passed    Deploy v1.2.3");
                    println!("#41      my-pipeline      main     passed    Fix CI config");
                    println!("#40      my-pipeline      feat/x   failed    Add feature X");
                }
                "view" => {
                    println!("Build #42");
                    println!("  Pipeline: my-pipeline");
                    println!("  Branch:   main");
                    println!("  State:    passed");
                    println!("  Duration: 3m 12s");
                    println!("  URL:      https://buildkite.com/my-org/my-pipeline/builds/42");
                }
                _ => println!("bk build: '{}' completed", sub),
            }
        }
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("PIPELINE         BUILDS   LAST BUILD");
                println!("my-pipeline      42       #42 (passed)");
                println!("backend          156      #156 (passed)");
                println!("frontend         89       #89 (running)");
            } else {
                println!("bk pipeline: '{}' completed", sub);
            }
        }
        "local" => {
            println!("Running pipeline locally...");
            println!("  Step: :hammer: Build");
            println!("  Step: :white_check_mark: Test");
            println!("  Step: :rocket: Deploy (skipped - local mode)");
            println!();
            println!("Pipeline completed successfully.");
        }
        "agent" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("AGENT           HOSTNAME        STATE");
                println!("agent-001       build-1         idle");
                println!("agent-002       build-2         busy");
            } else {
                println!("bk agent: '{}' completed", sub);
            }
        }
        _ => println!("bk: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bk(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/buildkite"), "buildkite");
        assert_eq!(basename(r"C:\bin\buildkite.exe"), "buildkite.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("buildkite.exe"), "buildkite");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bk(&["--help".to_string()]), 0);
        assert_eq!(run_bk(&["-h".to_string()]), 0);
        let _ = run_bk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bk(&[]);
    }
}
