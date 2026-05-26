#![deny(clippy::all)]
//! papaya-cli — personality CLI for Papaya Global, the global payroll
//! orchestration platform.
//!
//! Founded 2016 in Tel Aviv by Eynat Guez (CEO, ex-ConTeam global mobility),
//! Ruben Drong, and Ofer Herman. Distinct from the EOR pure-plays (Deel,
//! Remote, Oyster) — Papaya's first product was *global payroll
//! orchestration* on top of existing in-country payroll providers, layering
//! a single SaaS workflow + reporting + payments layer over a customer's
//! existing mosaic of local payroll vendors. EOR + contractor came later.
//! Reached ~$3.7B Series D Sep 2021 at the peak. Heavy use of fintech rails
//! (Papaya owns a fintech licence in Belgium/EU + US licences) so customers
//! can fund payroll in one transfer and Papaya pays out locally.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Papaya Global global-payroll orchestration personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Eynat Guez 2016 Tel Aviv; payroll orchestration first");
    println!("    orchestration How the local-provider orchestration model works");
    println!("    eor           Papaya EOR product line (added later than orchestration)");
    println!("    contractors   Contractor management + multi-country payouts");
    println!("    fintech       Papaya fintech licences + cross-border funding rails");
    println!("    analytics     Workforce analytics + benchmarking products");
    println!("    pricing       Hybrid orchestration / EOR / contractor pricing");
    println!("    customers     Selected enterprise customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("papaya-cli 0.1.0 (global-payroll-orchestration personality build)"); }

fn run_about() {
    println!("Papaya Global Ltd.");
    println!("  Founded:    2016, Tel Aviv.");
    println!("  Founders:   Eynat Guez (CEO; ex-ConTeam global mobility),");
    println!("              Ruben Drong, Ofer Herman.");
    println!("  HQ:         Tel Aviv + New York.");
    println!("  Backers:    Insight Partners, Tiger Global, Workday Ventures,");
    println!("              Sapphire, Greenoaks, Scale Venture Partners.");
    println!("  Funding:    $250M Series D Sep 2021 at ~$3.7B valuation;");
    println!("              ~$444M total raised.");
    println!("  Coverage:   160+ countries on payroll + EOR + contractor.");
    println!("  Differentiator: started as a *layer* on top of customers' existing");
    println!("              in-country payroll providers, not a replacement.");
}

fn run_orchestration() {
    println!("Global payroll orchestration model.");
    println!("  Large enterprise has a tangle: ADP in the US, Sage in the UK,");
    println!("  Datev in Germany, NGA HR in France, local providers in 30 more countries.");
    println!("  Each one with its own portal, file format, calendar, reporting.");
    println!("  Papaya sits on top:");
    println!("    1. One global HRIS-style data model for all employees worldwide.");
    println!("    2. Pushes payroll data to each local provider in their required format.");
    println!("    3. Pulls payslip + cost data back, normalises it, reports in one UI.");
    println!("    4. Optional: Papaya funds the whole payroll in customer's HQ currency,");
    println!("       converts + pays each local provider via fintech rails.");
    println!("  Original wedge: customer keeps existing providers (no rip-and-replace).");
}

fn run_eor() {
    println!("Papaya EOR product.");
    println!("  Added 2019-2020 to compete with Deel/Remote/Oyster head-on.");
    println!("  Owned + partner-entity hybrid model (closer to Deel than Remote).");
    println!("  Same Papaya UI as the orchestration product, so customers can mix:");
    println!("  use orchestration for 30 countries where they have entities,");
    println!("  use EOR for 5 emerging-market hires where they don't.");
    println!("  Larger enterprise focus than Deel/Remote — fewer SMB customers,");
    println!("  more Fortune 500 + Fortune 1000 enterprise references.");
}

fn run_contractors() {
    println!("Contractor management.");
    println!("  Same multi-country contractor flow as EOR competitors:");
    println!("  onboarding, contracts, classification analysis, multi-currency payouts.");
    println!("  Differentiator: settlement through Papaya's own fintech rails rather than");
    println!("  third-party FX, so customer sees less spread.");
    println!("  Tightly integrated with the orchestration platform — a customer's mix");
    println!("  of FTEs + EOR + contractors all show in one workforce-cost report.");
}

fn run_fintech() {
    println!("Papaya fintech licences.");
    println!("  Holds e-money / payment-institution licences in the EU (Belgium-issued");
    println!("  passportable across EEA) + state-level money transmitter licences in the US.");
    println!("  Customer funds an aggregated payroll in their HQ currency to Papaya's");
    println!("  payments entity; Papaya converts + disburses locally.");
    println!("  This is a structural moat against pure-SaaS competitors that have to");
    println!("  partner with banks or Wise for the money-movement leg.");
    println!("  Customer-facing benefit: tighter cutoffs, more transparent FX, one wire.");
}

fn run_analytics() {
    println!("Workforce analytics.");
    println!("  Real-time global headcount + cost dashboard normalised across countries.");
    println!("  Benchmarking against the Papaya customer base anonymised — salaries by");
    println!("  role + country + company-size cohort.");
    println!("  Compliance dashboard: per-country statutory obligations + due dates +");
    println!("  whose Papaya operation owns each line item.");
    println!("  Workday Ventures investment + tight product integration with Workday");
    println!("  HCM has driven much of the enterprise-analytics direction since 2021.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Orchestration: per-employee-per-month, in the ~$20-50 range depending");
    println!("                 on country mix + scale; enterprise quotes case-by-case.");
    println!("  EOR:           ~$650 per employee per month (slightly premium to Deel/Remote,");
    println!("                 reflecting enterprise positioning).");
    println!("  Contractors:   ~$25 per contractor per month.");
    println!("  Fintech leg:   transparent FX spread published per corridor.");
    println!("  Implementation fee typically waived above a customer-size threshold;");
    println!("  enterprise annual commits the norm above ~500 FTE.");
}

fn run_customers() {
    println!("Customer + adopter profile:");
    println!("  Sweet spot: 500-50,000 FTE multinationals with employees in 20+ countries.");
    println!("  Industry tilt: tech, life sciences, professional services, manufacturing.");
    println!("  Frequently named: Microsoft (selectively), Toyota (partial), Wix, Final,");
    println!("  General Dynamics divisions, several Fortune 100 component customers.");
    println!("  Common pattern: company with an existing patchwork of regional payroll");
    println!("  vendors — Papaya consolidates the *layer*, not the vendors.");
    println!("  Different ICP from Deel/Remote (which skew startup + scaleup).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "papaya-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "orchestration" => run_orchestration(),
        "eor" => run_eor(),
        "contractors" => run_contractors(),
        "fintech" => run_fintech(),
        "analytics" => run_analytics(),
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
        run_orchestration();
        run_eor();
        run_contractors();
        run_fintech();
        run_analytics();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("papaya-cli");
        print_version();
    }
}
