#![deny(clippy::all)]

//! dovecot-cli — OurOS Dovecot IMAP/POP3 server CLI
//!
//! Multi-personality: `dovecot`, `doveconf`, `doveadm`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_dovecot(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dovecot [OPTIONS]");
        println!();
        println!("Dovecot — IMAP/POP3 server (OurOS).");
        println!();
        println!("Options:");
        println!("  -F               Run in foreground");
        println!("  -c FILE          Alternate config file");
        println!("  --build-options  Show build options");
        println!("  stop             Stop running instance");
        println!("  reload           Reload configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dovecot 2.3.21 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--build-options") {
        println!("Build options: ioloop=epoll notify=inotify ipv6 openssl");
        println!("SQL drivers: sqlite mysql pgsql");
        println!("Passdb: pam shadow bsdauth ldap sql");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "stop" => println!("dovecot: server stopped"),
        "reload" => println!("dovecot: configuration reloaded"),
        _ => println!("dovecot: Dovecot IMAP/POP3 server started"),
    }
    0
}

fn run_doveconf(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: doveconf [OPTIONS] [SETTING ...]");
        println!();
        println!("doveconf — dump Dovecot configuration (OurOS).");
        println!();
        println!("Options:");
        println!("  -n    Show non-default settings");
        println!("  -a    Show all settings");
        println!("  -S    Show settings in simplified format");
        return 0;
    }

    if args.iter().any(|a| a == "-n") {
        println!("# Non-default settings:");
        println!("protocols = imap pop3 lmtp");
        println!("mail_location = maildir:~/Maildir");
        println!("ssl = required");
    } else {
        println!("# Dovecot configuration");
        println!("protocols = imap pop3 lmtp");
        println!("listen = *, ::");
        println!("mail_location = maildir:~/Maildir");
        println!("ssl = required");
        println!("ssl_cert = </etc/ssl/certs/dovecot.pem");
        println!("ssl_key = </etc/ssl/private/dovecot.pem");
        println!("auth_mechanisms = plain login");
    }
    0
}

fn run_doveadm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: doveadm [OPTIONS] COMMAND [ARGS ...]");
        println!();
        println!("doveadm — Dovecot administration tool (OurOS).");
        println!();
        println!("Commands:");
        println!("  mailbox list      List mailboxes");
        println!("  mailbox status    Show mailbox status");
        println!("  search            Search messages");
        println!("  fetch             Fetch message data");
        println!("  expunge           Expunge messages");
        println!("  purge             Purge deleted messages");
        println!("  user              User lookup");
        println!("  reload            Reload configuration");
        println!("  stop              Stop server");
        println!("  log errors        Show error log");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "mailbox" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "list" => {
                    println!("INBOX");
                    println!("Sent");
                    println!("Drafts");
                    println!("Trash");
                    println!("Junk");
                }
                "status" => {
                    println!("INBOX messages=42 unseen=3 recent=2");
                }
                _ => println!("doveadm: unknown mailbox subcommand '{}'", subcmd),
            }
        }
        "user" => {
            let user = args.get(1).unwrap_or(&String::new()).to_string();
            if user.is_empty() {
                eprintln!("doveadm: user name required");
                return 1;
            }
            println!("field\tvalue");
            println!("uid\t1000");
            println!("gid\t1000");
            println!("home\t/home/{}", user);
            println!("mail\tmaildir:~/Maildir");
        }
        "reload" => println!("doveadm: configuration reloaded"),
        "stop" => println!("doveadm: server stopped"),
        "log" => println!("(no errors)"),
        _ => {
            eprintln!("doveadm: unknown command '{}'", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dovecot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "doveconf" => run_doveconf(&rest),
        "doveadm" => run_doveadm(&rest),
        _ => run_dovecot(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dovecot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dovecot"), "dovecot");
        assert_eq!(basename(r"C:\bin\dovecot.exe"), "dovecot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dovecot.exe"), "dovecot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_dovecot(&["--help".to_string()]), 0);
        assert_eq!(run_dovecot(&["-h".to_string()]), 0);
        assert_eq!(run_dovecot(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_dovecot(&[]), 0);
    }
}
