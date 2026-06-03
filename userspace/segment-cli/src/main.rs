#![deny(clippy::all)]

//! segment-cli — OurOS Twilio Segment (CDP category creator, SF, acquired by Twilio 2020)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_segment(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: segment [OPTIONS]");
        println!("Twilio Segment (OurOS) — Customer Data Platform (the category creator)");
        println!();
        println!("Options:");
        println!("  --sources              350+ sources (SDKs, server-side, cloud apps)");
        println!("  --destinations         450+ destinations (warehouses, analytics, marketing)");
        println!("  --identity             Identity resolution + Unify");
        println!("  --personas             Audience builder (computed traits + segments)");
        println!("  --protocols            Schema enforcement + Tracking Plan");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Twilio Segment 2024 (OurOS)"); return 0; }
    println!("Twilio Segment 2024 (OurOS) — Customer Data Platform");
    println!("  Vendor: Twilio Segment (part of Twilio Inc., NYSE:TWLO since acquisition)");
    println!("  Founders: Peter Reinhardt, Calvin French-Owen, Ilya Volodarsky, Ian Storm Taylor, 2011");
    println!("          all four were MIT students — pivoted from classroom-feedback app to CDP");
    println!("          original idea was 'analytics.js' — single SDK that forwards events to many tools");
    println!("          coined the 'Customer Data Platform' category (Segment, mParticle, Tealium)");
    println!("          one of YC's iconic pivot stories");
    println!("          Peter left 2021 to found Charm Industrial (carbon removal)");
    println!("          Calvin French-Owen later joined OpenAI");
    println!("  Acquisition Nov 2020:");
    println!("         Twilio acquired Segment for $3.2B in stock (largest acquisition for Twilio)");
    println!("         strategic: CPaaS + CDP = 'customer engagement platform'");
    println!("         remained as Twilio Segment product line, separate brand");
    println!("         post-acquisition: layoffs in 2022-2023 as Twilio cut costs broadly");
    println!("  Funding history pre-acquisition:");
    println!("         Series E Nov 2019: $175M at ~$1.5B+ valuation (Accel, Meritech, GV, Thrive)");
    println!("         total raised: ~$284M before $3.2B exit");
    println!("  Strategic position: 'first-party customer data infrastructure':");
    println!("                    pitch: 'one API to collect, clean, and route customer data anywhere'");
    println!("                    target: digital-first companies wanting unified customer data");
    println!("                    primary competitor: mParticle, Tealium, RudderStack (OSS), Adobe Experience Platform");
    println!("                    secondary: Snowflake-native CDPs (Hightouch, Census), Salesforce CDP");
    println!("                    Segment's moat: largest source/destination catalog + first-mover brand");
    println!("                    challenger trend: 'composable CDP' (Hightouch + warehouse) erodes packaged CDP");
    println!("  Pricing (consumption + tier):");
    println!("    Free — up to 1K visitors/mo, 2 sources, basic features");
    println!("    Team — $120/mo + usage (small startups)");
    println!("    Business — custom $30K-$500K+/yr (mid-market + enterprise)");
    println!("    pricing pegged to monthly tracked users (MTUs)");
    println!("    historically expensive at scale — common complaint that drives Hightouch+warehouse defection");
    println!("  Core architecture:");
    println!("    - Sources: 350+ libraries (analytics.js, iOS, Android, server SDKs, cloud-app integrations)");
    println!("    - Destinations: 450+ tools (Mixpanel, Amplitude, Salesforce, Snowflake, BigQuery, etc.)");
    println!("    - In the middle: identity resolution, validation, transformations, routing");
    println!("    - Schema: 'Tracking Plan' enforces consistent event names + properties");
    println!("    - Replay: send historical data to new destinations after the fact");
    println!("  Identity (Unify):");
    println!("    - Stitch anonymous + identified user sessions");
    println!("    - Cross-device + cross-channel identity resolution");
    println!("    - Computed traits (e.g., 'lifetime value', 'last purchase date')");
    println!("    - Profile API: query unified user profile from any system");
    println!("  Personas (Audience Builder):");
    println!("    - Build audiences from event + trait conditions");
    println!("    - Sync audiences to ad networks (Facebook, Google, TikTok)");
    println!("    - Sync to email/marketing tools (Iterable, Braze, Klaviyo)");
    println!("    - Compete with: marketing automation audience tools, reverse-ETL");
    println!("  Protocols (data quality):");
    println!("    - Schema enforcement at ingest (block bad events)");
    println!("    - Tracking Plan: spec for what events should exist + properties");
    println!("    - Violations dashboard");
    println!("    - Less popular than the rest of the platform but important for enterprise");
    println!("  Functions (custom code):");
    println!("    - Write JavaScript to transform events inline");
    println!("    - Connect to custom destinations not in catalog");
    println!("    - Compete with: workflow tools (Zapier, Census Workflows)");
    println!("  Sources highlights:");
    println!("    - Web/Mobile SDKs: analytics.js, iOS Swift, Android Kotlin, React Native, Unity, Roku");
    println!("    - Server: Node, Python, Go, Ruby, Java, PHP, .NET");
    println!("    - Cloud sources: Stripe, Salesforce, Zendesk, HubSpot (pull events from APIs)");
    println!("    - Reverse ETL: sync from warehouse to Segment, then to destinations");
    println!("  Destinations highlights:");
    println!("    - Warehouses: Snowflake, BigQuery, Redshift, Databricks, Postgres");
    println!("    - Analytics: Mixpanel, Amplitude, Heap, GA4, Adobe Analytics");
    println!("    - Marketing: Iterable, Braze, Customer.io, Marketo, Klaviyo");
    println!("    - Ads: Google Ads, Facebook Ads, TikTok Ads (Conversions API)");
    println!("    - Support: Zendesk, Intercom, Salesforce Service Cloud");
    println!("  Segment CLI usage:");
    println!("    segment track --event 'Order Completed' --user-id 123 --properties '{{\"amount\": 99}}'");
    println!("    segment sources list");
    println!("    segment destinations list --enabled");
    println!("    segment tracking-plan validate --plan ecommerce");
    println!("    segment audiences list");
    println!("  Customers (~25K+ paying):");
    println!("    - Levi's, IBM, Domino's Pizza, Atlassian, Intuit");
    println!("    - Glassdoor, Crate & Barrel, Trivago, Vacasa, Lululemon");
    println!("    - 50%+ of Y Combinator companies use Segment historically");
    println!("    - sweet spot: e-commerce, SaaS, media, fintech");
    println!("    - global enterprise + heavy startup adoption");
    println!("  Critique: expensive at scale (MTU pricing surprises Fortune 500)");
    println!("           'composable CDP' trend (Hightouch/Census + warehouse) eroding packaged CDP value");
    println!("           Twilio acquisition layoffs + reorgs disrupted product velocity 2022-2024");
    println!("           data residency / EU compliance complicated by US-centric infrastructure");
    println!("           catalog maintenance burden (450+ destinations need updating)");
    println!("           Snowflake / Databricks pushing 'data sharing direct to ad networks' — disintermediation");
    println!("           Twilio stock pressure means continued cost discipline at Segment");
    println!("  Differentiator: original CDP brand + largest sources/destinations catalog + Tracking Plan + Twilio's CPaaS integration (SMS, voice, email) — the customer data infrastructure that defined the CDP category");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "segment".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_segment(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_segment};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/segment"), "segment");
        assert_eq!(basename(r"C:\bin\segment.exe"), "segment.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("segment.exe"), "segment");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_segment(&["--help".to_string()], "segment"), 0);
        assert_eq!(run_segment(&["-h".to_string()], "segment"), 0);
        assert_eq!(run_segment(&["--version".to_string()], "segment"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_segment(&[], "segment"), 0);
    }
}
