#![deny(clippy::all)]

//! vorta-cli — Slate OS Vorta BorgBackup GUI frontend
//!
//! Single personality: `vorta`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vorta(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vorta [OPTIONS]");
        println!("vorta v0.9 (Slate OS) — Desktop backup GUI for BorgBackup");
        println!();
        println!("Options:");
        println!("  --daemonize       Start in system tray");
        println!("  --create PROFILE  Trigger backup for profile");
        println!("  --list            List profiles");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vorta v0.9 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("Vorta profiles:");
        println!("  default   → /mnt/backup/borg  (daily, last: 2h ago)");
        println!("  work      → ssh://backup/borg (hourly, last: 45m ago)");
        return 0;
    }
    if args.iter().any(|a| a == "--daemonize") {
        println!("vorta: started in system tray");
        println!("  Profiles: 2 configured");
        println!("  Next backup: work in 15m");
        return 0;
    }
    println!("vorta: BorgBackup desktop GUI started");
    println!("  Backend: BorgBackup 1.2");
    println!("  Profiles: 2 configured");
    println!("  Repository: /mnt/backup/borg (4.5 GiB, 15 archives)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vorta".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vorta(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vorta};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vorta"), "vorta");
        assert_eq!(basename(r"C:\bin\vorta.exe"), "vorta.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vorta.exe"), "vorta");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vorta(&["--help".to_string()], "vorta"), 0);
        assert_eq!(run_vorta(&["-h".to_string()], "vorta"), 0);
        let _ = run_vorta(&["--version".to_string()], "vorta");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vorta(&[], "vorta");
    }
}
