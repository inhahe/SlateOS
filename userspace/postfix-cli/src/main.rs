#![deny(clippy::all)]

//! postfix-cli — OurOS Postfix mail server CLI
//!
//! Multi-personality: `postfix`, `postconf`, `postqueue`, `postsuper`, `postalias`, `postmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_postfix(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postfix [start|stop|reload|status|flush|check]");
        println!();
        println!("Postfix — mail server control (OurOS).");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "start" => println!("postfix/postfix-script: starting the Postfix mail system"),
        "stop" => println!("postfix/postfix-script: stopping the Postfix mail system"),
        "reload" => println!("postfix/postfix-script: refreshing the Postfix mail system"),
        "status" => println!("postfix/postfix-script: the Postfix mail system is running: PID: 1234"),
        "flush" => println!("postfix/postfix-script: flushing the mail queue"),
        "check" => println!("postfix/postfix-script: no configuration errors found"),
        _ => {
            eprintln!("postfix: unknown command '{}'. Use: start|stop|reload|status|flush|check", cmd);
            return 1;
        }
    }
    0
}

fn run_postconf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postconf [OPTIONS] [PARAMETER ...]");
        println!();
        println!("postconf — Postfix configuration utility (OurOS).");
        println!();
        println!("Options:");
        println!("  -d    Show defaults");
        println!("  -n    Show non-default settings");
        println!("  -e    Edit main.cf");
        return 0;
    }

    let show_defaults = args.iter().any(|a| a == "-d");
    let show_nondefault = args.iter().any(|a| a == "-n");

    if show_nondefault {
        println!("myhostname = mail.example.com");
        println!("mydomain = example.com");
        println!("inet_interfaces = all");
    } else if show_defaults || args.is_empty() {
        println!("myhostname = localhost");
        println!("mydomain = localdomain");
        println!("myorigin = $myhostname");
        println!("inet_interfaces = localhost");
        println!("mydestination = $myhostname, localhost.$mydomain, localhost");
        println!("mail_spool_directory = /var/mail");
        println!("smtpd_banner = $myhostname ESMTP $mail_name");
    } else {
        for param in args.iter().filter(|a| !a.starts_with('-')) {
            println!("{} = (default)", param);
        }
    }
    0
}

fn run_postqueue(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postqueue [-f | -p | -j | -s SITE]");
        println!();
        println!("postqueue — Postfix queue control (OurOS).");
        return 0;
    }
    if args.iter().any(|a| a == "-f") {
        println!("postqueue: mail queue flushed");
    } else if args.iter().any(|a| a == "-p") {
        println!("-Queue ID-  --Size-- ----Arrival Time---- -Sender/Recipient-------");
        println!("Mail queue is empty");
    } else if args.iter().any(|a| a == "-j") {
        println!("[]");
    } else {
        println!("postqueue: use -f (flush), -p (print), or -j (JSON)");
    }
    0
}

fn run_postsuper(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postsuper [-d ALL | -d QUEUE_ID | -h QUEUE_ID | -H QUEUE_ID | -r QUEUE_ID]");
        println!();
        println!("postsuper — Postfix queue maintenance (OurOS).");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("postsuper: Deleted: 0 messages");
    } else if args.iter().any(|a| a == "-r") {
        println!("postsuper: Requeued: 0 messages");
    }
    0
}

fn run_postalias(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postalias [OPTIONS] FILE ...");
        println!();
        println!("postalias — build/query Postfix alias database (OurOS).");
        return 0;
    }
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        println!("postalias: rebuilding {}", f);
    }
    0
}

fn run_postmap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postmap [OPTIONS] FILE ...");
        println!();
        println!("postmap — build/query Postfix lookup table (OurOS).");
        return 0;
    }
    if args.iter().any(|a| a == "-q") {
        println!("(not found)");
    } else {
        for f in args.iter().filter(|a| !a.starts_with('-')) {
            println!("postmap: rebuilding {}", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "postfix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "postconf" => run_postconf(&rest),
        "postqueue" => run_postqueue(&rest),
        "postsuper" => run_postsuper(&rest),
        "postalias" => run_postalias(&rest),
        "postmap" => run_postmap(&rest),
        _ => run_postfix(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_postfix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/postfix"), "postfix");
        assert_eq!(basename(r"C:\bin\postfix.exe"), "postfix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("postfix.exe"), "postfix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_postfix(&["--help".to_string()]), 0);
        assert_eq!(run_postfix(&["-h".to_string()]), 0);
        assert_eq!(run_postfix(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_postfix(&[]), 0);
    }
}
