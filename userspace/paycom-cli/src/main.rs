#![deny(clippy::all)]

//! paycom-cli — Slate OS Paycom (Oklahoma-based mid-market HCM, Beti employee-driven payroll)
//!
//! Single personality: `paycom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: paycom [OPTIONS]");
        println!("Paycom (Slate OS) — Mid-market HCM, employee-driven payroll");
        println!();
        println!("Options:");
        println!("  --beti                 Beti (Better Employee Transaction Interface — employees run their own payroll)");
        println!("  --talent-management    Talent acquisition + onboarding + learning");
        println!("  --time                 Time + attendance");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Paycom 2024 (Slate OS)"); return 0; }
    println!("Paycom 2024 (Slate OS)");
    println!("  Vendor: Paycom Software, Inc. (Oklahoma City — NYSE:PAYC)");
    println!("  Founder: Chad Richison (Oklahoma City, founded 1998)");
    println!("          Richison: Oklahoma football lineage, philanthropy, OKC Thunder NBA team minority owner");
    println!("  History: founded as one of the first cloud payroll companies (Application Service Provider era)");
    println!("          IPO 2014 (NYSE:PAYC)");
    println!("          consistent profitability + ~20% YoY growth rare in mid-market HCM");
    println!("  Scale: ~36,000 customers, $1.7B revenue (FY2023)");
    println!("        sweet spot: 100-5,000 employees (mid-market)");
    println!("        ~6,800 internal employees");
    println!("  Strategy: single-database HCM (every module shares one employee record — no duplicate data)");
    println!("           sales is famously aggressive (Glassdoor reviews + Indeed: Paycom comes up)");
    println!("  Pricing: undisclosed publicly — typically ~$15-30/employee/mo (varies by modules)");
    println!("  Killer concept — Beti (Better Employee Transaction Interface):");
    println!("    employees see and approve their own paycheck BEFORE it's processed");
    println!("    catches errors before payday (vs traditional 'find errors after the check is wrong')");
    println!("    Paycom advertises Beti as a 'first in industry' (launched 2021)");
    println!("    Super Bowl LVI ad campaign featured Beti (2022)");
    println!("  Modules (all on the single Paycom database):");
    println!("    - Payroll (US 50-state, plus Canada)");
    println!("    - Talent Acquisition (ATS + onboarding + e-verify)");
    println!("    - Time + Labor (mobile clock-in + GPS + biometric kiosk)");
    println!("    - Talent Management (performance + comp + succession + LMS)");
    println!("    - HR Management (benefits + COBRA + PTO + 401k)");
    println!("    - Direct Data Exchange (compliance with tax authority APIs)");
    println!("    - GONE (manager dashboard for time-off + scheduling)");
    println!("    - Vault (Paycom employee benefit debit card)");
    println!("  Customers: mid-market US — Hilton, Newegg, Toys R Us (RIP), Gentle Giant Moving, etc.");
    println!("  Critique: complex implementation (4-6 months typical), aggressive sales");
    println!("           weaker outside US/Canada (no global payroll)");
    println!("           Beti criticized for shifting payroll error responsibility to employees");
    println!("  Differentiator: single-database architecture + Beti employee self-service payroll");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "paycom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/paycom"), "paycom");
        assert_eq!(basename(r"C:\bin\paycom.exe"), "paycom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("paycom.exe"), "paycom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pc(&["--help".to_string()], "paycom"), 0);
        assert_eq!(run_pc(&["-h".to_string()], "paycom"), 0);
        let _ = run_pc(&["--version".to_string()], "paycom");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pc(&[], "paycom");
    }
}
