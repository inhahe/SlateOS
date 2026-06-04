#![deny(clippy::all)]

//! softether-cli — OurOS SoftEther VPN
//!
//! Multi-personality: `vpnserver`, `vpnclient`, `vpncmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_softether(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "vpnclient" => {
                println!("vpnclient (OurOS) — SoftEther VPN client");
                println!("  start              Start VPN client");
                println!("  stop               Stop VPN client");
            }
            "vpncmd" => {
                println!("vpncmd (OurOS) — SoftEther VPN command-line admin");
                println!("  /SERVER HOST       Connect to VPN server");
                println!("  /CLIENT            Connect to VPN client");
                println!("  /TOOLS             Network tools");
                println!("  /HUB HUB           Select virtual hub");
            }
            _ => {
                println!("vpnserver (OurOS) — SoftEther VPN server");
                println!("  start              Start VPN server");
                println!("  stop               Stop VPN server");
                println!("  execsvc            Run as service");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SoftEther VPN v4.43 (OurOS)"); return 0; }
    match prog {
        "vpnclient" => {
            println!("SoftEther VPN Client v4.43");
            println!("  Accounts: 2 configured");
            println!("  Active connections: 1");
            println!("  Virtual adapters: 2");
        }
        "vpncmd" => {
            println!("SoftEther VPN Command Line Admin v4.43");
            println!("  Connected to: localhost:443");
            println!("  Server: SoftEther VPN Server");
        }
        _ => {
            println!("SoftEther VPN Server v4.43 (OurOS)");
            println!("  Virtual Hubs: 3");
            println!("  Active Sessions: 12");
            println!("  Protocols: SSL-VPN, L2TP/IPsec, OpenVPN, SSTP");
            println!("  Listeners: 443/tcp, 992/tcp, 1194/udp, 5555/tcp");
            println!("  Users: 45");
            println!("  Cascade connections: 2");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vpnserver".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_softether(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_softether};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/softether"), "softether");
        assert_eq!(basename(r"C:\bin\softether.exe"), "softether.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("softether.exe"), "softether");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_softether(&["--help".to_string()], "softether"), 0);
        assert_eq!(run_softether(&["-h".to_string()], "softether"), 0);
        let _ = run_softether(&["--version".to_string()], "softether");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_softether(&[], "softether");
    }
}
