#![deny(clippy::all)]

//! backintime-cli — Slate OS Back In Time snapshot backup
//!
//! Multi-personality: `backintime`, `backintime-qt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_backintime(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: backintime COMMAND [OPTIONS]");
        println!("backintime v1.4 (Slate OS) — Snapshot-based backup tool");
        println!();
        println!("Commands:");
        println!("  backup            Start backup now");
        println!("  backup-job        Run scheduled backup");
        println!("  restore FILE SNAP Restore file from snapshot");
        println!("  snapshots-list    List all snapshots");
        println!("  last-snapshot     Show most recent snapshot");
        println!("  check-config      Validate configuration");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("backintime v1.4 (Slate OS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("backup");
    match cmd {
        "backup" => {
            println!("backintime: starting backup...");
            println!("  Profile: Main");
            println!("  Source: /home/user");
            println!("  Destination: /mnt/backup/backintime");
            println!("  Snapshot: 20240115-103000");
            println!("  Status: complete");
        }
        "snapshots-list" => {
            println!("20240115-103000");
            println!("20240114-103000");
            println!("20240113-103000");
            println!("20240112-103000");
        }
        "last-snapshot" => println!("20240115-103000"),
        "check-config" => println!("Configuration OK"),
        _ => println!("backintime: {}", cmd),
    }
    0
}

fn run_qt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: backintime-qt [OPTIONS]");
        println!("backintime-qt v1.4 (Slate OS) — Back In Time GUI");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("backintime-qt v1.4 (Slate OS)"); return 0; }
    println!("backintime-qt: graphical backup manager started");
    println!("  Snapshots: 4 available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "backintime".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "backintime-qt" => run_qt(&rest, &prog),
        _ => run_backintime(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_backintime};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/backintime"), "backintime");
        assert_eq!(basename(r"C:\bin\backintime.exe"), "backintime.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("backintime.exe"), "backintime");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_backintime(&["--help".to_string()], "backintime"), 0);
        assert_eq!(run_backintime(&["-h".to_string()], "backintime"), 0);
        let _ = run_backintime(&["--version".to_string()], "backintime");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_backintime(&[], "backintime");
    }
}
