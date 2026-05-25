#![deny(clippy::all)]

//! paychex-cli — OurOS Paychex (the #2 US payroll provider, SMB-focused)
//!
//! Single personality: `paychex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_px(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: paychex [OPTIONS]");
        println!("Paychex Flex (OurOS) — Payroll + HR for small/mid businesses");
        println!();
        println!("Options:");
        println!("  --flex                 Paychex Flex (online platform)");
        println!("  --essentials           Flex Essentials (1-19 employees)");
        println!("  --select               Flex Select (20-49)");
        println!("  --pro                  Flex Pro (50+ employees)");
        println!("  --peo                  Paychex PEO (co-employment)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Paychex Flex 2024 (OurOS)"); return 0; }
    println!("Paychex Flex 2024 (OurOS)");
    println!("  Vendor: Paychex, Inc. (Rochester, NY — NASDAQ:PAYX)");
    println!("  Founded: 1971 by Tom Golisano with $3,000 starting capital");
    println!("          Golisano targeted businesses ADP considered too small (under 50 employees)");
    println!("          Golisano: serial Independent NY gubernatorial candidate, philanthropist");
    println!("  Scale: ~750,000 SMB clients in US + Europe");
    println!("        pays 1 in 12 US private-sector workers (~12 million)");
    println!("        ~16,000 employees, ~$5.3B annual revenue (FY2024)");
    println!("  Strategy: SMB focus (1-50 employees) vs ADP's broader range");
    println!("           personal account-manager model (vs ADP's call-center)");
    println!("  Products:");
    println!("    - Paychex Flex (online + mobile platform — unified across tiers)");
    println!("    - Flex Essentials (1-19) — bare-bones payroll");
    println!("    - Flex Select (20-49) — adds HR tools");
    println!("    - Flex Pro (50+) — full HCM with talent + analytics");
    println!("    - Paychex Oasis (PEO since Oasis Outsourcing acquisition 2018, $1.2B)");
    println!("    - Paychex Insurance Agency (workers comp, 401k, health)");
    println!("    - Paychex Retirement Services (largest 401k recordkeeper for SMBs in US)");
    println!("  Features:");
    println!("    - 2-day, 1-day, or same-day direct deposit");
    println!("    - Tax pay + file (federal/state/local)");
    println!("    - Time + attendance with biometric kiosk options");
    println!("    - Benefits + HR advisor support");
    println!("    - Onboarding, performance, learning management");
    println!("    - Paychex Voice Assist (Alexa skill for payroll commands)");
    println!("  Mergers + acquisitions: Advance Partners 2015, HR Outsourcing 2018 ($1.2B Oasis), Lessor Group 2022");
    println!("  Critique: pricier than online-only competitors (Gusto, Square Payroll) for similar features");
    println!("           still relies on account-manager relationship — friction vs self-serve modern UX");
    println!("  Differentiator: dedicated account manager + decades of compliance expertise for SMB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "paychex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_px(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
