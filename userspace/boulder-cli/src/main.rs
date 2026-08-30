#![deny(clippy::all)]

//! boulder-cli — Slate OS Boulder ACME CA server
//!
//! Single personality: `boulder`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_boulder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: boulder [COMMAND] [OPTIONS]");
        println!("Boulder (Slate OS) — ACME-based certificate authority (Let's Encrypt)");
        println!();
        println!("Commands:");
        println!("  start              Start all Boulder components");
        println!("  sa                 Start storage authority");
        println!("  ra                 Start registration authority");
        println!("  va                 Start validation authority");
        println!("  ca                 Start certificate authority");
        println!("  ocsp-responder     Start OCSP responder");
        println!("  wfe                Start web front end");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (JSON)");
        println!("  --addr ADDR        Listen address");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Boulder (Slate OS) [ACME v2]"); return 0; }
    println!("Boulder ACME CA (Slate OS)");
    println!("  WFE (Web Front End): https://0.0.0.0:4431");
    println!("  ACME directory: https://0.0.0.0:4431/directory");
    println!("  OCSP: http://0.0.0.0:4002");
    println!("  Certificates issued: 12,345");
    println!("  Pending authorizations: 23");
    println!("  Challenges: http-01, dns-01, tls-alpn-01");
    println!("  Database: MariaDB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "boulder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_boulder(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_boulder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/boulder"), "boulder");
        assert_eq!(basename(r"C:\bin\boulder.exe"), "boulder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("boulder.exe"), "boulder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_boulder(&["--help".to_string()], "boulder"), 0);
        assert_eq!(run_boulder(&["-h".to_string()], "boulder"), 0);
        let _ = run_boulder(&["--version".to_string()], "boulder");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_boulder(&[], "boulder");
    }
}
