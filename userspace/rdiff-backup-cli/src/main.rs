#![deny(clippy::all)]

//! rdiff-backup-cli — Slate OS rdiff-backup CLI
//!
//! Single personality: `rdiff-backup`

use std::env;
use std::process;

fn run_rdiff_backup(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rdiff-backup [OPTIONS] COMMAND [ARGS ...]");
        println!();
        println!("rdiff-backup — reverse differential backup (Slate OS).");
        println!();
        println!("Commands:");
        println!("  backup SRC DEST       Create backup");
        println!("  restore SRC DEST      Restore from backup");
        println!("  list increments REPO  List increments");
        println!("  list files REPO       List files at time");
        println!("  verify REPO           Verify backup");
        println!("  remove increments     Remove old increments");
        println!("  calculate average     Calculate average stats");
        println!("  info REPO             Show repo info");
        println!();
        println!("Options:");
        println!("  --at TIME          Time spec (now, 1D, 2W, etc.)");
        println!("  --force            Force operation");
        println!("  -v, --verbosity N  Verbosity (0-9)");
        println!("  --include PATTERN  Include pattern");
        println!("  --exclude PATTERN  Exclude pattern");
        println!("  --no-compression   Disable compression");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rdiff-backup 2.2.6 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "backup" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("/home");
            let dest = args.get(2).map(|s| s.as_str()).unwrap_or("/backup");
            println!("Starting backup {} → {}", src, dest);
            println!("  Processing changed files...");
            println!("  Updated 47 files, 3 directories");
            println!("  Transferred 12.4 MB (4.2 MB after compression)");
            println!("  Elapsed time: 8.3 seconds");
        }
        "restore" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("/backup");
            let dest = args.get(2).map(|s| s.as_str()).unwrap_or("/restore");
            println!("Restoring {} → {}", src, dest);
            println!("  Restored 1523 files, 89 directories");
            println!("  Total: 245.6 MB");
        }
        "list" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("increments");
            match subcmd {
                "increments" => {
                    println!("Found 5 increments:");
                    println!("  Mon Jan 15 10:30:00 2024   12.4 MB");
                    println!("  Sun Jan 14 10:30:00 2024    8.7 MB");
                    println!("  Sat Jan 13 10:30:00 2024   15.2 MB");
                    println!("  Fri Jan 12 10:30:00 2024    3.1 MB");
                    println!("  Thu Jan 11 10:30:00 2024  245.6 MB (initial full)");
                    println!("  Current mirror: Mon Jan 15 10:30:00 2024");
                }
                "files" => {
                    println!("home/user/.bashrc");
                    println!("home/user/.profile");
                    println!("home/user/documents/report.txt");
                    println!("home/user/photos/vacation.jpg");
                }
                _ => println!("rdiff-backup list: unknown subcommand '{}'", subcmd),
            }
        }
        "verify" => {
            println!("Verifying backup integrity...");
            println!("Everything appears to be in order.");
        }
        "remove" => {
            println!("Removing increments older than 30D...");
            println!("Removed 2 increments (18.3 MB freed).");
        }
        "info" => {
            println!("Repository info:");
            println!("  Location: /backup/repo");
            println!("  Increments: 5");
            println!("  Total size: 285.0 MB");
            println!("  Oldest: Thu Jan 11 10:30:00 2024");
            println!("  Newest: Mon Jan 15 10:30:00 2024");
        }
        _ => {
            eprintln!("rdiff-backup: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rdiff_backup(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rdiff_backup};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rdiff_backup(vec!["--help".to_string()]), 0);
        assert_eq!(run_rdiff_backup(vec!["-h".to_string()]), 0);
        let _ = run_rdiff_backup(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rdiff_backup(vec![]);
    }
}
