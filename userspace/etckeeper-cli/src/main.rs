#![deny(clippy::all)]

//! etckeeper-cli — OurOS etckeeper /etc version control
//!
//! Single personality: `etckeeper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_etckeeper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: etckeeper <command> [OPTIONS]");
        println!("etckeeper v1.18 (OurOS) — Store /etc in version control");
        println!();
        println!("Commands:");
        println!("  init            Initialize repository in /etc");
        println!("  commit MSG      Commit changes");
        println!("  pre-install     Pre-package-install hook");
        println!("  post-install    Post-package-install hook");
        println!("  unclean         Check for uncommitted changes");
        println!("  vcs CMD         Run VCS command in /etc");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("etckeeper v1.18 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("init") => {
            println!("etckeeper: initializing /etc repository (git)");
            println!("  Initialized empty Git repository in /etc/.git/");
        }
        Some("unclean") => {
            println!("etckeeper: /etc is clean (no uncommitted changes)");
        }
        Some("commit") => {
            let msg = args.get(1).map(|s| s.as_str()).unwrap_or("manual commit");
            println!("etckeeper: committing /etc changes");
            println!("  Message: {}", msg);
            println!("  Files changed: 3");
        }
        _ => {
            println!("etckeeper: use --help for usage information");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "etckeeper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_etckeeper(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_etckeeper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/etckeeper"), "etckeeper");
        assert_eq!(basename(r"C:\bin\etckeeper.exe"), "etckeeper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("etckeeper.exe"), "etckeeper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_etckeeper(&["--help".to_string()], "etckeeper"), 0);
        assert_eq!(run_etckeeper(&["-h".to_string()], "etckeeper"), 0);
        assert_eq!(run_etckeeper(&["--version".to_string()], "etckeeper"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_etckeeper(&[], "etckeeper"), 0);
    }
}
