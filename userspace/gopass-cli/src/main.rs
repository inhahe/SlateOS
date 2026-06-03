#![deny(clippy::all)]

//! gopass-cli — OurOS gopass CLI
//!
//! Single personality: `gopass`

use std::env;
use std::process;

fn run_gopass(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gopass <COMMAND> [OPTIONS]");
        println!();
        println!("gopass — the team password manager (OurOS).");
        println!();
        println!("Commands:");
        println!("  init         Initialize store");
        println!("  ls           List entries");
        println!("  show         Show entry");
        println!("  insert       Insert new entry");
        println!("  edit         Edit entry");
        println!("  generate     Generate password");
        println!("  rm           Remove entry");
        println!("  mv           Move entry");
        println!("  cp           Copy entry");
        println!("  find         Search by name");
        println!("  grep         Search by content");
        println!("  sync         Sync all stores");
        println!("  clone        Clone a store");
        println!("  mounts       Manage mounted stores");
        println!("  recipients   Manage recipients");
        println!("  otp          One-time passwords");
        println!("  audit        Audit passwords");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gopass 1.15.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "init" => {
            println!("Initializing gopass store...");
            println!("  GPG key: user@example.com");
            println!("  Store initialized at /home/user/.local/share/gopass/stores/root");
            println!("  Git repository initialized");
            0
        }
        "ls" | "list" => {
            println!("gopass");
            println!("├── email/");
            println!("│   ├── personal");
            println!("│   └── work");
            println!("├── social/");
            println!("│   ├── github");
            println!("│   └── mastodon");
            println!("├── servers/");
            println!("│   ├── production");
            println!("│   └── staging");
            println!("└── team/ (mounted)");
            println!("    ├── shared-api-key");
            println!("    └── deploy-token");
            0
        }
        "show" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("email/work");
            let clip = args.iter().any(|a| a == "-c" || a == "--clip");
            let revision = args.windows(2).find(|w| w[0] == "--revision")
                .map(|w| w[1].as_str());
            if clip {
                println!("Copied {}/password to clipboard. Will clear in 45 seconds.", name);
            } else {
                if let Some(rev) = revision {
                    println!("(revision: {})", rev);
                }
                println!("Secret: s3cur3-p4ssw0rd-xyz!");
                println!("---");
                println!("user: user@work.com");
                println!("url: https://mail.work.com");
                println!("comment: Work email account");
            }
            0
        }
        "generate" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("new/entry");
            let length = args.get(2).map(|s| s.as_str()).unwrap_or("24");
            println!("Generated password for {}:", name);
            println!("  Xk9$mN2@pQ5!rT7&wX9*yB3");
            println!("  Length: {}", length);
            println!("  Strength: very strong");
            0
        }
        "sync" => {
            println!("Syncing all stores...");
            println!("  root: git pull && push... done");
            println!("  team: git pull && push... done");
            println!("All stores synced.");
            0
        }
        "clone" => {
            let url = args.get(1).map(|s| s.as_str()).unwrap_or("git@github.com:team/pass-store.git");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("team");
            println!("Cloning {} as '{}' ...", url, name);
            println!("  Mounted at '{}'", name);
            println!("  12 entries found");
            0
        }
        "mounts" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  Mount Point    Path                                    ");
                    println!("  team/          /home/user/.local/share/gopass/stores/team");
                }
                "add" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("shared");
                    println!("Mounted store as '{}'", name);
                }
                "remove" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("old-mount");
                    println!("Unmounted '{}'", name);
                }
                _ => { println!("Mount operation: {}", sub); }
            }
            0
        }
        "recipients" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Recipients for root store:");
                    println!("  0xABC123DEF456GHI7 - User Name <user@example.com>");
                    println!("Recipients for team mount:");
                    println!("  0xABC123DEF456GHI7 - User Name <user@example.com>");
                    println!("  0x901VWX234YZA567B - Colleague <colleague@example.com>");
                }
                "add" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("new@example.com");
                    println!("Added recipient {} to store", id);
                    println!("  Re-encrypting 14 entries...");
                }
                _ => { println!("Recipient operation: {}", sub); }
            }
            0
        }
        "otp" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("email/work");
            println!("OTP for {}: 345 678", name);
            println!("  Remaining: 18 seconds");
            0
        }
        "audit" => {
            println!("Auditing password store...");
            println!("  Checked 14 entries");
            println!();
            println!("  Weak passwords (2):");
            println!("    servers/staging - entropy: 28 bits (weak)");
            println!("    social/mastodon - entropy: 32 bits (fair)");
            println!();
            println!("  Duplicates (1 group):");
            println!("    email/personal and social/github share the same password");
            println!();
            println!("  Age (1 warning):");
            println!("    servers/production - last changed 365 days ago");
            0
        }
        "find" | "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("email");
            println!("Found entries matching '{}':", term);
            println!("  email/personal");
            println!("  email/work");
            0
        }
        _ => {
            // Bare name treated as "show"
            println!("(showing '{}')", cmd);
            println!("p4ssw0rd-placeholder");
            0
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gopass(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gopass};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gopass(vec!["--help".to_string()]), 0);
        assert_eq!(run_gopass(vec!["-h".to_string()]), 0);
        assert_eq!(run_gopass(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gopass(vec![]), 0);
    }
}
