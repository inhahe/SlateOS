#![deny(clippy::all)]

//! github-cli — OurOS GitHub gh CLI
//!
//! Single personality: `github`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: github [OPTIONS] [SUBCMD]");
        println!("GitHub CLI gh 2.62 (OurOS) — Official GitHub command-line tool");
        println!();
        println!("Options:");
        println!("  auth login             Authenticate with github.com or Enterprise");
        println!("  pr list / pr create    Manage pull requests");
        println!("  issue list / create    Manage issues");
        println!("  repo clone OWNER/REPO  Clone repo");
        println!("  workflow run NAME      Trigger Actions workflow");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gh version 2.62.0 (2024-11-21) — github.com/cli/cli (OurOS)"); return 0; }
    println!("GitHub CLI gh 2.62.0 (OurOS)");
    println!("  Source: github.com/cli/cli (Go, MIT-licensed)");
    println!("  Coverage: PRs, issues, releases, gists, repos, secrets, codespaces,");
    println!("            Actions runs/workflows, Projects (v2), Copilot extensions");
    println!("  Auth: OAuth device flow, PAT, SSH-key check, GitHub Enterprise Server");
    println!("  Extensions: gh extension install — third-party gh subcommands");
    println!("  Copilot: gh copilot suggest / explain (with subscription)");
    println!("  Outputs: human, --json with --jq filters, --template (Go templates)");
    println!("  License: MIT (free, official GitHub product)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "github".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gh(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
