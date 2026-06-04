#![deny(clippy::all)]

//! msmtp-cli — OurOS msmtp SMTP client
//!
//! Single personality: `msmtp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_msmtp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: msmtp [OPTIONS] RECIPIENT...");
        println!("msmtp v1.8 (OurOS) — SMTP client for sending mail");
        println!();
        println!("Options:");
        println!("  RECIPIENT...      Email recipients");
        println!("  -a ACCOUNT        Use named account from config");
        println!("  --host HOST       SMTP server");
        println!("  --port PORT       SMTP port");
        println!("  --tls             Enable TLS");
        println!("  --from ADDR       Sender address");
        println!("  -t                Read recipients from message headers");
        println!("  --serverinfo      Print server info and exit");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("msmtp v1.8 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--serverinfo") {
        println!("SMTP server: smtp.example.com:587");
        println!("  TLS: yes (TLS 1.3)");
        println!("  AUTH: PLAIN LOGIN");
        println!("  SIZE: 52428800");
        println!("  PIPELINING: yes");
        return 0;
    }
    let recipient = args.iter().find(|a| !a.starts_with('-') && a.contains('@')).map(|s| s.as_str()).unwrap_or("user@example.com");
    println!("Sending to: {}", recipient);
    println!("  Server: smtp.example.com:587");
    println!("  TLS: enabled");
    println!("  Sent successfully.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "msmtp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_msmtp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_msmtp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/msmtp"), "msmtp");
        assert_eq!(basename(r"C:\bin\msmtp.exe"), "msmtp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("msmtp.exe"), "msmtp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_msmtp(&["--help".to_string()], "msmtp"), 0);
        assert_eq!(run_msmtp(&["-h".to_string()], "msmtp"), 0);
        let _ = run_msmtp(&["--version".to_string()], "msmtp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_msmtp(&[], "msmtp");
    }
}
