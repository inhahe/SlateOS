#![deny(clippy::all)]

//! norton-cli — SlateOS Gen Digital Norton 360
//!
//! Single personality: `norton`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_norton(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: norton [OPTIONS]");
        println!("Norton 360 Deluxe 24.10 (SlateOS) — Consumer security suite (Gen Digital)");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/custom");
        println!("  --vpn                  Norton Secure VPN");
        println!("  --password-manager     Norton Password Manager");
        println!("  --darkweb              Dark Web Monitoring");
        println!("  --identity             LifeLock Identity Theft Protection (US)");
        println!("  --utilities-premium    Norton Utilities Premium");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Norton 360 Deluxe 24.10.0.13 (SlateOS)"); return 0; }
    println!("Norton 360 Deluxe 24.10.0.13 (SlateOS)");
    println!("  Owner: Gen Digital (Symantec → NortonLifeLock → Gen after Avast merger 2022)");
    println!("  Editions: AntiVirus Plus, 360 Standard, 360 Deluxe, 360 Premium, LifeLock");
    println!("  Engines: SONAR (heuristics), Insight (reputation), IPS, ML, exploit prevention");
    println!("  Features: AV, firewall, VPN, password mgr, parental, cloud backup (PC),");
    println!("            dark web monitoring, identity theft protection (US: LifeLock)");
    println!("  Platforms: Windows, macOS, Android, iOS (most features)");
    println!("  License: annual subscription (1-10 device plans + LifeLock tiers)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "norton".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_norton(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_norton};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/norton"), "norton");
        assert_eq!(basename(r"C:\bin\norton.exe"), "norton.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("norton.exe"), "norton");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_norton(&["--help".to_string()], "norton"), 0);
        assert_eq!(run_norton(&["-h".to_string()], "norton"), 0);
        let _ = run_norton(&["--version".to_string()], "norton");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_norton(&[], "norton");
    }
}
