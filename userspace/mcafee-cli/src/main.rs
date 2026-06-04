#![deny(clippy::all)]

//! mcafee-cli — OurOS McAfee+ consumer security
//!
//! Single personality: `mcafee`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mcafee(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mcafee [OPTIONS]");
        println!("McAfee+ Advanced 16.0 (OurOS) — Consumer security suite");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/custom");
        println!("  --webadvisor           McAfee WebAdvisor browser extension");
        println!("  --vpn                  Secure VPN (unlimited)");
        println!("  --identity-monitoring  Identity monitoring (dark web + SSN trace)");
        println!("  --trellix              Trellix (enterprise spin-off, McAfee Enterprise)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("McAfee+ Advanced 16.0.55 R32 (OurOS)"); return 0; }
    println!("McAfee+ Advanced 16.0.55 R32 (OurOS)");
    println!("  Brand history: McAfee Inc (1987) → Intel Security (2014) → McAfee (2017) →");
    println!("                 split 2022: McAfee (consumer) + Trellix (enterprise, w/ FireEye)");
    println!("  McAfee+ tiers: Basic, Essential, Premium, Advanced, Ultimate, Family");
    println!("  Features: AntiVirus, Firewall, Web Protection (WebAdvisor), VPN, Password Mgr,");
    println!("            File Lock (encrypted vault), Shredder, Identity Monitoring, $1M ID coverage");
    println!("  Engines: signature, heuristics, behavioral, ML, Real Protect cloud lookup");
    println!("  Platforms: Windows, macOS, Android, iOS, ChromeOS");
    println!("  License: annual subscription (per device or unlimited family)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mcafee".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mcafee(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mcafee};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mcafee"), "mcafee");
        assert_eq!(basename(r"C:\bin\mcafee.exe"), "mcafee.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mcafee.exe"), "mcafee");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mcafee(&["--help".to_string()], "mcafee"), 0);
        assert_eq!(run_mcafee(&["-h".to_string()], "mcafee"), 0);
        let _ = run_mcafee(&["--version".to_string()], "mcafee");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mcafee(&[], "mcafee");
    }
}
