#![deny(clippy::all)]

//! mixpanel-cli — SlateOS Mixpanel (the OG event analytics — predates Amplitude)
//!
//! Single personality: `mixpanel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mixpanel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mixpanel [OPTIONS]");
        println!("Mixpanel (Slate OS) — event-based product analytics");
        println!();
        println!("Options:");
        println!("  --free                 Free tier (1M events/mo)");
        println!("  --growth               Growth from $20/mo (sliding scale)");
        println!("  --enterprise           Enterprise (custom, six figures common)");
        println!("  --signal               Mixpanel Signal — causal analysis (Mar 2023)");
        println!("  --warehouse            Warehouse connectors (Snowflake/BigQuery/Databricks)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mixpanel 2024 (Slate OS)"); return 0; }
    println!("Mixpanel 2024 (Slate OS)");
    println!("  Vendor: Mixpanel, Inc. (San Francisco, CA — private)");
    println!("  Founders: Suhail Doshi (CEO until 2018), Tim Trefren, 2009");
    println!("          Doshi was 19 when he started Mixpanel; today runs Playground (image gen AI)");
    println!("          Trefren still at Mixpanel");
    println!("  Founded: 2009 — YC Summer 2009 batch");
    println!("          one of the earliest YC SaaS analytics companies");
    println!("  Funding: ~$77M raised across A/B/C");
    println!("          last big round 2014 Series C $65M at $865M valuation — never raised again");
    println!("          Bain Capital partial buyout 2019 — operates roughly profitable");
    println!("          ~$200M ARR (private, estimated)");
    println!("  Leadership: Amir Movafaghi has been CEO since 2018 (ex-Yammer, Microsoft)");
    println!("            steady-state focus on profitability + warehouse-native rearchitecture");
    println!("  Historical position: Mixpanel was THE event analytics tool 2010-2016");
    println!("                     Amplitude (founded 2012) overtook on UX + funnels by ~2017-2018");
    println!("                     Heap (founded 2013) ate the auto-capture niche");
    println!("                     Mixpanel renaissance 2022+ via warehouse-native + AI features");
    println!("  Pricing:");
    println!("    Free — 1M events/mo, unlimited users, basic reports");
    println!("    Growth — from $20/mo (event-volume sliding)");
    println!("    Enterprise — custom (six-figure deals common)");
    println!("    Pricing innovation: switched in 2022 from MTU-based to EVENT-based — generally cheaper than Amplitude at scale");
    println!("  Core features:");
    println!("    - Event tracking (events + properties + user profiles)");
    println!("    - Insights (any chart type from any combination of properties)");
    println!("    - Funnels (multi-step with custom conversion windows + breakdown by property)");
    println!("    - Retention (cohort retention curves with n-day analysis)");
    println!("    - Flows (sankey-style path analysis)");
    println!("    - JQL (JavaScript Query Language) — legacy power-user feature");
    println!("    - Signal (2023+) — causal analysis with confounder adjustment");
    println!("    - Lexicon — central data dictionary for events + properties");
    println!("    - Reports + Dashboards + Boards (auto-refresh)");
    println!("  Mixpanel Warehouse Connectors:");
    println!("    - Stream events DIRECTLY from Snowflake/BigQuery/Databricks/Redshift");
    println!("    - No re-ingestion, no double storage, queries hit your warehouse");
    println!("    - Pricing decoupled from event volume in warehouse mode");
    println!("    - 2023+ strategic bet — 'modern data stack-native analytics'");
    println!("  Mixpanel AI (2024+):");
    println!("    - Spark (Q4 2024) — natural language to chart");
    println!("    - 'Why' AI — auto-generate causal hypotheses for metric changes");
    println!("    - Embedded LLM assistant for query authoring");
    println!("  Data ingestion:");
    println!("    - Client SDKs: JS, iOS (Swift/ObjC), Android (Java/Kotlin), React Native, Flutter, Unity");
    println!("    - Server SDKs: Node, Python, Ruby, Java, Go, PHP, .NET");
    println!("    - HTTP /track endpoint for custom integrations");
    println!("    - Source: Segment, Rudder, mParticle, Snowplow upstream");
    println!("  Integrations: 100+ destinations");
    println!("              Slack, Salesforce, HubSpot, Marketo, Mailchimp, Iterable, Braze, Customer.io");
    println!("              Warehouses both ways (read + write)");
    println!("              webhooks + REST API");
    println!("  Customers: ~10,000+ paying customers");
    println!("            Uber (early flagship), Yelp, Yammer (pre-Microsoft), Twitch, Pinterest (early)");
    println!("            BMW, OpenTable, Expedia, Wealthfront, Codecademy, Sequoia");
    println!("            sweet spot: PLG B2B SaaS + consumer app + media/streaming");
    println!("  Critique: lost mindshare to Amplitude 2017-2021 — many growth teams default to AMPL");
    println!("           UI evolved more slowly than Amplitude's during that era");
    println!("           AI features still maturing vs aggressive bets from Heap/PostHog");
    println!("           perception gap: still seen as 'second-choice' by many growth PMs despite parity now");
    println!("  Differentiator: longest-running product analytics + first to bet on warehouse-native architecture");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mixpanel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mixpanel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mixpanel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mixpanel"), "mixpanel");
        assert_eq!(basename(r"C:\bin\mixpanel.exe"), "mixpanel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mixpanel.exe"), "mixpanel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mixpanel(&["--help".to_string()], "mixpanel"), 0);
        assert_eq!(run_mixpanel(&["-h".to_string()], "mixpanel"), 0);
        let _ = run_mixpanel(&["--version".to_string()], "mixpanel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mixpanel(&[], "mixpanel");
    }
}
