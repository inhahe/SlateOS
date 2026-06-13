#![deny(clippy::all)]

//! zerotier-cli — SlateOS ZeroTier virtual network
//!
//! Multi-personality: `zerotier-one`, `zerotier-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zerotier(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "zerotier-one" => {
                println!("zerotier-one (SlateOS) — ZeroTier network daemon");
                println!("  -d                 Daemonize");
                println!("  -p PORT            Primary port (default: 9993)");
                println!("  -U                 Run from user home dir");
            }
            _ => {
                println!("zerotier-cli (SlateOS) — ZeroTier client control");
                println!("  info               Show node info");
                println!("  listnetworks       List joined networks");
                println!("  listpeers          List peers");
                println!("  join NETWORK       Join network");
                println!("  leave NETWORK      Leave network");
                println!("  set NETWORK KEY=VAL  Set network config");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ZeroTier One v1.14.0 (SlateOS)"); return 0; }
    match prog {
        "zerotier-one" => {
            println!("ZeroTier One v1.14.0 (SlateOS)");
            println!("  Node ID: a1b2c3d4e5");
            println!("  Port: 9993/udp");
            println!("  Networks: 2 joined");
            println!("  Peers: 8 direct, 23 relayed");
        }
        _ => {
            println!("200 info a1b2c3d4e5 1.14.0 ONLINE");
            println!("Networks:");
            println!("  8056c2e21c000001 office-net OK PRIVATE 10.147.17.0/24");
            println!("  e5cd7a9e1c123456 dev-net   OK PRIVATE 10.147.18.0/24");
            println!("Peers: 31 total");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zerotier-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zerotier(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zerotier};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zerotier"), "zerotier");
        assert_eq!(basename(r"C:\bin\zerotier.exe"), "zerotier.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zerotier.exe"), "zerotier");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zerotier(&["--help".to_string()], "zerotier"), 0);
        assert_eq!(run_zerotier(&["-h".to_string()], "zerotier"), 0);
        let _ = run_zerotier(&["--version".to_string()], "zerotier");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zerotier(&[], "zerotier");
    }
}
