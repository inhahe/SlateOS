#![deny(clippy::all)]
//! ceridian-cli — personality CLI for Ceridian / Dayforce, the continuous-
//! calculation enterprise HCM platform.
//!
//! Ceridian's roots trace to Control Data Corporation, whose Comdata
//! subsidiary spun off as Ceridian Corporation in 1992. Modern Ceridian
//! was reshaped through the 2012 acquisition of Canadian HCM upstart
//! Dayforce (founded by David Ossip 2009 Toronto). Ossip became Ceridian's
//! CEO and bet the whole company on the Dayforce platform — a multi-tenant
//! HCM with a continuous-calculation engine (every transaction recomputes
//! pay + tax in real time, eliminating the traditional "payroll close" night).
//! IPO'd NYSE:CDAY April 2018; renamed itself to Dayforce, Inc. (NYSE:DAY)
//! in early 2024 — completing the brand transition from Ceridian → Dayforce.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Ceridian / Dayforce continuous-calculation HCM personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         CDC roots 1992; Ossip Dayforce 2009 Toronto; NYSE:DAY");
    println!("    continuous    Real-time continuous-calculation engine (the wedge)");
    println!("    dayforce      Dayforce platform: HCM + payroll + workforce + talent");
    println!("    wallet        Dayforce Wallet earned-wage-access + pay-card product");
    println!("    rebrand       2024 corporate rebrand Ceridian → Dayforce, Inc.");
    println!("    global        Multi-country payroll + global expansion strategy");
    println!("    pricing       Enterprise SaaS pricing; per-employee-per-month");
    println!("    customers     Enterprise + multi-national customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("ceridian-cli 0.1.0 (continuous-calculation HCM personality build)"); }

fn run_about() {
    println!("Dayforce, Inc. (formerly Ceridian HCM Holding Inc., NYSE:DAY).");
    println!("  Roots:      Control Data Corporation Comdata subsidiary, 1972 origins.");
    println!("  Spin-off:   Ceridian Corporation, 1992.");
    println!("  Pivot:      acquired Toronto-based Dayforce 2012; founder David Ossip");
    println!("              became Ceridian CEO + rebuilt the company around Dayforce.");
    println!("  PE era:     Thomas H. Lee Partners + Cannae majority-owners pre-IPO.");
    println!("  Listing:    NYSE:CDAY IPO Apr 2018; renamed Dayforce, Inc. (NYSE:DAY)");
    println!("              effective Feb 2024.");
    println!("  Scale:      6,000+ enterprise customers, 6M+ employees on platform.");
    println!("  Coverage:   170+ countries via native + partner payroll.");
}

fn run_continuous() {
    println!("Continuous-calculation engine (the defining differentiator).");
    println!("  Traditional payroll = batch: employees clock hours all pay period,");
    println!("  then payroll runs the night before payday, recalculating taxes +");
    println!("  garnishments + benefits deductions for everyone in one giant job.");
    println!("  Dayforce instead recalculates every employee's gross-to-net the moment");
    println!("  any input changes — a punch is added, a benefits enrolment occurs, a");
    println!("  retro-pay change is keyed.");
    println!("  Consequences:");
    println!("    1. Payroll close becomes essentially a confirmation step, not a job.");
    println!("    2. Compliance issues surface in real-time, not at month-end.");
    println!("    3. Earned-Wage-Access (Dayforce Wallet) is trivially supported because");
    println!("       gross-to-net is already known at any moment, not just after the run.");
    println!("  This architectural bet is what David Ossip + the Dayforce team built");
    println!("  the whole company on, and remains the platform's signature feature.");
}

fn run_dayforce() {
    println!("Dayforce platform.");
    println!("  Single multi-tenant codebase covering:");
    println!("    HR Core: employee master, org structure, lifecycle events.");
    println!("    Payroll: continuous calc, multi-jurisdiction tax, garnishments.");
    println!("    Workforce Management: scheduling, time + attendance, labour forecasting.");
    println!("    Talent: recruiting, onboarding, performance, succession, comp.");
    println!("    Benefits: open enrollment, ACA, carrier feeds.");
    println!("    Learning: LMS for compliance + skill development.");
    println!("  Unusual integration depth — same data model end-to-end, no internal ETL.");
    println!("  Strong in workforce-management-heavy industries (retail, hospitality,");
    println!("  manufacturing, healthcare) where shift scheduling + payroll have to be");
    println!("  tightly coupled.");
}

fn run_wallet() {
    println!("Dayforce Wallet.");
    println!("  Earned Wage Access product: employees draw earned-but-unpaid wages");
    println!("  any time during the pay period via a Dayforce-branded debit Mastercard.");
    println!("  Possible because the continuous-calculation engine knows exact net pay");
    println!("  earned at any moment.");
    println!("  No-fee-to-employee positioning on basic draws (vs. competitors that");
    println!("  charge $1-3 per advance) — funded by interchange + customer-paid fees.");
    println!("  Pay-card use cases: paid daily by some retailers + restaurants where");
    println!("  shift workers don't have stable bank accounts.");
    println!("  Customer-retention play more than a profit centre.");
}

fn run_rebrand() {
    println!("Ceridian → Dayforce rebrand (Feb 2024).");
    println!("  Legacy brand: Ceridian had baggage from the Comdata + LifeWorks +");
    println!("  multiple-platform era (older customers were on different legacy systems).");
    println!("  Reality: Dayforce was 90%+ of the new business + the future platform.");
    println!("  Corporate name change to Dayforce, Inc.; ticker NYSE:CDAY → NYSE:DAY.");
    println!("  Existing customer contracts migrated automatically — same legal entity,");
    println!("  new branding. Older Powerpay (Canadian SMB payroll) + LifeWorks (EAP)");
    println!("  products gradually retired or sold off.");
}

fn run_global() {
    println!("Global expansion strategy.");
    println!("  Native Dayforce payroll: US, Canada, UK, Ireland, Australia, New Zealand,");
    println!("  Mexico, Mauritius (engineering centre), select EU countries.");
    println!("  Global Payroll: 100+ countries via partner-aggregation model (similar to");
    println!("  Papaya's orchestration play, but inside Dayforce's tenant).");
    println!("  Acquired Excelity (Asia-Pacific payroll, 2018) + Ascender (APAC, 2022)");
    println!("  to extend native footprint.");
    println!("  Differentiator vs. Workday: continuous calc + workforce-management depth.");
    println!("  Differentiator vs. SAP SuccessFactors: more native multi-country payroll");
    println!("  in the core platform, less reliance on partner code.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Enterprise per-employee-per-month, typically in the $20-50 PEPM range");
    println!("  for the full Dayforce suite at mid-market sizes.");
    println!("  Implementation: substantial — usually 6-12 months, six-figure SI work.");
    println!("  Annual contracts standard; multi-year commits with annual price uplifts.");
    println!("  Workforce-management-only deployments cheaper; Dayforce Wallet PEPM-incremental.");
    println!("  Not a PEO — software-only, customer remains employer of record.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 1,000-50,000 employee mid-market + enterprise.");
    println!("  Industry tilt: retail, hospitality, healthcare, manufacturing,");
    println!("    transportation, professional services — anywhere shift scheduling +");
    println!("    payroll need to be tightly coupled.");
    println!("  Geographic: strong US + Canada base; UK + Australia growing; emerging");
    println!("    APAC presence post-Ascender.");
    println!("  Frequently named: Whole Foods (historical), Trader Joe's, Five Guys,");
    println!("    several large North American grocery + retail chains.");
    println!("  Replaces: ADP Enterprise, Workday HCM + payroll (in workforce-heavy verticals),");
    println!("    SAP SuccessFactors, Kronos / UKG.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ceridian-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "continuous" => run_continuous(),
        "dayforce" => run_dayforce(),
        "wallet" => run_wallet(),
        "rebrand" => run_rebrand(),
        "global" => run_global(),
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
        run_continuous();
        run_dayforce();
        run_wallet();
        run_rebrand();
        run_global();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("ceridian-cli");
        print_version();
    }
}
