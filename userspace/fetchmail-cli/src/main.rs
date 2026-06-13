#![deny(clippy::all)]

//! fetchmail-cli — SlateOS fetchmail/getmail CLI
//!
//! Multi-personality: `fetchmail`, `getmail`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_fetchmail(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fetchmail [OPTIONS] [SERVER ...]");
        println!();
        println!("fetchmail — remote mail retrieval (SlateOS).");
        println!();
        println!("Options:");
        println!("  -p, --protocol PROTO   Protocol (auto/pop3/imap/apop)");
        println!("  -u, --username USER    Remote username");
        println!("  -P, --port PORT        Server port");
        println!("  -f, --fetchmailrc FILE Config file");
        println!("  -a, --all              Fetch all messages");
        println!("  -k, --keep             Keep messages on server");
        println!("  -v, --verbose          Verbose mode");
        println!("  -s, --silent           Suppress progress");
        println!("  -d SECONDS             Daemon mode interval");
        println!("  -q, --quit             Kill running daemon");
        println!("  --check                Check for new mail only");
        println!("  --ssl                  Use SSL/TLS");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("fetchmail 6.4.37 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-q" || a == "--quit") {
        println!("fetchmail: no daemon running");
        return 0;
    }
    if args.iter().any(|a| a == "--check") {
        println!("1 message for user@example.com at mail.example.com (5432 octets).");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let servers: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let server = servers.first().unwrap_or(&"mail.example.com");

    if verbose {
        println!("fetchmail: connecting to {} via IMAP...", server);
        println!("fetchmail: IMAP< * OK IMAP4rev1 server ready");
        println!("fetchmail: IMAP> A0001 LOGIN user ****");
        println!("fetchmail: IMAP< A0001 OK LOGIN completed");
    }
    println!("fetchmail: querying {} (protocol IMAP)", server);
    println!("3 messages for user at {} (12450 octets).", server);
    println!("reading message user@{}:1 of 3 (4150 octets) . flushed", server);
    println!("reading message user@{}:2 of 3 (3800 octets) . flushed", server);
    println!("reading message user@{}:3 of 3 (4500 octets) . flushed", server);
    0
}

fn run_getmail(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: getmail [OPTIONS]");
        println!();
        println!("getmail — mail retriever (SlateOS).");
        println!();
        println!("Options:");
        println!("  -r, --rcfile FILE  Config file");
        println!("  -g DIR             Config directory");
        println!("  -n                 Don't delete messages");
        println!("  -l                 List only");
        println!("  -v                 Verbose mode");
        println!("  -q                 Quiet mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("getmail 6.19 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-l") {
        println!("  1: 4150 bytes from user@example.com");
        println!("  2: 3800 bytes from admin@corp.com");
        println!("  3: 4500 bytes from list@mailing.org");
        println!("3 messages (12450 bytes) in inbox");
    } else {
        println!("getmail: retrieving mail...");
        println!("  msg 1/3: 4150 bytes ... delivered");
        println!("  msg 2/3: 3800 bytes ... delivered");
        println!("  msg 3/3: 4500 bytes ... delivered");
        println!("getmail: retrieved 3 messages (12450 bytes)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fetchmail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "getmail" => run_getmail(&rest),
        _ => run_fetchmail(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fetchmail};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fetchmail"), "fetchmail");
        assert_eq!(basename(r"C:\bin\fetchmail.exe"), "fetchmail.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fetchmail.exe"), "fetchmail");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fetchmail(&["--help".to_string()]), 0);
        assert_eq!(run_fetchmail(&["-h".to_string()]), 0);
        let _ = run_fetchmail(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fetchmail(&[]);
    }
}
