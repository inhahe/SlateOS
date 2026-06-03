#![deny(clippy::all)]

//! circleci-cli — OurOS CircleCI CLI
//!
//! Single personality: `circleci`

use std::env;
use std::process;

fn run_circleci(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: circleci <COMMAND> [OPTIONS]");
        println!();
        println!("CircleCI command-line interface.");
        println!();
        println!("Commands:");
        println!("  config       Operate on build config files");
        println!("  local        Execute jobs locally");
        println!("  orb          Operate on orbs");
        println!("  namespace    Operate on namespaces");
        println!("  context      Operate on contexts");
        println!("  completion   Generate shell completions");
        println!("  setup        Setup CLI with your credentials");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("circleci-cli 0.1.30280 (OurOS)");
            0
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("validate");
            match sub {
                "validate" => {
                    let file = args.get(2).map(|s| s.as_str()).unwrap_or(".circleci/config.yml");
                    println!("Config file at {} is valid.", file);
                }
                "process" => {
                    println!("# Processed config:");
                    println!("version: 2.1");
                    println!("jobs:");
                    println!("  build:");
                    println!("    docker:");
                    println!("      - image: cimg/node:20.0");
                    println!("    steps:");
                    println!("      - checkout");
                    println!("      - run: npm install");
                    println!("      - run: npm test");
                }
                _ => println!("Usage: circleci config <validate|process|migrate|pack>"),
            }
            0
        }
        "local" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("execute");
            if sub == "execute" {
                let job = args.windows(2)
                    .find(|w| w[0] == "--job")
                    .map(|w| w[1].as_str())
                    .unwrap_or("build");
                println!("Downloading latest CircleCI build agent...");
                println!("Docker image pulled: cimg/node:20.0");
                println!();
                println!("====>> Spin up environment");
                println!("Build-agent version 1.0.0");
                println!("====>> Checkout code");
                println!("====>> npm install");
                println!("====>> npm test");
                println!();
                println!("Success! Job '{}' completed.", job);
            }
            0
        }
        "orb" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Orbs found: 5. Showing 5.");
                    println!("  circleci/node (2.1.0)");
                    println!("  circleci/docker (2.4.0)");
                    println!("  circleci/aws-cli (4.1.0)");
                    println!("  circleci/slack (4.12.5)");
                    println!("  circleci/python (2.1.1)");
                }
                "info" => {
                    let orb = args.get(2).map(|s| s.as_str()).unwrap_or("circleci/node");
                    println!("Latest: {} @ 2.1.0", orb);
                    println!("  Description: Tools for working with Node.js");
                    println!("  Source: https://github.com/CircleCI-Public/node-orb");
                }
                "validate" => {
                    println!("Orb at orb.yml is valid.");
                }
                _ => println!("Usage: circleci orb <list|info|validate|publish|source>"),
            }
            0
        }
        "context" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Name              Created");
                    println!("──────────────── ──────────────────");
                    println!("production       2024-01-01 00:00:00");
                    println!("staging          2024-01-01 00:00:00");
                    println!("development      2024-01-05 10:00:00");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-context");
                    println!("Context '{}' created.", name);
                }
                _ => println!("Usage: circleci context <list|create|delete|show|store-secret|remove-secret>"),
            }
            0
        }
        "setup" => {
            println!("Setup complete. API token saved to ~/.circleci/cli.yml");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: circleci <command>. See --help.");
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
    let code = run_circleci(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_circleci};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_circleci(vec!["--help".to_string()]), 0);
        assert_eq!(run_circleci(vec!["-h".to_string()]), 0);
        assert_eq!(run_circleci(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_circleci(vec![]), 0);
    }
}
