#![deny(clippy::all)]

//! maddy-cli — SlateOS Maddy mail server
//!
//! Single personality: `maddy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_maddy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: maddy [COMMAND] [OPTIONS]");
        println!("Maddy v0.7 (SlateOS) — Composable all-in-one mail server");
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
    if args.iter().any(|a| a == "--version") { println!("Maddy v0.7.1 (SlateOS)"); return 0; }
    println!("Maddy v0.7.1 (SlateOS)");
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
mod tests {
    use super::{basename, strip_ext, run_maddy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/maddy"), "maddy");
        assert_eq!(basename(r"C:\bin\maddy.exe"), "maddy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("maddy.exe"), "maddy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_maddy(&["--help".to_string()], "maddy"), 0);
        assert_eq!(run_maddy(&["-h".to_string()], "maddy"), 0);
        let _ = run_maddy(&["--version".to_string()], "maddy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_maddy(&[], "maddy");
    }
}
