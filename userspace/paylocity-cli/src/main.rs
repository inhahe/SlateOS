#![deny(clippy::all)]
//! paylocity-cli — personality CLI for Paylocity, the Chicago-area SMB +
//! mid-market HCM platform.
//!
//! Founded 1997 in Schaumburg, Illinois by Steve Sarowitz (still chairman),
//! IPO'd on Nasdaq:PCTY March 2014. Not a PEO — operates as a pure HRIS +
//! payroll vendor competing directly with ADP Workforce Now, Paychex Flex,
//! and Paycom. Particularly strong mid-market (~50-1,000 employee) traction
//! in the Midwest US. Modernized aggressively starting late-2010s into a
//! "Modern Workforce" platform with built-in collaboration features
//! (Community feed, recognition, surveys) that uniquely-for-payroll
//! resembles an internal social platform — a clear bet that next-gen HR
//! buyers want engagement features bundled with payroll.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Paylocity SMB + mid-market HCM personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Steve Sarowitz 1997 Schaumburg IL; Nasdaq:PCTY 2014");
    println!("    payroll       Payroll engine + tax filing + on-demand pay");
    println!("    community     Built-in social/community feed + recognition platform");
    println!("    talent        Recruiting, onboarding, performance, comp planning");
    println!("    benefits      Benefits administration + ACA + carrier connections");
    println!("    workflows     Workflows + Document Library no-code builder");
    println!("    pricing       PEPM software-only pricing (not a PEO)");
    println!("    customers     Mid-market US customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("paylocity-cli 0.1.0 (mid-market-HCM personality build)"); }

fn run_about() {
    println!("Paylocity Holding Corporation (Nasdaq:PCTY).");
    println!("  Founded:    1997, Schaumburg, Illinois.");
    println!("  Founder:    Steve Sarowitz (still serves as Chairman of the Board).");
    println!("  CEO:        Toby Williams (CEO since 2021; previously CFO).");
    println!("  Listing:    Nasdaq:PCTY IPO Mar 2014 at $17/share.");
    println!("  Scale:      ~37,000 customers, ~5M employees on platform.");
    println!("  Revenue:    ~$1.4B annualised (FY24).");
    println!("  Coverage:   US-only; all 50 states + DC payroll + tax-filing capable.");
}

fn run_payroll() {
    println!("Payroll engine.");
    println!("  Multi-state US payroll; integrated federal, state, local tax filing.");
    println!("  On-Demand Payment: employees draw earned wages between pay periods,");
    println!("  funded by Paylocity (Earned Wage Access) — competitor to DailyPay etc.");
    println!("  Direct-deposit, Paylocity branded paycard, paper-check options.");
    println!("  Off-cycle bonus + commission + reimbursement runs.");
    println!("  Garnishment processing + tip reporting + multi-EIN consolidation.");
    println!("  Year-end: W-2 + 1099 generation + e-file + employee self-service downloads.");
}

fn run_community() {
    println!("Community + Recognition (defining differentiator).");
    println!("  Built-in social feed at the centre of the Paylocity app — looks");
    println!("  more like Workplace by Meta than an HR portal.");
    println!("  Posts, comments, reactions, employee shout-outs, polls, surveys.");
    println!("  Impressions Recognition: peer + manager kudos with optional points");
    println!("  redeemable for branded swag / e-gift cards.");
    println!("  Bet: SMBs that don't have Slack / Workplace but do need internal");
    println!("  communication get it bundled with payroll for free.");
    println!("  Mobile-first: most employees access Paylocity via the mobile app.");
}

fn run_talent() {
    println!("Talent management suite.");
    println!("  Recruiting: branded careers site, ATS pipeline, indeed/LinkedIn sync.");
    println!("  Onboarding: tasklists, e-signature, I-9 verification, E-Verify integration.");
    println!("  Performance: continuous feedback, 360 reviews, goals, manager 1:1s.");
    println!("  Compensation planning: budget allocation, merit + promotion cycles.");
    println!("  Learning Management System: course catalog + compliance training assignments.");
    println!("  Surveys + employee-NPS pulse measurement.");
}

fn run_benefits() {
    println!("Benefits administration.");
    println!("  Open enrollment workflow with carrier-rate management + cost projections.");
    println!("  Carrier-connection feeds to major US health/dental/vision/life carriers.");
    println!("  ACA compliance: 1095-C generation + state filings.");
    println!("  HSA/FSA/COBRA administration via partners.");
    println!("  Total-rewards statement generation: shows employees the full cost of");
    println!("  employer-sponsored benefits as part of their compensation.");
}

fn run_workflows() {
    println!("Workflows + Document Library.");
    println!("  No-code workflow builder for HR-process automation:");
    println!("  triggers (employee data change, anniversary, etc.) → actions");
    println!("  (notify manager, generate document, assign task, change status).");
    println!("  Document Library: per-employee document storage with e-sign + visibility");
    println!("  rules.");
    println!("  HR Help Center: ticketing + knowledge base for employee self-service");
    println!("  HR questions.");
    println!("  Modern Workforce Index: Paylocity's customer-engagement metric, surfaced");
    println!("  to HR + executives.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Not a PEO — software-only PEPM pricing.");
    println!("  Approximate published pricing (varies heavily by deal):");
    println!("    Core HCM: ~$10-25 PEPM depending on modules + employee count.");
    println!("    Add-ons: Time + Attendance, Learning, Surveys, On-Demand Payment");
    println!("             each adds incremental PEPM.");
    println!("  Implementation fee one-time; annual contracts standard.");
    println!("  Per-payroll-run fees on some legacy contracts (being phased out).");
    println!("  Tends to undercut ADP Workforce Now on like-for-like deals in the SMB segment.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 50-1,000 employee US mid-market companies.");
    println!("  Industry mix: professional services, manufacturing, healthcare,");
    println!("  retail/hospitality, nonprofits, education.");
    println!("  Geography: nationwide US; over-indexed in Midwest (Illinois, Wisconsin,");
    println!("  Ohio, Michigan, Indiana) given Chicago-area HQ + go-to-market history.");
    println!("  Often replaces: ADP Workforce Now, Paychex Flex, Ultimate (now UKG)");
    println!("  in deals where the buyer wants a more 'modern' UI + collaboration feel.");
    println!("  Competes against: BambooHR (smaller), Paycom (similar size), ADP, UKG.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "paylocity-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "payroll" => run_payroll(),
        "community" => run_community(),
        "talent" => run_talent(),
        "benefits" => run_benefits(),
        "workflows" => run_workflows(),
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
        run_payroll();
        run_community();
        run_talent();
        run_benefits();
        run_workflows();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("paylocity-cli");
        print_version();
    }
}
