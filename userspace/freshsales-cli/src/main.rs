#![deny(clippy::all)]

//! freshsales-cli — Slate OS Freshsales (Freshworks CRM with built-in Freddy AI)
//!
//! Single personality: `freshsales`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_freshsales(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: freshsales [OPTIONS]");
        println!("Freshsales (Slate OS) — Freshworks AI-powered CRM");
        println!();
        println!("Options:");
        println!("  --free                 Free (3 users, basic features)");
        println!("  --growth               Growth $9/user/mo (billed annually)");
        println!("  --pro                  Pro $39/user/mo");
        println!("  --enterprise           Enterprise $59/user/mo");
        println!("  --freddy               Freddy AI Copilot — contact scoring + next-best-action");
        println!("  --suite                Customer-for-Life Cloud (Freshsales + Freshmarketer bundle)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Freshsales 2024 (Slate OS)"); return 0; }
    println!("Freshsales 2024 (Slate OS)");
    println!("  Vendor: Freshworks Inc. (San Mateo, CA + Chennai, India — NASDAQ:FRSH)");
    println!("  Founder: Girish Mathrubootham (CEO) + Shan Krishnasamy (CTO), 2010");
    println!("          founded as Freshdesk after Girish read Hacker News post about Zendesk price hike");
    println!("          built in Chennai, India — one of largest SaaS IPOs out of India");
    println!("  Freshsales launched: 2016 as 'a CRM that doesn't suck' for inside sales");
    println!("                     part of broader Freshworks suite");
    println!("  Company status: IPO Sep 2021 at $36, popped to $47 day-1");
    println!("                 currently around $13-18 (down significantly from IPO highs)");
    println!("                 ~$700M+ revenue, ~7,400 employees globally");
    println!("                 67,000+ customers across all Freshworks products");
    println!("  Pricing: Free tier (3 users, basic contacts + deals — best free CRM tier per G2)");
    println!("          Growth $9/user/mo (CRM with built-in chat, telephone, email)");
    println!("          Pro $39/user/mo (sales sequences, territories, custom reports)");
    println!("          Enterprise $59/user/mo (audit logs, custom roles, dedicated success mgr)");
    println!("          all annual; monthly +20-25%");
    println!("  Freddy AI (Freshworks' AI brand) features:");
    println!("    - Contact + deal scoring (predict close likelihood)");
    println!("    - Next-best-action recommendations to reps");
    println!("    - Auto-enrichment of contact records from web signals");
    println!("    - Conversational AI for chatbot interactions");
    println!("    - Forecast intelligence + anomaly detection");
    println!("    - AI-assisted email drafting (Freddy Copilot, 2023+)");
    println!("  CRM features:");
    println!("    - Visual pipeline + Kanban + list views");
    println!("    - Built-in phone (Freshcaller integration — Freshworks owns the dialer)");
    println!("    - Built-in chat (Freshchat integration)");
    println!("    - Email integration (Gmail, Outlook 2-way)");
    println!("    - Sales sequences with multi-channel steps");
    println!("    - Territory management + lead routing");
    println!("    - Custom modules + custom objects (Pro+)");
    println!("    - Workflow automation (no-code builder)");
    println!("  Customer-for-Life Cloud bundle:");
    println!("    - Freshsales + Freshmarketer (marketing automation)");
    println!("    - Unified customer record across sales + marketing");
    println!("    - $29/user/mo Growth tier for bundle");
    println!("  Integrations: 100+ Freshworks Marketplace apps");
    println!("              Slack, Zoom, Mailchimp, DocuSign, QuickBooks, Xero, Microsoft Teams");
    println!("              native: Freshdesk (helpdesk), Freshchat, Freshcaller — same vendor stack");
    println!("              public REST API + webhooks + Zapier");
    println!("  Customers: SMB + mid-market, strong in APAC + Europe");
    println!("            Bridgestone, Klarna (early), PharmEasy, Decathlon, Pearson");
    println!("            ~12,000+ paying Freshsales customers (subset of Freshworks' 67K)");
    println!("  Critique: AI features feel partially shipped — Freddy Copilot lags ChatGPT-integrated rivals");
    println!("           upsell pressure can feel heavy from sales team");
    println!("           reporting customization less powerful than Salesforce");
    println!("           Freshworks split-personality — Freshdesk strong, Freshsales still proving itself");
    println!("  Differentiator: best free tier in CRM + native phone/chat/email under one vendor + Freddy AI suite");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "freshsales".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_freshsales(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_freshsales};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freshsales"), "freshsales");
        assert_eq!(basename(r"C:\bin\freshsales.exe"), "freshsales.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freshsales.exe"), "freshsales");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_freshsales(&["--help".to_string()], "freshsales"), 0);
        assert_eq!(run_freshsales(&["-h".to_string()], "freshsales"), 0);
        let _ = run_freshsales(&["--version".to_string()], "freshsales");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_freshsales(&[], "freshsales");
    }
}
