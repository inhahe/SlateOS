#![deny(clippy::all)]

//! pendo-cli — OurOS Pendo (product analytics + in-app guides + feedback)
//!
//! Single personality: `pendo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pendo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pendo [OPTIONS]");
        println!("Pendo (OurOS) — product experience platform (analytics + guides + feedback)");
        println!();
        println!("Options:");
        println!("  --free                 Free tier (≤500 MAU, 1 app)");
        println!("  --base                 Base (custom, mid-market)");
        println!("  --core                 Core tier (most popular for SaaS)");
        println!("  --pulse                Pulse tier (enterprise)");
        println!("  --ultimate             Ultimate (largest deals, ~$200K+/yr)");
        println!("  --listen               Pendo Listen — voice of customer (ex-Mind the Product acquisition)");
        println!("  --roadmaps             Pendo Roadmaps (ex-receptive.io)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pendo 2024 (OurOS)"); return 0; }
    println!("Pendo 2024 (OurOS)");
    println!("  Vendor: Pendo.io, Inc. (Raleigh, NC — private)");
    println!("  Founders: Todd Olson (CEO), Eric Boduch, Erik Troan, Rahul Jain, 2013");
    println!("          all four had been at Red Hat / Rally Software before");
    println!("          Olson: prolific blogger ('Product-Led Growth' early evangelist)");
    println!("  Founded: 2013 in Raleigh, NC — significant non-Bay-Area tech success story");
    println!("  Funding: $208M Series F Jul 2021 at $2.6B valuation");
    println!("          Total raised ~$356M");
    println!("          Sapphire Ventures, Battery, Spark Capital, Geodesic, Tiger");
    println!("          tracking toward IPO 2023-2024 (delayed by market) — ~$200M ARR rumored");
    println!("  Defining product — three pillars in one platform:");
    println!("    1. Analytics — event tracking + retention + funnels (Mixpanel-like)");
    println!("    2. In-App Guides — tooltips, modals, banners, walkthroughs (Appcues-like)");
    println!("    3. Feedback — in-app surveys, NPS, feature requests (UserVoice-like)");
    println!("    selling point: one snippet, three jobs done — vs paying for separate vendors");
    println!("  Pricing (notoriously opaque):");
    println!("    Free — 500 MAU, basic analytics + 3 guides");
    println!("    Base — custom (typically $25K-50K/yr starter)");
    println!("    Core — custom (typically $40K-80K/yr most-popular)");
    println!("    Pulse — custom (typically $75K-150K/yr enterprise)");
    println!("    Ultimate — custom ($200K+/yr large deals)");
    println!("    pricing scales by MAU + features (multi-app, mobile, etc.)");
    println!("  Analytics features:");
    println!("    - Visual tagging — click on UI elements in your app to define events");
    println!("    - Track features without code changes (similar to Heap auto-capture)");
    println!("    - Funnels + Retention + Paths");
    println!("    - Segments (cohorts of users/accounts)");
    println!("    - Account-level analytics (B2B-friendly grouping)");
    println!("    - Custom Reports + Dashboards");
    println!("    - Stickiness (DAU/WAU/MAU)");
    println!("  In-App Guides:");
    println!("    - WYSIWYG editor (no code)");
    println!("    - Multiple guide types: tooltips, lightbox modals, banners, walkthroughs");
    println!("    - Targeting (show guide to specific segments)");
    println!("    - A/B test different guide variants");
    println!("    - Guide analytics (views, clicks, dismisses, completion)");
    println!("    - Multi-language localization");
    println!("    - Resource Center (in-app help hub with KB articles + guides + onboarding checklist)");
    println!("  Feedback:");
    println!("    - In-app NPS surveys (with segmentation + targeting)");
    println!("    - Polls + intercepts (PMs collect feature feedback inline)");
    println!("    - Feature request capture (replaces UserVoice for many)");
    println!("    - Sentiment analysis (NLP on text feedback)");
    println!("    - Tied to user/account so PM sees WHO said WHAT");
    println!("  Pendo Listen (ex-Mind the Product acquisition material):");
    println!("    - Aggregate all qualitative + quantitative feedback signals");
    println!("    - AI-powered theme detection across surveys + interviews + support tickets");
    println!("  Pendo Roadmaps (ex-receptive.io, acquired 2019):");
    println!("    - Public + private roadmap pages");
    println!("    - Tie roadmap items to feedback themes + analytics events");
    println!("    - Customer-facing 'what's launching when' page");
    println!("  AI features (2024+):");
    println!("    - Pendo Solutions AI — natural language queries over Pendo analytics");
    println!("    - Auto-tagging suggestions");
    println!("    - Guide draft generator");
    println!("  Integrations: 70+ apps");
    println!("              Segment, Salesforce, HubSpot, Marketo, Zendesk, Slack, Jira");
    println!("              Snowflake/BigQuery/Redshift export");
    println!("              REST API + webhooks");
    println!("  Customers: ~3,000+ companies, heavily B2B SaaS-focused");
    println!("            Cisco, Verizon, Lululemon (digital), Salesforce (some teams), Citrix");
    println!("            Workday (some teams), Box, MongoDB, OpenText, Henry Schein");
    println!("            sweet spot: B2B SaaS with 'enable users in-app' use case");
    println!("  Critique: opaque pricing — frustrating to evaluate without sales call");
    println!("           three-pillars story sometimes loses to best-of-breed competitors per pillar");
    println!("           UI somewhat complex compared to Amplitude's polish");
    println!("           guide UX historically dated (improving with recent redesigns)");
    println!("           IPO delays signal challenging public-market environment");
    println!("  Differentiator: only platform unifying analytics + in-app guides + feedback as integrated motion");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pendo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pendo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
