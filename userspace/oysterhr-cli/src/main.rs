#![deny(clippy::all)]
//! oysterhr-cli — personality CLI for Oyster, the distributed-by-design EOR.
//!
//! Founded 2020 by Tony Jamous (Nexmo founder, sold to Vonage 2016) and
//! Jack Mardack as a global Employer of Record built for fully-distributed
//! companies. Headquartered nowhere — both founders openly anti-HQ, the
//! company is operationally remote-first across 70+ countries. Reached
//! unicorn status (~$1B) Apr 2022 at the peak of the remote-work boom on a
//! $150M Series C led by Stripes + Coatue. Certified B Corporation; an
//! explicit social-mission flavour (lifting wages in lower-cost-of-living
//! countries via cross-border hiring access).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Oyster global EOR personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Tony Jamous + Jack Mardack 2020; distributed-by-design");
    println!("    eor           Employer-of-Record model across 180+ countries");
    println!("    contractors   Contractor management + multi-country invoicing");
    println!("    benefits      Local-market health + retirement benefits per country");
    println!("    misclass      Worker-classification audit + analyzer tools");
    println!("    bcorp         Certified B Corp + mission-driven hiring positioning");
    println!("    pricing       Flat per-employee-per-month, no take-rate on salary");
    println!("    customers     Selected customer + adopter profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("oysterhr-cli 0.1.0 (distributed-EOR personality build)"); }

fn run_about() {
    println!("Oyster HR, Inc.");
    println!("  Founded:    2020.");
    println!("  Founders:   Tony Jamous (CEO; founded Nexmo, sold to Vonage 2016 ~$230M)");
    println!("              + Jack Mardack (Co-founder).");
    println!("  HQ:         None — fully distributed company, by design.");
    println!("  Backers:    Stripes, Coatue, Emergence, The Slack Fund, Avid Ventures,");
    println!("              Connect Ventures, Tribe Capital.");
    println!("  Funding:    $150M Series C Apr 2022 at ~$1B valuation (unicorn).");
    println!("              ~$226M total raised.");
    println!("  Coverage:   180+ countries on contractors, ~70+ countries on EOR.");
}

fn run_eor() {
    println!("Employer-of-Record model.");
    println!("  Customer wants to hire a developer in Argentina without opening an");
    println!("  Argentine entity. Oyster spins up the local employment relationship:");
    println!("    1. Oyster's Argentine entity becomes the legal employer.");
    println!("    2. Oyster runs local payroll, files local taxes + social security.");
    println!("    3. Oyster provides locally-compliant employment contract.");
    println!("    4. Oyster provides locally-required benefits + statutory leave.");
    println!("  Customer pays Oyster monthly invoice in their HQ currency.");
    println!("  Customer manages the employee day-to-day; Oyster is the back office.");
    println!("  Hire-time is typically 2-5 business days for owned-entity countries.");
}

fn run_contractors() {
    println!("Contractor management.");
    println!("  Onboard + pay contractors in 180+ countries.");
    println!("  Local-currency payouts (XE/Wise rails under the hood).");
    println!("  Generated invoices + locally-compliant contractor agreements.");
    println!("  Tax form collection (W-8BEN, W-9, local equivalents).");
    println!("  Misclassification analyzer flags contractors who legally look like");
    println!("  employees in their local jurisdiction — recommends EOR conversion");
    println!("  to avoid back-tax + permanent-establishment risk.");
}

fn run_benefits() {
    println!("Benefits administration.");
    println!("  Local-standard healthcare in each market (private supplement on top of");
    println!("  national systems where applicable).");
    println!("  Pension / retirement contributions matching local statutory + market norms.");
    println!("  Equity-administration support — Oyster acts as the local payroll for");
    println!("  RSU/ESPP/option exercises with their local tax implications.");
    println!("  Premium benefits packages selectable per employee, charged through.");
    println!("  Statutory leave (parental, sick, vacation) tracked + funded automatically.");
}

fn run_misclass() {
    println!("Misclassification analyzer.");
    println!("  Free public tool: classify-a-worker quiz that asks ~15 questions about");
    println!("  the working relationship and outputs a jurisdiction-specific risk score.");
    println!("  Inside the platform: bulk audit across your contractor base, with");
    println!("  per-country rules-engine (Dynamex 'ABC' test for California, EU PWD,");
    println!("  UK IR35, Spanish 'Riders Law', etc.).");
    println!("  This is the wedge that pulls customers from 'just paying contractors via");
    println!("  Wise' into the EOR product.");
}

fn run_bcorp() {
    println!("B-Corp + mission positioning.");
    println!("  Certified B Corporation since 2021.");
    println!("  Public mission framing: 'a more equal world by making it possible for");
    println!("  every person to thrive economically' — i.e., hiring in lower-COL countries");
    println!("  at globally-competitive wages.");
    println!("  Impact metrics published (talent-flow into emerging markets, etc.).");
    println!("  Marketing differentiates against Deel (no B-Corp, more enterprise-aggressive)");
    println!("  by leaning into mission-aligned customer base.");
    println!("  Internal: 4-day workweek pilot, transparent salary bands, async-default ops.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  EOR:        flat ~$499 per employee per month, no take-rate on salary.");
    println!("              (vs. Deel/Remote pricing in same band ~$599 PEPM).");
    println!("  Contractor: ~$29 per contractor per month, all 180+ countries.");
    println!("  Annual commit discounts; per-month month-to-month also available.");
    println!("  Misclassification analyzer is free, including for non-customers — funnel.");
    println!("  No setup fee, no deposit on most countries (some require local-entity deposit).");
}

fn run_customers() {
    println!("Customer + adopter profile:");
    println!("  Sweet spot: 50-1,000 FTE distributed-first companies, especially Web3,");
    println!("  developer-tools, climate-tech, B-Corp-aligned brands.");
    println!("  Publicly named customers: Buffer, Calendly (selectively), Bumble (early),");
    println!("  Grammarly (partial), Bolt, Hack The Box, several Y Combinator portfolios.");
    println!("  Common pattern: fully-remote startup hiring from emerging markets,");
    println!("  values-driven choice of EOR vendor over sheer feature-count.");
    println!("  Many customers also use Deel for some countries + Oyster for others —");
    println!("  multi-vendor EOR is common at the 200+ FTE band.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "oysterhr-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "eor" => run_eor(),
        "contractors" => run_contractors(),
        "benefits" => run_benefits(),
        "misclass" => run_misclass(),
        "bcorp" => run_bcorp(),
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
        run_eor();
        run_contractors();
        run_benefits();
        run_misclass();
        run_bcorp();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("oysterhr-cli");
        print_version();
    }
}
