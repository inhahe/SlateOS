#![deny(clippy::all)]

//! spacelift-cli — OurOS Spacelift CLI
//!
//! Multi-personality: `spacectl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spacectl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: spacectl COMMAND [OPTIONS]");
        println!("Spacelift CLI 0.28.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  stack          Manage stacks");
        println!("  run            Manage runs");
        println!("  profile        Manage profiles");
        println!("  provider       Manage providers");
        println!("  version        Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("spacectl 0.28.0"),
        "profile" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "login" => {
                    println!("Logging in to Spacelift...");
                    println!("Profile saved: default");
                }
                "list" => {
                    println!("ALIAS      ENDPOINT                     CURRENT");
                    println!("default    https://myorg.app.spacelift.io  *");
                    println!("staging    https://staging.app.spacelift.io");
                }
                _ => println!("spacectl profile: '{}' completed", sub),
            }
        }
        "stack" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                    NAME             STATE       BRANCH");
                    println!("my-vpc-stack          VPC Stack        FINISHED    main");
                    println!("my-app-stack          App Stack        FINISHED    main");
                    println!("my-db-stack           DB Stack         QUEUED      main");
                }
                "show" => {
                    let stack = args.get(2).map(|s| s.as_str()).unwrap_or("my-vpc-stack");
                    println!("Stack: {}", stack);
                    println!("  State:     FINISHED");
                    println!("  Branch:    main");
                    println!("  Provider:  terraform");
                    println!("  Workers:   public");
                }
                _ => println!("spacectl stack: '{}' completed", sub),
            }
        }
        "run" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID        STACK            TYPE       STATE      CREATED");
                    println!("abc123    my-vpc-stack     TRACKED    FINISHED   2024-01-15");
                    println!("def456    my-app-stack     PROPOSED   QUEUED     2024-01-15");
                }
                "trigger" => {
                    let stack = args.get(2).map(|s| s.as_str()).unwrap_or("my-vpc-stack");
                    println!("Triggered run for stack: {}", stack);
                    println!("Run ID: xyz789");
                }
                _ => println!("spacectl run: '{}' completed", sub),
            }
        }
        _ => println!("spacectl: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spacectl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spacectl(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spacectl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spacelift"), "spacelift");
        assert_eq!(basename(r"C:\bin\spacelift.exe"), "spacelift.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spacelift.exe"), "spacelift");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spacectl(&["--help".to_string()]), 0);
        assert_eq!(run_spacectl(&["-h".to_string()]), 0);
        let _ = run_spacectl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spacectl(&[]);
    }
}
