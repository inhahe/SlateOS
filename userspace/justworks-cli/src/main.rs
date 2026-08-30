#![deny(clippy::all)]
//! justworks-cli — personality CLI for Justworks, the NYC SMB PEO.
//!
//! Founded 2012 in New York by Isaac Oates (ex-Etsy engineering). Operates
//! as a Professional Employer Organization (PEO) — Justworks becomes the
//! "employer of record" for the client's US W-2 employees, pools all PEO
//! customers into a single large insurance risk pool, and offers
//! small-and-mid-market companies access to Fortune-500-grade health
//! insurance, 401(k), and benefits administration that they could never
//! negotiate alone. Has done a Series E and is one of the largest pure-play
//! SMB PEOs outside TriNet/Insperity/ADP TotalSource.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Justworks NYC SMB PEO personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Isaac Oates 2012 NYC; PEO/co-employer model");
    println!("    peo           How the co-employment + risk-pool model works");
    println!("    benefits      Health, dental, vision, 401(k), commuter, FSA/HSA");
    println!("    payroll       Run, taxes, W-2s, contractor 1099s, multi-state");
    println!("    hours         Justworks Hours (formerly Boomr) time-tracking");
    println!("    international Justworks International EOR for hiring abroad");
    println!("    pricing       Per-employee-per-month tiered pricing");
    println!("    customers     Selected customer base profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("justworks-cli 0.1.0 (SMB-PEO personality build)"); }

fn run_about() {
    println!("Justworks, Inc.");
    println!("  Founded:    2012, New York.");
    println!("  Founder:    Isaac Oates (CEO; ex-Etsy engineering).");
    println!("  Backers:    Bain Capital Ventures, Index Ventures, Spark Capital,");
    println!("              Redpoint, Thrive Capital.");
    println!("  Funding:    $93M Series E Jan 2022 from Spark Capital + Redpoint;");
    println!("              S-1 filed Jan 2022, IPO withdrawn Mar 2022 in market chill.");
    println!("  Scale:      8,000+ SMB customers, 130,000+ employees on platform.");
    println!("  Differentiator: smooth small-business onboarding + the most polished");
    println!("              SMB-PEO UX in the segment; competes mainly with Gusto");
    println!("              (which is HR/payroll-not-PEO by default) on simplicity.");
}

fn run_peo() {
    println!("PEO / co-employment model.");
    println!("  Justworks files all federal + state payroll taxes under its FEIN.");
    println!("  Customer's employees become joint W-2 employees of Justworks (co-employer)");
    println!("  for tax/benefits purposes — customer remains the worksite employer for");
    println!("  day-to-day management + termination decisions.");
    println!("  Liability sharing: Justworks handles payroll-tax compliance + benefits");
    println!("  administration + workers' comp; customer keeps performance + supervision.");
    println!("  Risk pool: all PEO clients are one large health-insurance group, getting");
    println!("  enterprise rates that a 5-person company could never negotiate alone.");
    println!("  Certified PEO (CPEO) status with IRS — federal recognition of tax authority.");
}

fn run_benefits() {
    println!("Benefits administration.");
    println!("  Health: Aetna, UnitedHealthcare, Kaiser Permanente plans nationally.");
    println!("  Dental + vision via Guardian / MetLife.");
    println!("  401(k): Slavic401k partner, with employer-match administration.");
    println!("  FSA / HSA / commuter benefits, COBRA administration, life + disability.");
    println!("  Open enrollment runs through the Justworks portal, with employee self-service.");
    println!("  PTO + sick-leave policies configurable + tracked in-platform.");
}

fn run_payroll() {
    println!("Payroll engine.");
    println!("  Multi-state US payroll, all 50 states + DC, with state withholding,");
    println!("  unemployment insurance, local taxes (NYC/SF/etc.), wage-garnishment handling.");
    println!("  W-2 + 1099 generation + e-file at year-end.");
    println!("  Contractor payments alongside W-2 — full mixed-workforce support.");
    println!("  Off-cycle bonus + commission + reimbursement runs.");
    println!("  Direct-deposit + paper-check + Justworks debit-card options.");
}

fn run_hours() {
    println!("Justworks Hours.");
    println!("  Time-tracking product, originally acquired as Boomr in 2021.");
    println!("  Mobile + web clock-in, GPS verification, geofencing for job sites.");
    println!("  Overtime + break-rule compliance per state law.");
    println!("  Pushes hours directly to payroll — eliminates the export/import gap.");
    println!("  Targets the SMB-services segment (agencies, dental practices, salons,");
    println!("  small construction crews) that previously left Justworks for TSheets/Homebase.");
}

fn run_international() {
    println!("Justworks International (EOR).");
    println!("  Launched ~2022 as a separate product line, not bundled with the PEO.");
    println!("  Customer hires a worker in (say) Portugal; Justworks International is");
    println!("  the local Employer of Record, runs local payroll + benefits + compliance.");
    println!("  Covers 100+ countries through a mix of owned entities + local partners.");
    println!("  Positioned against Deel, Remote, Oyster, Velocity Global, Papaya Global.");
    println!("  Same Justworks UI, so customers don't context-switch between US-PEO + EOR.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Basic:    ~$59 per employee per month (PEPM), payroll + tax + compliance,");
    println!("            no health insurance access included.");
    println!("  Plus:     ~$99 PEPM, adds Justworks-sponsored medical/dental/vision +");
    println!("            HSA/FSA + 401(k) access at PEO group rates.");
    println!("  Discounts: lower PEPM at higher headcount tiers (50+/100+).");
    println!("  Contractors: ~$29 per contractor per month, no employer-cost burden.");
    println!("  No annual lock-in; month-to-month is the standard SMB-PEO model.");
}

fn run_customers() {
    println!("Customer base profile:");
    println!("  Sweet spot: 5-50 W-2 employees, professional services, tech startups,");
    println!("  agencies, e-commerce brands, small VC-backed companies.");
    println!("  Heavy NYC + SF + LA + Boston + Austin concentration (founder cities).");
    println!("  Common pattern: VC-backed seed/Series-A company that needs real health");
    println!("  insurance to recruit but is too small to negotiate group rates directly.");
    println!("  Many customers eventually graduate to Rippling or in-house HR at ~250 FTE.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "justworks-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "peo" => run_peo(),
        "benefits" => run_benefits(),
        "payroll" => run_payroll(),
        "hours" => run_hours(),
        "international" => run_international(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_peo();
        run_benefits();
        run_payroll();
        run_hours();
        run_international();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("justworks-cli");
        print_version();
    }
}
