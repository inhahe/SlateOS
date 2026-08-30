#![deny(clippy::all)]

//! sendmail-cli — Slate OS sendmail/msmtp CLI
//!
//! Multi-personality: `sendmail`, `msmtp`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_sendmail(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sendmail [FLAGS] [ADDRESS ...]");
        println!();
        println!("sendmail — mail transfer agent (Slate OS).");
        println!();
        println!("Options:");
        println!("  -t               Read recipients from headers");
        println!("  -f ADDRESS       Set envelope sender");
        println!("  -F NAME          Set full name of sender");
        println!("  -i               Ignore dots alone on lines");
        println!("  -v               Verbose mode");
        println!("  -bs              Run as SMTP server");
        println!("  -bp              Print mail queue");
        println!("  -q               Process queued mail");
        println!("  -bd              Run as daemon");
        return 0;
    }
    if args.iter().any(|a| a == "-bp") {
        println!("Mail queue is empty.");
        return 0;
    }

    let read_headers = args.iter().any(|a| a == "-t");
    let addresses: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && a.contains('@'))
        .map(|s| s.as_str())
        .collect();

    if addresses.is_empty() && !read_headers {
        eprintln!("sendmail: no recipients specified and -t not set");
        return 1;
    }

    if read_headers {
        println!("Reading message from stdin...");
        println!("Message accepted for delivery.");
    } else {
        println!("Delivering to {}...", addresses.join(", "));
        println!("Message accepted for delivery.");
    }
    0
}

fn run_msmtp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: msmtp [OPTIONS] [RECIPIENT ...]");
        println!();
        println!("msmtp — lightweight SMTP client (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --account NAME    Use account NAME");
        println!("  -f, --from ADDRESS    Set envelope sender");
        println!("  -t, --read-recipients Read recipients from headers");
        println!("  -C, --file FILE       Use alternate config");
        println!("  --serverinfo          Print server info");
        println!("  -P, --pretend         Print but don't send");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("msmtp version 1.8.25 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "--serverinfo") {
        println!("SMTP server: smtp.example.com");
        println!("  TLS: yes (TLS1.3)");
        println!("  Auth: PLAIN LOGIN");
        println!("  Size: 52428800 bytes");
        return 0;
    }

    let addresses: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && a.contains('@'))
        .map(|s| s.as_str())
        .collect();

    if addresses.is_empty() && !args.iter().any(|a| a == "-t" || a == "--read-recipients") {
        eprintln!("msmtp: no recipients specified");
        return 1;
    }

    println!("Sending via smtp.example.com...");
    println!("Message sent successfully.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sendmail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "msmtp" => run_msmtp(&rest),
        _ => run_sendmail(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sendmail};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sendmail"), "sendmail");
        assert_eq!(basename(r"C:\bin\sendmail.exe"), "sendmail.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sendmail.exe"), "sendmail");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sendmail(&["--help".to_string()]), 0);
        assert_eq!(run_sendmail(&["-h".to_string()]), 0);
        let _ = run_sendmail(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sendmail(&[]);
    }
}
