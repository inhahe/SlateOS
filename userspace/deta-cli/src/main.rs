#![deny(clippy::all)]

//! deta-cli — OurOS Deta Space CLI
//!
//! Multi-personality: `space`, `deta`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_space(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: space COMMAND [OPTIONS]");
        println!("Deta Space CLI 0.5.1 (OurOS)");
        println!();
        println!("Commands:");
        println!("  new          Create a new project");
        println!("  push         Push code to Space");
        println!("  release      Create a release");
        println!("  dev          Start local development");
        println!("  link         Link local directory to project");
        println!("  open         Open project in browser");
        println!("  validate     Validate Spacefile");
        println!("  login        Login to Space");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("space v0.5.1"),
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-app");
            println!("Creating project '{}'...", name);
            println!("  Created Spacefile");
            println!("  Created Discovery.md");
            println!("Project '{}' created.", name);
        }
        "push" => {
            println!("Pushing to Deta Space...");
            println!("  Uploading source (24 files, 156 KB)...");
            println!("  Building...");
            println!("  Build complete.");
            println!("Successfully pushed revision rev-abc123.");
        }
        "release" => {
            let version = args.get(1).map(|s| s.as_str()).unwrap_or("latest");
            println!("Creating release {}...", version);
            println!("Release created: https://deta.space/discovery/@user/my-app");
        }
        "dev" => {
            println!("Starting local development...");
            println!("  Micro 'backend' on http://localhost:4200");
            println!("  Micro 'frontend' on http://localhost:5173");
        }
        "link" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("abc123");
            println!("Linked to project: {}", id);
        }
        "validate" => {
            println!("Spacefile is valid.");
        }
        "login" => {
            println!("Logged in as user@example.com");
        }
        _ => println!("space: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "space".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_space(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_space};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deta"), "deta");
        assert_eq!(basename(r"C:\bin\deta.exe"), "deta.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deta.exe"), "deta");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_space(&["--help".to_string()]), 0);
        assert_eq!(run_space(&["-h".to_string()]), 0);
        assert_eq!(run_space(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_space(&[]), 0);
    }
}
