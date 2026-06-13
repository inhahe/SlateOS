#![deny(clippy::all)]

//! hightouch-cli — SlateOS Hightouch (the reverse-ETL category creator)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ht(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hightouch [OPTIONS]");
        println!("Hightouch (Slate OS) — reverse-ETL platform (warehouse → SaaS apps)");
        println!();
        println!("Options:");
        println!("  --starter              Starter — free (up to 3 syncs)");
        println!("  --pro                  Pro — $450/mo (10 syncs, advanced features)");
        println!("  --business             Business — custom (typically $20K-100K+/yr)");
        println!("  --customer-studio      Customer Studio (composable CDP layer)");
        println!("  --personalization-api  Personalization API (real-time activation)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Hightouch 2024 (Slate OS)"); return 0; }
    println!("Hightouch 2024 (Slate OS)");
    println!("  Vendor: Hightouch, Inc. (San Francisco, CA — private)");
    println!("  Founders: Kashish Gupta (CEO), Tejas Manohar (President), Joshua Curl (CTO), 2018");
    println!("          three engineers (ex-Segment, ex-Splunk, ex-startups), all under 30 at founding");
    println!("          coined the term 'Reverse ETL' to define a new category");
    println!("          loud + opinionated public technical writing — Tejas Manohar widely read");
    println!("          aggressive product velocity — multiple major feature launches per quarter");
    println!("  Founded: 2018 in San Francisco");
    println!("          raised $54M Series B (Sapphire Ventures, 2022) at ~$450M valuation");
    println!("          ~$50M+ ARR estimated (private)");
    println!("          ~250 employees");
    println!("          fierce rivalry with Census (founded 2018-2019, similar pitch, similar funding stage)");
    println!("  Strategic position: 'data warehouse is the new system of record — Hightouch activates it':");
    println!("                    primary competitor: Census (very close rival), Polytomic, Workato (less data-focused)");
    println!("                    competing against legacy CDPs: Segment, mParticle, Tealium (Hightouch pitch: 'don't pay CDP, use your warehouse')");
    println!("                    'Composable CDP' category leader");
    println!("                    sells deep into data teams + marketing ops + sales ops");
    println!("                    bigger than Census by most measures (deals, ARR, employee count)");
    println!("  Pricing (consumption + tier-based, opaque at enterprise):");
    println!("    Starter — Free (up to 3 syncs, 1M MTUs)");
    println!("    Pro — $450/mo (10 syncs, advanced sync features)");
    println!("    Business — custom (typically $20K-100K+/yr based on MTU + syncs)");
    println!("    pricing axis: MTUs (Monthly Tracked Users) + syncs + advanced features");
    println!("    typical enterprise customers pay $50K-$500K+/yr");
    println!("  Core architecture (reverse ETL):");
    println!("    - Connect to data warehouse as source (Snowflake, BigQuery, Redshift, Databricks, Postgres)");
    println!("    - Define 'Model' via SQL — what data + computed fields you want to send");
    println!("    - Define 'Sync' — destination + field mapping + sync mode (full sync, change-data-capture)");
    println!("    - Run on schedule or trigger via webhook + dbt completion");
    println!("    - 200+ destinations: Salesforce, HubSpot, Marketo, Klaviyo, Braze, Iterable, Customer.io,");
    println!("      Meta Ads, Google Ads, TikTok, LinkedIn Ads, Snowflake, Snowpipe, Segment, mParticle,");
    println!("      Notion, Slack, Mixpanel, Amplitude, Zendesk, Intercom, Stripe, Snowflake, Pinterest, etc.");
    println!("    - Match Booster (Hightouch's CDP-like identity resolution over warehouse data)");
    println!("  Customer Studio (composable CDP, 2023+ push):");
    println!("    - Audience builder UI (drag-and-drop segments on top of warehouse tables)");
    println!("    - Customer 360 views from warehouse data");
    println!("    - Eventing + computed traits at activation time");
    println!("    - Marketer-friendly UI sitting on top of Hightouch's sync infrastructure");
    println!("    - Direct competitor to: Segment Personas, mParticle, Twilio Segment");
    println!("    - 'don't buy a CDP, build one in your warehouse'");
    println!("  Personalization API (Apr 2024 launch):");
    println!("    - Real-time API serving warehouse-defined audiences + traits");
    println!("    - Sub-100ms p99 lookup latency");
    println!("    - Enables real-time personalization on app/web/marketing channels");
    println!("    - Hightouch's first 'reverse-ETL output is not just syncs' move");
    println!("    - Competes with: Segment Edge, mParticle real-time API");
    println!("  Hightouch Eventing:");
    println!("    - Hightouch can also be a SOURCE for events (Segment-like JS SDK)");
    println!("    - Captures from web/app, ships to warehouse + downstream destinations");
    println!("    - Hightouch becoming a full 'composable CDP' stack: ingest + storage + identity + activation");
    println!("  Integration depth:");
    println!("    - Bidirectional sync where supported (Salesforce, HubSpot writeback)");
    println!("    - Incremental syncs (only changed rows since last sync) using primary keys");
    println!("    - Field mapping with type coercion + transformations");
    println!("    - Sync mode options: upsert, insert-only, update-only, mirror");
    println!("    - Error logging + Slack/email alerts on sync failures");
    println!("    - Detailed audit logs for compliance");
    println!("  AI Decisioning (2024 launch):");
    println!("    - Built-in ML/AI for: best send time, next-best action, churn prediction");
    println!("    - Train models on warehouse data, deploy as audiences/computed traits");
    println!("    - Hightouch's bet on 'data + AI activation' becoming the next wedge");
    println!("  Integrations: 200+ destinations across:");
    println!("              Ad platforms (Meta, Google, TikTok, LinkedIn, Snap, Pinterest, Reddit, Microsoft, Criteo)");
    println!("              CRM (Salesforce, HubSpot, Pipedrive, Microsoft Dynamics, Outreach, Salesloft, Apollo)");
    println!("              ESPs (Marketo, Klaviyo, Braze, Iterable, Customer.io, ActiveCampaign, Mailchimp)");
    println!("              Support (Zendesk, Intercom, Freshdesk, Front, Help Scout)");
    println!("              Product analytics (Mixpanel, Amplitude, Heap, Pendo, FullStory, Hotjar, PostHog)");
    println!("              Data warehouses + lakehouses as both source + destination");
    println!("              REST + GraphQL APIs + Webhooks");
    println!("  dbt-native + warehouse-native:");
    println!("    - 'dbt Cloud Activation' partnership: trigger syncs from dbt jobs");
    println!("    - Syncs visible in dbt Catalog");
    println!("    - Models can be authored as dbt models directly");
    println!("    - Hightouch is widely considered 'best-in-class' for the dbt-modeled stack");
    println!("  Customers: 500+ paying customers");
    println!("            Spotify (some teams), PetSmart, Calendly, Cars.com, Plaid, Warner Bros Discovery");
    println!("            Imperfect Foods, Sweetgreen, Lucid, Ramp, Brex, Carta");
    println!("            sweet spot: data-mature companies with Snowflake/BigQuery + dbt + activation needs");
    println!("            very strong in: D2C brands, fintech, SaaS, media + entertainment");
    println!("  Critique: pricing opaque + scales aggressively for high-MTU users");
    println!("           sync latency: minutes (not real-time) for warehouse-driven syncs — Personalization API closes this");
    println!("           Census + Polytomic compete on near-identical positioning + features");
    println!("           composable CDP narrative still niche vs Segment dominance");
    println!("           UI improvements ongoing — feature density can overwhelm new users");
    println!("           customer-studio adoption requires data team buy-in (not pure marketing tool)");
    println!("  Differentiator: category creator + best-in-class destination coverage + dbt-native + composable CDP push + AI Decisioning — for data-mature orgs that want activation without buying Segment");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hightouch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ht(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ht};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hightouch"), "hightouch");
        assert_eq!(basename(r"C:\bin\hightouch.exe"), "hightouch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hightouch.exe"), "hightouch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ht(&["--help".to_string()], "hightouch"), 0);
        assert_eq!(run_ht(&["-h".to_string()], "hightouch"), 0);
        let _ = run_ht(&["--version".to_string()], "hightouch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ht(&[], "hightouch");
    }
}
