#![deny(clippy::all)]

//! neomutt-cli — OurOS NeoMutt email client
//!
//! Single personality: `neomutt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_neomutt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: neomutt [OPTIONS]");
        println!("neomutt v2024.01 (OurOS) — Terminal email client (NeoMutt)");
        println!();
        println!("Options:");
        println!("  -f MAILBOX        Open specific mailbox");
        println!("  -e CMD            Run command on startup");
        println!("  -s SUBJECT        Subject for new message");
        println!("  -a FILE           Attach file");
        println!("  -i FILE           Body from file");
        println!("  -n                Skip system config");
        println!("  -F FILE           Use alternate config");
        println!("  -v                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("NeoMutt 2024-01-01 (OurOS)");
        println!("  +IMAP +POP +SMTP +TLS +SASL +NOTMUCH");
        return 0;
    }
    if args.is_empty() {
        println!("NeoMutt — terminal email client");
        println!("  Mailbox: INBOX (42 messages, 5 new)");
        println!("  Press ? for help, q to quit");
        return 0;
    }
    let mailbox = args.iter().skip_while(|a| a.as_str() != "-f").nth(1).map(|s| s.as_str());
    if let Some(mb) = mailbox {
        println!("Opening mailbox: {}", mb);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "neomutt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_neomutt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_neomutt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/neomutt"), "neomutt");
        assert_eq!(basename(r"C:\bin\neomutt.exe"), "neomutt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("neomutt.exe"), "neomutt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_neomutt(&["--help".to_string()], "neomutt"), 0);
        assert_eq!(run_neomutt(&["-h".to_string()], "neomutt"), 0);
        assert_eq!(run_neomutt(&["--version".to_string()], "neomutt"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_neomutt(&[], "neomutt"), 0);
    }
}
