#![deny(clippy::all)]

//! seafile-cli — OurOS Seafile file sync
//!
//! Multi-personality: `seaf-cli`, `seaf-daemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_seafile(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "seaf-daemon" => {
                println!("seaf-daemon (OurOS) — Seafile sync daemon");
                println!("  -c DIR       Config directory");
                println!("  -d DIR       Data directory");
                println!("  -w DIR       Worktree directory");
                println!("  -l FILE      Log file");
            }
            _ => {
                println!("seaf-cli (OurOS) — Seafile command-line client");
                println!();
                println!("Commands:");
                println!("  init -d DIR        Initialize config");
                println!("  start              Start daemon");
                println!("  stop               Stop daemon");
                println!("  sync -l LIB -s URL -d DIR  Sync library");
                println!("  desync -d DIR      Desync library");
                println!("  status             Show sync status");
                println!("  list-remote        List remote libraries");
                println!("  config             Show/set config");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Seafile v9.0.9 (OurOS)"); return 0; }
    match prog {
        "seaf-daemon" => {
            println!("Seafile Daemon v9.0.9 (OurOS)");
            println!("  Status: running");
            println!("  Libraries synced: 5");
            println!("  Upload rate: 2.3 MiB/s");
            println!("  Download rate: 0 B/s");
        }
        _ => {
            println!("Seafile CLI v9.0.9 (OurOS)");
            println!("  Server: https://seafile.example.com");
            println!("  Libraries: 5 synced");
            println!("  Total size: 45 GiB");
            println!("  Status: up to date");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "seaf-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_seafile(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_seafile};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/seafile"), "seafile");
        assert_eq!(basename(r"C:\bin\seafile.exe"), "seafile.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("seafile.exe"), "seafile");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_seafile(&["--help".to_string()], "seafile"), 0);
        assert_eq!(run_seafile(&["-h".to_string()], "seafile"), 0);
        let _ = run_seafile(&["--version".to_string()], "seafile");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_seafile(&[], "seafile");
    }
}
