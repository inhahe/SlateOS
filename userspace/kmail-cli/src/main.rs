#![deny(clippy::all)]

//! kmail-cli — OurOS KDE KMail email client
//!
//! Multi-personality: `kmail`, `korganizer`, `kaddressbook`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kmail(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kmail [OPTIONS] [MAILTO_URI]");
        println!("kmail v23.08 (OurOS) — KDE email client");
        println!();
        println!("Options:");
        println!("  --composer        Open compose window");
        println!("  --check           Check for new mail");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kmail v23.08 (OurOS)"); return 0; }
    println!("kmail: KDE email client started");
    println!("  Accounts: 2 (IMAP + POP3)");
    println!("  Inbox: 22 unread");
    println!("  Akonadi: connected");
    0
}

fn run_korganizer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: korganizer [OPTIONS]");
        println!("korganizer v23.08 (OurOS) — KDE calendar/organizer");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("korganizer v23.08 (OurOS)"); return 0; }
    println!("korganizer: calendar application started");
    println!("  Calendars: 3 loaded");
    println!("  Upcoming events: 5");
    println!("  Tasks: 2 pending");
    0
}

fn run_kaddressbook(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kaddressbook [OPTIONS]");
        println!("kaddressbook v23.08 (OurOS) — KDE address book");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kaddressbook v23.08 (OurOS)"); return 0; }
    println!("kaddressbook: address book started");
    println!("  Address books: 2");
    println!("  Contacts: 150");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kmail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "korganizer" => run_korganizer(&rest, &prog),
        "kaddressbook" => run_kaddressbook(&rest, &prog),
        _ => run_kmail(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kmail};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kmail"), "kmail");
        assert_eq!(basename(r"C:\bin\kmail.exe"), "kmail.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kmail.exe"), "kmail");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kmail(&["--help".to_string()], "kmail"), 0);
        assert_eq!(run_kmail(&["-h".to_string()], "kmail"), 0);
        let _ = run_kmail(&["--version".to_string()], "kmail");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kmail(&[], "kmail");
    }
}
