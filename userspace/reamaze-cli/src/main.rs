#![deny(clippy::all)]
//! reamaze-cli — personality CLI for Re:amaze, the Shopify-centric
//! customer-comms platform.
//!
//! Founded 2012 in San Francisco by Mike Nguyen. Long-running independent
//! customer-support + live-chat + chatbot platform that found durable
//! product-market fit in the Shopify + BigCommerce + WooCommerce
//! ecosystem. Deep order-context integrations make it especially popular
//! with DTC brands + multi-store e-commerce operators that want one inbox
//! across email + chat + Facebook + Instagram + WhatsApp + SMS — with the
//! customer's actual order history surfacing alongside every conversation.
//! Acquired by GoDaddy in mid-2021 and folded under the GoDaddy commerce +
//! customer-engagement suite while retaining its product identity.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Re:amaze e-commerce customer-comms personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Mike Nguyen 2012 SF; GoDaddy acquired 2021");
    println!("    shopify       Deep Shopify + BigCommerce + WooCommerce integration");
    println!("    inbox         Unified inbox across email + chat + social + SMS");
    println!("    chatbot       Cues + chatbot + auto-response engine");
    println!("    fbm           Facebook Messenger + Instagram DM + WhatsApp coverage");
    println!("    classic       Status pages + FAQ + knowledge-base + video calls");
    println!("    pricing       Per-staff-member-per-month tiered pricing");
    println!("    customers     DTC e-commerce brand customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("reamaze-cli 0.1.0 (ecommerce-customer-comms personality build)"); }

fn run_about() {
    println!("Re:amaze (GoDaddy-owned product).");
    println!("  Founded:    2012, San Francisco.");
    println!("  Founder:    Mike Nguyen (CEO pre + post acquisition).");
    println!("  Acquired:   GoDaddy mid-2021; folded into GoDaddy Commerce / customer-engagement.");
    println!("  Position:   Shopify + BigCommerce + WooCommerce-centric customer-support");
    println!("              + live-chat + chatbot platform.");
    println!("  Scale:      tens of thousands of e-commerce brands, primarily SMB DTC.");
    println!("  Retained product identity post-acquisition; not renamed under GoDaddy brand.");
}

fn run_shopify() {
    println!("Deep e-commerce-platform integration.");
    println!("  Shopify, BigCommerce, WooCommerce, Magento, Wix Stores, eBay.");
    println!("  In-conversation order panel: agent sees customer's order history,");
    println!("  current cart, last shipment status, lifetime value, store-credit balance.");
    println!("  Macros / cued responses include order-context merge tags — 'Your order");
    println!("  {{ORDER_ID}} shipped on {{SHIP_DATE}} and tracking is...'.");
    println!("  Agent can trigger refunds, resend tracking, edit subscription orders");
    println!("  without leaving Re:amaze.");
    println!("  Subscription-commerce integrations: Recharge, Bold, Stay.");
}

fn run_inbox() {
    println!("Unified inbox.");
    println!("  Single team inbox across email + live chat + Facebook + Instagram + WhatsApp +");
    println!("  SMS + Twitter + Apple Messages for Business + Slack.");
    println!("  Conversation threading by customer across all channels (replies that arrive");
    println!("  on a different channel still join the same logical thread).");
    println!("  Assignment, internal notes, status, tagging — standard help-desk grammar.");
    println!("  Multi-brand: one Re:amaze account can serve multiple stores with separate");
    println!("  branding + signatures + agents.");
}

fn run_chatbot() {
    println!("Cues + chatbot.");
    println!("  Cues: targeted on-page messages triggered by browsing behavior (cart");
    println!("  abandonment, time on page, exit-intent). Conversational equivalent of");
    println!("  classic e-commerce popups.");
    println!("  Chatbot: rule-based + newer LLM-backed answers from KB + order data.");
    println!("  Common flow: cues prompt → bot answers FAQs + checks order status → handoff");
    println!("  to live agent only for non-deflectable cases.");
    println!("  Reduces ticket volume meaningfully for high-volume stores during peak season.");
}

fn run_fbm() {
    println!("Social + messaging channel coverage.");
    println!("  Facebook Messenger + Facebook page posts + comments.");
    println!("  Instagram DM + Instagram comments under brand posts.");
    println!("  WhatsApp Business Cloud API.");
    println!("  SMS via Twilio + Re:amaze-managed numbers.");
    println!("  Apple Messages for Business + Google Business Messages.");
    println!("  Particularly heavy use of Instagram + WhatsApp by DTC + fashion brands —");
    println!("  Re:amaze threads these into the same inbox as email + chat.");
}

fn run_classic() {
    println!("Classic supporting modules.");
    println!("  Status page: branded uptime / incident page (smaller than StatusPage but");
    println!("  built-in).");
    println!("  FAQ + knowledge base with customer-facing search + branded subdomain.");
    println!("  Video calls: lightweight WebRTC-backed customer-video calls right out of");
    println!("  the agent UI — used for high-ticket DTC service + product walkthroughs.");
    println!("  Push notifications + announcement panels to in-store visitors.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Basic:    ~$29 per staff member per month, basic inbox + chat + macros.");
    println!("  Pro:      ~$49 per staff member per month, adds reports + automation + cues.");
    println!("  Plus:     ~$69 per staff member per month, adds advanced bot + integrations.");
    println!("  Starter Plan: flat ~$59/month for very small teams (3 staff included).");
    println!("  Add-ons: Live + Cues + Chatbot tier-incremental.");
    println!("  Annual contracts discount; volume pricing for multi-brand parent companies.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: DTC + e-commerce brands $5M-$100M annual GMV.");
    println!("  Heavy Shopify Plus + BigCommerce + WooCommerce concentration.");
    println!("  Industries: apparel, beauty, supplements, food + beverage subscription,");
    println!("  pet, home goods, hobbyist products — the typical DTC mix.");
    println!("  Common customer story: started with email + free chat widget, hit limits,");
    println!("  picked Re:amaze over Gorgias/Zendesk for the deeper Shopify hooks + price.");
    println!("  Competes most directly with Gorgias (more Shopify-narrow), Zendesk Suite,");
    println!("  Front, Help Scout in the DTC + e-commerce segment.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "reamaze-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "shopify" => run_shopify(),
        "inbox" => run_inbox(),
        "chatbot" => run_chatbot(),
        "fbm" => run_fbm(),
        "classic" => run_classic(),
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
        run_shopify();
        run_inbox();
        run_chatbot();
        run_fbm();
        run_classic();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("reamaze-cli");
        print_version();
    }
}
