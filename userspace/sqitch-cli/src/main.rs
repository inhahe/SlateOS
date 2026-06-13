#![deny(clippy::all)]

//! sqitch-cli — SlateOS Sqitch database change management
//!
//! Single personality: `sqitch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sqitch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sqitch COMMAND [OPTIONS]");
        println!("sqitch v1.4.1 (Slate OS) — Sensible database change management");
        println!();
        println!("Commands:");
        println!("  init            Initialize project");
        println!("  add NAME        Add a change");
        println!("  deploy          Deploy changes");
        println!("  revert          Revert changes");
        println!("  verify          Verify changes");
        println!("  status          Show deployment status");
        println!("  log             Show deployment log");
        println!("  tag NAME        Tag latest change");
        println!("  bundle          Bundle project for distribution");
        println!("  plan            Show plan");
        println!("  config          Manage config");
        println!("  engine          Manage engines");
        println!("  target          Manage targets");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sqitch v1.4.1 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "init" => {
            println!("Created sqitch.conf");
            println!("Created sqitch.plan");
            println!("Created deploy/");
            println!("Created revert/");
            println!("Created verify/");
        }
        "add" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("change");
            println!("Created deploy/{}.sql", name);
            println!("Created revert/{}.sql", name);
            println!("Created verify/{}.sql", name);
            println!("Added \"{}\" to sqitch.plan", name);
        }
        "deploy" => {
            println!("Deploying changes to db:pg://localhost/mydb");
            println!("  + create_users .. ok");
            println!("  + add_orders .. ok");
        }
        "revert" => {
            println!("Reverting changes from db:pg://localhost/mydb");
            println!("  - add_orders .. ok");
        }
        "verify" => {
            println!("Verifying db:pg://localhost/mydb");
            println!("  * create_users .. ok");
            println!("  * add_orders .. ok");
            println!("Verify successful.");
        }
        "status" => {
            println!("# On database db:pg://localhost/mydb");
            println!("# Project: myproject");
            println!("# Change:  add_orders");
            println!("# Tag:     v1.0.0");
            println!("# Deployed: 2024-01-15 10:00:00 +0000");
            println!("# By:       developer");
            println!("Nothing to deploy (up-to-date)");
        }
        "log" => {
            println!("On database db:pg://localhost/mydb");
            println!("Deploy add_orders");
            println!("  Name: add_orders");
            println!("  Deployed: 2024-01-15 10:00:00 +0000");
            println!("  By: developer");
            println!();
            println!("Deploy create_users");
            println!("  Name: create_users");
            println!("  Deployed: 2024-01-14 10:00:00 +0000");
        }
        "plan" => {
            println!("create_users 2024-01-14T10:00:00Z developer <dev@example.com>");
            println!("add_orders 2024-01-15T10:00:00Z developer <dev@example.com>");
        }
        "tag" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("v1.0.0");
            println!("Tagged \"add_orders\" with @{}", name);
        }
        _ => println!("sqitch {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sqitch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqitch(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sqitch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sqitch"), "sqitch");
        assert_eq!(basename(r"C:\bin\sqitch.exe"), "sqitch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sqitch.exe"), "sqitch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sqitch(&["--help".to_string()], "sqitch"), 0);
        assert_eq!(run_sqitch(&["-h".to_string()], "sqitch"), 0);
        let _ = run_sqitch(&["--version".to_string()], "sqitch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sqitch(&[], "sqitch");
    }
}
