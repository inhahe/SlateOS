#![deny(clippy::all)]
//! zenefits-cli — personality CLI for Zenefits / TriNet Zenefits, the
//! free-HR-monetised-via-broker-commissions pioneer (and cautionary tale).
//!
//! Founded 2013 in San Francisco by Parker Conrad (CEO; later founder of
//! Rippling after his ouster) and Laks Srini. The novel commercial model:
//! give away HR + benefits software for free, monetise as the licensed
//! health-insurance broker of record for the customer's group health plan.
//! Reached $4.5B valuation 2015. Spectacularly imploded 2016 when it
//! emerged that hundreds of unlicensed sales reps had been selling
//! insurance, that Conrad had built a macro to fake licensing-course
//! completion, and the resulting California DOI settlement, mass layoffs,
//! and Conrad's resignation. Several years of rebuilding under David
//! Sacks (interim) + Jay Fulcher; acquired by TriNet Mar 2022 and folded
//! into TriNet Zenefits.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Zenefits / TriNet Zenefits HR-via-broker-commission personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Parker Conrad 2013 SF; broker-commission model");
    println!("    model         Free HR, monetise as broker of record");
    println!("    scandal       The 2016 unlicensed-broker scandal + Conrad ouster");
    println!("    rebuild       Sacks / Fulcher rebuild years 2016-2021");
    println!("    trinet        Mar 2022 TriNet acquisition + rebrand to TriNet Zenefits");
    println!("    product       HR + payroll + benefits + time + scheduling");
    println!("    pricing       Per-employee-per-month, post-broker-commission era");
    println!("    customers     SMB customer base profile + Rippling overlap");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("zenefits-cli 0.1.0 (broker-commission cautionary-tale personality build)"); }

fn run_about() {
    println!("Zenefits (now TriNet Zenefits, a TriNet brand).");
    println!("  Founded:    2013, San Francisco (YC W13).");
    println!("  Founders:   Parker Conrad (CEO 2013-2016), Laks Srini.");
    println!("  Backers (peak): Andreessen Horowitz, Fidelity, TPG, Insight, Founders Fund.");
    println!("  Peak:       $4.5B Series C valuation May 2015 — fastest SaaS company");
    println!("              to $20M ARR at the time.");
    println!("  Acquisition: TriNet acquired Feb 2022 for ~$190M cash + stock; rebranded");
    println!("              as 'TriNet Zenefits' and sold into the existing TriNet base.");
}

fn run_model() {
    println!("Original commercial model.");
    println!("  Give away HR software for free: onboarding, PTO, employee directory,");
    println!("  later payroll + scheduling + compliance.");
    println!("  Become the licensed broker of record for the customer's group health,");
    println!("  dental, vision, life, disability, 401(k).");
    println!("  Earn carrier commissions (~3-6%% of premiums) on those policies.");
    println!("  Pitch to SMBs: 'your benefits broker should be giving you HR software");
    println!("  for free — anyone charging you is double-dipping'.");
    println!("  This model was the breakthrough innovation + the foundation of the scandal.");
}

fn run_scandal() {
    println!("The 2016 unlicensed-broker scandal.");
    println!("  Nov 2015 Buzzfeed exposed that many California Zenefits sales reps had");
    println!("  not completed the required 52-hour pre-licensing coursework.");
    println!("  Subsequent investigation revealed an internal browser macro built by");
    println!("  Parker Conrad himself that automated 'completing' the licensing-hours");
    println!("  online course on autopilot — bypassing California DOI requirements.");
    println!("  Feb 2016: Conrad resigned at board insistence. David Sacks (then COO)");
    println!("  took over as interim CEO.");
    println!("  Aug 2017: $7M settlement with California DOI; further state settlements.");
    println!("  Layoffs of >450 staff in 2016. Multiple rounds of valuation reset (down to");
    println!("  ~$2B then lower).");
    println!("  Conrad later founded Rippling 2016, explicitly framed as 'doing Zenefits");
    println!("  properly this time' — and has since out-grown his old company by orders of magnitude.");
}

fn run_rebuild() {
    println!("Rebuild years (2016-2021).");
    println!("  David Sacks: regulatory clean-up + Z2 pivot away from broker-only revenue.");
    println!("  Jay Fulcher (CEO from late 2017): focus on bottom-of-market SMBs + payroll.");
    println!("  Z2 launched 2017: 'choose your own broker' — break broker-of-record lock-in,");
    println!("  let customers bring an existing broker + pay Zenefits a per-employee fee.");
    println!("  Several smaller acquisitions to round out platform (HR Hero compliance, etc.).");
    println!("  Never re-attained the 2015 hype valuation or growth rate.");
}

fn run_trinet() {
    println!("TriNet acquisition (Feb 2022).");
    println!("  TriNet (NYSE:TNET), a much older PEO, bought Zenefits for ~$190M.");
    println!("  Rationale: TriNet's PEO is enterprise + mid-market; Zenefits SMB +");
    println!("  pure-software complement; combined offers SMB-to-mid-market continuum.");
    println!("  Rebranded 'TriNet Zenefits'; product continues to exist as TriNet's");
    println!("  software-only sub-brand for the sub-PEO customer.");
    println!("  Engineering organisation largely retained; brand and identity preserved.");
}

fn run_product() {
    println!("Current product stack (TriNet Zenefits).");
    println!("  HR core: onboarding, employee directory, document management, e-signature.");
    println!("  Payroll: native multi-state, tax filing, W-2/1099 year-end.");
    println!("  Benefits administration: either via TriNet Insurance Services as broker,");
    println!("           or BYOB ('bring your own broker') with sync.");
    println!("  Time + scheduling: clock-in, shift planning, overtime rules.");
    println!("  Compliance: state-specific posters, ACA reporting, EEO-1.");
    println!("  Mobile app for employee self-service.");
}

fn run_pricing() {
    println!("Pricing model (post-acquisition).");
    println!("  Essentials:   ~$10 PEPM, HR core only.");
    println!("  Growth:       ~$20 PEPM, adds performance + comp + people analytics.");
    println!("  Zen:          ~$33 PEPM, adds engagement + advanced reporting.");
    println!("  Add-ons:      payroll +~$6 PEPM, benefits-admin add-on, etc.");
    println!("  Broker-commission revenue still exists when customer chooses TriNet");
    println!("  Insurance Services as broker, but is no longer the only revenue model.");
}

fn run_customers() {
    println!("Customer base profile:");
    println!("  Sweet spot: 5-50 employee US SMBs (very low end of HR-software market).");
    println!("  Industry: professional services, e-commerce, agencies, dental practices,");
    println!("  small medical offices, fitness studios.");
    println!("  Historical: at peak ran payroll/benefits for ~10,000 small businesses.");
    println!("  Heavy overlap with the Rippling customer base — the running joke in the");
    println!("  industry is that Rippling is what Zenefits could have become without 2016.");
    println!("  Modern position: SMB option below Gusto on price, below Rippling on features.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "zenefits-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "model" => run_model(),
        "scandal" => run_scandal(),
        "rebuild" => run_rebuild(),
        "trinet" => run_trinet(),
        "product" => run_product(),
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
        run_model();
        run_scandal();
        run_rebuild();
        run_trinet();
        run_product();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("zenefits-cli");
        print_version();
    }
}
