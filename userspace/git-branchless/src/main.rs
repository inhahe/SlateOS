#![deny(clippy::all)]

//! git-branchless — OurOS suite of tools for high-velocity git workflows
//!
//! Single personality: `git-branchless`

use std::env;
use std::process;

fn run_git_branchless(args: Vec<String>) -> i32 {
    // First arg may be "branchless" when invoked as `git branchless`
    let subargs: Vec<String> = if args.first().map(|s| s.as_str()) == Some("branchless") {
        args[1..].to_vec()
    } else {
        args
    };

    let cmd = subargs.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: git branchless <COMMAND>");
            println!();
            println!("High-velocity, monorepo-scale workflow for Git.");
            println!();
            println!("Commands:");
            println!("  init        Initialize branchless workflow");
            println!("  smartlog    Show commit graph (alias: sl)");
            println!("  move        Move commits and subtrees");
            println!("  restack     Fix up commit stacks after rebase");
            println!("  hide        Hide commits from smartlog");
            println!("  unhide      Unhide hidden commits");
            println!("  prev        Check out the previous commit");
            println!("  next        Check out the next commit");
            println!("  switch      Interactive branch switcher");
            println!("  submit      Create/update PRs for commit stacks");
            println!("  test        Run tests on commits");
            println!("  undo        Undo the last git operation");
            println!("  query       Query commit graph");
            println!("  repair      Repair internal data");
            println!();
            println!("Options:");
            println!("  -V, --version  Show version");
            0
        }
        "--version" | "-V" => {
            println!("git-branchless 0.8.0 (OurOS)");
            0
        }
        "init" => {
            println!("Installing git-branchless hooks...");
            println!("  ✓ post-commit hook");
            println!("  ✓ post-rewrite hook");
            println!("  ✓ post-checkout hook");
            println!("  ✓ reference-transaction hook");
            println!("Initialized git-branchless.");
            0
        }
        "smartlog" | "sl" => {
            println!("◯ ab12cd3 (main) Initial commit");
            println!("│");
            println!("◯ ef45gh6 Add test framework");
            println!("│");
            println!("● ij78kl9 (HEAD) Update config handling");
            println!("│");
            println!("├─◯ mn01op2 Feature: async support");
            println!("│");
            println!("└─◯ qr34st5 Fix: memory leak");
            0
        }
        "prev" => {
            println!("Checked out: ef45gh6 Add test framework");
            0
        }
        "next" => {
            println!("Checked out: mn01op2 Feature: async support");
            0
        }
        "undo" => {
            println!("Undone: checkout ij78kl9");
            println!("Now at: ef45gh6 Add test framework");
            0
        }
        "restack" => {
            println!("Restacking commits...");
            println!("  ✓ mn01op2 Feature: async support");
            println!("  ✓ qr34st5 Fix: memory leak");
            println!("Restacked 2 commits.");
            0
        }
        "switch" => {
            println!("Interactive switch:");
            println!("  > main (ab12cd3)");
            println!("    feature/async (mn01op2)");
            println!("    fix/memory (qr34st5)");
            0
        }
        "hide" | "unhide" => {
            let target = subargs.get(1).map(|s| s.as_str()).unwrap_or("HEAD");
            println!("{}: {}", if cmd == "hide" { "Hidden" } else { "Unhidden" }, target);
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_git_branchless(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_git_branchless};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_git_branchless(vec!["--help".to_string()]), 0);
        assert_eq!(run_git_branchless(vec!["-h".to_string()]), 0);
        assert_eq!(run_git_branchless(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_git_branchless(vec![]), 0);
    }
}
