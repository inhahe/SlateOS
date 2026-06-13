#![deny(clippy::all)]

//! pass-cli — Slate OS pass (password-store) CLI
//!
//! Single personality: `pass`

use std::env;
use std::process;

fn run_pass(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pass <COMMAND> [OPTIONS]");
        println!();
        println!("pass — the standard Unix password manager (Slate OS).");
        println!();
        println!("Commands:");
        println!("  init         Initialize password store");
        println!("  ls           List passwords");
        println!("  show         Show a password");
        println!("  insert       Insert a password");
        println!("  edit         Edit a password");
        println!("  generate     Generate a password");
        println!("  rm           Remove a password");
        println!("  mv           Move a password");
        println!("  cp           Copy a password");
        println!("  find         Find passwords by name");
        println!("  grep         Search password content");
        println!("  git          Git operations");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pass 1.7.4 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "init" => {
            let gpg_id = args.get(1).map(|s| s.as_str()).unwrap_or("user@example.com");
            println!("Password store initialized for {}", gpg_id);
            println!("  Created /home/user/.password-store/.gpg-id");
            0
        }
        "ls" | "list" => {
            println!("Password Store");
            println!("├── email");
            println!("│   ├── personal");
            println!("│   └── work");
            println!("├── social");
            println!("│   ├── github");
            println!("│   ├── twitter");
            println!("│   └── mastodon");
            println!("├── servers");
            println!("│   ├── production");
            println!("│   └── staging");
            println!("└── banking");
            println!("    └── main-account");
            0
        }
        "show" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("email/work");
            let clip = args.iter().any(|a| a == "-c" || a == "--clip");
            if clip {
                println!("Copied {}/password to clipboard. Will clear in 45 seconds.", name);
            } else {
                println!("s3cur3-p4ssw0rd-xyz!");
                println!("---");
                println!("user: user@work.com");
                println!("url: https://mail.work.com");
                println!("notes: Work email account");
            }
            0
        }
        "insert" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("new/entry");
            let multiline = args.iter().any(|a| a == "-m" || a == "--multiline");
            if multiline {
                println!("Enter contents of {} and press Ctrl+D when finished:", name);
            } else {
                println!("Enter password for {}:", name);
                println!("Retype password for {}:", name);
            }
            println!("[master abc123de] Added {} to store.", name);
            0
        }
        "generate" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("generated/new");
            let length = args.get(2).map(|s| s.as_str()).unwrap_or("25");
            let no_symbols = args.iter().any(|a| a == "-n" || a == "--no-symbols");
            let clip = args.iter().any(|a| a == "-c" || a == "--clip");
            println!("The generated password for {} is:", name);
            if no_symbols {
                println!("Kj8mN2pQ5rT7wX9yB3dF4gH6");
            } else {
                println!("Kj8$mN2@pQ5!rT7&wX9*yB3%d");
            }
            println!("  Length: {}", length);
            if clip {
                println!("  Copied to clipboard. Will clear in 45 seconds.");
            }
            0
        }
        "rm" | "remove" | "delete" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("old/entry");
            println!("Are you sure you would like to delete {}? [y/N] y", name);
            println!("removed '{}'", name);
            0
        }
        "mv" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("old/path");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("new/path");
            println!("'{}' => '{}'", src, dst);
            0
        }
        "cp" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("original");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("copy");
            println!("'{}' => '{}'", src, dst);
            0
        }
        "find" | "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("email");
            println!("Search Terms: {}", term);
            println!("├── email/personal");
            println!("└── email/work");
            0
        }
        "grep" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("user");
            println!("Searching for '{}' in passwords...", pattern);
            println!("email/work:");
            println!("  user: user@work.com");
            println!("social/github:");
            println!("  user: myuser");
            0
        }
        "git" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("log");
            match sub {
                "init" => {
                    println!("Initialized git repository in /home/user/.password-store/");
                }
                "push" => {
                    println!("Pushing password store to origin...");
                    println!("  Everything up-to-date");
                }
                "pull" => {
                    println!("Pulling password store from origin...");
                    println!("  Already up to date.");
                }
                "log" => {
                    println!("abc123d Added new/entry to store.");
                    println!("def456g Edited email/work.");
                    println!("ghi789j Added social/mastodon to store.");
                }
                _ => { println!("git {}", sub); }
            }
            0
        }
        _ => {
            // Bare argument is treated as "show"
            println!("(showing password for '{}')", cmd);
            println!("p4ssw0rd-placeholder");
            0
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pass(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pass};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pass(vec!["--help".to_string()]), 0);
        assert_eq!(run_pass(vec!["-h".to_string()]), 0);
        let _ = run_pass(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pass(vec![]);
    }
}
