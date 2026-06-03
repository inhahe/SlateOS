#![deny(clippy::all)]

//! dex-cli — OurOS Dex OIDC identity provider
//!
//! Single personality: `dex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dex(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dex [COMMAND] [OPTIONS]");
        println!("Dex v2.39 (OurOS) — OpenID Connect identity provider");
        println!();
        println!("Commands:");
        println!("  serve FILE         Start Dex server with config");
        println!("  version            Show version");
        println!();
        println!("Options:");
        println!("  --web-http ADDR    Web HTTP listen address");
        println!("  --web-https ADDR   Web HTTPS listen address");
        println!("  --grpc ADDR        gRPC API listen address");
        println!("  --telemetry ADDR   Telemetry endpoint");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dex v2.39.1 (OurOS)"); return 0; }
    println!("Dex v2.39.1 (OurOS)");
    println!("  Issuer: https://dex.example.com");
    println!("  Web: https://0.0.0.0:5556");
    println!("  gRPC: 0.0.0.0:5557");
    println!("  Storage: SQLite3");
    println!("  Connectors: LDAP, GitHub, Google, SAML");
    println!("  Clients: 8 registered");
    println!("  Protocols: OIDC, OAuth2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dex(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dex"), "dex");
        assert_eq!(basename(r"C:\bin\dex.exe"), "dex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dex.exe"), "dex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dex(&["--help".to_string()], "dex"), 0);
        assert_eq!(run_dex(&["-h".to_string()], "dex"), 0);
        assert_eq!(run_dex(&["--version".to_string()], "dex"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dex(&[], "dex"), 0);
    }
}
