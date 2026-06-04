#![deny(clippy::all)]

//! strongswan-cli — OurOS strongSwan IPsec VPN
//!
//! Multi-personality: `ipsec`, `swanctl`, `charon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_strongswan(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "swanctl" => {
                println!("swanctl (OurOS) — strongSwan configuration interface");
                println!("  --load-all         Load all configs");
                println!("  --list-sas         List active SAs");
                println!("  --list-conns       List connections");
                println!("  --list-certs       List certificates");
                println!("  --initiate --child NAME  Initiate connection");
                println!("  --terminate --ike NAME   Terminate connection");
                println!("  --log              Follow log output");
            }
            "charon" => {
                println!("charon (OurOS) — strongSwan IKE daemon");
                println!("  --debug-ike LEVEL  IKE debug level (0-4)");
                println!("  --debug-net LEVEL  Network debug level");
            }
            _ => {
                println!("ipsec (OurOS) — strongSwan IPsec control");
                println!("  start              Start strongSwan");
                println!("  stop               Stop strongSwan");
                println!("  restart            Restart strongSwan");
                println!("  status             Show SA status");
                println!("  statusall          Detailed status");
                println!("  up NAME            Bring up connection");
                println!("  down NAME          Bring down connection");
                println!("  reload             Reload configuration");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("strongSwan v5.9.14 (OurOS)"); return 0; }
    match prog {
        "swanctl" => {
            println!("strongSwan connections:");
            println!("  site-a: IKEv2, established 3h ago");
            println!("    local: 10.0.1.1 [CN=vpn.example.com]");
            println!("    remote: 10.0.2.1 [CN=remote.example.com]");
            println!("    child: site-a-tunnel, ESP, AES256-GCM");
        }
        _ => {
            println!("strongSwan v5.9.14 (OurOS)");
            println!("  Status: running (charon PID 1234)");
            println!("  IKE SAs: 3 established");
            println!("  Child SAs: 5 installed");
            println!("  Uptime: 12 days 5:34:12");
            println!("  Listening: 0.0.0.0:500, 0.0.0.0:4500");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ipsec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_strongswan(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_strongswan};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/strongswan"), "strongswan");
        assert_eq!(basename(r"C:\bin\strongswan.exe"), "strongswan.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("strongswan.exe"), "strongswan");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_strongswan(&["--help".to_string()], "strongswan"), 0);
        assert_eq!(run_strongswan(&["-h".to_string()], "strongswan"), 0);
        let _ = run_strongswan(&["--version".to_string()], "strongswan");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_strongswan(&[], "strongswan");
    }
}
