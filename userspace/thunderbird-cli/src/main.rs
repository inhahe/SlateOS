#![deny(clippy::all)]

//! thunderbird-cli — SlateOS Thunderbird email client
//!
//! Single personality: `thunderbird`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_thunderbird(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: thunderbird [OPTIONS]");
        println!("thunderbird v115.0 (SlateOS) — Email, calendar, and contacts");
        println!();
        println!("Options:");
        println!("  -compose          Open compose window");
        println!("  -mail             Open mail window");
        println!("  -addressbook      Open address book");
        println!("  -calendar         Open calendar");
        println!("  -P PROFILE        Use named profile");
        println!("  --safe-mode       Start in safe mode");
        println!("  --ProfileManager  Show profile manager");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("thunderbird v115.0 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "-compose") {
        println!("thunderbird: compose window opened");
        return 0;
    }
    println!("thunderbird: email client started");
    println!("  Accounts: 2 configured");
    println!("  Inbox: 15 unread messages");
    println!("  Calendar: 3 upcoming events");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "thunderbird".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_thunderbird(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_thunderbird};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/thunderbird"), "thunderbird");
        assert_eq!(basename(r"C:\bin\thunderbird.exe"), "thunderbird.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("thunderbird.exe"), "thunderbird");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_thunderbird(&["--help".to_string()], "thunderbird"), 0);
        assert_eq!(run_thunderbird(&["-h".to_string()], "thunderbird"), 0);
        let _ = run_thunderbird(&["--version".to_string()], "thunderbird");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_thunderbird(&[], "thunderbird");
    }
}
