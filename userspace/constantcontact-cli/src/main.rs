#![deny(clippy::all)]
//! constantcontact-cli — personality CLI for Constant Contact, the
//! original US small-business email marketing tool.
//!
//! Founded 1995 in Waltham, Massachusetts by Randy Parker as "Roving
//! Software" — predating the modern web SaaS playbook by nearly a
//! decade. Renamed Constant Contact in 2004. IPO'd on Nasdaq in 2007
//! as CTCT, was acquired by Endurance International Group in 2015 for
//! $1.1B, then spun out and sold to Clearlake Capital + Siris Capital
//! in 2021 for $1B+, and now operates independently. The customer base
//! is the US main-street SMB: restaurants, non-profits, churches,
//! schools, salons, real-estate agents — small lists, recurring sends.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Constant Contact main-street-SMB personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Randy Parker 1995, IPO 2007, multiple PE sales");
    println!("    email         Templates + drag-drop editor, SMB-first");
    println!("    events        Event signup + invite + RSVP tooling");
    println!("    social        Facebook/Instagram/Google Ads in one console");
    println!("    automations   Path Builder visual flows");
    println!("    sms           Lead generation + SMS marketing");
    println!("    pricing       Per-contact tiers, 30-day money-back");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("constantcontact-cli 0.1.0 (1995-era main-street personality build)"); }

fn run_about() {
    println!("Constant Contact, Inc.");
    println!("  Founded:    1995, Waltham, MA, originally as Roving Software.");
    println!("  Founder:    Randy Parker.");
    println!("  Rename:     Roving Software -> Constant Contact in 2004.");
    println!("  IPO:        Nasdaq:CTCT 2007.");
    println!("  Endurance:  Acquired by Endurance Intl Group 2015 for ~$1.1B.");
    println!("  Spin-out:   Endurance restructured 2021, Clearlake + Siris");
    println!("              Capital took Constant Contact private at ~$1B+.");
    println!("  Posture:    US SMB main-street, not enterprise; large customer");
    println!("              service org with US-based phone support.");
}

fn run_email() {
    println!("Email marketing — the original product.");
    println!("  Drag-drop block editor with hundreds of templates.");
    println!("  Industry templates: restaurant menus, non-profit appeals,");
    println!("  real-estate listings, school newsletters, etc.");
    println!("  AI Content Generator added 2023-2024 for subject + body drafts.");
    println!("  Branded Templates: pulls fonts/colours/logo from a website URL.");
    println!("  A/B testing on subject line; engagement-based send optimisation.");
}

fn run_events() {
    println!("Events — distinctive Constant Contact feature.");
    println!("  Event-signup pages with custom registration fields.");
    println!("  RSVP tracking, ticket sales (via Stripe/PayPal), waitlists.");
    println!("  Event-specific email automations: invite, reminder, day-of, thank-you.");
    println!("  Useful for non-profits + community orgs + classes + workshops.");
    println!("  Many SMBs use Constant Contact for events alone.");
}

fn run_social() {
    println!("Social + ads — one console for SMB advertisers.");
    println!("  Schedule + publish to Facebook, Instagram, X, LinkedIn, Google.");
    println!("  Boost a post or run a lead-ad from inside Constant Contact.");
    println!("  Audience sync: email list -> Facebook custom audience.");
    println!("  Designed for the SMB owner who doesn't want a separate Hootsuite.");
}

fn run_automations() {
    println!("Path Builder — visual automation flows.");
    println!("  Trigger -> wait -> condition -> send sequences.");
    println!("  Triggers: new contact, link clicked, opened email, custom field.");
    println!("  Templates: welcome series, win-back, post-purchase, event follow-up.");
    println!("  Designed for the user who can install a printer but not write SQL.");
}

fn run_sms() {
    println!("SMS marketing.");
    println!("  Per-message US SMS, sub-account for the merchant's number.");
    println!("  Email + SMS in the same Path Builder flow.");
    println!("  Compliance: TCPA opt-in language + STOP keyword auto-handled.");
    println!("  Cross-sell to existing Constant Contact email customers.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Lite        ~$12/mo for 500 contacts (basic email + signup).");
    println!("  Standard    adds automations + segmentation, per-contact tiers.");
    println!("  Premium     adds custom reporting + dynamic content + dedicated mgr.");
    println!("  SMS         credits separate from contact tier.");
    println!("  30-day money-back guarantee on first signup.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Hundreds of thousands of US small businesses, non-profits,");
    println!("  churches, schools, restaurants, real-estate offices, salons,");
    println!("  fitness studios, professional service firms.");
    println!("  Long-tail main-street US base; not big-brand showcases.");
    println!("  Phone + chat support staffed in the US is a differentiator.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "constantcontact-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "email" => run_email(),
        "events" => run_events(),
        "social" => run_social(),
        "automations" => run_automations(),
        "sms" => run_sms(),
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
        run_email();
        run_events();
        run_social();
        run_automations();
        run_sms();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("constantcontact-cli");
        print_version();
    }
}
