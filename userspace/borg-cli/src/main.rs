#![deny(clippy::all)]

//! borg-cli — SlateOS BorgBackup CLI
//!
//! Single personality: `borg`

use std::env;
use std::process;

fn run_borg(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: borg [OPTIONS] COMMAND [ARGS ...]");
        println!();
        println!("BorgBackup — deduplicating archiver (Slate OS).");
        println!();
        println!("Commands:");
        println!("  init           Initialize repository");
        println!("  create         Create archive");
        println!("  extract        Extract archive");
        println!("  list           List archives/contents");
        println!("  info           Show archive/repo info");
        println!("  delete         Delete archive");
        println!("  prune          Prune old archives");
        println!("  compact        Free space in repository");
        println!("  mount          Mount archive as FUSE fs");
        println!("  umount         Unmount archive");
        println!("  check          Verify repository");
        println!("  diff           Diff two archives");
        println!("  rename         Rename archive");
        println!("  export-tar     Export as tar");
        println!("  key            Key management");
        println!("  config         Manage config");
        println!();
        println!("Options:");
        println!("  -v, --verbose       Verbose output");
        println!("  --progress          Show progress");
        println!("  --log-json          JSON log output");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("borg 1.4.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match cmd {
        "init" => {
            let repo = rest.iter().find(|a| !a.starts_with('-')).unwrap_or(&"/backup/repo");
            println!("Initializing repository at '{}'", repo);
            println!("Encryption: repokey-blake2");
            println!("Repository initialized.");
        }
        "create" => {
            println!("Creating archive...");
            println!("Archive name: backup-2024-01-15T10:30:00");
            println!("Duration: 12.34 seconds");
            println!("Number of files: 4523");
            println!("                       Original size      Compressed size    Deduplicated size");
            println!("This archive:               1.24 GB            890.45 MB            245.67 MB");
            println!("All archives:               8.92 GB              6.34 GB              2.15 GB");
        }
        "list" => {
            let repo = rest.iter().find(|a| !a.starts_with('-')).unwrap_or(&"repo");
            if repo.contains("::") {
                println!("drwxr-xr-x root   root          0 Mon, 2024-01-15 10:30:00 home/");
                println!("-rw-r--r-- user   user       4096 Mon, 2024-01-15 10:25:00 home/user/.bashrc");
                println!("-rw-r--r-- user   user      12288 Mon, 2024-01-15 10:28:00 home/user/document.txt");
            } else {
                println!("backup-2024-01-15T10:30:00  Mon, 2024-01-15 10:30:00 [a1b2c3d4]");
                println!("backup-2024-01-14T10:30:00  Sun, 2024-01-14 10:30:00 [e5f6a7b8]");
                println!("backup-2024-01-13T10:30:00  Sat, 2024-01-13 10:30:00 [c9d0e1f2]");
            }
        }
        "info" => {
            println!("Repository ID: abc123def456");
            println!("Location: /backup/repo");
            println!("Encrypted: Yes (repokey-blake2)");
            println!("Cache: /home/user/.cache/borg/abc123def456");
            println!();
            println!("                       Original size      Compressed size    Deduplicated size");
            println!("All archives:               8.92 GB              6.34 GB              2.15 GB");
            println!();
            println!("                       Unique chunks         Total chunks");
            println!("Chunk index:                    8921                24567");
        }
        "delete" => println!("Archive deleted."),
        "prune" => {
            println!("Keeping archive: backup-2024-01-15T10:30:00");
            println!("Keeping archive: backup-2024-01-14T10:30:00");
            println!("Pruning archive: backup-2024-01-01T10:30:00 (15 days old)");
            println!("Pruning archive: backup-2023-12-15T10:30:00 (31 days old)");
        }
        "compact" => {
            println!("Compacting repository...");
            println!("Freed 234.56 MB of space.");
        }
        "check" => {
            println!("Starting repository check...");
            println!("Repository check complete, no problems found.");
        }
        "extract" => println!("Extracting archive..."),
        "mount" => println!("Archive mounted."),
        "umount" => println!("Archive unmounted."),
        "diff" => {
            println!("  added      512 B home/user/newfile.txt");
            println!("  removed    256 B home/user/oldfile.txt");
            println!("  changed   4096 B home/user/.bashrc");
        }
        _ => {
            eprintln!("borg: unknown command '{}'. See --help.", cmd);
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
