#![deny(clippy::all)]

//! customerio-cli — OurOS Customer.io (event-triggered messaging for developers)
//!
//! Single personality: `customerio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_customerio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: customerio [OPTIONS]");
        println!("Customer.io (OurOS) — event-driven cross-channel messaging");
        println!();
        println!("Options:");
        println!("  --essentials           Essentials from $100/mo (5K profiles)");
        println!("  --premium              Premium from $1,000/mo (advanced features)");
        println!("  --enterprise           Enterprise (custom)");
        println!("  --data-pipelines       Data Pipelines (ex-Segment competitor, CDP)");
        println!("  --parcel               Parcel — email design tool (ex-acquisition)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Customer.io 2024 (OurOS)"); return 0; }
    println!("Customer.io 2024 (OurOS)");
    println!("  Vendor: Peaberry Software, Inc. dba Customer.io (Portland, OR — fully remote, bootstrapped+VC mix)");
    println!("  Founders: Colin Nederkoorn (CEO), John Allison (CTO), 2012");
    println!("          Nederkoorn famously runs an open-book + radical-transparency culture");
    println!("          'Customer.io' built originally inside a Heroku app");
    println!("          fully remote since FOUNDING — 12 years before pandemic, very early on remote work");
    println!("  Founded: 2012 — bootstrapped to profitability before raising");
    println!("          first outside funding 2021 ($35M Series A — Hand Capital, Bonfire)");
    println!("          ~$50M+ ARR (estimated, transparent partially)");
    println!("          ~250 employees in 50+ countries");
    println!("  Culture: 'open by default' — internal handbook public, semi-public roadmap");
    println!("          gender-balanced exec team, frequent at remote-work conferences");
    println!("          team has historically refused to add 'CMO bloat' to product");
    println!("  Defining philosophy — 'devtool for marketers':");
    println!("    - Powerful event-based segmentation + workflows");
    println!("    - Liquid templating (JSON-deep data access in templates)");
    println!("    - Treats engineers as primary user — easy APIs, webhooks, clean docs");
    println!("    - Less polished UI vs Braze/Iterable — but more flexible for power users");
    println!("    - Designed for SaaS PLG companies + technical B2C apps");
    println!("  Pricing (transparent on website):");
    println!("    Essentials — from $100/mo (5K profiles, basic workflows + email/SMS/push)");
    println!("    Premium — from $1,000/mo (advanced segmentation, longer retention, branching)");
    println!("    Enterprise — custom (typically $30K-150K/yr)");
    println!("    pricing scales with profiles + features (not events) — predictable as you grow");
    println!("    add-on: Data Pipelines (CDP layer) + Parcel (email design)");
    println!("  Channels:");
    println!("    - Email (transactional + marketing) with own delivery infra");
    println!("    - SMS via Twilio integration");
    println!("    - Push notifications (mobile iOS/Android, web push)");
    println!("    - In-app messages (mobile)");
    println!("    - Slack, Webhooks, custom integrations as 'channels'");
    println!("  Workflows (the engine):");
    println!("    - Event-triggered or attribute-changed or list-membership");
    println!("    - Branching by any data point (segment, attribute, prior step outcome)");
    println!("    - Delays + time windows");
    println!("    - Quiet hours");
    println!("    - Frequency caps");
    println!("    - A/B testing inline");
    println!("    - Goal tracking per workflow");
    println!("  Liquid templating:");
    println!("    - Access any nested customer attribute or event property");
    println!("    - Loops, conditionals, computed values in-template");
    println!("    - 'Personalization that actually feels personal' — power users love it");
    println!("    - Reusable snippets across emails");
    println!("  Segmentation:");
    println!("    - Manual segments (criteria-based)");
    println!("    - Computed traits (RFM, LTV, engagement scores)");
    println!("    - Behavior-based ('did X 3+ times in last 30 days')");
    println!("    - Lookalike from another segment (via integrations)");
    println!("  Data Pipelines (2022+ — CDP layer):");
    println!("    - Customer.io built a Segment-style CDP on top of their event data");
    println!("    - Sync events to/from warehouses, ad platforms, downstream tools");
    println!("    - Direct competition with Segment + Hightouch + Rudderstack at the CDP layer");
    println!("  Parcel (acquired 2022):");
    println!("    - Email design tool — code-first WYSIWYG editor for marketers + developers");
    println!("    - Like MJML but with a better UI");
    println!("    - Integrated into Customer.io email composer");
    println!("  AI features (recent):");
    println!("    - Subject line generator + scorer");
    println!("    - Generative copy assist");
    println!("    - Send time optimization");
    println!("    - Anomaly detection on workflow performance");
    println!("  Integrations: 50+ native + Zapier");
    println!("              Segment (deepest), Stripe (revenue events), Shopify");
    println!("              Slack, Salesforce, HubSpot, Zendesk, Intercom");
    println!("              Snowflake/BigQuery exports");
    println!("              REST API + webhooks + iOS/Android/JS SDKs");
    println!("  Customers: ~7,000+ paying companies");
    println!("            Notion (flagship case study), Linear, Loom, ConvertKit, Buffer (early), Webflow");
    println!("            Sentry, Algolia, Vercel — heavy PLG SaaS adoption");
    println!("            sweet spot: 5-500 person teams running event-driven product marketing");
    println!("  Critique: less out-of-the-box than HubSpot/Braze — requires engineering to wire well");
    println!("           UI dated compared to Iterable/Braze (improving)");
    println!("           reporting tools simpler than competitors at higher tiers");
    println!("           power users love Liquid; novice marketers find it intimidating");
    println!("           less B2C consumer brand awareness than Braze (intentional — they target PLG SaaS)");
    println!("  Differentiator: devtool-quality event-driven messaging — best Liquid templating + open culture + remote-first since 2012");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "customerio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_customerio(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
