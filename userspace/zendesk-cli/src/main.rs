#![deny(clippy::all)]

//! zendesk-cli — OurOS Zendesk Support (the original SaaS helpdesk)
//!
//! Single personality: `zendesk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zendesk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zendesk [OPTIONS]");
        println!("Zendesk (OurOS) — customer service / helpdesk platform");
        println!();
        println!("Options:");
        println!("  --support              Zendesk Support (ticketing core)");
        println!("  --suite                Suite (Support + Guide + Chat + Talk + Explore)");
        println!("  --guide                Guide (Knowledge Base + Help Center)");
        println!("  --chat                 Chat (live chat, ex-Zopim)");
        println!("  --talk                 Talk (cloud-based call center)");
        println!("  --explore              Explore (analytics + dashboards)");
        println!("  --sunshine             Sunshine (open CRM platform)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zendesk 2024 (OurOS)"); return 0; }
    println!("Zendesk 2024 (OurOS)");
    println!("  Vendor: Zendesk, Inc. (San Francisco, CA — private since June 2022)");
    println!("  Founders: Mikkel Svane (CEO), Alexander Aghassipour, Morten Primdahl");
    println!("          three Danes — built v1 in a Copenhagen loft 2007");
    println!("          'Zendesk' name = Zen (calm) + Desk (helpdesk) — opposite of legacy Remedy/ServiceNow chaos");
    println!("  Founded: 2007 in Copenhagen → SF HQ 2009 → IPO 2014 (NYSE:ZEN $9/share)");
    println!("  Privatization: taken private June 28 2022 by Hellman & Friedman + Permira for $10.2B");
    println!("                hostile activist Jana Partners had pushed for sale earlier in 2022");
    println!("                deal completed at $77.50/share — debated as undervalued");
    println!("  Scale: ~100,000 paying customer accounts across 160+ countries");
    println!("        ~$2B revenue (last public report FY2021), now private");
    println!("        ~6,000 employees globally");
    println!("  Pricing tiers (Support + Suite):");
    println!("    Support Team — $19/agent/mo (basic ticketing)");
    println!("    Support Professional — $55/agent/mo (SLAs, business hours)");
    println!("    Support Enterprise — $115/agent/mo (custom roles, sandbox)");
    println!("    Suite Team — $55/agent/mo (Support + Chat + Talk + Guide bundle)");
    println!("    Suite Professional — $115/agent/mo");
    println!("    Suite Enterprise — $169/agent/mo");
    println!("    Suite Enterprise Plus — $249/agent/mo");
    println!("    annual billing; monthly +25%");
    println!("  Support (core ticketing) features:");
    println!("    - Email/web/social/chat/phone tickets unified in single agent workspace");
    println!("    - Triggers + automations (workflow engine, 'When X then Y')");
    println!("    - SLA policies + business hours + holidays");
    println!("    - Macros (canned response templates)");
    println!("    - CSAT/NPS survey on ticket close");
    println!("    - Light agents (free read-only access for SMEs)");
    println!("    - Side conversations (loop in non-customer reviewer)");
    println!("  Guide (Knowledge Base):");
    println!("    - Branded help center with categorized articles");
    println!("    - Community forum (Q&A) + ideas portal");
    println!("    - Multi-brand (separate help centers per product brand)");
    println!("    - Content cues — AI suggests articles to update based on ticket trends");
    println!("    - Federated search across multiple sources (Confluence, SharePoint, etc.)");
    println!("  Chat (live chat + messaging):");
    println!("    - Pre-chat forms, departments routing");
    println!("    - Triggers ('show chat after 30s on pricing page')");
    println!("    - Bots via Sunshine Conversations (multi-channel messaging)");
    println!("    - WhatsApp Business, Facebook Messenger, Apple Business Chat, Instagram DM unified");
    println!("  Talk (cloud call center):");
    println!("    - IVR + skill-based routing");
    println!("    - Call recording + voicemail-to-ticket");
    println!("    - Local numbers in 70+ countries");
    println!("    - Warm transfer + conference calls");
    println!("  Explore (analytics):");
    println!("    - Pre-built dashboards (CSAT, FCR, agent productivity, SLA attainment)");
    println!("    - Custom report builder");
    println!("    - Schedule + share via email/Slack/PDF");
    println!("  AI features (Advanced AI add-on, $50/agent/mo):");
    println!("    - Intelligent triage (auto-classify ticket intent + sentiment + language)");
    println!("    - Suggested macros to agents");
    println!("    - Article recommendations to end users via bot");
    println!("    - Advanced bot with generative replies (2023+, OpenAI-powered)");
    println!("  Sunshine platform:");
    println!("    - Open CRM data layer — store any object type (orders, accounts, devices)");
    println!("    - Conversations API for omnichannel routing");
    println!("    - Custom objects + events for unified customer view");
    println!("  Acquisitions: Zopim (live chat) 2014 → Chat product");
    println!("              Base CRM 2018 → Zendesk Sell");
    println!("              Smooch 2019 → Sunshine Conversations");
    println!("              Cleverly.ai 2021 → automation/triage AI");
    println!("              Tymeshift (workforce mgmt) 2023");
    println!("  Integrations: 1,500+ marketplace apps");
    println!("              Salesforce, Jira (deepest after Atlassian's own), Slack, Shopify, Stripe");
    println!("              Sunshine SDKs for native iOS/Android/Web embedded messaging");
    println!("  Customers: 100,000+ accounts");
    println!("            Uber, Slack (yes, Slack uses Zendesk), Airbnb, Tesco, Vimeo, Shopify, Etsy");
    println!("            Slack's outage updates flow through Zendesk Support");
    println!("            sweet spot: SMB to enterprise (5 agents to 5,000+)");
    println!("  Critique: post-PE prices have crept up; some customers complain about renewal pressure");
    println!("           UI is comprehensive but can feel cluttered");
    println!("           upmarket move + price hikes pushed SMBs toward Freshdesk + Help Scout");
    println!("           Advanced AI add-on stacks on top of already-pricey Suite tiers");
    println!("  Differentiator: 17 years of category-defining product depth + largest support app marketplace");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zendesk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zendesk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zendesk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zendesk"), "zendesk");
        assert_eq!(basename(r"C:\bin\zendesk.exe"), "zendesk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zendesk.exe"), "zendesk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zendesk(&["--help".to_string()], "zendesk"), 0);
        assert_eq!(run_zendesk(&["-h".to_string()], "zendesk"), 0);
        let _ = run_zendesk(&["--version".to_string()], "zendesk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zendesk(&[], "zendesk");
    }
}
