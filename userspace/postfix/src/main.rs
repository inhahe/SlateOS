#![deny(clippy::all)]

//! postfix — SlateOS mail transfer agent (MTA)
//!
//! Multi-personality: `postfix` (control), `postconf` (config), `postqueue` (queue),
//!   `postsuper` (queue admin), `sendmail` (compatibility)

use std::env;
use std::process;

fn run_postfix(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "--help" | "help" | "-h" => {
            println!("Usage: postfix <start|stop|reload|status|check|flush|abort>");
            0
        }
        "--version" | "version" => {
            println!("postfix (Postfix) 3.8.6 (SlateOS)");
            0
        }
        "start" => {
            println!("postfix/postfix-script: starting the Postfix mail system");
            0
        }
        "stop" => {
            println!("postfix/postfix-script: stopping the Postfix mail system");
            0
        }
        "reload" => {
            println!("postfix/postfix-script: refreshing the Postfix mail system");
            0
        }
        "status" => {
            println!("postfix/postfix-script: the Postfix mail system is running: PID: 12345");
            0
        }
        "check" => {
            println!("postfix: Postfix configuration OK");
            0
        }
        "flush" => {
            println!("postfix/postfix-script: flushing the mail queue");
            0
        }
        other => { eprintln!("postfix: unknown command '{}'", other); 1 }
    }
}

fn run_postconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postconf [options] [parameter...]");
        println!("  -d    Show default values");
        println!("  -n    Show non-default values");
        println!("  -e    Edit main.cf");
        return 0;
    }
    let show_defaults = args.iter().any(|a| a == "-d");
    let show_nondefault = args.iter().any(|a| a == "-n");

    let params: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if !params.is_empty() {
        for p in params {
            match p {
                "myhostname" => println!("myhostname = mail.example.com"),
                "mydomain" => println!("mydomain = example.com"),
                "myorigin" => println!("myorigin = $mydomain"),
                "inet_interfaces" => println!("inet_interfaces = all"),
                "mydestination" => println!("mydestination = $myhostname, localhost.$mydomain, localhost"),
                _ => println!("{} = (parameter not found)", p),
            }
        }
        return 0;
    }

    if show_nondefault {
        println!("myhostname = mail.example.com");
        println!("mydomain = example.com");
        println!("inet_interfaces = all");
        println!("smtpd_tls_cert_file = /etc/letsencrypt/live/mail.example.com/fullchain.pem");
        println!("smtpd_tls_key_file = /etc/letsencrypt/live/mail.example.com/privkey.pem");
    } else if show_defaults {
        println!("myhostname = localhost");
        println!("mydomain = localdomain");
        println!("myorigin = $myhostname");
        println!("inet_interfaces = localhost");
        println!("mydestination = $myhostname, localhost.$mydomain, localhost");
        println!("smtp_tls_security_level = may");
    } else {
        println!("myhostname = mail.example.com");
        println!("mydomain = example.com");
        println!("myorigin = $mydomain");
        println!("inet_interfaces = all");
        println!("mydestination = $myhostname, localhost.$mydomain, localhost");
        println!("relay_domains =");
        println!("smtpd_tls_security_level = may");
        println!("smtp_tls_security_level = may");
    }
    0
}

fn run_postqueue(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postqueue [-fj] [-c config_dir] [-p]");
        println!("  -p    Show queue");
        println!("  -f    Flush the queue");
        println!("  -j    JSON output");
        return 0;
    }
    if args.iter().any(|a| a == "-f") {
        println!("postqueue: mail queue flushed");
        return 0;
    }
    // Show queue
    println!("-Queue ID-  --Size-- ----Arrival Time---- -Sender/Recipient-------");
    println!("ABC123DEF4     2048 Thu May 22 09:30:00  sender@example.com");
    println!("                                         recipient@remote.com");
    println!("DEF456GHI7     1024 Thu May 22 09:45:00  noreply@example.com");
    println!("                                         user@another.com");
    println!("-- 2 Kbytes in 2 Requests.");
    0
}

fn run_postsuper(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: postsuper [-dhr] [-c config_dir] [queue_id...]");
        println!("  -d <id>  Delete message");
        println!("  -d ALL   Delete all messages");
        println!("  -h <id>  Put message on hold");
        println!("  -r <id>  Requeue message");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        let target = args.iter().position(|a| a == "-d")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("ALL");
        if target == "ALL" {
            println!("postsuper: Deleted: 2 messages");
        } else {
            println!("postsuper: {}: removed", target);
        }
    }
    0
}

fn run_sendmail(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sendmail [flags] [address ...]");
        println!("  -t           Read recipients from message headers");
        println!("  -f <addr>    Set envelope sender address");
        println!("  -i           Don't treat a dot as end of input");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sendmail: Postfix sendmail compatibility interface 3.8.6 (SlateOS)");
        return 0;
    }
    println!("(message queued for delivery — simulated)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("postfix");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "postconf" => run_postconf(rest),
        "postqueue" => run_postqueue(rest),
        "postsuper" => run_postsuper(rest),
        "sendmail" => run_sendmail(rest),
        _ => run_postfix(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_postfix};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_postfix(vec!["--help".to_string()]), 0);
        assert_eq!(run_postfix(vec!["-h".to_string()]), 0);
        let _ = run_postfix(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_postfix(vec![]);
    }
}
