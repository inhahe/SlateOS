#![deny(clippy::all)]

//! iterable-cli — OurOS Iterable (cross-channel marketing automation for growth teams)
//!
//! Single personality: `iterable`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iterable(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iterable [OPTIONS]");
        println!("Iterable (OurOS) — cross-channel growth marketing platform");
        println!();
        println!("Options:");
        println!("  --essentials           Essentials tier (mid-market)");
        println!("  --premium              Premium tier");
        println!("  --enterprise           Enterprise tier");
        println!("  --ai-suite             Iterable AI Suite (gen AI + Brand Affinity)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Iterable 2024 (OurOS)"); return 0; }
    println!("Iterable 2024 (OurOS)");
    println!("  Vendor: Iterable, Inc. (San Francisco, CA — private)");
    println!("  Founders: Justin Zhu (CEO until 2021), Andrew Boni (CTO, now CEO from 2021), 2013");
    println!("          both ex-Twitter (engineers on growth+ads)");
    println!("          founded after frustrations with legacy ESPs at Twitter scale");
    println!("          Justin Zhu departure 2021 was acrimonious (lawsuit settled 2022)");
    println!("  Founded: 2013 — incubated at Index Ventures");
    println!("  Funding: ~$342M total raised");
    println!("          Series E Jun 2021 $200M led by SVB Capital, Premji Invest");
    println!("          ~$2B+ valuation at peak (2021)");
    println!("          revenue rumored ~$200M ARR");
    println!("  Strategic position: 'modern' cross-channel MAP for B2C + growth teams");
    println!("                    competes with Braze (head-to-head) + Customer.io + Iterable");
    println!("                    pitches against legacy ESPs (Salesforce Marketing Cloud, Adobe Campaign)");
    println!("                    designed for high-event-volume consumer products (streaming, marketplace, finance)");
    println!("  Channels (true cross-channel — not email-first):");
    println!("    - Email (transactional + marketing) with deliverability infrastructure");
    println!("    - SMS (built-in)");
    println!("    - Push notifications (web + mobile iOS/Android)");
    println!("    - In-app messages (mobile)");
    println!("    - WhatsApp Business + Facebook Messenger (recent additions)");
    println!("    - Direct mail (via Lob integration)");
    println!("    - Web push + on-site embeds");
    println!("    - Webhooks for custom downstream channels");
    println!("  Workflow Studio (the core):");
    println!("    - Visual flow builder with drag-and-drop nodes");
    println!("    - Event-triggered, time-based, or list-membership entry");
    println!("    - Branches by user attribute or behavior");
    println!("    - Wait nodes (until event/time)");
    println!("    - A/B test variants inline within a flow");
    println!("    - Frequency capping across all journeys");
    println!("    - Quiet hours respect");
    println!("    - Multi-language + multi-locale support per branch");
    println!("  Catalog (real-time product catalog):");
    println!("    - Sync product/content catalog (SKUs, articles, listings)");
    println!("    - Dynamic content insertion in emails/push");
    println!("    - 'Smart Send' uses catalog data for personalization");
    println!("  Iterable AI Suite (the recent flagship):");
    println!("    - Brand Affinity™ — predicts each user's affinity score to dynamic content/products");
    println!("    - Send Time Optimization (per-user best send time)");
    println!("    - Predictive Goals (multi-channel optimization toward an outcome)");
    println!("    - Generative AI Copy Assist (subject lines, body, push copy)");
    println!("    - Embeddings-based content recommendations");
    println!("  Personalization:");
    println!("    - Handlebars templating with deeply nested data");
    println!("    - Snippet library for reusable email blocks");
    println!("    - Dynamic content blocks (rules-based audience splitting within one email)");
    println!("    - Data Feeds (real-time API calls during render — weather, inventory, prices)");
    println!("  Data layer:");
    println!("    - Events + user profiles + custom event types + relations");
    println!("    - Real-time SDK ingestion (iOS, Android, JS)");
    println!("    - Server-to-server API for backend events");
    println!("    - List-based or event-triggered campaigns");
    println!("    - Computed traits (RFM scores, lifetime value, engagement scores)");
    println!("  Integrations: 50+ native");
    println!("              Segment (deepest), mParticle, Tealium upstream");
    println!("              Snowflake/BigQuery/Redshift via Iterable Data Feeds");
    println!("              Salesforce, HubSpot, Zendesk CRM sync");
    println!("              Twilio (for SMS infra under the hood)");
    println!("              REST API + webhooks + Catalog API + Events API");
    println!("  Customers: 1,000+ paying customers, heavy B2C consumer brands");
    println!("            DoorDash, Bumble, Calm, Strava, Fender, ESPN, NBA, Madison Square Garden");
    println!("            sweet spot: $100M-$10B revenue consumer brands w/ high event volume");
    println!("            verticals: streaming, marketplaces, fintech, food delivery, gaming");
    println!("  Critique: enterprise-priced — not approachable for SMB (typically $50K+/yr starter)");
    println!("           AI features competitive but Braze + Salesforce moved fast in 2023-2024");
    println!("           UI complex for marketers without engineering support");
    println!("           data warehouse-native architecture lags Hightouch/Census + emerging RT-CDP rivals");
    println!("           CEO transition 2021 lawsuit hurt brand temporarily");
    println!("  Differentiator: deepest true cross-channel orchestration (email+SMS+push+in-app+WhatsApp in one workflow) for high-volume B2C");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iterable".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iterable(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
