#![deny(clippy::all)]
//! remote-cli — personality CLI for Remote, the owned-entity EOR.
//!
//! Founded 2019 by Job van der Voort (ex-GitLab VP of Product) and Marcelo
//! Lebre (ex-GitLab Director of Engineering). Headquartered nowhere (fully
//! distributed, like Oyster). The differentiator versus the rest of the
//! global-EOR pack: Remote owns its local entities in every country it
//! offers EOR in — no reseller arrangements behind the scenes. This makes
//! the model more expensive to scale but reduces the data-handling +
//! compliance + permanent-establishment risk for the end customer. Reached
//! ~$3B valuation Apr 2022 on a $300M Series C led by SoftBank Vision Fund 2.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Remote owned-entity EOR personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         van der Voort + Lebre 2019; ex-GitLab; owned entities");
    println!("    eor           Owned-entity Employer-of-Record model");
    println!("    contractors   Contractor management + Remote Talent + Equity APIs");
    println!("    api           Remote API for embedded HR/EOR in other products");
    println!("    deel          Public Deel-vs-Remote rivalry + lawsuit context");
    println!("    talent        Remote Talent recruitment + global-hire pipeline");
    println!("    pricing       Flat per-employee-per-month, owned-entity premium");
    println!("    customers     Selected customer + adopter profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("remote-cli 0.1.0 (owned-entity-EOR personality build)"); }

fn run_about() {
    println!("Remote Technology, Inc.");
    println!("  Founded:    2019.");
    println!("  Founders:   Job van der Voort (CEO; ex-GitLab VP Product)");
    println!("              + Marcelo Lebre (President; ex-GitLab Director of Engineering).");
    println!("  HQ:         None — fully distributed company.");
    println!("  Backers:    SoftBank Vision Fund 2, Accel, Sequoia, Index, Two Sigma,");
    println!("              General Catalyst, Base10.");
    println!("  Funding:    $300M Series C Apr 2022 at ~$3B; $200M Series B 2021;");
    println!("              ~$496M total raised.");
    println!("  Coverage:   180+ countries, ~70 of them via Remote-owned local entities.");
}

fn run_eor() {
    println!("Owned-entity EOR model.");
    println!("  Most competitors (Deel, parts of Oyster, Velocity Global) use a partner");
    println!("  network: local-entity firms in each country, contracted to handle the");
    println!("  employment relationship. Remote's pitch is the opposite: own the entity");
    println!("  end-to-end in every country we operate.");
    println!("  Why it matters:");
    println!("    1. Employee data never leaves Remote's chain of custody.");
    println!("    2. Customer's local-compliance exposure is to one counterparty.");
    println!("    3. No counterparty risk from partner financial trouble.");
    println!("    4. Permanent-establishment risk is cleaner to argue.");
    println!("  Cost: significantly more capital to spin up + maintain each entity.");
}

fn run_contractors() {
    println!("Contractor management.");
    println!("  Onboard + pay contractors in 180+ countries from a single dashboard.");
    println!("  Generated locally-compliant contracts; misclassification risk flags.");
    println!("  Local-currency settlement on Remote's rails.");
    println!("  Equity-administration support for international contractors (RSU / option");
    println!("  exercise tax handling — historically a hard problem).");
    println!("  Self-serve onboarding flow for the contractor (no back-and-forth with HR).");
}

fn run_api() {
    println!("Remote API.");
    println!("  Public REST API for embedded HR / EOR / contractor functionality.");
    println!("  Partners use it to add 'hire globally' features to their own products:");
    println!("  job boards, ATSs, payroll software, freelance marketplaces.");
    println!("  Endpoints: create employment, create contractor agreement, run payroll,");
    println!("  fetch employee record, push time-off, push expenses.");
    println!("  This is one of the more aggressive 'EOR as a developer API' plays —");
    println!("  competes most directly with Deel's APIs + Rippling's PEPM-API hybrid.");
}

fn run_deel() {
    println!("Deel rivalry context (public + litigation).");
    println!("  Remote + Deel are direct competitors in the global EOR space.");
    println!("  Mar 2025: Deel sued by Rippling alleging a Deel employee infiltrated");
    println!("  Rippling Slack as a spy — Remote is not a party to that suit but the");
    println!("  surrounding industry narrative names Remote as a similarly-aggrieved peer.");
    println!("  Remote has separately + publicly criticised Deel for opaque pricing,");
    println!("  partner-network model, and unfavourable contractor terms — see");
    println!("  the 'Remote vs Deel' comparison page Remote maintains as a marketing asset.");
    println!("  The EOR market is unusually publicly-pugilistic for an enterprise SaaS category.");
}

fn run_talent() {
    println!("Remote Talent.");
    println!("  Recruitment-marketplace product: job board + sourcing tools focused on");
    println!("  global-remote hires.");
    println!("  Tightly integrated with the EOR product — post a role, source candidates,");
    println!("  hire compliantly through Remote, all in one tenant.");
    println!("  Newer add-on (post-2023); positioning push is 'we don't just process your");
    println!("  global hires, we help you find them'.");
    println!("  Also: Global Employee Wellness, Global Benefits, Remote Equity Calculator");
    println!("  rounds out the platform-side play.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  EOR:        ~$599 per employee per month flat, all countries.");
    println!("              No take-rate on salary, no deposit on most countries.");
    println!("  Contractor: ~$29 per contractor per month.");
    println!("  Remote API: separate enterprise pricing, usage-based on calls + employees.");
    println!("  Annual prepay = ~15-20%% discount; monthly is default.");
    println!("  Higher headline price than partner-network EORs but explicitly justified");
    println!("  as 'you're paying for owned-entity compliance, not a thinner reseller margin'.");
}

fn run_customers() {
    println!("Customer + adopter profile:");
    println!("  Sweet spot: 200-5,000 FTE distributed-first companies, especially open-source");
    println!("  ecosystem alumni (lots of GitLab + HashiCorp + Mozilla-style customers via");
    println!("  the founder network).");
    println!("  Publicly named customers: GitLab, DoorDash (selectively), HelloFresh, Loom,");
    println!("  several large open-source companies, multiple Y Combinator portfolios.");
    println!("  Many large customers go multi-vendor: Remote for tier-1 markets where");
    println!("  owned-entity matters, Deel/Oyster for long-tail countries.");
    println!("  Heavy adoption in EU + LATAM + APAC hires from US-HQ companies.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "remote-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "eor" => run_eor(),
        "contractors" => run_contractors(),
        "api" => run_api(),
        "deel" => run_deel(),
        "talent" => run_talent(),
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
        run_api();
        run_deel();
        run_talent();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("remote-cli");
        print_version();
    }
}
