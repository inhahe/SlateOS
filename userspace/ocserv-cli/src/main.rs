#![deny(clippy::all)]

//! ocserv-cli — Slate OS OpenConnect VPN server
//!
//! Multi-personality: `ocserv`, `occtl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ocserv(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "occtl" => {
                println!("occtl (Slate OS) — OpenConnect server control");
                println!("  show status        Server status");
                println!("  show users         Connected users");
                println!("  show sessions      Active sessions");
                println!("  show ip bans       Banned IPs");
                println!("  disconnect user U  Disconnect user");
                println!("  disconnect id N    Disconnect session");
                println!("  reload             Reload config");
                println!("  stop               Stop server");
            }
            _ => {
                println!("ocserv (Slate OS) — OpenConnect VPN server");
                println!("  -c FILE            Config file");
                println!("  -f                 Foreground mode");
                println!("  -d LEVEL           Debug level (1-9)");
                println!("  -t                 Test config");
                println!("  -p FILE            PID file");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ocserv v1.3.0 (Slate OS)"); return 0; }
    match prog {
        "occtl" => {
            println!("OpenConnect Server Status:");
            println!("  Version: 1.3.0");
            println!("  Uptime: 5 days 8:12:34");
            println!("  Connected users: 8");
            println!("  Active sessions: 12");
            println!("  Total auth: 234 success, 5 failures");
            println!("  IP bans: 2");
        }
        _ => {
            println!("ocserv v1.3.0 (Slate OS)");
            println!("  Listening: 0.0.0.0:443 (TCP+UDP)");
            println!("  TLS: enabled (certificate from /etc/ocserv/)");
            println!("  Auth: PAM + RADIUS");
            println!("  DNS: 1.1.1.1, 8.8.8.8");
            println!("  IPv4 pool: 10.10.10.0/24");
            println!("  Max clients: 128");
            println!("  DTLS: enabled (UDP acceleration)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ocserv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ocserv(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ocserv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ocserv"), "ocserv");
        assert_eq!(basename(r"C:\bin\ocserv.exe"), "ocserv.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ocserv.exe"), "ocserv");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ocserv(&["--help".to_string()], "ocserv"), 0);
        assert_eq!(run_ocserv(&["-h".to_string()], "ocserv"), 0);
        let _ = run_ocserv(&["--version".to_string()], "ocserv");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ocserv(&[], "ocserv");
    }
}
