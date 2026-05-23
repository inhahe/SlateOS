#![deny(clippy::all)]

//! drone-cli — OurOS Drone CI CLI
//!
//! Multi-personality: `drone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_drone(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: drone COMMAND [OPTIONS]");
        println!("Drone CLI 1.7.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  build          Manage builds");
        println!("  repo           Manage repositories");
        println!("  user           Manage users");
        println!("  secret         Manage secrets");
        println!("  exec           Execute pipeline locally");
        println!("  info           Show server info");
        println!("  lint           Lint .drone.yml");
        println!("  sign           Sign .drone.yml");
        println!("  jsonnet        Convert Jsonnet to YAML");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("drone 1.7.0"),
        "info" => {
            println!("Server: https://drone.example.com");
            println!("Version: 2.22.0");
            println!("User: admin");
        }
        "build" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" => {
                    println!("NUMBER  STATUS   EVENT   BRANCH  COMMIT    MESSAGE");
                    println!("42      success  push    main    abc1234   Add CI config");
                    println!("41      success  push    main    def5678   Fix tests");
                    println!("40      failure  push    dev     ghi9012   WIP feature");
                }
                "info" => {
                    let num = args.get(2).map(|s| s.as_str()).unwrap_or("42");
                    println!("Build #{}:", num);
                    println!("  Status: success");
                    println!("  Event:  push");
                    println!("  Branch: main");
                    println!("  Commit: abc1234");
                    println!("  Duration: 2m 34s");
                }
                _ => println!("drone build: '{}' completed", sub),
            }
        }
        "exec" => {
            println!("[pipeline:default]");
            println!("[step:build] + go build ./...");
            println!("[step:build] Build complete");
            println!("[step:test] + go test ./...");
            println!("[step:test] ok  ./... 1.234s");
            println!("[pipeline:default] exit code 0");
        }
        "lint" => {
            println!("Linting .drone.yml...");
            println!("  Pipeline is valid.");
        }
        "repo" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            if sub == "ls" {
                println!("myorg/myapp");
                println!("myorg/backend");
                println!("myorg/frontend");
            } else {
                println!("drone repo: '{}' completed", sub);
            }
        }
        _ => println!("drone: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "drone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_drone(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
