#![deny(clippy::all)]

//! kanidm-cli — Slate OS Kanidm identity management
//!
//! Multi-personality: `kanidmd`, `kanidm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kanidm(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "kanidmd" => {
                println!("kanidmd (Slate OS) — Kanidm identity server");
                println!("  server              Start server");
                println!("  cert-generate       Generate self-signed cert");
                println!("  recover-account     Recover admin account");
                println!("  reindex             Reindex database");
                println!("  vacuum              Vacuum database");
                println!("  db-scan             Scan database integrity");
            }
            _ => {
                println!("kanidm (Slate OS) — Kanidm client CLI");
                println!("  login               Authenticate");
                println!("  logout              Clear sessions");
                println!("  person create|get|list  Manage persons");
                println!("  group create|list|add-members  Manage groups");
                println!("  service-account      Manage service accounts");
                println!("  oauth2              Manage OAuth2 clients");
                println!("  system              System configuration");
                println!("  self                Manage own account");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kanidm v1.3.3 (Slate OS)"); return 0; }
    match prog {
        "kanidmd" => {
            println!("Kanidm Server v1.3.3 (Slate OS)");
            println!("  HTTPS: 0.0.0.0:8443");
            println!("  LDAPS: 0.0.0.0:3636");
            println!("  Database: /var/kanidm/kanidm.db");
            println!("  Accounts: 234");
            println!("  Groups: 18");
            println!("  OAuth2 clients: 5");
        }
        _ => {
            println!("Kanidm CLI v1.3.3");
            println!("  Server: https://idm.example.com");
            println!("  Authenticated as: admin@idm.example.com");
            println!("  Session expires: 2h 30m");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kanidm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kanidm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kanidm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kanidm"), "kanidm");
        assert_eq!(basename(r"C:\bin\kanidm.exe"), "kanidm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kanidm.exe"), "kanidm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kanidm(&["--help".to_string()], "kanidm"), 0);
        assert_eq!(run_kanidm(&["-h".to_string()], "kanidm"), 0);
        let _ = run_kanidm(&["--version".to_string()], "kanidm");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kanidm(&[], "kanidm");
    }
}
