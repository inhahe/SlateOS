#![deny(clippy::all)]

//! ukg-cli — OurOS UKG (Ultimate Kronos Group — merged UltiPro + Kronos, enterprise HCM)
//!
//! Single personality: `ukg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ukg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ukg [OPTIONS]");
        println!("UKG Pro (OurOS) — Enterprise HCM (formerly UltiPro)");
        println!();
        println!("Options:");
        println!("  --pro                  UKG Pro (large enterprise, formerly UltiPro)");
        println!("  --ready                UKG Ready (mid-market, formerly Kronos Workforce Ready)");
        println!("  --dimensions           UKG Dimensions (workforce management, formerly Kronos)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("UKG Pro 2024 (OurOS)"); return 0; }
    println!("UKG Pro 2024 (OurOS)");
    println!("  Vendor: Ultimate Kronos Group (Lowell, MA + Weston, FL)");
    println!("  Formation: April 2020 merger of Kronos Incorporated + Ultimate Software ($22B)");
    println!("            taken private 2017 (Kronos Hellman & Friedman) + 2019 (Ultimate Software H&F + Blackstone)");
    println!("            merged under shared H&F ownership");
    println!("  Original companies:");
    println!("    - Kronos: founded 1977 (MIT — Mark Ain), workforce-mgmt + time-clock pioneer");
    println!("              the famous Kronos InTouch time clocks (badge swipe + biometric)");
    println!("    - Ultimate Software (UltiPro): founded 1990, enterprise HRIS + payroll");
    println!("  Scale: ~80,000 customers across 150+ countries");
    println!("        ~50 million employees worldwide");
    println!("        $4B+ revenue (private — Blackstone/H&F backed)");
    println!("  Kronos ransomware attack (Dec 2021):");
    println!("    UKG's Kronos Private Cloud hit by ransomware");
    println!("    Many customers — including New York public schools, hospitals — couldn't run payroll");
    println!("    Recovery took months; class-action lawsuits followed");
    println!("    Cautionary tale for cloud HCM vendor concentration");
    println!("  Products:");
    println!("    - UKG Pro (large enterprise — payroll + HRIS + benefits + talent — formerly UltiPro)");
    println!("    - UKG Ready (SMB → mid-market — formerly Kronos Workforce Ready)");
    println!("    - UKG Dimensions (workforce management, scheduling, time + attendance — formerly Kronos)");
    println!("    - UKG Pro Workforce Management (Dimensions integrated into Pro suite)");
    println!("  Features:");
    println!("    - Global payroll (50+ countries via partner network + native US/CA/UK/PR)");
    println!("    - Time + attendance with physical time clocks (Kronos InTouch DX, biometric)");
    println!("    - Scheduling with employee self-scheduling + AI shift recommendations");
    println!("    - Talent acquisition + onboarding + performance + learning");
    println!("    - Compensation planning + total rewards statements");
    println!("    - AI assistant Bryte (people-data insights)");
    println!("    - UKG D&I Suite (DEI metrics + pay equity analytics)");
    println!("  Customers: large enterprise — Marriott, Hard Rock Cafe, Tesla, Cisco, Aramark");
    println!("            heavy in retail, hospitality, healthcare, manufacturing (shift-worker industries)");
    println!("  Critique: complex implementation (6-18 months), legacy UI in places");
    println!("           still recovering reputation from 2021 ransomware");
    println!("  Differentiator: best-in-class workforce mgmt (Kronos heritage) + enterprise HCM (UltiPro) unified");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ukg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ukg(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ukg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ukg"), "ukg");
        assert_eq!(basename(r"C:\bin\ukg.exe"), "ukg.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ukg.exe"), "ukg");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ukg(&["--help".to_string()], "ukg"), 0);
        assert_eq!(run_ukg(&["-h".to_string()], "ukg"), 0);
        let _ = run_ukg(&["--version".to_string()], "ukg");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ukg(&[], "ukg");
    }
}
