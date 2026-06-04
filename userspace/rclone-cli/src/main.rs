#![deny(clippy::all)]

//! rclone-cli — OurOS rclone CLI
//!
//! Single personality: `rclone`

use std::env;
use std::process;

fn run_rclone(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rclone <COMMAND> [OPTIONS]");
        println!();
        println!("Rclone cloud storage sync CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  config       Manage configuration");
        println!("  copy         Copy files");
        println!("  sync         Sync directories");
        println!("  move         Move files");
        println!("  ls           List objects");
        println!("  lsd          List directories");
        println!("  lsl          List objects with size/date");
        println!("  mkdir        Make directory");
        println!("  rmdir        Remove directory");
        println!("  delete       Delete files");
        println!("  mount        Mount remote as filesystem");
        println!("  check        Check file integrity");
        println!("  about        Show remote quota");
        println!("  serve        Serve remote over HTTP/FTP");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rclone v1.65.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" => {
                    println!("[gdrive]");
                    println!("type = drive");
                    println!("scope = drive");
                    println!();
                    println!("[s3]");
                    println!("type = s3");
                    println!("provider = AWS");
                    println!("region = us-east-1");
                    println!();
                    println!("[dropbox]");
                    println!("type = dropbox");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("myremote");
                    println!("Remote '{}' created successfully", name);
                }
                _ => { println!("Config operation: {}", sub); }
            }
            0
        }
        "copy" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("./data");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("gdrive:backup/data");
            println!("Copying from {} to {}...", src, dst);
            println!("Transferred:      250.3 MiB / 250.3 MiB, 100%, 45.2 MiB/s, ETA 0s");
            println!("Transferred:       12 / 12, 100%");
            println!("Elapsed time:      5.5s");
            0
        }
        "sync" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("./data");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("s3:mybucket/data");
            println!("Syncing from {} to {}...", src, dst);
            println!("Transferred:      125.6 MiB / 125.6 MiB, 100%, 38.1 MiB/s, ETA 0s");
            println!("Transferred:        8 / 8, 100%");
            println!("Deleted:            2 (files), 0 (dirs)");
            println!("Elapsed time:      3.3s");
            0
        }
        "ls" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("gdrive:");
            println!("Listing {}...", path);
            println!("   131584 documents/report.pdf");
            println!("    45056 documents/slides.pptx");
            println!("  5242880 photos/vacation.jpg");
            println!("  1048576 projects/code.zip");
            0
        }
        "lsd" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("gdrive:");
            println!("Listing directories in {}...", path);
            println!("           -1 2024-01-15 14:00:00        -1 documents");
            println!("           -1 2024-01-14 10:00:00        -1 photos");
            println!("           -1 2024-01-13 08:00:00        -1 projects");
            0
        }
        "mount" => {
            let remote = args.get(1).map(|s| s.as_str()).unwrap_or("gdrive:");
            let mountpoint = args.get(2).map(|s| s.as_str()).unwrap_or("/mnt/gdrive");
            println!("Mounting {} at {}...", remote, mountpoint);
            println!("  VFS cache mode: full");
            println!("  Cache dir: /tmp/rclone-cache");
            println!("  Mount ready. Use Ctrl+C to unmount.");
            0
        }
        "check" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("./data");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("s3:mybucket/data");
            println!("Checking {} vs {}...", src, dst);
            println!("  12 files matched");
            println!("  0 files missing from source");
            println!("  0 files missing from destination");
            println!("  0 files differ");
            0
        }
        "about" => {
            let remote = args.get(1).map(|s| s.as_str()).unwrap_or("gdrive:");
            println!("Remote: {}", remote);
            println!("  Total:   15 GiB");
            println!("  Used:    8.2 GiB");
            println!("  Free:    6.8 GiB");
            println!("  Trashed: 512 MiB");
            0
        }
        "serve" => {
            let proto = args.get(1).map(|s| s.as_str()).unwrap_or("http");
            let remote = args.get(2).map(|s| s.as_str()).unwrap_or("gdrive:");
            let addr = args.windows(2).find(|w| w[0] == "--addr").map(|w| w[1].as_str()).unwrap_or("localhost:8080");
            println!("Serving {} via {} at {}", remote, proto, addr);
            println!("  Access at: {}://{}", proto, addr);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: rclone <command>. See --help.");
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
    let code = run_rclone(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rclone};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rclone(vec!["--help".to_string()]), 0);
        assert_eq!(run_rclone(vec!["-h".to_string()]), 0);
        let _ = run_rclone(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rclone(vec![]);
    }
}
