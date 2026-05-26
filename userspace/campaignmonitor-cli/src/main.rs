#![deny(clippy::all)]
//! campaignmonitor-cli — personality CLI for Campaign Monitor, the
//! designer-oriented Australian ESP.
//!
//! Founded 2004 in Sydney, Australia by Ben Richardson and David Greiner —
//! two web designers who built the product to solve their own problem:
//! freelance designers needed to send email newsletters on behalf of
//! clients with full brand control and no Mailchimp-style "powered by"
//! footer. Famous for the Code Your Own approach: hand-coded HTML email
//! templates uploaded to the platform, with deep designer ergonomics. Took
//! its first outside investment (Insight Partners) in 2014 — $250M at a
//! reported $410M valuation. Sold to Marlin Equity Partners 2022 and
//! merged into the Marlin-owned email-marketing holding group along with
//! Vision6, Sailthru, Liveclicker, Selligent, and Emma.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Campaign Monitor Sydney designer-ESP personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Richardson+Greiner 2004 Sydney; Insight 2014; Marlin 2022");
    println!("    designer      Code-your-own templates + drag-drop builder");
    println!("    automation    Visual journey builder + transactional");
    println!("    agencies      White-label client management for design shops");
    println!("    transactional Transactional API + SMTP service");
    println!("    analytics     Worldview map, link analytics, comparison");
    println!("    pricing       Per-subscriber tiers, pay-as-you-go option");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("campaignmonitor-cli 0.1.0 (designer-first personality build)"); }

fn run_about() {
    println!("Campaign Monitor (CM Group / now Marlin EQT).");
    println!("  Founded:    2004, Sydney, Australia.");
    println!("  Founders:   Ben Richardson, David Greiner — web designers.");
    println!("  Origin:     Built to solve their own designer-agency workflow:");
    println!("              email newsletters on behalf of clients without the");
    println!("              old 'powered by' footer or Mailchimp branding.");
    println!("  Funding:    Bootstrapped to 2014; $250M Insight Partners 2014.");
    println!("  Sales:      2017 holdco CM Group rolled up Sailthru, Liveclicker,");
    println!("              Vuture, Emma. 2022 sold to Marlin Equity Partners.");
}

fn run_designer() {
    println!("Designer-first email building.");
    println!("  Code-your-own: upload hand-coded HTML, use template language");
    println!("  for editable regions client can edit but not break.");
    println!("  Drag-drop builder: also available, but the brand identity is");
    println!("  'we don't pretend designers want WYSIWYG-only'.");
    println!("  Inbox preview across 20+ clients (Litmus-style screenshots).");
    println!("  Dark-mode-aware rendering preview.");
}

fn run_automation() {
    println!("Visual Journeys — automation builder.");
    println!("  Canvas of triggers + delays + decisions + branches.");
    println!("  Triggers: list join, date, segment entry, custom event, API call.");
    println!("  Branches: based on opens, clicks, custom field values.");
    println!("  Per-step performance: open + click + conversion per journey node.");
}

fn run_agencies() {
    println!("Agencies — white-label client management.");
    println!("  An 'agency' admin owns many 'clients'; each client is a customer.");
    println!("  Agency can rebill the client with markup at any rate.");
    println!("  Per-client branding, sender domain, login, segregated data.");
    println!("  Long popular among design + comms agencies in AU + NZ + UK.");
}

fn run_transactional() {
    println!("Transactional sending.");
    println!("  Transactional API for receipts, password resets, order updates.");
    println!("  SMTP relay endpoint for legacy systems.");
    println!("  Same deliverability infra as marketing sends, separate billing.");
    println!("  Templated transactional emails editable in the Campaign Monitor UI.");
}

fn run_analytics() {
    println!("Reporting + analytics.");
    println!("  Worldview map: geographic distribution of opens + clicks.");
    println!("  Link reporting: per-link click counts, top performers.");
    println!("  Campaign comparison: side-by-side performance vs prior sends.");
    println!("  Email Client + Device breakdown.");
    println!("  Forwards-to-friends and social-share tracking.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Basic        per-subscriber; basic features.");
    println!("  Unlimited    per-subscriber; unlimited sends + journeys.");
    println!("  Premier      adds advanced segmentation, time zone send,");
    println!("               premium support, advanced templates.");
    println!("  Pay-as-you-go credits for infrequent senders.");
    println!("  Agencies: special pricing tier with client markup.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Adidas, BuzzFeed, Birchbox, Topshop, Bose, Rip Curl, Coca-Cola,");
    println!("  many design + marketing agencies (Brand Union, R/GA-class).");
    println!("  Especially strong in AU + NZ + UK design-agency community.");
    println!("  Public case studies on campaignmonitor.com showcase brand-led emails.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "campaignmonitor-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "designer" => run_designer(),
        "automation" => run_automation(),
        "agencies" => run_agencies(),
        "transactional" => run_transactional(),
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
        run_designer();
        run_automation();
        run_agencies();
        run_transactional();
        run_analytics();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("campaignmonitor-cli");
        print_version();
    }
}
