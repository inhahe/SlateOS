#![deny(clippy::all)]

//! dovecot — SlateOS IMAP/POP3 mail server
//!
//! Multi-personality: `dovecot` (server), `doveconf` (config dumper), `doveadm` (admin)

use std::env;
use std::process;

fn run_dovecot(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dovecot [options]");
        println!("  -F              Foreground mode");
        println!("  -c <config>     Config file path");
        println!("  --version       Show version");
        println!("  stop            Stop a running dovecot");
        println!("  reload          Reload configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dovecot (Slate OS) 2.3.21");
        return 0;
    }
    if args.iter().any(|a| a == "stop") {
        println!("Stopping dovecot");
        return 0;
    }
    if args.iter().any(|a| a == "reload") {
        println!("dovecot: configuration reloaded");
        return 0;
    }
    println!("May 22 10:00:00 dovecot: master: Dovecot v2.3.21 (Slate OS) starting up");
    println!("May 22 10:00:00 dovecot: master: Listening: imap(0.0.0.0:143), imaps(0.0.0.0:993)");
    println!("May 22 10:00:00 dovecot: master: Listening: pop3(0.0.0.0:110), pop3s(0.0.0.0:995)");
    println!("May 22 10:00:00 dovecot: master: Listening: lmtp(/var/run/dovecot/lmtp)");
    0
}

fn run_doveconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: doveconf [options] [setting...]");
        println!("  -n    Show only non-default settings");
        println!("  -a    Show all settings");
        println!("  -S    Show settings in dovectl format");
        return 0;
    }
    let nondefault = args.iter().any(|a| a == "-n");
    let params: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    if !params.is_empty() {
        for p in params {
            match p {
                "mail_location" => println!("mail_location = maildir:~/Maildir"),
                "protocols" => println!("protocols = imap pop3 lmtp"),
                "ssl" => println!("ssl = required"),
                "ssl_cert" => println!("ssl_cert = </etc/letsencrypt/live/mail.example.com/fullchain.pem"),
                "ssl_key" => println!("ssl_key = </etc/letsencrypt/live/mail.example.com/privkey.pem"),
                _ => println!("{} = (unknown setting)", p),
            }
        }
        return 0;
    }

    if nondefault {
        println!("# 2.3.21 (Slate OS): /etc/dovecot/dovecot.conf");
        println!("protocols = imap pop3 lmtp");
        println!("mail_location = maildir:~/Maildir");
        println!("ssl = required");
        println!("ssl_cert = </etc/letsencrypt/live/mail.example.com/fullchain.pem");
        println!("ssl_key = </etc/letsencrypt/live/mail.example.com/privkey.pem");
    } else {
        println!("# 2.3.21 (Slate OS): /etc/dovecot/dovecot.conf");
        println!("protocols = imap pop3 lmtp");
        println!("listen = *, ::");
        println!("base_dir = /var/run/dovecot/");
        println!("login_greeting = Dovecot ready.");
        println!("mail_location = maildir:~/Maildir");
        println!("ssl = required");
        println!("ssl_cert = </etc/letsencrypt/live/mail.example.com/fullchain.pem");
        println!("ssl_key = </etc/letsencrypt/live/mail.example.com/privkey.pem");
        println!("auth_mechanisms = plain login");
    }
    0
}

fn run_doveadm(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: doveadm <command> [options]");
            println!();
            println!("Commands:");
            println!("  reload          Reload Dovecot configuration");
            println!("  stop            Stop Dovecot");
            println!("  mailbox         Manage mailboxes");
            println!("  user            User lookup");
            println!("  who             Show connected users");
            println!("  kick            Disconnect users");
            println!("  search          Search messages");
            println!("  fetch           Fetch messages");
            println!("  expunge         Expunge messages");
            println!("  purge           Purge deleted messages");
            println!("  quota           Show/recalculate quota");
            println!("  log             Log commands");
            0
        }
        "who" => {
            println!("username                  # proto (pids)           (ips)");
            println!("alice                     2 imap  (12345 12346)   (192.168.1.100)");
            println!("bob                       1 imap  (12347)          (192.168.1.101)");
            println!("charlie                   1 pop3  (12348)          (10.0.0.50)");
            0
        }
        "mailbox" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    let user = cmd_args.iter().position(|a| a == "-u")
                        .and_then(|i| cmd_args.get(i + 1))
                        .map(|s| s.as_str())
                        .unwrap_or("alice");
                    let _ = user;
                    println!("INBOX");
                    println!("Drafts");
                    println!("Sent");
                    println!("Trash");
                    println!("Junk");
                    println!("Archive");
                }
                "status" => {
                    println!("INBOX messages=142 recent=3 unseen=12");
                }
                "create" => println!("Mailbox created"),
                "delete" => println!("Mailbox deleted"),
                _ => println!("Usage: doveadm mailbox <list|status|create|delete>"),
            }
            0
        }
        "user" => {
            let user = cmd_args.first().map(|s| s.as_str()).unwrap_or("alice");
            println!("field\tvalue");
            println!("uid\t1000");
            println!("gid\t1000");
            println!("home\t/home/{}", user);
            println!("mail\tmaildir:~/Maildir");
            0
        }
        "quota" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "get" => {
                    println!("Quota name   Type     Value  Limit  %");
                    println!("User quota   STORAGE  28456  102400 27");
                    println!("User quota   MESSAGE  142    10000  1");
                }
                "recalc" => println!("Quota recalculated"),
                _ => println!("Usage: doveadm quota <get|recalc>"),
            }
            0
        }
        "kick" => {
            let user = cmd_args.first().map(|s| s.as_str()).unwrap_or("user");
            println!("Kicked {} connections for {}", 2, user);
            0
        }
        "reload" => { println!("Configuration reloaded"); 0 }
        "stop" => { println!("Dovecot stopped"); 0 }
        "log" => {
            println!("May 22 10:00:00 imap(alice)<12345>: Logged out in=142 out=28456 bytes=28456");
            0
        }
        other => { eprintln!("doveadm: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("dovecot");
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
        "doveconf" => run_doveconf(rest),
        "doveadm" => run_doveadm(rest),
        _ => run_dovecot(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dovecot};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dovecot(vec!["--help".to_string()]), 0);
        assert_eq!(run_dovecot(vec!["-h".to_string()]), 0);
        let _ = run_dovecot(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dovecot(vec![]);
    }
}
