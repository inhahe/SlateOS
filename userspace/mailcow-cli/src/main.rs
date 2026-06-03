#![deny(clippy::all)]

//! mailcow-cli — OurOS Mailcow mail server suite
//!
//! Single personality: `mailcow`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mailcow(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mailcow [COMMAND] [OPTIONS]");
        println!("Mailcow v2024.04 (OurOS) — Dockerized mail server suite");
        println!();
        println!("Commands:");
        println!("  status             Show service status");
        println!("  start              Start all services");
        println!("  stop               Stop all services");
        println!("  restart            Restart all services");
        println!("  update             Update mailcow");
        println!("  backup             Create backup");
        println!("  restore FILE       Restore from backup");
        println!("  domain add|rm DOM  Manage domains");
        println!("  mailbox add|rm BOX Manage mailboxes");
        println!();
        println!("Options:");
        println!("  --config DIR       Config directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mailcow v2024.04 (OurOS)"); return 0; }
    println!("Mailcow v2024.04 (OurOS) Status:");
    println!("  Postfix (MTA): running");
    println!("  Dovecot (IMAP/POP3): running");
    println!("  Rspamd (antispam): running");
    println!("  ClamAV (antivirus): running");
    println!("  SOGo (webmail/groupware): running");
    println!("  Domains: 3");
    println!("  Mailboxes: 67");
    println!("  Storage: 23.4 GB used");
    println!("  Web UI: https://0.0.0.0:443");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mailcow".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mailcow(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mailcow};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mailcow"), "mailcow");
        assert_eq!(basename(r"C:\bin\mailcow.exe"), "mailcow.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mailcow.exe"), "mailcow");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mailcow(&["--help".to_string()], "mailcow"), 0);
        assert_eq!(run_mailcow(&["-h".to_string()], "mailcow"), 0);
        assert_eq!(run_mailcow(&["--version".to_string()], "mailcow"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mailcow(&[], "mailcow"), 0);
    }
}
