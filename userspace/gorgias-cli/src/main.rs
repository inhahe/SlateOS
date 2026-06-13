#![deny(clippy::all)]

//! gorgias-cli — SlateOS Gorgias (e-commerce-native helpdesk built for Shopify)
//!
//! Single personality: `gorgias`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gorgias(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gorgias [OPTIONS]");
        println!("Gorgias (SlateOS) — e-commerce helpdesk for Shopify/Magento/BigCommerce");
        println!();
        println!("Options:");
        println!("  --starter              Starter $10/mo (50 tickets, 3 users)");
        println!("  --basic                Basic $60/mo (300 tickets)");
        println!("  --pro                  Pro $360/mo (2,000 tickets)");
        println!("  --advanced             Advanced $900/mo (5,000 tickets)");
        println!("  --enterprise           Enterprise (custom volume)");
        println!("  --automate             Automate add-on (auto-resolve %s pricing)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gorgias 2024 (SlateOS)"); return 0; }
    println!("Gorgias 2024 (SlateOS)");
    println!("  Vendor: Gorgias, Inc. (SF + Paris — fully remote-friendly)");
    println!("  Founders: Romain Lapeyre (CEO) + Alex Plugaru + Aasif Osmany, 2015");
    println!("          met at YC Demo Day after independent prior attempts at email tools");
    println!("          name 'Gorgias' = Greek philosopher / rhetorician (ironic for support tool)");
    println!("  Founded: 2015 — YC W16 batch");
    println!("          original product was Gmail keyboard shortcuts → pivoted to helpdesk 2016");
    println!("  Funding: Series C 2022 $29M led by SaaStr Fund + Sapphire Ventures");
    println!("          Total ~$60M raised, ~$700M valuation 2022");
    println!("          ~$50M+ ARR (private)");
    println!("  Strategy: dominate the Shopify ecosystem instead of being yet-another-generalist-helpdesk");
    println!("           'helpdesk that knows your customer's order history natively'");
    println!("  Pricing (volume-based, unusual in helpdesk):");
    println!("    Starter $10/mo — 50 tickets/mo, 3 users (built for tiny stores)");
    println!("    Basic $60/mo — 300 tickets/mo");
    println!("    Pro $360/mo — 2,000 tickets/mo (most popular)");
    println!("    Advanced $900/mo — 5,000 tickets/mo");
    println!("    overages: ~$25-40 per 100 extra tickets depending on tier");
    println!("    Automate add-on: % uplift based on tickets auto-resolved by AI/macros");
    println!("  Shopify integration (the killer feature):");
    println!("    - One-click connect; agent sidebar shows full Shopify order history");
    println!("    - Issue refunds + cancellations + edit orders WITHOUT leaving Gorgias");
    println!("    - View tracking + fulfillment status inline");
    println!("    - Customer LTV displayed on every ticket");
    println!("    - Multi-store support (one Gorgias for many Shopify stores)");
    println!("    - Same for Magento, BigCommerce (deep — not just basic webhooks)");
    println!("  Channels:");
    println!("    - Email, web chat, live chat, SMS, WhatsApp, FB Messenger, Instagram DM");
    println!("    - Voice via Aircall / RingCentral integrations");
    println!("    - Reviews (Loox, Yotpo, Stamped) — escalate bad reviews into tickets");
    println!("    - Contact form widget for the storefront");
    println!("  Macros + Rules:");
    println!("    - Rule engine: 'If email contains \"refund\" AND customer LTV > $500 → assign to senior agent'");
    println!("    - Macros can include dynamic Shopify data (refund amount, tracking link)");
    println!("    - One-click 'refund + reply' (executes Shopify refund and sends customer email together)");
    println!("    - Snooze + assignment + tagging automations");
    println!("  Gorgias Automate (AI add-on):");
    println!("    - Auto-respond to top intents (where is my order, refund, etc.) with personalized data");
    println!("    - Pulls customer's actual tracking number + refund amount inline");
    println!("    - Pricing: based on % tickets auto-resolved (usage-based)");
    println!("    - Powered by OpenAI + Gorgias's own intent classifier");
    println!("  Convert (revenue-generating support):");
    println!("    - Track revenue per agent — tickets that lead to sales");
    println!("    - Pre-sales chat support (turn chat into checkout)");
    println!("    - Campaigns: outbound triggered messages (cart abandonment via support tool)");
    println!("  Integrations: 100+ apps focused on e-commerce stack");
    println!("              Klaviyo (deep), Recharge, Loop Returns, Loox, Yotpo, Postscript, Attentive");
    println!("              Slack, Aircall, Klaus (QA), Stripe");
    println!("  Customers: 14,000+ e-commerce brands");
    println!("            Steve Madden, Olipop, Marine Layer, Princess Polly, Allbirds (parts), Decathlon (online)");
    println!("            sweet spot: $1M-$500M GMV DTC brands");
    println!("            heavy DTC, beauty, apparel, food/beverage, supplements");
    println!("  Critique: ticket-based pricing surprises growing stores — easy to blow through tier mid-month");
    println!("           less useful outside e-commerce (Shopify-first design assumptions don't fit B2B SaaS)");
    println!("           reporting tools simpler than Zendesk Explore");
    println!("           Automate AI sometimes too eager — needs careful guardrails");
    println!("  Differentiator: deepest Shopify+Magento integration of any helpdesk — turns support into revenue");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gorgias".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gorgias(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gorgias};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gorgias"), "gorgias");
        assert_eq!(basename(r"C:\bin\gorgias.exe"), "gorgias.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gorgias.exe"), "gorgias");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gorgias(&["--help".to_string()], "gorgias"), 0);
        assert_eq!(run_gorgias(&["-h".to_string()], "gorgias"), 0);
        let _ = run_gorgias(&["--version".to_string()], "gorgias");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gorgias(&[], "gorgias");
    }
}
