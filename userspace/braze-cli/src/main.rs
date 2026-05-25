#![deny(clippy::all)]

//! braze-cli — OurOS Braze (B2C customer engagement, NASDAQ:BRZE)
//!
//! Single personality: `braze`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_braze(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: braze [OPTIONS]");
        println!("Braze (OurOS) — comprehensive customer engagement platform");
        println!();
        println!("Options:");
        println!("  --essential            Essential (mid-market, custom)");
        println!("  --pro                  Pro (large enterprise, custom)");
        println!("  --enterprise           Enterprise (largest deals, $1M+/yr common)");
        println!("  --canvas               Canvas — visual journey flow builder");
        println!("  --currents             Currents — real-time event streaming to data warehouse");
        println!("  --catalogs             Catalogs — product/content catalog feed");
        println!("  --brazeai              BrazeAI Suite (gen AI + Predictive)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Braze 2024 (OurOS)"); return 0; }
    println!("Braze 2024 (OurOS)");
    println!("  Vendor: Braze, Inc. (NYC, NY — NASDAQ:BRZE)");
    println!("  Founders: Bill Magnuson (CEO), Jon Hyman (CTO), Mark Ghermezian (left 2018), 2011");
    println!("          all three were ex-Bridgewater Associates engineers");
    println!("          originally called 'Appboy' until rebrand to Braze 2017");
    println!("          rebrand happened because 'Appboy' implied mobile-only (they'd expanded to web + omnichannel)");
    println!("  Founded: 2011 in NYC");
    println!("  IPO: Nov 2021 NASDAQ:BRZE at $65 (popped to $103) — one of last 'normal' tech IPOs of zero-rate era");
    println!("       now ~$40-50 (compressed but better than many peers)");
    println!("       FY2024 revenue ~$590M (+33% YoY), guiding to $1B+ FY2026");
    println!("       ~2,000+ employees, ~2,400 paying customers");
    println!("  Strategic position: 'modern customer engagement platform' for B2C brands at scale");
    println!("                    primary head-to-head competitor: Iterable (similar pitch, NYC vs SF)");
    println!("                    pitches against Salesforce Marketing Cloud + Adobe Campaign + Oracle Responsys");
    println!("                    leader in Gartner MQ for Multichannel Marketing Hubs");
    println!("                    differentiator vs legacy: real-time event-driven (legacy = list-batch)");
    println!("  Pricing (very opaque — typically 6-7 figure deals):");
    println!("    Essential — custom (typically $50K-150K/yr starter)");
    println!("    Pro — custom (typically $200K-500K/yr mid-market)");
    println!("    Enterprise — custom ($1M+/yr common for large B2C brands)");
    println!("    pricing scales by Monthly Active Users + channels + add-ons");
    println!("    AI Suite + Catalogs + Currents are paid add-ons on top of base");
    println!("  Channels (broadest in the industry):");
    println!("    - Email (own infrastructure, deep deliverability tools)");
    println!("    - Push notifications (web + mobile iOS/Android with rich media)");
    println!("    - In-app messages (mobile + web with branded UI templates)");
    println!("    - Content Cards (in-app feed/inbox, like Instagram Stories for in-app)");
    println!("    - SMS + MMS (international, partner network)");
    println!("    - WhatsApp Business (recent — strategic channel)");
    println!("    - LINE (Japan)");
    println!("    - Webhooks (any custom channel — voice via Twilio, direct mail via Lob)");
    println!("    - Connected Audiences (ad platform sync — Meta, Google, TikTok)");
    println!("  Canvas Flow (the workflow engine):");
    println!("    - Visual journey builder — drag-and-drop multi-step flows");
    println!("    - Conditional branches (any attribute or event)");
    println!("    - Wait steps (time-based or event-triggered)");
    println!("    - A/B + multivariate testing inline");
    println!("    - Frequency capping + quiet hours");
    println!("    - Goal tracking + multi-channel attribution");
    println!("    - Decision Split (route via predictive AI)");
    println!("    - Audience Sync nodes (push cohorts to ad platforms in middle of journey)");
    println!("  Personalization:");
    println!("    - Liquid templating (similar to Customer.io)");
    println!("    - Connected Content (call external APIs during render — weather, inventory, ML APIs)");
    println!("    - Catalogs — sync product catalog data and reference in messages");
    println!("    - Recommendations — collaborative filtering on catalog items");
    println!("  BrazeAI Suite:");
    println!("    - Sage AI (generative AI assistant) — drafts copy, subject lines, push titles");
    println!("    - Predictive Churn — likelihood-to-churn score per user");
    println!("    - Predictive Purchase — likelihood-to-buy score");
    println!("    - Send Time Optimization (per-user best send time)");
    println!("    - Intelligent Selection (choose best variant per user in real-time)");
    println!("    - launched 2023, doubled down on gen AI 2024");
    println!("  Currents (data export):");
    println!("    - Real-time event stream out of Braze into your warehouse");
    println!("    - Snowflake, BigQuery, Redshift, S3, Azure native sinks");
    println!("    - Enables warehouse-based analytics on engagement data");
    println!("  Catalogs:");
    println!("    - Sync product catalog (SKUs, articles, locations)");
    println!("    - Reference in templates for dynamic content");
    println!("    - Drive recommendations + collaborative filtering");
    println!("  Recent acquisitions:");
    println!("    - North Star (CDP) — Jan 2024 for ~$150M — adds 'composable CDP' to Braze");
    println!("  Integrations: 200+ marketplace");
    println!("              Segment, mParticle, Tealium (deepest with mParticle)");
    println!("              Salesforce, HubSpot CRM sync");
    println!("              Snowflake (native Currents)");
    println!("              REST API + webhooks + iOS/Android/Web SDKs");
    println!("              Connected Content for live API calls during message render");
    println!("  Customers: ~2,400 paying customers");
    println!("            HBO Max, Burger King, NBA, Sephora, Postmates, KFC, Lyft, Sky, ITV");
    println!("            Eurosport, Anker, Headspace, BoxLunch, Burger King");
    println!("            sweet spot: $500M-$50B revenue B2C consumer brands");
    println!("  Critique: enterprise-priced — not approachable for SMB/mid-market");
    println!("           UI/setup complexity requires dedicated marketing ops + engineering");
    println!("           seat-based admin pricing layered on top of MAU base — total cost confusing");
    println!("           AI features competitive but Salesforce + Adobe have larger AI budgets");
    println!("           some customer pushback on price hikes at renewals");
    println!("  Differentiator: broadest in-app + mobile channel set + real-time architecture + largest deployed B2C brand reference list");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "braze".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_braze(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
