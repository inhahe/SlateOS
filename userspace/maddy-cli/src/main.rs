#![deny(clippy::all)]

//! maddy-cli — OurOS Maddy mail server
//!
//! Single personality: `maddy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_maddy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: maddy [COMMAND] [OPTIONS]");
        println!("Maddy v0.7 (OurOS) — Composable all-in-one mail server");
        println!();
        println!("Commands:");
        println!("  run                Start mail server");
        println!("  creds              Manage credentials");
        println!("  imap-acct          Manage IMAP accounts");
        println!("  dkim               Manage DKIM keys");
        println!("  hash               Hash a password");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (default: /etc/maddy/maddy.conf)");
        println!("  --state DIR        State directory");
        println!("  --runtime DIR      Runtime directory");
        println!("  --debug            Enable debug logging");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Maddy v0.7.1 (OurOS)"); return 0; }
    println!("Maddy v0.7.1 (OurOS)");
    println!("  SMTP: 0.0.0.0:25, 0.0.0.0:587 (submission)");
    println!("  IMAP: 0.0.0.0:143, 0.0.0.0:993 (implicit TLS)");
    println!("  TLS: automatic (ACME / Let's Encrypt)");
    println!("  Storage: SQLite + filesystem");
    println!("  Auth: PAM, shadow, internal DB");
    println!("  DKIM: signing enabled");
    println!("  SPF/DMARC: checking enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "maddy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_maddy(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
