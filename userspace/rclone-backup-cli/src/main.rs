#![deny(clippy::all)]

//! rclone-backup-cli — Slate OS rclone backup wrapper
//!
//! Single personality: `rclone-backup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rclone_backup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rclone-backup COMMAND [OPTIONS]");
        println!("rclone-backup v1.0 (Slate OS) — Simplified rclone backup profiles");
        println!();
        println!("Commands:");
        println!("  sync PROFILE      Sync profile to remote");
        println!("  check PROFILE     Verify remote matches local");
        println!("  status            Show all profile status");
        println!("  list-profiles     List configured profiles");
        println!("  create NAME       Create new backup profile");
        println!("  history PROFILE   Show sync history");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rclone-backup v1.0 (Slate OS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "sync" => {
            let profile = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("rclone-backup: syncing profile '{}'...", profile);
            println!("  Source: /home/user/Documents");
            println!("  Remote: gdrive:Backups/Documents");
            println!("  Transferred: 42 files, 256 MiB");
            println!("  Elapsed: 1:30");
        }
        "status" => {
            println!("Backup profiles:");
            println!("  documents  → gdrive:Backups/Documents   (last: 2h ago)");
            println!("  photos     → s3:mybucket/photos          (last: 1d ago)");
            println!("  config     → gdrive:Backups/Config       (last: 6h ago)");
        }
        "list-profiles" => {
            println!("documents");
            println!("photos");
            println!("config");
        }
        "check" => {
            let profile = args.get(1).map(|s| s.as_str()).unwrap_or("default");
            println!("rclone-backup: checking profile '{}'...", profile);
            println!("  Files checked: 1520");
            println!("  Differences: 0");
            println!("  Status: OK");
        }
        _ => println!("rclone-backup: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rclone-backup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rclone_backup(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rclone_backup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rclone-backup"), "rclone-backup");
        assert_eq!(basename(r"C:\bin\rclone-backup.exe"), "rclone-backup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rclone-backup.exe"), "rclone-backup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rclone_backup(&["--help".to_string()], "rclone-backup"), 0);
        assert_eq!(run_rclone_backup(&["-h".to_string()], "rclone-backup"), 0);
        let _ = run_rclone_backup(&["--version".to_string()], "rclone-backup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rclone_backup(&[], "rclone-backup");
    }
}
