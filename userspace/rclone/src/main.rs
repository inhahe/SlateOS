#![deny(clippy::all)]

//! rclone — SlateOS cloud storage sync tool
//!
//! Single personality: `rclone`

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Remote {
    name: String,
    remote_type: String,
    _config: Vec<(String, String)>,
}

fn sample_remotes() -> Vec<Remote> {
    vec![
        Remote { name: "gdrive".to_string(), remote_type: "drive".to_string(), _config: vec![] },
        Remote { name: "s3backup".to_string(), remote_type: "s3".to_string(), _config: vec![] },
        Remote { name: "dropbox".to_string(), remote_type: "dropbox".to_string(), _config: vec![] },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_rclone(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rclone <command> [flags] [source:path] [dest:path]");
            println!();
            println!("Cloud storage sync and management tool.");
            println!();
            println!("Commands:");
            println!("  copy        Copy files (skip existing)");
            println!("  sync        Sync destination to source");
            println!("  move        Move files");
            println!("  ls          List objects with size and path");
            println!("  lsd         List directories only");
            println!("  lsl         List objects with mod time, size, path");
            println!("  mkdir       Make directory path");
            println!("  rmdir       Remove empty directory");
            println!("  delete      Remove contents of path");
            println!("  purge       Remove path and contents");
            println!("  check       Check source vs destination");
            println!("  config      Enter interactive configuration");
            println!("  listremotes List configured remotes");
            println!("  about       Get quota information");
            println!("  mount       Mount remote as filesystem");
            println!("  serve       Serve remote over HTTP/FTP/WebDAV");
            println!("  --version   Show version");
            0
        }
        "--version" | "version" => { println!("rclone v0.1.0 (Slate OS)"); 0 }
        "copy" | "sync" | "move" => {
            let dry_run = cmd_args.iter().any(|a| a == "--dry-run" || a == "-n");
            let verbose = cmd_args.iter().any(|a| a == "-v" || a == "--verbose");
            let src = cmd_args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("source:");
            let dst = cmd_args.get(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("dest:");

            println!("{}: {} → {}{}", cmd, src, dst,
                if dry_run { " (dry run)" } else { "" });
            if verbose {
                println!("  Transferred:   125.00 MiB / 48.93 GiB, 0%");
                println!("  Checks:        56 / 56, 100%");
                println!("  Transferred:   12 / 12, 100%");
                println!("  Elapsed time:  3m5s");
            }
            println!("{}: operation complete (simulated)", cmd);
            0
        }
        "ls" => {
            println!("    2048 Documents/report.pdf");
            println!("    4096 Documents/spreadsheet.xlsx");
            println!("     512 .bashrc");
            println!("  16384 Pictures/photo.jpg");
            0
        }
        "lsd" => {
            println!("          -1 2025-05-20 10:00:00        -1 Documents");
            println!("          -1 2025-05-20 10:00:00        -1 Pictures");
            println!("          -1 2025-05-20 10:00:00        -1 Music");
            0
        }
        "listremotes" => {
            let remotes = sample_remotes();
            for r in &remotes {
                println!("{}:", r.name);
            }
            0
        }
        "config" => {
            println!("Current remotes:");
            let remotes = sample_remotes();
            for (i, r) in remotes.iter().enumerate() {
                println!(" {:2}) {} ({})", i + 1, r.name, r.remote_type);
            }
            println!();
            println!("e) Edit existing remote");
            println!("n) New remote");
            println!("d) Delete remote");
            println!("q) Quit config");
            0
        }
        "about" => {
            let remote = cmd_args.first().map(|s| s.as_str()).unwrap_or("gdrive:");
            println!("Total:   15.00 GiB");
            println!("Used:    8.50 GiB");
            println!("Free:    6.50 GiB");
            println!("Trashed: 0.20 GiB");
            println!("({})", remote);
            0
        }
        "check" => { println!("0 differences found (simulated)"); 0 }
        "mkdir" | "rmdir" | "delete" | "purge" => {
            let path = cmd_args.first().map(|s| s.as_str()).unwrap_or("remote:path");
            println!("{}: {} (simulated)", cmd, path);
            0
        }
        other => { eprintln!("rclone: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rclone(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remotes() {
        let remotes = sample_remotes();
        assert_eq!(remotes.len(), 3);
        assert_eq!(remotes[0].remote_type, "drive");
    }
}
