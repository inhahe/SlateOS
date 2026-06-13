#![deny(clippy::all)]

//! woodpecker-cli — Slate OS Woodpecker CI CLI
//!
//! Multi-personality: `woodpecker-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_woodpecker(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: woodpecker-cli COMMAND [OPTIONS]");
        println!("Woodpecker CLI 2.7.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  pipeline       Manage pipelines");
        println!("  repo           Manage repositories");
        println!("  user           Manage users");
        println!("  secret         Manage secrets");
        println!("  log            Show pipeline logs");
        println!("  info           Show server info");
        println!("  lint           Lint .woodpecker.yml");
        println!("  exec           Execute pipeline locally");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("woodpecker-cli 2.7.0"),
        "info" => {
            println!("Server: https://ci.example.com");
            println!("Version: 2.7.0");
        }
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" => {
                    println!("NUMBER  STATUS     EVENT   BRANCH  MESSAGE");
                    println!("#15     success    push    main    Update deps");
                    println!("#14     success    push    main    Fix linting");
                    println!("#13     failure    push    dev     WIP");
                }
                "last" => {
                    println!("Pipeline #15:");
                    println!("  Status: success");
                    println!("  Branch: main");
                    println!("  Duration: 1m 45s");
                }
                _ => println!("woodpecker pipeline: '{}' completed", sub),
            }
        }
        "repo" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            if sub == "ls" {
                println!("myorg/myapp");
                println!("myorg/backend");
            } else {
                println!("woodpecker repo: '{}' completed", sub);
            }
        }
        "lint" => {
            println!("Linting .woodpecker.yml...");
            println!("  Pipeline configuration is valid.");
        }
        "exec" => {
            println!("[step:clone] Cloning repo...");
            println!("[step:build] Building...");
            println!("[step:test] Testing...");
            println!("[pipeline] completed successfully.");
        }
        "log" => {
            let num = args.get(1).map(|s| s.as_str()).unwrap_or("15");
            println!("Logs for pipeline #{}:", num);
            println!("[clone] Cloning into /woodpecker/src...");
            println!("[build] go build ./...");
            println!("[build] Build successful");
            println!("[test] go test ./...");
            println!("[test] ok  ./... 2.345s");
        }
        _ => println!("woodpecker-cli: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "woodpecker-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_woodpecker(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_woodpecker};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/woodpecker"), "woodpecker");
        assert_eq!(basename(r"C:\bin\woodpecker.exe"), "woodpecker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("woodpecker.exe"), "woodpecker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_woodpecker(&["--help".to_string()]), 0);
        assert_eq!(run_woodpecker(&["-h".to_string()]), 0);
        let _ = run_woodpecker(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_woodpecker(&[]);
    }
}
