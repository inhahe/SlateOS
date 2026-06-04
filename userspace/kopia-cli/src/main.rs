#![deny(clippy::all)]

//! kopia-cli — OurOS Kopia backup CLI
//!
//! Single personality: `kopia`

use std::env;
use std::process;

fn run_kopia(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kopia <COMMAND> [OPTIONS]");
        println!();
        println!("Kopia fast and secure backup tool (OurOS).");
        println!();
        println!("Commands:");
        println!("  repository   Manage repository");
        println!("  snapshot     Create and manage snapshots");
        println!("  restore      Restore snapshot");
        println!("  policy       Manage policies");
        println!("  server       Start Kopia server");
        println!("  mount        Mount repository");
        println!("  content      Manage content");
        println!("  cache        Manage cache");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("kopia 0.15.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "repository" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "create" => {
                    let backend = args.get(2).map(|s| s.as_str()).unwrap_or("filesystem");
                    let path = args.windows(2).find(|w| w[0] == "--path")
                        .map(|w| w[1].as_str()).unwrap_or("/backup/kopia");
                    println!("Initializing repository with {} backend...", backend);
                    println!("  Path: {}", path);
                    println!("  Encryption: AES-256-GCM");
                    println!("  Splitter: DYNAMIC-4M-BUZHASH");
                    println!("  Repository initialized.");
                }
                "connect" => {
                    let backend = args.get(2).map(|s| s.as_str()).unwrap_or("filesystem");
                    println!("Connected to {} repository.", backend);
                }
                "status" => {
                    println!("Config file:      /home/user/.config/kopia/repository.config");
                    println!("Description:      My Kopia Repository");
                    println!("Storage:          Filesystem: /backup/kopia");
                    println!("Encryption:       AES-256-GCM");
                    println!("Splitter:         DYNAMIC-4M-BUZHASH");
                    println!("Unique ID:        abc123def456ghi789");
                    println!("Format version:   3");
                }
                _ => { println!("Repository operation: {}", sub); }
            }
            0
        }
        "snapshot" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "create" => {
                    let path = args.get(2).map(|s| s.as_str()).unwrap_or("/home/user");
                    println!("Snapshotting {}...", path);
                    println!("  * 0 hashing, 10234 hashed (2.3 GB), 8901 cached (1.9 GB), uploaded 234 MB");
                    println!("  Created snapshot with root kabc123def456 and target {}", path);
                    println!("  Files: 10234");
                    println!("  Dirs:  1023");
                    println!("  Size: 2.35 GB");
                    println!("  Duration: 45s");
                }
                "list" => {
                    println!("  user@myhost:/home/user");
                    println!("    2024-01-15 14:00:00 UTC kabc123def456  2.35 GB  drwxr-xr-x files:10234");
                    println!("    2024-01-14 14:00:00 UTC kdef456ghi789  2.31 GB  drwxr-xr-x files:10230");
                    println!("    2024-01-13 14:00:00 UTC kghi789jkl012  2.28 GB  drwxr-xr-x files:10225");
                }
                "delete" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("kghi789jkl012");
                    println!("Deleted snapshot {}", id);
                }
                _ => { println!("Snapshot operation: {}", sub); }
            }
            0
        }
        "restore" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("kabc123def456");
            let target = args.get(2).map(|s| s.as_str()).unwrap_or("/tmp/restore");
            println!("Restoring snapshot {} to {}...", id, target);
            println!("  Restored 10234 files, 1023 directories (2.35 GB)");
            0
        }
        "policy" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" => {
                    println!("Policy for user@myhost:/home/user:");
                    println!("  Retention:");
                    println!("    Keep latest:   10");
                    println!("    Keep hourly:   48");
                    println!("    Keep daily:    7");
                    println!("    Keep weekly:   4");
                    println!("    Keep monthly:  12");
                    println!("    Keep annual:   3");
                    println!("  Compression:     zstd");
                    println!("  Scheduling:      every 1 hour");
                }
                _ => { println!("Policy operation: {}", sub); }
            }
            0
        }
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("start");
            match sub {
                "start" => {
                    let addr = args.windows(2).find(|w| w[0] == "--address")
                        .map(|w| w[1].as_str()).unwrap_or("0.0.0.0:51515");
                    println!("Starting Kopia server at https://{}...", addr);
                    println!("  TLS enabled with auto-generated certificate");
                    println!("  Server ready.");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        "cache" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Cache directory: /home/user/.cache/kopia");
                    println!("  Contents:  456 items, 234 MB");
                    println!("  Metadata:  123 items, 12 MB");
                    println!("  Total:     246 MB");
                }
                "clear" => {
                    println!("Cache cleared (246 MB freed)");
                }
                _ => { println!("Cache operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: kopia <command>. See --help.");
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
    let code = run_kopia(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_kopia};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kopia(vec!["--help".to_string()]), 0);
        assert_eq!(run_kopia(vec!["-h".to_string()]), 0);
        let _ = run_kopia(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kopia(vec![]);
    }
}
