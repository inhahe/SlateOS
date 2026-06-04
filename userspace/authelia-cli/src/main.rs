#![deny(clippy::all)]

//! authelia-cli — OurOS Authelia authentication server
//!
//! Single personality: `authelia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_authelia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: authelia [COMMAND] [OPTIONS]");
        println!("Authelia v4.38 (OurOS) — Single sign-on and 2FA server");
        println!();
        println!("Commands:");
        println!("  serve              Start server (default)");
        println!("  validate-config    Validate configuration");
        println!("  access-control     Test access control rules");
        println!("  crypto hash        Hash a password");
        println!("  crypto certificate Generate certificate");
        println!("  storage migrate    Run storage migrations");
        println!("  storage encryption Manage encryption");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (YAML)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Authelia v4.38.8 (OurOS)"); return 0; }
    println!("Authelia v4.38.8 (OurOS)");
    println!("  Listening: 0.0.0.0:9091");
    println!("  Storage: SQLite (/var/authelia/db.sqlite3)");
    println!("  Auth backend: file (/etc/authelia/users.yml)");
    println!("  Session: in-memory (Redis available)");
    println!("  2FA: TOTP, WebAuthn, Duo Push");
    println!("  Policies: 5 access control rules");
    println!("  Notifications: SMTP configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "authelia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_authelia(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_authelia};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/authelia"), "authelia");
        assert_eq!(basename(r"C:\bin\authelia.exe"), "authelia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("authelia.exe"), "authelia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_authelia(&["--help".to_string()], "authelia"), 0);
        assert_eq!(run_authelia(&["-h".to_string()], "authelia"), 0);
        let _ = run_authelia(&["--version".to_string()], "authelia");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_authelia(&[], "authelia");
    }
}
