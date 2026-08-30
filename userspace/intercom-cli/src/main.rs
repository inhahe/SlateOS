#![deny(clippy::all)]

//! intercom-cli — Slate OS Intercom (conversational support, AI-first under Fin)
//!
//! Single personality: `intercom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_intercom(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: intercom [OPTIONS]");
        println!("Intercom (Slate OS) — AI-first customer service platform");
        println!();
        println!("Options:");
        println!("  --fin                  Fin AI agent ($0.99/resolution since 2023)");
        println!("  --messenger            In-app/web Messenger widget");
        println!("  --inbox                Team inbox (omnichannel)");
        println!("  --help-center          Help Center (KB + articles)");
        println!("  --workflows            Workflows automation builder");
        println!("  --essential            Essential $39/seat/mo");
        println!("  --advanced             Advanced $99/seat/mo");
        println!("  --expert               Expert $139/seat/mo");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Intercom 2024 (Slate OS)"); return 0; }
    println!("Intercom 2024 (Slate OS)");
    println!("  Vendor: Intercom, Inc. (San Francisco, CA — private)");
    println!("  Founders: Eoghan McCabe, Des Traynor, Ciaran Lee, David Barrett (2011, Dublin)");
    println!("          all four Irish, met at Trinity College Dublin");
    println!("          McCabe: famously brash CEO, returned as CEO 2022 after Karen Peacock era");
    println!("          Traynor: legendary product thought leader, 'Inside Intercom' podcast/blog");
    println!("  Founded: 2011 in Dublin → SF HQ 2012");
    println!("  Funding: $240M+ raised, last round 2018 Series D at ~$1.275B valuation");
    println!("          Index Ventures, Bessemer, GV, ICONIQ, Kleiner Perkins");
    println!("          revenue rumored ~$300M ARR (private)");
    println!("  2022-2024 AI pivot: McCabe returned, fired ~25% of workforce, full company refocused on AI");
    println!("                     'Fin' AI agent launched March 2023 (GPT-4-powered)");
    println!("                     repositioned from 'conversational marketing' to 'AI-first customer service'");
    println!("  Fin AI agent (the bet that defines them now):");
    println!("    - Powered by OpenAI GPT-4 + Anthropic Claude (multi-model)");
    println!("    - Answers from your help center + macros + private docs");
    println!("    - Resolves up to 50% of incoming tickets autonomously");
    println!("    - Pricing: $0.99 per AI resolution (success-based, novel in industry)");
    println!("    - Hands off cleanly to human agent with full context when stuck");
    println!("    - 'Fin Voice' (phone), 'Fin Tasks' (perform actions like refunds)");
    println!("  Pricing: Essential $39/seat/mo (basic Inbox + Messenger)");
    println!("          Advanced $99/seat/mo (workflows, multi-channel)");
    println!("          Expert $139/seat/mo (SSO, advanced security, multi-brand)");
    println!("          Fin AI add-on: $0.99 per autonomous resolution");
    println!("          'Proactive Support' add-on: from $99/mo");
    println!("  Messenger features:");
    println!("    - Customizable in-app widget (web + iOS + Android SDKs)");
    println!("    - Identity verification (HMAC) for logged-in users");
    println!("    - Real-time targeting (show different messages based on attributes/events)");
    println!("    - Outbound product tours, banners, push notifications, surveys");
    println!("    - Tooltips + carousels for in-app onboarding");
    println!("  Inbox features:");
    println!("    - Unified team inbox: chat, email, WhatsApp, Instagram, SMS, Twitter DM");
    println!("    - Conversation routing by team, skill, priority, language");
    println!("    - SLAs + business hours");
    println!("    - Side conversations + macros + notes");
    println!("    - AI Copilot — drafts replies, summarizes long threads (Expert tier)");
    println!("  Workflows:");
    println!("    - Visual flow builder for bots + automations");
    println!("    - Triggers: event-based, attribute-based, time-based");
    println!("    - 'Custom Objects' for routing complex data");
    println!("  Help Center:");
    println!("    - Public KB + branded help site");
    println!("    - Multi-language, multi-collection");
    println!("    - AI-generated article suggestions from past tickets");
    println!("    - Switch model — auto-rewrites Help Center in different reader 'voices'");
    println!("  Integrations: 350+ marketplace apps");
    println!("              Slack, Salesforce, Hubspot, Jira, GitHub, Stripe, Shopify");
    println!("              data warehouse exports to Snowflake/BigQuery/Redshift");
    println!("              REST API + webhooks + Canvas Kit for custom embedded apps");
    println!("  Customers: 25,000+ businesses");
    println!("            Atlassian, Amazon, Microsoft, Lyft, Shopify, Coda, Linktree, Notion (early)");
    println!("            sweet spot: SaaS startups + e-commerce, 10-2,000 employees");
    println!("  Critique: pricing got punishingly expensive going up tiers");
    println!("           lots of customer goodwill burned during the 2022-2023 layoffs + product refocus");
    println!("           Fin AI great but $0.99/resolution adds up at scale (vs flat-fee competitors)");
    println!("           less ticketing-power-user friendly than Zendesk for enterprise support orgs");
    println!("  Differentiator: most aggressive bet on AI replacing tier-1 support — Fin's success-based pricing");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "intercom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_intercom(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_intercom};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/intercom"), "intercom");
        assert_eq!(basename(r"C:\bin\intercom.exe"), "intercom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("intercom.exe"), "intercom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_intercom(&["--help".to_string()], "intercom"), 0);
        assert_eq!(run_intercom(&["-h".to_string()], "intercom"), 0);
        let _ = run_intercom(&["--version".to_string()], "intercom");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_intercom(&[], "intercom");
    }
}
