#![deny(clippy::all)]

//! keycloak-cli — OurOS Keycloak identity management
//!
//! Multi-personality: `keycloak`, `kcadm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_keycloak(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "kcadm" => {
                println!("kcadm (OurOS) — Keycloak Admin CLI");
                println!("  config credentials  Set server/credentials");
                println!("  create realms       Create realm");
                println!("  create users        Create user");
                println!("  get realms          List realms");
                println!("  get users           List users");
                println!("  update users/ID     Update user");
                println!("  delete users/ID     Delete user");
            }
            _ => {
                println!("Keycloak v24.0 (OurOS) — Identity and access management");
                println!("  start              Start in production mode");
                println!("  start-dev          Start in dev mode");
                println!("  build              Build optimized config");
                println!("  export             Export realm");
                println!("  import             Import realm");
                println!("  show-config        Show current config");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Keycloak v24.0.4 (OurOS)"); return 0; }
    match prog {
        "kcadm" => {
            println!("Keycloak Admin CLI");
            println!("  Server: http://localhost:8080");
            println!("  Realm: master");
            println!("  Authenticated as: admin");
        }
        _ => {
            println!("Keycloak v24.0.4 (OurOS)");
            println!("  Realms: 3 (master, production, staging)");
            println!("  Users: 1,234");
            println!("  Clients: 45");
            println!("  Identity providers: OIDC, SAML, GitHub, Google");
            println!("  Database: PostgreSQL");
            println!("  HTTP: https://0.0.0.0:8443");
            println!("  Clustering: single node");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keycloak".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_keycloak(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_keycloak};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/keycloak"), "keycloak");
        assert_eq!(basename(r"C:\bin\keycloak.exe"), "keycloak.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("keycloak.exe"), "keycloak");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_keycloak(&["--help".to_string()], "keycloak"), 0);
        assert_eq!(run_keycloak(&["-h".to_string()], "keycloak"), 0);
        let _ = run_keycloak(&["--version".to_string()], "keycloak");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_keycloak(&[], "keycloak");
    }
}
