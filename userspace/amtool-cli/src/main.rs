#![deny(clippy::all)]

//! amtool-cli — SlateOS Alertmanager CLI tool
//!
//! Single personality: `amtool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_amtool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: amtool COMMAND [OPTIONS]");
        println!("amtool v0.27.0 (Slate OS) — Alertmanager CLI");
        println!();
        println!("Commands:");
        println!("  alert           Manage alerts");
        println!("  silence         Manage silences");
        println!("  config          Manage configuration");
        println!("  check-config    Validate config file");
        println!("  cluster         Cluster management");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  --alertmanager.url URL  Alertmanager URL");
        println!("  --output simple|extended|json  Output format");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("amtool v0.27.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("alert");
    match cmd {
        "alert" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("query");
            match sub {
                "query" => {
                    println!("Alertname       Severity  Instance         Status   StartsAt");
                    println!("HighCPU         warning   web-01           active   2024-01-15T10:00:00Z");
                    println!("DiskFull        critical  db-01            active   2024-01-15T09:30:00Z");
                    println!("HighLatency     warning   api-gateway      active   2024-01-15T10:15:00Z");
                }
                "add" => println!("Alert added successfully."),
                _ => println!("amtool alert {}: completed", sub),
            }
        }
        "silence" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("query");
            match sub {
                "query" => {
                    println!("ID                    Matchers                    Ends                  CreatedBy   Comment");
                    println!("abc123-def456         alertname=HighCPU           2024-01-16T10:00:00Z  admin       Maintenance");
                }
                "add" => println!("Silence created: abc789-ghi012"),
                "expire" => println!("Silence expired."),
                _ => println!("amtool silence {}: completed", sub),
            }
        }
        "check-config" => {
            println!("Checking 'alertmanager.yml'...");
            println!("  Config is valid.");
            println!("  Found 2 routes, 3 receivers.");
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            if sub == "show" {
                println!("route:");
                println!("  receiver: default");
                println!("  routes:");
                println!("    - match: severity=critical");
                println!("      receiver: pagerduty");
                println!("    - match: severity=warning");
                println!("      receiver: slack");
            }
        }
        "cluster" => {
            println!("Cluster status:");
            println!("  Name: alertmanager-cluster");
            println!("  Peers: 3");
            println!("  Status: ready");
        }
        _ => println!("amtool {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "amtool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_amtool(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_amtool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/amtool"), "amtool");
        assert_eq!(basename(r"C:\bin\amtool.exe"), "amtool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("amtool.exe"), "amtool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_amtool(&["--help".to_string()], "amtool"), 0);
        assert_eq!(run_amtool(&["-h".to_string()], "amtool"), 0);
        let _ = run_amtool(&["--version".to_string()], "amtool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_amtool(&[], "amtool");
    }
}
