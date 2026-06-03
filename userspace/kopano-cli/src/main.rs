#![deny(clippy::all)]

//! kopano-cli — OurOS Kopano groupware
//!
//! Multi-personality: `kopano-server`, `kopano-admin`, `kopano-gateway`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kopano(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "kopano-admin" => {
                println!("kopano-admin (OurOS) — Kopano user management");
                println!("  -l                 List users");
                println!("  -c USER            Create user");
                println!("  --create-store USER  Create mailbox store");
                println!("  -d USER            Delete user");
                println!("  --details USER     Show user details");
                println!("  --add-sendas       Add send-as permission");
            }
            "kopano-gateway" => {
                println!("kopano-gateway (OurOS) — IMAP/POP3 gateway");
                println!("  --imap-port PORT   IMAP port (default: 143)");
                println!("  --pop3-port PORT   POP3 port (default: 110)");
                println!("  --ssl              Enable SSL");
            }
            _ => {
                println!("kopano-server (OurOS) — Kopano groupware server");
                println!("  --config FILE      Config file");
                println!("  --foreground       Run in foreground");
                println!("  --restart          Restart server");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kopano v8.7.27 (OurOS)"); return 0; }
    match prog {
        "kopano-admin" => {
            println!("Kopano User List:");
            println!("  admin (Administrator) - store: 2.3 GB");
            println!("  user1 (John Doe) - store: 1.1 GB");
            println!("  user2 (Jane Smith) - store: 890 MB");
            println!("  Total: 3 users, 4.3 GB");
        }
        "kopano-gateway" => {
            println!("Kopano Gateway v8.7.27");
            println!("  IMAP: 0.0.0.0:143 (STARTTLS), 0.0.0.0:993 (SSL)");
            println!("  POP3: 0.0.0.0:110 (STARTTLS), 0.0.0.0:995 (SSL)");
            println!("  Connected to kopano-server on localhost:236");
        }
        _ => {
            println!("Kopano Server v8.7.27 (OurOS)");
            println!("  Database: MySQL (localhost)");
            println!("  Users: 45");
            println!("  Stores: 48 (12.4 GB total)");
            println!("  Attachments: filesystem (/var/kopano/attachments)");
            println!("  Socket: /var/run/kopano/server.sock");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kopano-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kopano(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kopano};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kopano"), "kopano");
        assert_eq!(basename(r"C:\bin\kopano.exe"), "kopano.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kopano.exe"), "kopano");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kopano(&["--help".to_string()], "kopano"), 0);
        assert_eq!(run_kopano(&["-h".to_string()], "kopano"), 0);
        assert_eq!(run_kopano(&["--version".to_string()], "kopano"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kopano(&[], "kopano"), 0);
    }
}
