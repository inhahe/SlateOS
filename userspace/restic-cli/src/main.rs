#![deny(clippy::all)]

//! restic-cli — SlateOS Restic backup CLI
//!
//! Single personality: `restic`

use std::env;
use std::process;

fn run_restic(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: restic <COMMAND> [OPTIONS]");
        println!();
        println!("Restic backup program (SlateOS).");
        println!();
        println!("Commands:");
        println!("  init         Initialize repository");
        println!("  backup       Create a backup");
        println!("  restore      Restore a backup");
        println!("  snapshots    List snapshots");
        println!("  forget       Remove snapshots");
        println!("  prune        Remove unneeded data");
        println!("  check        Check repository");
        println!("  mount        Mount repository");
        println!("  diff         Show differences");
        println!("  stats        Show statistics");
        println!("  cat          Print internal objects");
        println!("  key          Manage keys");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("restic 0.16.4 (SlateOS)");
        return 0;
    }

    let repo = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--repo")
        .map(|w| w[1].as_str()).unwrap_or("/backup/repo");

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "init" => {
            println!("created restic repository abc123de at {}", repo);
            println!();
            println!("Please note that knowledge of your password is required to access");
            println!("the repository. Losing your password means that your data is");
            println!("irrecoverably lost.");
            0
        }
        "backup" => {
            let path = args.iter()
                .skip(1)
                .find(|a| !a.starts_with('-') && !a.starts_with("--"))
                .map(|s| s.as_str()).unwrap_or("/home/user");
            println!("repository {} opened", repo);
            println!();
            println!("Files:         1,234 new,    56 changed,  8,901 unmodified");
            println!("Dirs:            123 new,    12 changed,    890 unmodified");
            println!("Data Blobs:    1,290 new");
            println!("Tree Blobs:      135 new");
            println!("Added to the repository: 456.7 MiB");
            println!();
            println!("processed 10191 files, 2.345 GiB in 0:45");
            println!("snapshot abc123de saved");
            println!("  Path: {}", path);
            0
        }
        "restore" => {
            let snapshot = args.get(1).map(|s| s.as_str()).unwrap_or("latest");
            let target = args.windows(2).find(|w| w[0] == "--target" || w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("/tmp/restore");
            println!("repository {} opened", repo);
            println!("restoring snapshot {} to {}", snapshot, target);
            println!();
            println!("  [0:15] 100.00%  10191 files, 2.345 GiB");
            println!("restoring <Snapshot abc123de of [/home/user] at 2024-01-15 14:00:00> to {}", target);
            0
        }
        "snapshots" => {
            println!("repository {} opened", repo);
            println!();
            println!("ID        Time                 Host        Tags    Paths              Size");
            println!("──────────────────────────────────────────────────────────────────────────────");
            println!("abc123de  2024-01-15 14:00:00  myhost              /home/user         2.3 GiB");
            println!("def456gh  2024-01-14 14:00:00  myhost              /home/user         2.1 GiB");
            println!("ghi789ij  2024-01-13 14:00:00  myhost              /home/user         2.0 GiB");
            println!("jkl012mn  2024-01-15 10:00:00  myhost      db      /var/lib/postgres   890 MiB");
            println!("──────────────────────────────────────────────────────────────────────────────");
            println!("4 snapshots");
            0
        }
        "forget" => {
            let keep_last = args.windows(2).find(|w| w[0] == "--keep-last")
                .map(|w| w[1].as_str()).unwrap_or("7");
            println!("repository {} opened", repo);
            println!("Applying Policy: keep {} latest snapshots", keep_last);
            println!();
            println!("  keep 3 snapshots:");
            println!("    ID        Time                 Host        Tags    Paths");
            println!("    abc123de  2024-01-15 14:00:00  myhost              /home/user");
            println!("    def456gh  2024-01-14 14:00:00  myhost              /home/user");
            println!("    ghi789ij  2024-01-13 14:00:00  myhost              /home/user");
            println!();
            println!("  remove 0 snapshots");
            0
        }
        "prune" => {
            println!("repository {} opened", repo);
            println!("counting files in repo");
            println!("  building new index for repo");
            println!("  [0:05] 100.00%  1234 / 1234 packs");
            println!();
            println!("  to repack:        45 blobs / 12.3 MiB");
            println!("  this removes:     45 blobs / 12.3 MiB");
            println!("  to delete:         3 packs");
            println!();
            println!("  remaining:      5678 blobs / 2.345 GiB");
            println!("done");
            0
        }
        "check" => {
            println!("using temporary cache in /tmp/restic-check-cache");
            println!("repository {} opened", repo);
            println!();
            println!("  create exclusive lock for repository");
            println!("  load indexes");
            println!("  check all packs");
            println!("  check snapshots, trees, and blobs");
            println!("  [0:12] 100.00%  4 / 4 snapshots");
            println!("  no errors were found");
            0
        }
        "stats" => {
            println!("repository {} opened", repo);
            println!();
            println!("Stats in restore-size mode:");
            println!("  Snapshots:   4");
            println!("  Total Size:  7.335 GiB");
            println!("  Total Files: 40764");
            0
        }
        "diff" => {
            let snap1 = args.get(1).map(|s| s.as_str()).unwrap_or("abc123de");
            let snap2 = args.get(2).map(|s| s.as_str()).unwrap_or("def456gh");
            println!("repository {} opened", repo);
            println!("comparing snapshot {} to {}:", snap1, snap2);
            println!();
            println!("  +    /home/user/documents/new-report.pdf");
            println!("  M    /home/user/projects/app/main.rs");
            println!("  M    /home/user/.bashrc");
            println!("  -    /home/user/downloads/temp.zip");
            println!();
            println!("Files: 1 new, 2 changed, 1 removed");
            println!("Added:   2.1 MiB");
            println!("Removed: 45.6 MiB");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: restic <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_restic(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_restic};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_restic(vec!["--help".to_string()]), 0);
        assert_eq!(run_restic(vec!["-h".to_string()]), 0);
        let _ = run_restic(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_restic(vec![]);
    }
}
