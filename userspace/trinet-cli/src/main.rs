#![deny(clippy::all)]
//! trinet-cli — personality CLI for TriNet, the established mid-market PEO.
//!
//! Founded 1988 in San Leandro, California by Martin Babinec. One of the
//! original Professional Employer Organizations alongside Insperity (1986)
//! and ADP TotalSource — predates the entire VC-funded HR-tech wave by 25+
//! years. NYSE-listed since 2014 (NYSE:TNET) after PE control by General
//! Atlantic. Distinguished by deep vertical specialisation (technology,
//! life sciences, financial services, professional services, nonprofits) —
//! TriNet offers vertically-tuned benefits plans + HR consulting for each.
//! Acquired Zenefits Feb 2022 to extend down-market into pure-software SMB.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — TriNet established mid-market PEO personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Martin Babinec 1988 San Leandro; NYSE:TNET 2014");
    println!("    peo           Co-employment + risk-pool model (vertical-tuned)");
    println!("    verticals     Tech, life sci, financial services, nonprofits, etc.");
    println!("    benefits      Group medical, dental, vision, 401(k), worksite benefits");
    println!("    consulting    HR consulting + compliance advisory services");
    println!("    zenefits      Zenefits acquisition Feb 2022 + sub-PEO strategy");
    println!("    pricing       Percentage-of-payroll vs PEPM (PEO industry split)");
    println!("    customers     Selected mid-market customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("trinet-cli 0.1.0 (established-PEO personality build)"); }

fn run_about() {
    println!("TriNet Group, Inc. (NYSE:TNET).");
    println!("  Founded:    1988, San Leandro, California.");
    println!("  Founder:    Martin Babinec (founding CEO; now retired board member).");
    println!("  HQ:         Dublin, California.");
    println!("  Listing:    NYSE:TNET since Mar 2014 IPO at $16/share.");
    println!("  History:    PE-controlled by General Atlantic mid-2000s → IPO.");
    println!("  Scale:      ~23,000 SMB + mid-market customers, ~370,000 worksite employees.");
    println!("  Coverage:   US-only (all 50 states + DC). Multiple Certified PEO (CPEO).");
}

fn run_peo() {
    println!("Co-employment + risk-pool model.");
    println!("  TriNet becomes co-employer of record alongside the client.");
    println!("  Files payroll taxes under TriNet's FEIN, sponsors the benefits plans");
    println!("  the worksite employees participate in.");
    println!("  Client manages day-to-day operations + supervision; TriNet handles");
    println!("  payroll-tax compliance, benefits, workers' comp, EPLI.");
    println!("  Distinctive: TriNet operates *vertically-segmented* risk pools rather");
    println!("  than one giant general pool — separate pools for tech, life sciences,");
    println!("  financial services. Lets it offer richer benefits packages tuned to");
    println!("  the demographics of each vertical (e.g. high-deductible HDHPs aren't");
    println!("  what life-sciences postdocs want).");
}

fn run_verticals() {
    println!("Vertical specialisation.");
    println!("  TriNet Tech: SaaS companies, Series A-D venture-backed startups,");
    println!("    dev shops. Equity-aware HR, RSU + ISO administration support.");
    println!("  TriNet Life Sciences: biotech + pharma, lab-safety compliance,");
    println!("    fellowship + grant-funded employee handling, NIH grant compliance.");
    println!("  TriNet Financial Services: RIAs, fund managers, hedge-fund GPs,");
    println!("    SEC + FINRA-aware HR records, deferred-comp plans.");
    println!("  TriNet Professional Services: law firms, accounting, consulting.");
    println!("  TriNet Nonprofit: 501(c)(3)s, grant-funded staff, board governance.");
    println!("  TriNet Main Street: long tail of SMBs that don't fit a vertical.");
}

fn run_benefits() {
    println!("Benefits sponsorship.");
    println!("  Sponsors group health plans through Aetna, UnitedHealthcare, Kaiser,");
    println!("  Blue Shield (CA + nationally), and several regional carriers.");
    println!("  Vertical pools = vertically-tuned premium curves; tech-pool customer");
    println!("  typically sees lower premiums than mixed-pool peers.");
    println!("  Sponsors a TriNet 401(k) plan with Empower as recordkeeper.");
    println!("  Voluntary worksite benefits: pet, legal, identity, supplemental life.");
    println!("  Commuter, FSA, HSA, COBRA administration in-platform.");
}

fn run_consulting() {
    println!("HR consulting services.");
    println!("  Each customer gets a named HR business partner — a TriNet employee,");
    println!("  not a Zendesk queue. Difference vs Gusto/Rippling self-service is");
    println!("  a meaningful part of why mid-market customers pay PEO pricing.");
    println!("  Compliance advisory: ACA, FMLA, state leave laws, harassment training,");
    println!("  termination guidance, RIF / layoff support.");
    println!("  Recruitment + onboarding playbooks per vertical.");
    println!("  Worker's-comp claims handling — TriNet's risk team owns claim resolution.");
}

fn run_zenefits() {
    println!("Zenefits acquisition (Feb 2022).");
    println!("  Bought Zenefits ~$190M cash + stock; rebranded 'TriNet Zenefits'.");
    println!("  Strategic logic: TriNet is mid-market PEO ($35-1,000 PEPM-equivalent);");
    println!("  Zenefits is SMB pure-software ($10-33 PEPM). The combination gives a");
    println!("  graduation path — start a customer on Zenefits, upsell to TriNet PEO");
    println!("  once headcount + benefits-complexity grow.");
    println!("  Some sales overlap remains; the two brands operate semi-independently.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  TriNet PEO: traditionally percentage-of-payroll (~5-8%% of base salary),");
    println!("              now optionally per-employee-per-month (~$80-150 PEPM) for");
    println!("              new customers. Per-employee model preferred by customers");
    println!("              with high-comp employees who don't want %%-of-salary fees.");
    println!("  TriNet Zenefits: software-only tiers $10-33 PEPM.");
    println!("  All-in workers' comp + EPLI typically bundled into PEPM in newer pricing.");
    println!("  Annual contracts standard for PEO; monthly for Zenefits.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 20-1,000 employee US mid-market companies.");
    println!("  Named examples across verticals: many name-brand venture-backed tech");
    println!("  companies (mostly under NDA), Genentech-style biotech alumni firms,");
    println!("  small-to-mid-size RIAs, regional law/accounting/consulting firms.");
    println!("  Tenure: TriNet customers typically 5-10 years on platform — much");
    println!("  longer than the SMB software comparison set.");
    println!("  Geography: nationwide US; California base is historical strength.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "trinet-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "peo" => run_peo(),
        "verticals" => run_verticals(),
        "benefits" => run_benefits(),
        "consulting" => run_consulting(),
        "zenefits" => run_zenefits(),
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
        run_verticals();
        run_benefits();
        run_consulting();
        run_zenefits();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("trinet-cli");
        print_version();
    }
}
