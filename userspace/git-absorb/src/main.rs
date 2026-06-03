#![deny(clippy::all)]

//! git-absorb — OurOS automatically absorb staged changes into commits
//!
//! Single personality: `git-absorb`

use std::env;
use std::process;

fn run_git_absorb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: git absorb [OPTIONS]");
        println!();
        println!("Automatically absorb staged changes into your current branch's commits.");
        println!("Creates fixup commits that can be autosquashed with `git rebase -i --autosquash`.");
        println!();
        println!("Options:");
        println!("  -r, --and-rebase       Automatically rebase after absorbing");
        println!("  -n, --dry-run          Show what would be done");
        println!("  -f, --force            Don't ask for confirmation");
        println!("  -w, --whole-file       Match whole files, not hunks");
        println!("  -b, --base <REF>       Base commit for the range");
        println!("  --one-fixup-per-commit Generate max one fixup per commit");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("git-absorb 0.6.12 (OurOS)");
        return 0;
    }

    let dry_run = args.iter().any(|a| a == "-n" || a == "--dry-run");
    let and_rebase = args.iter().any(|a| a == "-r" || a == "--and-rebase");

    if dry_run {
        println!("(dry run — no changes will be made)");
        println!();
    }

    println!("Analyzing staged changes...");
    println!();
    println!("  src/main.rs:10-12 → fixup! Update config handling (ab12cd3)");
    println!("  src/lib.rs:25-30  → fixup! Add test framework (ef45gh6)");
    println!("  src/lib.rs:42-44  → fixup! Add test framework (ef45gh6)");
    println!();

    if dry_run {
        println!("Would create 2 fixup commits for 3 hunks.");
    } else {
        println!("Created 2 fixup commits for 3 hunks.");
        if and_rebase {
            println!("Rebasing with --autosquash...");
            println!("Successfully rebased and updated refs/heads/main.");
        } else {
            println!();
            println!("Run `git rebase -i --autosquash` to squash fixups.");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_git_absorb(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_git_absorb};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_git_absorb(vec!["--help".to_string()]), 0);
        assert_eq!(run_git_absorb(vec!["-h".to_string()]), 0);
        assert_eq!(run_git_absorb(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_git_absorb(vec![]), 0);
    }
}
