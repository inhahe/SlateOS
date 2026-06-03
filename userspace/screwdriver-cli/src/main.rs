#![deny(clippy::all)]

//! screwdriver-cli — OurOS Screwdriver CD platform
//!
//! Single personality: `screwdriver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_screwdriver(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: screwdriver [COMMAND] [OPTIONS]");
        println!("Screwdriver v2024 (OurOS) — Continuous delivery build system");
        println!();
        println!("Commands:");
        println!("  pipeline list|get|create  Manage pipelines");
        println!("  build list|get|start|stop Manage builds");
        println!("  job list|get              Manage jobs");
        println!("  secret list|create|delete Manage secrets");
        println!("  token create|list|delete  Auth tokens");
        println!();
        println!("Options:");
        println!("  --api URL          Screwdriver API URL");
        println!("  --token TOKEN      Auth token");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Screwdriver CLI v2024.1 (OurOS)"); return 0; }
    println!("Screwdriver v2024.1 (OurOS)");
    println!("  API: https://screwdriver.example.com");
    println!("  Pipelines: 34");
    println!("  Jobs: 156");
    println!("  Builds: 78 (last 24h)");
    println!("  Templates: 12");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "screwdriver".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_screwdriver(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_screwdriver};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/screwdriver"), "screwdriver");
        assert_eq!(basename(r"C:\bin\screwdriver.exe"), "screwdriver.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("screwdriver.exe"), "screwdriver");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_screwdriver(&["--help".to_string()], "screwdriver"), 0);
        assert_eq!(run_screwdriver(&["-h".to_string()], "screwdriver"), 0);
        assert_eq!(run_screwdriver(&["--version".to_string()], "screwdriver"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_screwdriver(&[], "screwdriver"), 0);
    }
}
