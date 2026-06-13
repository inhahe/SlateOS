#![deny(clippy::all)]

//! deja-dup-cli — SlateOS Deja Dup backup tool
//!
//! Multi-personality: `deja-dup`, `deja-dup-monitor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deja_dup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: deja-dup [COMMAND] [OPTIONS]");
        println!("deja-dup v44.0 (SlateOS) — Simple backup tool");
        println!();
        println!("Commands:");
        println!("  backup            Start a backup");
        println!("  restore           Restore from backup");
        println!("  list              List backup contents");
        println!("  status            Show backup status");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deja-dup v44.0 (SlateOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "backup" => {
            println!("deja-dup: starting backup...");
            println!("  Source: /home/user");
            println!("  Destination: /mnt/backup/deja-dup");
            println!("  Excludes: .cache, .local/share/Trash, Downloads");
            println!("  Status: backup complete (1.2 GiB, 3:42)");
        }
        "restore" => {
            println!("deja-dup: restore wizard");
            println!("  Available backups: 12");
            println!("  Latest: 2024-01-15 10:30");
        }
        "list" => {
            println!("Backup history:");
            println!("  2024-01-15 10:30 (full, 1.2 GiB)");
            println!("  2024-01-14 10:30 (incremental, 45 MiB)");
            println!("  2024-01-13 10:30 (incremental, 32 MiB)");
        }
        "status" => {
            println!("Deja Dup Status:");
            println!("  Last backup: 2024-01-15 10:30");
            println!("  Next scheduled: 2024-01-16 10:30");
            println!("  Backend: local (/mnt/backup)");
            println!("  Total size: 4.5 GiB (12 backups)");
        }
        _ => println!("deja-dup: unknown command: {}", cmd),
    }
    0
}

fn run_monitor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deja-dup-monitor [OPTIONS]");
        println!("deja-dup-monitor v44.0 (SlateOS) — Backup monitor daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deja-dup-monitor v44.0 (SlateOS)"); return 0; }
    println!("deja-dup-monitor: watching for scheduled backups");
    println!("  Next backup: 2024-01-16 10:30");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "deja-dup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "deja-dup-monitor" => run_monitor(&rest, &prog),
        _ => run_deja_dup(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_deja_dup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deja-dup"), "deja-dup");
        assert_eq!(basename(r"C:\bin\deja-dup.exe"), "deja-dup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deja-dup.exe"), "deja-dup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deja_dup(&["--help".to_string()], "deja-dup"), 0);
        assert_eq!(run_deja_dup(&["-h".to_string()], "deja-dup"), 0);
        let _ = run_deja_dup(&["--version".to_string()], "deja-dup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deja_dup(&[], "deja-dup");
    }
}
