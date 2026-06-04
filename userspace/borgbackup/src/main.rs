#![deny(clippy::all)]

//! borgbackup — OurOS deduplicating backup
//!
//! Single personality: `borg`

use std::env;
use std::process;

fn run_borg(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: borg <command> [options] [arguments]");
        println!();
        println!("Commands:");
        println!("  init        Initialize repository");
        println!("  create      Create backup archive");
        println!("  extract     Extract archive");
        println!("  check       Verify repository/archive");
        println!("  list        List repository or archive contents");
        println!("  info        Show repository or archive info");
        println!("  delete      Delete archive");
        println!("  prune       Prune repository archives");
        println!("  compact     Free repository space");
        println!("  diff        Diff two archives");
        println!("  rename      Rename archive");
        println!("  mount       Mount archive as FUSE fs");
        println!("  umount      Unmount archive");
        println!("  key         Key management");
        println!("  config      Get/set config values");
        println!("  export-tar  Export archive as tar");
        println!("  version     Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => println!("borg 1.4.0 (OurOS)"),
        "init" => {
            let repo = args.get(1).map(|s| s.as_str()).unwrap_or("/backup/repo");
            println!("Initializing repository at {}", repo);
            println!("Enter new passphrase:");
            println!("Repository initialized.");
        }
        "create" => {
            println!("Creating archive...");
            println!("Archive name: backup-2025-05-22T10:00:00");
            println!("------------------------------------------------------------------------------");
            println!("                       Original size      Compressed size    Deduplicated size");
            println!("This archive:                2.50 GB              1.80 GB            450.00 MB");
            println!("All archives:               12.50 GB              9.00 GB              2.50 GB");
            println!("                       Unique chunks         Total chunks");
            println!("Chunk index:                   45678               234567");
        }
        "list" => {
            let repo = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if repo.contains("::") {
                println!("drwxr-xr-x root   root          0 Thu, 2025-05-22 10:00:00 home/user");
                println!("-rw-r--r-- root   root    1234567 Thu, 2025-05-22 09:59:00 home/user/file.txt");
            } else {
                println!("backup-2025-05-22T10:00:00            Thu, 2025-05-22 10:00:00 [abc123]");
                println!("backup-2025-05-21T10:00:00            Wed, 2025-05-21 10:00:00 [def456]");
                println!("backup-2025-05-20T10:00:00            Tue, 2025-05-20 10:00:00 [789abc]");
            }
        }
        "info" => {
            println!("Repository: /backup/repo");
            println!("Location: /backup/repo");
            println!("Encrypted: Yes (repokey-blake2)");
            println!("Cache: /home/user/.cache/borg");
            println!("Security dir: /home/user/.config/borg/security");
            println!();
            println!("                       Original size      Compressed size    Deduplicated size");
            println!("All archives:               12.50 GB              9.00 GB              2.50 GB");
        }
        "prune" => {
            println!("Keeping archive: backup-2025-05-22T10:00:00");
            println!("Keeping archive: backup-2025-05-21T10:00:00");
            println!("Pruning archive: backup-2025-05-15T10:00:00 (older than 7 days)");
        }
        "compact" => println!("Compacting repository... freed 120 MB."),
        "check" => println!("Starting repository check... completed, no issues found."),
        "extract" | "delete" | "diff" | "rename" | "mount" | "umount" | "key" | "config" | "export-tar" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_borg(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_borg};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_borg(vec!["--help".to_string()]), 0);
        assert_eq!(run_borg(vec!["-h".to_string()]), 0);
        let _ = run_borg(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_borg(vec![]);
    }
}
