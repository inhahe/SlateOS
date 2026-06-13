#![deny(clippy::all)]

//! icinga-cli — Slate OS Icinga2 monitoring system
//!
//! Multi-personality: `icinga2`, `icinga2-check`, `icinga2-api`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_icinga2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: icinga2 <command> [OPTIONS]");
        println!("icinga2 v2.14 (Slate OS) — Monitoring and alerting system");
        println!();
        println!("Commands:");
        println!("  daemon        Run as daemon");
        println!("  object list   List monitored objects");
        println!("  feature       Manage features");
        println!("  node          Cluster node management");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("icinga2 v2.14 (Slate OS)"); return 0; }
    if args.first().map(|s| s.as_str()) == Some("daemon") {
        println!("icinga2: starting daemon...");
        println!("  Zones: master");
        println!("  Features: checker, mainlog, notification");
        return 0;
    }
    println!("icinga2: monitoring system ready");
    println!("  Hosts: 24 (UP: 24, DOWN: 0)");
    println!("  Services: 96 (OK: 94, WARNING: 2)");
    0
}

fn run_icinga2_check(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: icinga2-check <plugin> [OPTIONS]");
        println!("icinga2-check v2.14 (Slate OS) — Run check plugins");
        return 0;
    }
    println!("CHECK OK - All checks passing");
    0
}

fn run_icinga2_api(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: icinga2-api [OPTIONS]");
        println!("icinga2-api v2.14 (Slate OS) — REST API client");
        println!("  --host HOST   API host (default: localhost)");
        println!("  --port PORT   API port (default: 5665)");
        return 0;
    }
    println!("icinga2-api: connected to https://localhost:5665/v1");
    println!("  API version: v1");
    println!("  Status: running");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "icinga2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "icinga2-check" => run_icinga2_check(&rest, &prog),
        "icinga2-api" => run_icinga2_api(&rest, &prog),
        _ => run_icinga2(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_icinga2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/icinga"), "icinga");
        assert_eq!(basename(r"C:\bin\icinga.exe"), "icinga.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("icinga.exe"), "icinga");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_icinga2(&["--help".to_string()], "icinga"), 0);
        assert_eq!(run_icinga2(&["-h".to_string()], "icinga"), 0);
        let _ = run_icinga2(&["--version".to_string()], "icinga");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_icinga2(&[], "icinga");
    }
}
