#![deny(clippy::all)]

//! avast-cli — Slate OS Gen Digital Avast One
//!
//! Single personality: `avast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_avast(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: avast [OPTIONS]");
        println!("Avast One 24.10 (Slate OS) — Consumer security/privacy/performance");
        println!();
        println!("Options:");
        println!("  --scan TYPE            smart/deep/targeted/boot-time/usb");
        println!("  --secureline-vpn       Avast SecureLine VPN");
        println!("  --cleanup              Avast Cleanup Premium");
        println!("  --avg                  AVG branded edition (sister product)");
        println!("  --business             Avast Business (managed AV)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Avast One 24.10.9223 (Slate OS)"); return 0; }
    println!("Avast One 24.10.9223 (Slate OS)");
    println!("  Owner: Gen Digital (Avast + AVG + Norton + LifeLock + CCleaner merged)");
    println!("  Avast One: unified product replacing Avast Free/Premium Security");
    println!("  Free tier: AV, basic firewall, web shield, ransomware shield, smart scan");
    println!("  Premium tiers: VPN, cleanup, driver updater, BreachGuard, AntiTrack, $1M ID");
    println!("  Engines: file/web/mail/network/behavior shields, CyberCapture cloud");
    println!("  Mobile: Avast Mobile Security (Android), Avast Security & Privacy (iOS)");
    println!("  Sister products: AVG (same engine, separate brand), CCleaner");
    println!("  History: Czech company founded 1988 in Prague; merged Norton 2022");
    println!("  Past controversy: Jumpshot subsidiary (sold browsing data) — shut down 2020");
    println!("  License: free tier + paid annual subscriptions");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "avast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_avast(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_avast};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/avast"), "avast");
        assert_eq!(basename(r"C:\bin\avast.exe"), "avast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("avast.exe"), "avast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_avast(&["--help".to_string()], "avast"), 0);
        assert_eq!(run_avast(&["-h".to_string()], "avast"), 0);
        let _ = run_avast(&["--version".to_string()], "avast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_avast(&[], "avast");
    }
}
