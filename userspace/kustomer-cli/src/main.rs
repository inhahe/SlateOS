#![deny(clippy::all)]

//! kustomer-cli — OurOS Kustomer (customer-centric CRM/support, Meta acquired 2022)
//!
//! Single personality: `kustomer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kustomer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kustomer [OPTIONS]");
        println!("Kustomer (OurOS) — customer-centric (not ticket-centric) support platform");
        println!();
        println!("Options:");
        println!("  --enterprise           Enterprise tier (custom pricing, typically $89-$139/seat/mo)");
        println!("  --ultimate             Ultimate tier (custom)");
        println!("  --kiq                  KIQ — Kustomer IQ AI suite");
        println!("  --conversation         Conversation Builder (multi-channel routing)");
        println!("  --self-service         Self-Service portal + KB");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kustomer 2024 (OurOS)"); return 0; }
    println!("Kustomer 2024 (OurOS)");
    println!("  Vendor: Kustomer, Inc. (NYC, NY — owned by Meta then divested back to founders 2023)");
    println!("  Founders: Brad Birnbaum (CEO) + Jeremy Suriel (CTO), 2015");
    println!("          previously co-founded eAssist (helpdesk) → sold to Kana 2002");
    println!("          and Assistly → sold to Salesforce 2011 (became Desk.com)");
    println!("          Kustomer is their third helpdesk startup — clearly know the space cold");
    println!("  Founded: 2015 in NYC");
    println!("  Big moments:");
    println!("    - Series E ~$60M Jan 2020 ($600M valuation)");
    println!("    - Meta (then Facebook) acquired Nov 2020 for $1B announced (delayed by EU regulators to Feb 2022)");
    println!("    - Meta divested back to original investors (incl. founders) Jul 2023");
    println!("      reasons: Meta deprioritized B2B, EU competition pressure post-WhatsApp Business focus");
    println!("    - now independent again, Brad Birnbaum CEO");
    println!("  Pricing: Enterprise from ~$89/seat/mo (typical 25-seat min)");
    println!("          Ultimate (custom — large enterprises)");
    println!("          all annual contracts");
    println!("  Defining concept — Customer Timeline:");
    println!("    - NOT 'ticket-based' like Zendesk");
    println!("    - Every customer has ONE unified Timeline showing ALL interactions across all channels");
    println!("    - Order history, conversations, returns, page views, app sessions — all in chronological order");
    println!("    - Agents see the whole customer, not just the current ticket");
    println!("  Channels supported:");
    println!("    - Email, chat (web + in-app), SMS, WhatsApp, Facebook Messenger, Instagram DM");
    println!("    - Voice (via partner integrations)");
    println!("    - Apple Business Chat, Google Business Messages, Twitter DM");
    println!("    - SDK for embedded mobile chat (iOS + Android)");
    println!("  KIQ (Kustomer IQ) AI:");
    println!("    - Conversation Classifier (auto-tag intent/sentiment/language)");
    println!("    - Suggested Reply (next-action recommendations to agents)");
    println!("    - Conversation Summary (post-chat summary)");
    println!("    - Self-service Chatbot (Conversational Assistant)");
    println!("    - Smart Suggest (KB article suggestions inline)");
    println!("    - Sentiment Detection (real-time, escalates angry customers)");
    println!("  Conversation Builder:");
    println!("    - Visual flow editor for bots + automations");
    println!("    - Conditional logic + branching");
    println!("    - Integration with custom APIs / CRM lookups inside flow");
    println!("    - Multi-channel — same bot logic works on web/SMS/WhatsApp");
    println!("  Custom Objects + Searches:");
    println!("    - Define any data model (orders, subscriptions, devices, accounts, families)");
    println!("    - Pull data from external systems and display on customer timeline");
    println!("    - 'Searches' = saved smart filters that act like dynamic queues");
    println!("  Integrations: 60+ pre-built");
    println!("              Shopify, Magento, BigCommerce (deep e-commerce focus)");
    println!("              Slack, Salesforce, Stripe, Recharge (subscriptions)");
    println!("              Twilio, Five9, Talkdesk for voice");
    println!("              full GraphQL API + webhooks + Klasses (custom event ingestion)");
    println!("  Customers: ~500+ enterprise customers, heavy e-commerce / consumer brands");
    println!("            Glovo, Sweetgreen, Ring, ThirdLove, Rent the Runway, Glossier, Away, Bombas");
    println!("            sweet spot: 50-500 agents, B2C high-volume + relationship-driven");
    println!("  Critique: enterprise-priced — not for SMB");
    println!("           Meta era distracted product roadmap; recovery still in progress under independence");
    println!("           docs harder to navigate vs Zendesk's mature ecosystem");
    println!("           dependence on Apple/Meta channel APIs creates platform risk");
    println!("  Differentiator: most genuinely customer-centric (not ticket-centric) support data model — Timeline is the product");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kustomer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kustomer(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kustomer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kustomer"), "kustomer");
        assert_eq!(basename(r"C:\bin\kustomer.exe"), "kustomer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kustomer.exe"), "kustomer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kustomer(&["--help".to_string()], "kustomer"), 0);
        assert_eq!(run_kustomer(&["-h".to_string()], "kustomer"), 0);
        assert_eq!(run_kustomer(&["--version".to_string()], "kustomer"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kustomer(&[], "kustomer"), 0);
    }
}
