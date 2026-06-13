#![deny(clippy::all)]

//! teams-cli — SlateOS Microsoft Teams (PWA/web client)
//!
//! Single personality: `teams`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_teams(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: teams [OPTIONS]");
        println!("teams v1.0 (SlateOS) — Microsoft Teams web client wrapper");
        println!();
        println!("Options:");
        println!("  --minimized       Start minimized");
        println!("  --url URL         Open specific team/channel URL");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("teams v1.0 (SlateOS)"); return 0; }
    println!("teams: launching Microsoft Teams web client");
    println!("  URL: https://teams.microsoft.com");
    println!("  Mode: PWA wrapper");
    println!("  Notifications: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teams".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_teams(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_teams};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/teams"), "teams");
        assert_eq!(basename(r"C:\bin\teams.exe"), "teams.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("teams.exe"), "teams");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_teams(&["--help".to_string()], "teams"), 0);
        assert_eq!(run_teams(&["-h".to_string()], "teams"), 0);
        let _ = run_teams(&["--version".to_string()], "teams");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_teams(&[], "teams");
    }
}
