#![deny(clippy::all)]

//! hubspot-cli — SlateOS HubSpot (inbound marketing/sales/service CRM)
//!
//! Single personality: `hubspot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hubspot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hubspot [OPTIONS]");
        println!("HubSpot (Slate OS) — inbound CRM (marketing + sales + service)");
        println!();
        println!("Options:");
        println!("  --marketing-hub        Marketing Hub (email, landing pages, automation)");
        println!("  --sales-hub            Sales Hub (CRM, pipeline, sequences)");
        println!("  --service-hub          Service Hub (ticketing, knowledge base)");
        println!("  --cms-hub              CMS Hub (website + blog)");
        println!("  --operations-hub       Operations Hub (data sync + cleanup)");
        println!("  --commerce-hub         Commerce Hub (payments + quotes, 2023+)");
        println!("  --free-crm             Free CRM (unlimited users, basic features)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("HubSpot 2024 (Slate OS)"); return 0; }
    println!("HubSpot 2024 (Slate OS)");
    println!("  Vendor: HubSpot, Inc. (Cambridge, MA — NYSE:HUBS)");
    println!("  Founders: Brian Halligan + Dharmesh Shah (MIT Sloan grads, 2006)");
    println!("          Halligan: coined 'inbound marketing' — antithesis of cold-calling outbound");
    println!("          Shah: started OnStartups blog, became HubSpot's CTO + Culture Code author");
    println!("  Founded: 2006 — IPO'd 2014 at $25/share");
    println!("          2024 market cap ~$30-35B, stock around $600+");
    println!("  Scale: 228,000+ customers across 135+ countries");
    println!("        ~$2.6B revenue FY2024 (~24% YoY growth)");
    println!("        ~8,000 employees globally");
    println!("  Pricing tiers:");
    println!("    Free Tools — $0, unlimited users, basic CRM");
    println!("    Starter — from $20/mo (1 seat), then per-seat add-ons");
    println!("    Professional — from $890/mo (Marketing) / $100/seat/mo (Sales)");
    println!("    Enterprise — from $3,600/mo (Marketing) / $150/seat/mo (Sales)");
    println!("  Marketing Hub features:");
    println!("    - Email marketing with drag-and-drop builder + AI subject lines");
    println!("    - Landing page + form builder (no-code)");
    println!("    - Marketing automation workflows (drip campaigns, lead scoring, nurture)");
    println!("    - SEO recommendations + content strategy tool");
    println!("    - Social media scheduling + monitoring (Twitter/X, LinkedIn, Facebook, Instagram)");
    println!("    - ABM (Account-Based Marketing) at Enterprise tier");
    println!("  Sales Hub features:");
    println!("    - Deal pipeline with drag-and-drop stages");
    println!("    - Email sequences with auto-stop on reply");
    println!("    - Meeting scheduler (HubSpot Meetings — Calendly competitor)");
    println!("    - Call recording + AI conversation intelligence");
    println!("    - Quote generation + e-signature");
    println!("    - Predictive lead scoring (Enterprise)");
    println!("  Service Hub features:");
    println!("    - Ticketing with SLAs + routing");
    println!("    - Live chat + chatbots");
    println!("    - Knowledge base (public help center)");
    println!("    - Customer feedback surveys (NPS, CSAT, CES)");
    println!("    - Customer portal (account-level ticket history)");
    println!("  Differentiators vs Salesforce:");
    println!("    - All-in-one platform — marketing + sales + service in ONE database");
    println!("    - Free CRM tier (unlimited users) drives bottom-up adoption");
    println!("    - Much easier admin UX — no consultants needed for basic config");
    println!("    - Inbound methodology baked into product (educational content focus)");
    println!("  Acquisitions: Clearbit (data enrichment) 2023 for ~$150M");
    println!("              Hustle Co. (cold outreach) 2021");
    println!("              The Hustle (newsletter) 2021");
    println!("  Customers: SMB to mid-market sweet spot (10-1,000 employees)");
    println!("            DoorDash, Trello, Shopify Plus partners, Reddit, Atlassian (early days)");
    println!("  Critique: scales painfully past mid-market — bumps into Salesforce ceiling");
    println!("           pricing jumps brutal between Pro and Enterprise");
    println!("           bundles can lock you in (per-seat across hubs)");
    println!("  Differentiator: best-in-class UX + free tier + inbound thought leadership");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hubspot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hubspot(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hubspot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hubspot"), "hubspot");
        assert_eq!(basename(r"C:\bin\hubspot.exe"), "hubspot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hubspot.exe"), "hubspot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hubspot(&["--help".to_string()], "hubspot"), 0);
        assert_eq!(run_hubspot(&["-h".to_string()], "hubspot"), 0);
        let _ = run_hubspot(&["--version".to_string()], "hubspot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hubspot(&[], "hubspot");
    }
}
