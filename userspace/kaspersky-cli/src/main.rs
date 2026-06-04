#![deny(clippy::all)]

//! kaspersky-cli — OurOS Kaspersky Premium security
//!
//! Single personality: `kaspersky`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ksp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kaspersky [OPTIONS]");
        println!("Kaspersky Premium 21.18 (OurOS) — Consumer + business security");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/critical/custom");
        println!("  --kse                  Kaspersky Endpoint Security (enterprise)");
        println!("  --rescue-disk          Kaspersky Rescue Disk (bootable)");
        println!("  --tdsskiller           Free TDSSKiller rootkit remover");
        println!("  --safe-money           Safe Money sandboxed browser");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kaspersky Premium 21.18.13.471 (OurOS)"); return 0; }
    println!("Kaspersky Premium 21.18.13.471 (OurOS)");
    println!("  Editions: Standard, Plus, Premium (consumer); KES (business)");
    println!("  KSN: Kaspersky Security Network (cloud reputation, 400M+ users)");
    println!("  Engines: signature + heuristics + behavioral + ML + anti-cryptor + IDS");
    println!("  Features: AV, firewall, anti-phishing, Safe Money, parental, VPN (Plus+),");
    println!("            password manager (Premium), data leak checker, ID theft (US)");
    println!("  Threat research: top-tier APT research (Equation, Stuxnet, Carbanak, DarkPulsar)");
    println!("  Regulatory: US banned 2024 (federal use prior); Russian origin remains debated");
    println!("  HQ: London (relocated from Moscow); Global Transparency Initiative");
    println!("  License: annual subscription (consumer); per-node (KES business)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kaspersky".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ksp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ksp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kaspersky"), "kaspersky");
        assert_eq!(basename(r"C:\bin\kaspersky.exe"), "kaspersky.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kaspersky.exe"), "kaspersky");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ksp(&["--help".to_string()], "kaspersky"), 0);
        assert_eq!(run_ksp(&["-h".to_string()], "kaspersky"), 0);
        let _ = run_ksp(&["--version".to_string()], "kaspersky");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ksp(&[], "kaspersky");
    }
}
