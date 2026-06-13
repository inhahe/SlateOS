#![deny(clippy::all)]

//! gusto-cli — SlateOS Gusto (modern self-serve SMB payroll, formerly ZenPayroll)
//!
//! Single personality: `gusto`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gusto(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gusto [OPTIONS]");
        println!("Gusto (Slate OS) — Self-serve payroll + benefits + HR for SMBs");
        println!();
        println!("Options:");
        println!("  --run-payroll          Run payroll");
        println!("  --simple               Simple ($40 + $6/employee)");
        println!("  --plus                 Plus ($80 + $12/employee)");
        println!("  --premium              Premium (custom — dedicated CSM)");
        println!("  --contractor-only      $35/mo contractor-only plan");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gusto 2024 (Slate OS)"); return 0; }
    println!("Gusto 2024 (Slate OS)");
    println!("  Vendor: ZenPayroll Inc. dba Gusto (San Francisco, founded 2011)");
    println!("  Founders: Joshua Reeves + Edward Kim + Tomer London (Y Combinator W12)");
    println!("  Rebrand: ZenPayroll → Gusto in 2015 (broader HR + benefits, not just payroll)");
    println!("  Funding: Generation Investment, Google Ventures, T. Rowe Price + others");
    println!("          ~$700M raised, last valuation ~$9.6B (2021)");
    println!("          IPO repeatedly speculated, still private as of 2024");
    println!("  Strategy: 'payroll + benefits + HR for the people who hated ADP'");
    println!("           consumer-app UX brought to payroll");
    println!("           transparent flat pricing (vs ADP's quote-based sales)");
    println!("  Pricing: Simple $40/mo + $6/employee — payroll + benefits + 401k");
    println!("          Plus $80/mo + $12/employee — adds time tracking + PTO + project tracking");
    println!("          Premium — custom (dedicated support + compliance review)");
    println!("          Contractor-only — $35/mo + $6/contractor (no W-2 employees)");
    println!("  Customers: 300,000+ small businesses (most heavily 5-50 employees)");
    println!("            popular with startups, restaurants, gyms, dental practices");
    println!("  Features:");
    println!("    - Full-service payroll (all 50 states + DC + PR)");
    println!("    - Automatic federal/state/local tax filing");
    println!("    - Health insurance brokerage (37 states) — quotes + enrollment");
    println!("    - 401k retirement (Guideline integration + Gusto's own plans)");
    println!("    - Workers comp (pay-as-you-go via Next Insurance partnership)");
    println!("    - HSA, FSA, commuter benefits");
    println!("    - Direct deposit (2-day standard, 4-day with no Gusto Wallet)");
    println!("    - Gusto Wallet (debit card + early payday + savings account for employees)");
    println!("    - International contractors (75+ countries, 2022 launch)");
    println!("    - Offer letters + e-signed onboarding docs");
    println!("    - Time tracking + PTO + scheduling (Plus tier)");
    println!("    - Accountant-friendly: 30+ accounting tool integrations (QuickBooks, Xero, etc.)");
    println!("    - Gusto Embedded (white-label API — used by Square Payroll, Bench, etc.)");
    println!("  Cultural angle: famous 'wall of love' (customer photos at HQ), explicit company values");
    println!("  Critique: light on enterprise features (no global payroll, weak in 200+ employee org)");
    println!("           Plus tier needed for time tracking is annoying for SMBs");
    println!("  Differentiator: best UX in SMB payroll — onboarding new employees feels like a consumer app");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gusto".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gusto(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gusto};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gusto"), "gusto");
        assert_eq!(basename(r"C:\bin\gusto.exe"), "gusto.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gusto.exe"), "gusto");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gusto(&["--help".to_string()], "gusto"), 0);
        assert_eq!(run_gusto(&["-h".to_string()], "gusto"), 0);
        let _ = run_gusto(&["--version".to_string()], "gusto");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gusto(&[], "gusto");
    }
}
