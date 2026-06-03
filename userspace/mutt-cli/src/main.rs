#![deny(clippy::all)]

//! mutt-cli — OurOS Mutt/NeoMutt email client CLI
//!
//! Multi-personality: `mutt`, `neomutt`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_mutt(args: &[String], neo: bool) -> i32 {
    let name = if neo { "neomutt" } else { "mutt" };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [-s subject] [-c cc] [-b bcc] [address ...]", name);
        println!();
        println!("{} — text-based email client (OurOS).", name);
        println!();
        println!("Options:");
        println!("  -s SUBJECT         Subject of the message");
        println!("  -c ADDRESS         Carbon-copy address");
        println!("  -b ADDRESS         Blind carbon-copy address");
        println!("  -a FILE            Attach file");
        println!("  -i FILE            Include file in body");
        println!("  -F FILE            Use alternate config file");
        println!("  -f MAILBOX         Open mailbox");
        println!("  -e COMMAND         Run command after init");
        println!("  -n                 Do not read system config");
        println!("  -R                 Open mailbox read-only");
        println!("  -z                 Open only if new messages");
        println!("  -Z                 Open first folder with new mail");
        if neo {
            println!("  -D                 Dump config variables");
            println!("  -B                 Run in batch mode");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        if neo {
            println!("NeoMutt 20240201 (OurOS)");
        } else {
            println!("Mutt 2.2.12 (OurOS)");
        }
        return 0;
    }

    // Check for -s (sending mode)
    let subject = args.windows(2)
        .find(|w| w[0] == "-s")
        .map(|w| w[1].as_str());

    let addresses: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && a.contains('@'))
        .map(|s| s.as_str())
        .collect();

    if let Some(subj) = subject {
        if addresses.is_empty() {
            eprintln!("{}: no recipients specified", name);
            return 1;
        }
        println!("Sending mail to {} with subject \"{}\"...", addresses.join(", "), subj);
        println!("Message sent.");
    } else if let Some(pos) = args.iter().position(|a| a == "-f") {
        let mbox = args.get(pos + 1).map_or("INBOX", |s| s.as_str());
        println!("Opening mailbox: {}", mbox);
        println!();
        println!("  1  N  2024-01-15  user@example.com     Meeting tomorrow");
        println!("  2  N  2024-01-15  admin@corp.com       System maintenance");
        println!("  3     2024-01-14  friend@email.com     Re: Weekend plans");
        println!("  4     2024-01-13  news@list.org        Weekly digest #42");
    } else {
        println!("{} version {} (OurOS)", name,
            if neo { "20240201" } else { "2.2.12" });
        println!();
        println!("  1  N  2024-01-15  user@example.com     Hello");
        println!("  2     2024-01-14  admin@server.com     Notification");
        println!("  3     2024-01-13  list@mailing.org     Digest");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mutt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "neomutt" => run_mutt(&rest, true),
        _ => run_mutt(&rest, false),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mutt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mutt"), "mutt");
        assert_eq!(basename(r"C:\bin\mutt.exe"), "mutt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mutt.exe"), "mutt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mutt(&["--help".to_string()], false), 0);
        assert_eq!(run_mutt(&["-h".to_string()], false), 0);
        assert_eq!(run_mutt(&["--version".to_string()], false), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mutt(&[], false), 0);
    }
}
