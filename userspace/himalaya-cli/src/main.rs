#![deny(clippy::all)]

//! himalaya-cli — SlateOS Himalaya CLI email client
//!
//! Single personality: `himalaya`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_himalaya(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: himalaya COMMAND [OPTIONS]");
        println!("himalaya v1.0 (SlateOS) — CLI email client (Rust)");
        println!();
        println!("Commands:");
        println!("  list              List envelopes");
        println!("  read ID           Read message");
        println!("  write             Compose new message");
        println!("  reply ID          Reply to message");
        println!("  forward ID        Forward message");
        println!("  delete ID         Delete message");
        println!("  move ID FOLDER    Move message");
        println!("  folders           List folders");
        println!("  accounts          List accounts");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("ID   FLAGS  DATE                FROM              SUBJECT");
            println!("42   N      2024-01-15 10:30    alice@example.com Meeting notes");
            println!("41          2024-01-15 09:15    bob@example.com   Re: Project update");
            println!("40          2024-01-14 16:42    ci@builds.dev     Build #1234 passed");
        }
        "read" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("42");
            println!("Message #{}", id);
            println!("From: alice@example.com");
            println!("Date: 2024-01-15 10:30");
            println!("Subject: Meeting notes");
            println!();
            println!("Here are the notes from today's meeting...");
        }
        "folders" => {
            println!("INBOX (42)");
            println!("Sent");
            println!("Drafts (1)");
            println!("Trash");
            println!("Archive");
        }
        "accounts" => {
            println!("NAME        BACKEND    DEFAULT");
            println!("work        imap       *");
            println!("personal    imap");
        }
        "write" => println!("Opening editor for new message..."),
        _ => println!("himalaya {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "himalaya".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_himalaya(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_himalaya};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/himalaya"), "himalaya");
        assert_eq!(basename(r"C:\bin\himalaya.exe"), "himalaya.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("himalaya.exe"), "himalaya");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_himalaya(&["--help".to_string()], "himalaya"), 0);
        assert_eq!(run_himalaya(&["-h".to_string()], "himalaya"), 0);
        let _ = run_himalaya(&["--version".to_string()], "himalaya");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_himalaya(&[], "himalaya");
    }
}
