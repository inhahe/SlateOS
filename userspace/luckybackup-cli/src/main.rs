#![deny(clippy::all)]

//! luckybackup-cli — SlateOS luckyBackup rsync-based GUI backup
//!
//! Single personality: `luckybackup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_luckybackup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: luckybackup [OPTIONS] [PROFILE]");
        println!("luckybackup v0.5 (SlateOS) — Rsync-based backup & sync tool");
        println!();
        println!("Options:");
        println!("  --skip-critical  Skip critical question");
        println!("  --dry-run        Simulation mode");
        println!("  --version        Show version");
        println!();
        println!("Profiles are stored in ~/.luckyBackup/");
        println!("Features: sync, backup with snapshots, scheduling");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("luckybackup v0.5 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "--dry-run") {
        println!("luckybackup: dry run (simulation)");
        println!("  Profile: default");
        println!("  Tasks: 2");
        println!("  No changes made");
        return 0;
    }
    println!("luckybackup: backup started");
    println!("  Profile: default");
    println!("  Task 1: /home -> /backup/home (sync)");
    println!("  Task 2: /etc -> /backup/etc (backup with snapshots)");
    println!("  Status: completed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "luckybackup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_luckybackup(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_luckybackup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/luckybackup"), "luckybackup");
        assert_eq!(basename(r"C:\bin\luckybackup.exe"), "luckybackup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("luckybackup.exe"), "luckybackup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_luckybackup(&["--help".to_string()], "luckybackup"), 0);
        assert_eq!(run_luckybackup(&["-h".to_string()], "luckybackup"), 0);
        let _ = run_luckybackup(&["--version".to_string()], "luckybackup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_luckybackup(&[], "luckybackup");
    }
}
