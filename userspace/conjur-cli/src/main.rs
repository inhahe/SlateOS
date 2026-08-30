#![deny(clippy::all)]

//! conjur-cli — Slate OS CyberArk Conjur secrets management
//!
//! Single personality: `conjur`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_conjur(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: conjur [COMMAND] [OPTIONS]");
        println!("Conjur v5.0 (Slate OS) — Secrets management for DevOps");
        println!();
        println!("Commands:");
        println!("  init               Initialize CLI");
        println!("  login              Authenticate");
        println!("  logout             Clear session");
        println!("  policy load FILE   Load RBAC policy");
        println!("  variable get ID    Get secret value");
        println!("  variable set ID    Set secret value");
        println!("  list               List resources");
        println!("  whoami             Show current identity");
        println!("  role exists ROLE   Check role exists");
        println!();
        println!("Options:");
        println!("  --account ACCT     Conjur account");
        println!("  --url URL          Conjur server URL");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Conjur CLI v8.0.1 (Slate OS)"); return 0; }
    println!("Conjur CLI v8.0.1 (Slate OS)");
    println!("  Server: https://conjur.example.com");
    println!("  Account: myorg");
    println!("  Logged in as: admin");
    println!("  Secrets: 456");
    println!("  Policies: 12");
    println!("  Roles: 89");
    println!("  Hosts: 34 (machine identities)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "conjur".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_conjur(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_conjur};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/conjur"), "conjur");
        assert_eq!(basename(r"C:\bin\conjur.exe"), "conjur.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("conjur.exe"), "conjur");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_conjur(&["--help".to_string()], "conjur"), 0);
        assert_eq!(run_conjur(&["-h".to_string()], "conjur"), 0);
        let _ = run_conjur(&["--version".to_string()], "conjur");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_conjur(&[], "conjur");
    }
}
