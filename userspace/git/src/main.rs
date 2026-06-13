#![deny(clippy::all)]

//! git — Slate OS distributed version control system
//!
//! Single personality: `git`

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _GIT_DIR: &str = ".git";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct RepoStatus {
    branch: String,
    _head: String,
    staged: Vec<String>,
    modified: Vec<String>,
    untracked: Vec<String>,
    _ahead: u32,
    _behind: u32,
}

fn sample_status() -> RepoStatus {
    RepoStatus {
        branch: "main".to_string(),
        _head: "abc1234".to_string(),
        staged: vec!["src/main.rs".to_string()],
        modified: vec!["README.md".to_string(), "Cargo.toml".to_string()],
        untracked: vec!["todo.txt".to_string()],
        _ahead: 2,
        _behind: 0,
    }
}

#[derive(Clone, Debug)]
struct LogEntry {
    hash: String,
    _author: String,
    date: String,
    message: String,
}

fn sample_log() -> Vec<LogEntry> {
    vec![
        LogEntry { hash: "abc1234".to_string(), _author: "Dev <dev@slateos.local>".to_string(),
            date: "2025-05-22".to_string(), message: "Add feature X".to_string() },
        LogEntry { hash: "def5678".to_string(), _author: "Dev <dev@slateos.local>".to_string(),
            date: "2025-05-21".to_string(), message: "Fix bug in parser".to_string() },
        LogEntry { hash: "ghi9012".to_string(), _author: "Dev <dev@slateos.local>".to_string(),
            date: "2025-05-20".to_string(), message: "Initial commit".to_string() },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_git(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("usage: git <command> [<args>]");
            println!();
            println!("These are common Git commands:");
            println!();
            println!("start a working area:");
            println!("   clone    Clone a repository");
            println!("   init     Create an empty Git repository");
            println!();
            println!("work on the current change:");
            println!("   add      Add file contents to the index");
            println!("   rm       Remove files from the working tree and index");
            println!("   mv       Move or rename a file/directory");
            println!("   restore  Restore working tree files");
            println!();
            println!("examine the history and state:");
            println!("   status   Show the working tree status");
            println!("   log      Show commit logs");
            println!("   diff     Show changes between commits");
            println!("   show     Show various types of objects");
            println!();
            println!("grow, mark and tweak your common history:");
            println!("   branch   List, create, or delete branches");
            println!("   commit   Record changes to the repository");
            println!("   merge    Join two or more development histories");
            println!("   rebase   Reapply commits on top of another base");
            println!("   tag      Create, list, delete tags");
            println!();
            println!("collaborate:");
            println!("   fetch    Download objects and refs from another repository");
            println!("   pull     Fetch from and integrate with a remote branch");
            println!("   push     Update remote refs");
            println!("   remote   Manage set of tracked repositories");
            0
        }
        "--version" => { println!("git version 0.1.0 (Slate OS)"); 0 }
        "init" => { println!("Initialized empty Git repository in .git/ (simulated)"); 0 }
        "clone" => {
            let url = cmd_args.first().map(|s| s.as_str()).unwrap_or("https://example.com/repo.git");
            println!("Cloning into '{}'...", url.rsplit('/').next().unwrap_or("repo").trim_end_matches(".git"));
            println!("remote: Enumerating objects: 150, done.");
            println!("remote: Counting objects: 100%");
            println!("remote: Compressing objects: 100%");
            println!("Receiving objects: 100%, 2.50 MiB | 5.00 MiB/s, done.");
            println!("Resolving deltas: 100%");
            0
        }
        "status" => {
            let s = sample_status();
            println!("On branch {}", s.branch);
            if !s.staged.is_empty() {
                println!("Changes to be committed:");
                for f in &s.staged { println!("\tmodified:   {}", f); }
            }
            if !s.modified.is_empty() {
                println!();
                println!("Changes not staged for commit:");
                for f in &s.modified { println!("\tmodified:   {}", f); }
            }
            if !s.untracked.is_empty() {
                println!();
                println!("Untracked files:");
                for f in &s.untracked { println!("\t{}", f); }
            }
            0
        }
        "log" => {
            let oneline = cmd_args.iter().any(|a| a == "--oneline");
            let entries = sample_log();
            if oneline {
                for e in &entries {
                    println!("{} {}", e.hash, e.message);
                }
            } else {
                for e in &entries {
                    println!("commit {}", e.hash);
                    println!("Author: {}", e._author);
                    println!("Date:   {}", e.date);
                    println!();
                    println!("    {}", e.message);
                    println!();
                }
            }
            0
        }
        "diff" => {
            println!("diff --git a/README.md b/README.md");
            println!("index 1234567..abcdefg 100644");
            println!("--- a/README.md");
            println!("+++ b/README.md");
            println!("@@ -1,3 +1,4 @@");
            println!(" # Project");
            println!("+## New Section");
            println!(" Description here.");
            0
        }
        "add" => {
            let files: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
            if files.is_empty() || files.contains(&"-h") {
                println!("usage: git add <pathspec>...");
            } else {
                for f in &files { println!("add '{}'", f); }
            }
            0
        }
        "commit" => {
            let msg = cmd_args.iter().position(|a| a == "-m")
                .and_then(|i| cmd_args.get(i + 1));
            match msg {
                Some(m) => {
                    println!("[main abc1234] {}", m);
                    println!(" 1 file changed, 10 insertions(+), 2 deletions(-)");
                }
                None => println!("Aborting commit due to empty commit message (simulated)."),
            }
            0
        }
        "branch" => {
            if cmd_args.is_empty() {
                println!("* main");
                println!("  feature/new-ui");
                println!("  bugfix/parser");
            } else {
                let name = &cmd_args[0];
                if cmd_args.iter().any(|a| a == "-d" || a == "-D") {
                    println!("Deleted branch {} (was abc1234).", name);
                } else {
                    println!("Created branch '{}'", name);
                }
            }
            0
        }
        "checkout" | "switch" => {
            let target = cmd_args.first().map(|s| s.as_str()).unwrap_or("main");
            if cmd_args.iter().any(|a| a == "-b") {
                println!("Switched to a new branch '{}'", target);
            } else {
                println!("Switched to branch '{}'", target);
            }
            0
        }
        "merge" => {
            let branch = cmd_args.first().map(|s| s.as_str()).unwrap_or("feature");
            println!("Merge made by the 'ort' strategy.");
            println!(" src/main.rs | 10 +++++++---");
            println!(" 1 file changed, 7 insertions(+), 3 deletions(-)");
            println!("(merged {} into current branch, simulated)", branch);
            0
        }
        "remote" => {
            let subcmd = cmd_args.first().map(|s| s.as_str()).unwrap_or("show");
            match subcmd {
                "-v" | "show" => {
                    println!("origin\thttps://github.com/user/repo.git (fetch)");
                    println!("origin\thttps://github.com/user/repo.git (push)");
                }
                "add" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("origin");
                    let url = cmd_args.get(2).map(|s| s.as_str()).unwrap_or("https://example.com/repo.git");
                    println!("Added remote '{}' → {}", name, url);
                }
                _ => println!("origin"),
            }
            0
        }
        "push" | "pull" | "fetch" => {
            let remote = cmd_args.first().map(|s| s.as_str()).unwrap_or("origin");
            println!("{}: from {} (simulated)", cmd, remote);
            if cmd == "push" { println!("To {}", remote); println!("   abc1234..def5678  main -> main"); }
            0
        }
        "tag" => {
            if cmd_args.is_empty() {
                println!("v0.1.0");
                println!("v0.2.0");
            } else {
                println!("Created tag '{}'", cmd_args[0]);
            }
            0
        }
        "stash" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("push");
            match sub {
                "push" | "save" => println!("Saved working directory (simulated)"),
                "pop" | "apply" => println!("Applied stash@{{0}} (simulated)"),
                "list" => println!("stash@{{0}}: WIP on main: abc1234 Add feature X"),
                _ => println!("stash: {} (simulated)", sub),
            }
            0
        }
        "show" => { println!("commit abc1234"); println!("Author: Dev"); println!("    Add feature X"); 0 }
        "rebase" => { println!("rebase: rebasing onto {} (simulated)", cmd_args.first().map(|s| s.as_str()).unwrap_or("main")); 0 }
        other => { eprintln!("git: '{}' is not a git command.", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_git(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status() {
        let s = sample_status();
        assert_eq!(s.branch, "main");
        assert!(!s.staged.is_empty());
    }

    #[test]
    fn test_log() {
        let log = sample_log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].hash, "abc1234");
    }
}
