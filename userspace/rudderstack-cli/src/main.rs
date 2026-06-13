#![deny(clippy::all)]

//! rudderstack-cli — SlateOS RudderStack (warehouse-native open-source CDP, SF + Bangalore)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rudderstack [OPTIONS]");
        println!("RudderStack (SlateOS) — warehouse-native open-source CDP (the Segment alternative)");
        println!();
        println!("Options:");
        println!("  --sources              SDKs + server + cloud sources (200+)");
        println!("  --destinations         200+ destinations");
        println!("  --transformations      JavaScript transformations on events");
        println!("  --warehouse-first      Warehouse-first architecture (Snowflake/BigQuery as source of truth)");
        println!("  --self-host            Run RudderStack OSS on your own infra");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("RudderStack 2024 (SlateOS)"); return 0; }
    println!("RudderStack 2024 (SlateOS) — Warehouse-Native CDP");
    println!("  Vendor: RudderStack, Inc. (San Francisco + Bangalore)");
    println!("  Founders: Soumyadeb Mitra (CEO), 2019");
    println!("          Soumyadeb: ex-8x8 + 1010data + IBM (data engineering background)");
    println!("          founded RudderStack to be 'the open-source Segment'");
    println!("          original positioning: 'Segment is too expensive, let's build OSS'");
    println!("          pivoted post-2021 to 'warehouse-first CDP' (better moat than just cheaper-Segment)");
    println!("  Funding: ~$82M total");
    println!("         Series B Oct 2021: $56M led by Insight Partners");
    println!("         Series A Jun 2021: $21M led by Insight Partners");
    println!("         seed 2019: Kleiner Perkins, S28 Capital");
    println!("  Strategic position: 'warehouse-native CDP — data lives in your warehouse, not ours':");
    println!("                    pitch: 'unify customer data in YOUR Snowflake, then route from there'");
    println!("                    target: data-engineering-led teams using Snowflake/BigQuery/Databricks");
    println!("                    primary competitor: Segment, mParticle (packaged CDPs)");
    println!("                    secondary: Hightouch + Census (composable CDP / reverse-ETL)");
    println!("                    RudderStack's wedge: open-source + warehouse-first + cheaper than Segment");
    println!("                    'composable CDP' evangelist — won mid-market data teams");
    println!("                    Indian engineering team = cost advantage + 24/7 dev cycles");
    println!("  Pricing (tier + open-source):");
    println!("    RudderStack OSS — FREE, AGPL (self-host on your own infrastructure)");
    println!("    Free Cloud — 5K MTUs/mo, 2 destinations");
    println!("    Starter — $500/mo (10K-100K MTUs)");
    println!("    Growth — $2K-10K/mo (mid-market)");
    println!("    Enterprise — $50K-500K+/yr (Fortune 500)");
    println!("    typically 50-70% cheaper than Segment at same scale");
    println!("  Core architecture (warehouse-first):");
    println!("    - Send events from SDKs → RudderStack → simultaneously to warehouse + downstream tools");
    println!("    - Warehouse becomes single source of truth");
    println!("    - Cloud Extract: pull data FROM cloud apps INTO warehouse");
    println!("    - Reverse-ETL: sync from warehouse TO destinations (compete with Hightouch)");
    println!("    - Profiles: identity resolution in your warehouse via SQL");
    println!("  RudderStack OSS:");
    println!("    - Go-based event router (high-throughput, low-latency)");
    println!("    - Postgres-backed event store");
    println!("    - Docker + Kubernetes deployment");
    println!("    - 5K+ GitHub stars, 50+ contributors");
    println!("    - AGPL license (some restrictions on commercial cloud hosting)");
    println!("  Sources (200+):");
    println!("    - SDKs: JavaScript, iOS Swift, Android Kotlin, React Native, Flutter, Unity");
    println!("    - Server: Node, Python, Go, Ruby, Java, PHP, .NET, Rust");
    println!("    - Cloud Extract: Stripe, Salesforce, Zendesk, Mailchimp, HubSpot (API-based pulls)");
    println!("    - Streaming sources: Kafka, AWS Kinesis, GCP Pub/Sub");
    println!("    - Event Stream API");
    println!("  Destinations (200+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres, ClickHouse, S3");
    println!("    - Analytics: Mixpanel, Amplitude, Heap, GA4, Adobe Analytics");
    println!("    - Marketing: Iterable, Braze, Customer.io, Marketo, Klaviyo");
    println!("    - Ads: Facebook CAPI, Google Ads, TikTok, LinkedIn (Conversions APIs)");
    println!("    - Support: Zendesk, Intercom, Salesforce Service Cloud");
    println!("  Transformations (custom JavaScript):");
    println!("    - Write JS code to transform events before forwarding");
    println!("    - Filter, enrich, redact PII, format data");
    println!("    - Test transformations with sample events");
    println!("    - Common use cases: PII hashing, geo-IP enrichment, custom routing logic");
    println!("  Profiles (identity resolution in-warehouse):");
    println!("    - Run identity stitching SQL in your warehouse");
    println!("    - Build computed traits (LTV, churn risk, lifecycle stage)");
    println!("    - Materialize unified profile tables in Snowflake/BigQuery");
    println!("    - Compete with: Segment Personas, Hightouch Audiences");
    println!("  RudderStack CLI usage:");
    println!("    rudderstack login");
    println!("    rudderstack source list");
    println!("    rudderstack destination connect --warehouse snowflake --config snowflake.yml");
    println!("    rudderstack transformation create --name pii-redact --code transformation.js");
    println!("    rudderstack profiles run --project main");
    println!("  Customers (~500+ paying):");
    println!("    - Hinge, Allbirds, Crossbeam, Wynn Las Vegas, Acorns");
    println!("    - Stripe (internal use), Shopify (internal), Mattermost");
    println!("    - sweet spot: data-engineering-led teams at $10M-$1B ARR companies");
    println!("    - heavy in: e-commerce, SaaS, marketplaces");
    println!("    - 50K+ OSS deployments worldwide");
    println!("  Critique: AGPL license discourages some cloud vendors from embedding");
    println!("           OSS version requires DevOps work to scale (Postgres + Go)");
    println!("           fewer destinations than Segment (200 vs 450)");
    println!("           Twilio Segment's enterprise install base hard to displace");
    println!("           Hightouch + Census attack from 'pure reverse-ETL' angle");
    println!("           identity resolution still less mature than mParticle's IDSync");
    println!("           growing competition from Snowflake Streamlit + native data sharing");
    println!("  Differentiator: open-source + warehouse-first architecture + cheaper than Segment + Indian-engineering scale economics + 'composable CDP' evangelist — the CDP choice for data-engineering-led teams already invested in Snowflake/BigQuery");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rudderstack".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rudderstack"), "rudderstack");
        assert_eq!(basename(r"C:\bin\rudderstack.exe"), "rudderstack.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rudderstack.exe"), "rudderstack");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rs(&["--help".to_string()], "rudderstack"), 0);
        assert_eq!(run_rs(&["-h".to_string()], "rudderstack"), 0);
        let _ = run_rs(&["--version".to_string()], "rudderstack");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rs(&[], "rudderstack");
    }
}
