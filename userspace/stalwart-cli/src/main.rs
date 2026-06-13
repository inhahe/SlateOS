#![deny(clippy::all)]

//! stalwart-cli — Slate OS Stalwart Mail Server
//!
//! Single personality: `stalwart-mail`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stalwart(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stalwart-mail [COMMAND] [OPTIONS]");
        println!("Stalwart Mail v0.8 (Slate OS) — All-in-one mail server");
        println!();
        println!("Commands:");
        println!("  serve              Start mail server");
        println!("  account            Manage accounts");
        println!("  domain             Manage domains");
        println!("  import             Import mailboxes (mbox/maildir)");
        println!("  export             Export mailboxes");
        println!("  queue              Manage mail queue");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file (TOML)");
        println!("  --data-dir DIR     Data directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Stalwart Mail v0.8.5 (Slate OS)"); return 0; }
    println!("Stalwart Mail v0.8.5 (Slate OS)");
    println!("  SMTP: 0.0.0.0:25, 0.0.0.0:587");
    println!("  IMAP4: 0.0.0.0:143, 0.0.0.0:993");
    println!("  JMAP: 0.0.0.0:8080");
    println!("  ManageSieve: 0.0.0.0:4190");
    println!("  Accounts: 234");
    println!("  Domains: 5");
    println!("  Storage: RocksDB");
    println!("  Full-text search: built-in");
    println!("  DKIM/SPF/DMARC/ARC: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stalwart-mail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stalwart(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stalwart};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stalwart"), "stalwart");
        assert_eq!(basename(r"C:\bin\stalwart.exe"), "stalwart.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stalwart.exe"), "stalwart");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stalwart(&["--help".to_string()], "stalwart"), 0);
        assert_eq!(run_stalwart(&["-h".to_string()], "stalwart"), 0);
        let _ = run_stalwart(&["--version".to_string()], "stalwart");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stalwart(&[], "stalwart");
    }
}
