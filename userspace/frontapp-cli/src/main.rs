#![deny(clippy::all)]

//! frontapp-cli — Slate OS Front (shared inbox / customer ops hybrid email-and-helpdesk)
//!
//! Single personality: `front`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_front(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: front [OPTIONS]");
        println!("Front (Slate OS) — shared inbox + customer operations platform");
        println!();
        println!("Options:");
        println!("  --starter              Starter $19/user/mo (3-10 seats)");
        println!("  --growth               Growth $59/user/mo");
        println!("  --scale                Scale $99/user/mo");
        println!("  --premier              Premier $229/user/mo");
        println!("  --chatbot              AI Chatbot add-on");
        println!("  --analytics            Premium Analytics add-on");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Front 2024 (Slate OS)"); return 0; }
    println!("Front 2024 (Slate OS)");
    println!("  Vendor: FrontApp, Inc. dba Front (San Francisco, CA — private)");
    println!("  Founders: Mathilde Collin (CEO) + Laurent Perrin (CTO), 2013");
    println!("          two French co-founders, met at Polytechnique → moved to SF for YC W14");
    println!("          Mathilde: one of the most visible women CEOs in SaaS — Forbes 30u30");
    println!("  Founded: 2013 in Paris → YC Winter 2014 → SF HQ");
    println!("  Funding: Series D Jan 2022 $65M at $1.7B valuation — Sequoia, Insight, Aspect");
    println!("          Total ~$208M raised");
    println!("          ~$100M+ ARR (rumored, private)");
    println!("  Category-defining concept — Shared Inbox (not 'ticketing'):");
    println!("    - Email/SMS/WhatsApp/social DMs/voicemails arrive in a SHARED inbox");
    println!("    - Looks/feels like Apple Mail or Gmail — not Zendesk");
    println!("    - Multiple agents can collaborate on a SINGLE message thread (assign, comment, draft together)");
    println!("    - Replies go from real shared address (support@, sales@, hello@) — not 'ticket #1234'");
    println!("    - Best for teams that handle conversational, relationship-heavy work (logistics, B2B sales, finance ops, real estate)");
    println!("  Pricing: Starter $19/user/mo (3 user min, basic shared inbox)");
    println!("          Growth $59/user/mo (rules, knowledge base, advanced API)");
    println!("          Scale $99/user/mo (analytics, custom roles, SSO, sandbox)");
    println!("          Premier $229/user/mo (priority support, AI suite, advanced security)");
    println!("          50 user minimum on Premier");
    println!("  Channels:");
    println!("    - Email (any IMAP, Gmail, Outlook 365)");
    println!("    - SMS, WhatsApp, Facebook Messenger, Instagram DM, X (Twitter) DM");
    println!("    - Voice — via Aircall, RingCentral, Dialpad integrations");
    println!("    - Live chat widget");
    println!("    - Internal team channels (replace some Slack DM use)");
    println!("  Collaboration features:");
    println!("    - @-mention teammates inside email threads (private from customer)");
    println!("    - Shared drafts — collaborate on a single reply before sending");
    println!("    - Assignments + reassignments");
    println!("    - 'Internal Conversations' — like Slack DMs but tied to a customer/ticket");
    println!("    - Comments live next to the email; no context-switching");
    println!("  Rules engine:");
    println!("    - 'If sender is in 'VIP' tag → assign to Sarah within 15 min'");
    println!("    - Time-based (out-of-office routing, SLAs)");
    println!("    - Cross-channel (rule applies to SMS + email together)");
    println!("    - Multi-step workflows");
    println!("  Front AI features (2023+):");
    println!("    - AI Compose — improve, summarize, change tone of agent draft");
    println!("    - AI Summary — collapse long threads to bullets");
    println!("    - AI Chatbot — KB-backed customer-facing chat with handoff");
    println!("    - AI Topic Tagging — classify and route by intent");
    println!("  Analytics:");
    println!("    - Conversation volume + handle time + first-response time");
    println!("    - Team performance + individual rep dashboards");
    println!("    - SLA tracking + workload balance views");
    println!("    - Premium Analytics — explore by tag/channel/sender domain/custom field");
    println!("  Integrations: 80+ native + Zapier");
    println!("              Salesforce, HubSpot, Slack, Jira, Asana, Linear, Notion");
    println!("              Shopify, Stripe, QuickBooks, Xero");
    println!("              REST API + webhooks + Plugins SDK for custom in-Front apps");
    println!("  Customers: 8,000+ companies — heavy on logistics, fintech, real estate, B2B services");
    println!("            ClickUp, MongoDB, Lyft (parts), Stripe (parts), Shopify (parts), Cushman & Wakefield, Flexport");
    println!("            sweet spot: 10-500 person teams that hate ticket interfaces");
    println!("  Critique: not a replacement for a traditional ticketing helpdesk at high volume B2C");
    println!("           pricing on higher tiers approaches Zendesk Suite Enterprise");
    println!("           reporting historically weaker than Zendesk Explore (improving)");
    println!("           hybrid email/CRM positioning sometimes confuses prospects");
    println!("  Differentiator: only platform where shared inbox + ticketing + light CRM feel like one product");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "front".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_front(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_front};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/frontapp"), "frontapp");
        assert_eq!(basename(r"C:\bin\frontapp.exe"), "frontapp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("frontapp.exe"), "frontapp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_front(&["--help".to_string()], "frontapp"), 0);
        assert_eq!(run_front(&["-h".to_string()], "frontapp"), 0);
        let _ = run_front(&["--version".to_string()], "frontapp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_front(&[], "frontapp");
    }
}
