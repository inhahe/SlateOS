#![deny(clippy::all)]
//! softr-cli — personality CLI for Softr, the no-code app + portal
//! builder on top of Airtable + Google Sheets.
//!
//! Founded 2019 in Berlin by Mariam Hakobyan (CEO, ex-Lufthansa
//! engineering) and Artur Mkrtchyan (CTO, ex-Lufthansa engineering),
//! both originally from Armenia. Softr's pitch is non-technical: turn
//! an Airtable base or a Google Sheet into a branded customer portal,
//! members-only site, marketplace, or internal directory without
//! writing code. The defining vs-Bubble distinction is template-first
//! + data-source-bound — Softr customers are typically operators or
//! founders, not engineers. Picked up Series A from FirstMark in 2022.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Softr no-code portal-on-Airtable personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Hakobyan + Mkrtchyan 2019 Berlin; Series A FirstMark 2022");
    println!("    blocks        Pre-built blocks: list, detail, form, calendar, kanban");
    println!("    datasources   Airtable-first; Google Sheets; Notion; HubSpot; SmartSuite");
    println!("    portals       Members-only client portals + role-based access");
    println!("    templates     Template marketplace: directories, marketplaces, intranets");
    println!("    pricing       Free + Basic + Professional + Business tiers");
    println!("    customers     Operators + founders + agencies + non-technical SMB");
    println!("    nocode        No-code positioning vs Bubble + Webflow");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("softr-cli 0.1.0 (no-code-portal-on-airtable personality build)"); }

fn run_about() {
    println!("Softr (Softr Studio GmbH).");
    println!("  Founded:    2019, Berlin, Germany.");
    println!("  Founders:   Mariam Hakobyan (CEO; ex-Lufthansa engineering) +");
    println!("              Artur Mkrtchyan (CTO; ex-Lufthansa engineering).");
    println!("              Both originally from Armenia.");
    println!("  Backers:    FirstMark Capital (Series A lead), Atlantic Labs, Slack Fund,");
    println!("              Y Combinator, prominent operator angels.");
    println!("  Funding:    ~$15M Series A late 2022; product-led-growth-funded otherwise.");
    println!("  Position:   non-technical no-code — turn Airtable into a real product.");
    println!("  Anti-segment: developers (who go to Bubble, Retool, or actual code).");
}

fn run_blocks() {
    println!("Pre-built blocks (the core building unit).");
    println!("  List block:    grid + cards + table of records from a data source.");
    println!("  Detail block:  single-record page with fields + actions.");
    println!("  Form block:    create / update record via a styled form.");
    println!("  Calendar:      date-keyed records on a month / week / day grid.");
    println!("  Kanban:        records grouped by a status field.");
    println!("  Inbox + chat:  for back-and-forth between portal users + admins.");
    println!("  Charts, FAQ, hero, pricing, testimonials, navbar, footer — full landing kit.");
    println!("  Blocks are styled + bound to data via the inspector — no canvas drawing.");
}

fn run_datasources() {
    println!("Data sources.");
    println!("  Airtable: the original + still primary integration — bases as backend.");
    println!("  Google Sheets: sheets-as-tables for the spreadsheet-native crowd.");
    println!("  Notion: databases as Softr data sources.");
    println!("  HubSpot: CRM-backed portals — pull contacts / deals / tickets.");
    println!("  SmartSuite: enterprise alt-to-Airtable, growing integration.");
    println!("  Native Softr Database: built-in option for users who do not want Airtable.");
    println!("  No raw SQL — the whole abstraction is 'records in a table'.");
}

fn run_portals() {
    println!("Members-only portals (the flagship use case).");
    println!("  User authentication: email + magic-link, Google, Apple, custom SSO.");
    println!("  Role-based access: filter records, hide blocks, restrict pages per group.");
    println!("  Logged-in-user filters: 'show only records where the assignee is me'.");
    println!("  Stripe-backed paywalls + recurring memberships for paid portals.");
    println!("  Custom domain + branding + email template customisation.");
    println!("  Common shapes: client portals, partner portals, alumni networks, course");
    println!("  delivery, community directories, internal staff intranets.");
}

fn run_templates() {
    println!("Template marketplace.");
    println!("  Hundreds of pre-built templates ship with both the design + the Airtable");
    println!("  schema — clone and have a working product in minutes.");
    println!("  Categories: directories, job boards, marketplaces, freelancer portfolios,");
    println!("  client portals, internal tools, online courses, membership sites,");
    println!("  resource hubs, simple landing pages, Airbnb-style booking pages.");
    println!("  The template-first onboarding is core to Softr's product-led growth funnel.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:         build + share a Softr site, Softr branding, basic features.");
    println!("  Basic:        ~$59/month, custom domain + remove Softr branding.");
    println!("  Professional: ~$167/month, 5,000 internal app users + advanced features.");
    println!("  Business:     ~$323/month, white-label + audit logs + SAML SSO + priority.");
    println!("  Pricing is per-workspace + per-app-user tier, not per-builder seat —");
    println!("  unusual model in low-code, geared toward portal volume not team size.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: non-technical founders + operators + agencies + small NGOs.");
    println!("  Industries: coaching + courses, real-estate listings, association directories,");
    println!("  consulting client portals, community-led businesses, recruitment agencies.");
    println!("  Geographic: heavy EU + US; growing APAC; particularly strong in the EU SMB");
    println!("  segment where 'data lives in Airtable' is already the default operations stack.");
    println!("  Common origin: 'I run my business out of Airtable + I need a public face for it'.");
    println!("  Anti-segment: engineers (go to Bubble or write code) + enterprises (go to Retool).");
}

fn run_nocode() {
    println!("No-code positioning.");
    println!("  Softr:    template + data-source first; you assemble pre-built blocks.");
    println!("  Bubble:   pixel-level visual programming; closer to building a real app.");
    println!("  Webflow:  designer-first; powerful CMS but weak app + portal logic.");
    println!("  Glide:    mobile-first apps from Google Sheets; web is secondary.");
    println!("  Softr wins on time-to-first-working-portal + on non-developer onboarding.");
    println!("  Softr loses on UI customisability vs Bubble + Webflow.");
    println!("  Trade-off chosen deliberately: opinionated blocks > infinite flexibility.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "softr-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "blocks" => run_blocks(),
        "datasources" => run_datasources(),
        "portals" => run_portals(),
        "templates" => run_templates(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "nocode" => run_nocode(),
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
        run_blocks();
        run_datasources();
        run_portals();
        run_templates();
        run_pricing();
        run_customers();
        run_nocode();
    }

    #[test]
    fn help_and_version() {
        print_help("softr-cli");
        print_version();
    }
}
