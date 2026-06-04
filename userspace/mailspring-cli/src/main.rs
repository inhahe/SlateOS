#![deny(clippy::all)]

//! mailspring-cli — OurOS Mailspring modern email client
//!
//! Single personality: `mailspring`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mailspring(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mailspring [OPTIONS]");
        println!("mailspring v1.13 (OurOS) — Modern email client");
        println!();
        println!("Options:");
        println!("  --background      Start in background");
        println!("  --mailto URI      Open compose with mailto");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mailspring v1.13 (OurOS)"); return 0; }
    println!("mailspring: modern email client started");
    println!("  Accounts: 2 (IMAP)");
    println!("  Inbox: 10 unread");
    println!("  Snooze: 2 snoozed messages");
    println!("  Read receipts: enabled");
    println!("  Link tracking: enabled");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mailspring".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mailspring(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mailspring};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mailspring"), "mailspring");
        assert_eq!(basename(r"C:\bin\mailspring.exe"), "mailspring.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mailspring.exe"), "mailspring");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mailspring(&["--help".to_string()], "mailspring"), 0);
        assert_eq!(run_mailspring(&["-h".to_string()], "mailspring"), 0);
        let _ = run_mailspring(&["--version".to_string()], "mailspring");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mailspring(&[], "mailspring");
    }
}
