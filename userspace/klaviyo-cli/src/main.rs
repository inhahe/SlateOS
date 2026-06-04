#![deny(clippy::all)]

//! klaviyo-cli — OurOS Klaviyo (e-commerce email/SMS marketing, NYSE:KVYO)
//!
//! Single personality: `klaviyo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_klaviyo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: klaviyo [OPTIONS]");
        println!("Klaviyo (OurOS) — owned marketing for e-commerce (email + SMS + reviews + CDP)");
        println!();
        println!("Options:");
        println!("  --free                 Free up to 250 profiles + 500 emails/mo");
        println!("  --email                Email scaling tiers (by profile count)");
        println!("  --email-sms            Email + SMS combined tiers");
        println!("  --reviews              Klaviyo Reviews (UGC, ex-OutSmart acquisition)");
        println!("  --cdp                  Klaviyo CDP (data layer + audiences)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Klaviyo 2024 (OurOS)"); return 0; }
    println!("Klaviyo 2024 (OurOS)");
    println!("  Vendor: Klaviyo, Inc. (Boston, MA — NYSE:KVYO)");
    println!("  Founders: Andrew Bialecki (CEO), Ed Hallen (Chief Strategy Officer), 2012");
    println!("          Bialecki: ex-Applied Predictive Technologies (Mastercard subsidiary)");
    println!("          Hallen: ex-McKinsey + 4 years at Applied Predictive");
    println!("  Founded: 2012 in Boston — bootstrapped to $1M+ ARR before any funding");
    println!("          built profitable + capital-efficient: only ~$22M raised pre-IPO");
    println!("          one of best-run Boston SaaS stories (alongside HubSpot)");
    println!("  IPO: Sep 2023 NYSE:KVYO at $30 — popped to $36 day-1");
    println!("       now ~$30-40 (strongest tech IPO of 2023, breaking IPO drought)");
    println!("       FY2024 revenue ~$770M (+34% YoY), well-profitable");
    println!("       ~143,000 paying customer accounts");
    println!("       Shopify owns 11% stake from 2022 strategic investment ($100M)");
    println!("  Strategic position: 'the email + SMS + CDP for Shopify-era e-commerce':");
    println!("                    won by being deeply tied to Shopify ecosystem early");
    println!("                    primary competitor: Mailchimp (smaller deals), Attentive (SMS-only), Omnisend");
    println!("                    upmarket competitor: Iterable, Braze (Klaviyo less polished but cheaper)");
    println!("  Pricing (transparent — calculated by profile count + send volume):");
    println!("    Free — 250 profiles, 500 emails/mo (real free tier, very generous)");
    println!("    Email tiers — profile-count-based pricing:");
    println!("      500 profiles, 5K emails/mo — $20/mo");
    println!("      1.5K profiles, 15K emails/mo — $45/mo");
    println!("      5K profiles, 50K emails/mo — $100/mo");
    println!("      25K profiles, 250K emails/mo — $385/mo");
    println!("      100K profiles, 1M emails/mo — $1,380/mo");
    println!("      scales smoothly to seven-figure profile counts");
    println!("    SMS tiers — separate, also profile-based + message volume");
    println!("    Email + SMS bundled discount");
    println!("  E-commerce-native features (the killer differentiator):");
    println!("    - Shopify integration: deepest of any ESP, sync orders/carts/products/customers in seconds");
    println!("    - BigCommerce, Magento, WooCommerce equally deep");
    println!("    - Pre-built flow library: abandoned cart, post-purchase, win-back, browse abandonment, replenishment");
    println!("    - Product feed dynamic blocks: send price + image + 'buy now' link with real-time stock check");
    println!("    - Revenue attribution: see exact $ each campaign + flow generated, with multi-touch credit");
    println!("    - Profitability per email — revenue minus delivery cost");
    println!("    - Predictive analytics: CLV, churn risk, expected next order date per profile");
    println!("    - Built-in audience splitting: predicted high-value vs low-value customers");
    println!("  Klaviyo Flows (the workflow engine):");
    println!("    - Visual flow builder with drag-and-drop (vastly simpler than Marketo)");
    println!("    - Trigger flows on Shopify events (placed order, cart abandoned, viewed product, etc.)");
    println!("    - Welcome series, browse abandonment, abandoned cart, post-purchase, replenishment");
    println!("    - Win-back, sunset, VIP segmentation flows pre-built");
    println!("    - Conditional logic + delays + A/B testing inline");
    println!("    - Multi-message flows mixing email + SMS based on user opt-ins");
    println!("  Klaviyo Reviews (ex-OutSmart acquisition Jul 2023):");
    println!("    - Product reviews + ratings collected post-purchase via flow trigger");
    println!("    - Display on store via Shopify theme integration");
    println!("    - UGC photos + videos");
    println!("    - Compete with Yotpo, Loox, Stamped");
    println!("  Klaviyo CDP (Apr 2024 launched):");
    println!("    - Reverse ETL from data warehouse (Snowflake/BigQuery/Redshift)");
    println!("    - Sync any customer attribute or computed trait into Klaviyo profiles");
    println!("    - Audience Sync to Meta + Google ad platforms");
    println!("    - Direct competitor to Hightouch/Census for e-commerce use cases");
    println!("  AI features (Klaviyo AI):");
    println!("    - Subject line generator (Klaviyo's most-used AI feature)");
    println!("    - Send Time Optimization (per-recipient best send time)");
    println!("    - Form responses (chat-style customer support inside email widgets)");
    println!("    - Predictive Purchase Date for replenishment flows");
    println!("    - Generative image creation (gen AI for product imagery)");
    println!("  Integrations: 350+ marketplace");
    println!("              Shopify (deepest), Shopify Plus, Magento, BigCommerce, WooCommerce, Salesforce Commerce Cloud");
    println!("              Recharge (subscriptions), Loop Returns, Gorgias (support), Smile.io (loyalty)");
    println!("              Segment as upstream + downstream");
    println!("              REST API + webhooks + Klaviyo Object Library + JS Onsite SDK");
    println!("  Customers: 143,000+ paying customers");
    println!("            Living Proof, Liquid Death, Nomad, Brooklinen, Steve Madden");
    println!("            Allbirds, Olipop, Bombas, Marine Layer, Andie Swim, Bonobos");
    println!("            sweet spot: $1M-$500M GMV DTC + e-commerce brands");
    println!("            extremely strong on Shopify (where Mailchimp was historically dominant)");
    println!("  Critique: not designed for B2B — flow library/triggers all assume e-commerce data model");
    println!("           SMS pricing surprising as profile counts grow + international message rates expensive");
    println!("           reporting can feel surface-level vs Braze/Iterable for very large brands");
    println!("           UI dated in places (improving with redesigns)");
    println!("           CDP layer launched late vs Iterable/Braze investments — playing catch-up");
    println!("  Differentiator: deepest e-commerce data model + Shopify integration + transparent pricing — built-for-DTC marketing automation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "klaviyo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_klaviyo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_klaviyo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/klaviyo"), "klaviyo");
        assert_eq!(basename(r"C:\bin\klaviyo.exe"), "klaviyo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("klaviyo.exe"), "klaviyo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_klaviyo(&["--help".to_string()], "klaviyo"), 0);
        assert_eq!(run_klaviyo(&["-h".to_string()], "klaviyo"), 0);
        let _ = run_klaviyo(&["--version".to_string()], "klaviyo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_klaviyo(&[], "klaviyo");
    }
}
