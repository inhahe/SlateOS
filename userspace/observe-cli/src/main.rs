#![deny(clippy::all)]

//! observe-cli — OurOS Observe (Snowflake-based observability, San Mateo CA, private Sutter Hill-backed)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_observe(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: observe [OPTIONS]");
        println!("Observe (OurOS) — Snowflake-native observability cloud (private, Sutter Hill-backed)");
        println!();
        println!("Options:");
        println!("  --datastreams          Data Streams (logs, metrics, traces input)");
        println!("  --datasets             Datasets (typed, queryable, transformed data)");
        println!("  --o11ai                O11.ai (LLM Copilot for ops, 2024)");
        println!("  --gameday              GameDay (incident management)");
        println!("  --opal                 OPAL (Observe Processing + Analytics Language)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Observe 2024 (OurOS)"); return 0; }
    println!("Observe 2024 (OurOS) — Observability Cloud on Snowflake");
    println!("  Vendor: Observe, Inc. (San Mateo, CA — private)");
    println!("  Founders: Jeremy Burton (CEO) + Jon Watte (CTO) + Yvan Sukur, 2018");
    println!("          Jeremy Burton: ex-Dell EMC CMO + ex-VMware EVP — enterprise marketing veteran");
    println!("          Sutter Hill Ventures incubated — same firm that backed Snowflake from seed");
    println!("          founded with thesis: 'observability is a data problem; use the best data platform (Snowflake)'");
    println!("  Private funding:");
    println!("         Series B Aug 2022: $70M at ~$300M valuation (Snowflake Ventures, Sutter Hill)");
    println!("         total raised: ~$112M");
    println!("         Sutter Hill, Snowflake Ventures, Capital One Ventures backers");
    println!("         estimated $20-40M ARR (private, growing)");
    println!("  Strategic position: 'observability cloud built on Snowflake — separate compute + storage':");
    println!("                    pitch: 'pay for what you query, not what you ingest — observability priced like Snowflake'");
    println!("                    target: cloud-native + data-savvy enterprises");
    println!("                    primary competitor: Datadog, Splunk, Sumo Logic, Logz.io, Honeycomb");
    println!("                    secondary: Chronosphere, Mezmo, Edge Delta");
    println!("                    Observe's wedge: Snowflake architecture (separate compute/storage) + unlimited retention + 'datasets' model");
    println!("                    'Observability cloud' = Snowflake-pattern (warehouse for telemetry) applied to ops data");
    println!("  Pricing (compute + storage decoupled — Snowflake-style):");
    println!("    Compute: $0.50/credit-hour starting (Snowflake compute pass-through)");
    println!("    Storage: $0.02/GB/month for active datasets ($0.005/GB for cold tier)");
    println!("    Ingestion: free (don't pay to put data in — pay to query)");
    println!("    Typical small deployment: $5K/month");
    println!("    Typical mid-market: $30K-$100K/month");
    println!("    typically 30-60% cheaper than Datadog for similar query workloads");
    println!("  Product portfolio:");
    println!("    1. Data Streams (ingestion):");
    println!("       - Logs, metrics, traces, events ingested as raw streams");
    println!("       - 100+ ingestion sources (OpenTelemetry, FluentBit, Filebeat, AWS, etc.)");
    println!("       - Stored cheaply in Snowflake-backed tables");
    println!("    2. Datasets (the core abstraction):");
    println!("       - Typed, transformed views of telemetry");
    println!("       - Created via OPAL (Observe Processing + Analytics Language)");
    println!("       - 'Resources' (long-lived entities — pods, hosts, users)");
    println!("       - 'Events' (point-in-time facts — errors, deploys)");
    println!("       - 'Intervals' (time-bounded states — pod alive from T1 to T2)");
    println!("       - Datasets can derive from other datasets — dependency DAG");
    println!("    3. OPAL (Observe Processing + Analytics Language):");
    println!("       - Pipe-based query DSL (similar to Splunk SPL, kusto)");
    println!("       - Compiles to Snowflake SQL under the hood");
    println!("       - Type-aware: 'Resources' vs 'Events' vs 'Intervals'");
    println!("       - Lazy + materialized hybrid execution");
    println!("    4. O11.ai (2024 — the LLM ops copilot):");
    println!("       - Natural-language to OPAL queries");
    println!("       - Conversational debugging + incident triage");
    println!("       - Big bet on LLM-augmented observability");
    println!("    5. GameDay (incident management):");
    println!("       - Incident timeline + chronology");
    println!("       - Slack-native incident channels");
    println!("       - Post-mortem authoring");
    println!("       - Compete with: PagerDuty, Incident.io, FireHydrant, Rootly");
    println!("    6. Dashboards + Worksheets:");
    println!("       - Worksheets = interactive query notebook");
    println!("       - Dashboards = published views for non-engineers");
    println!("    7. App Library:");
    println!("       - Pre-built integrations + dashboards (AWS, K8s, MySQL, Redis, etc.)");
    println!("       - 'Apps' = bundle of datasets + dashboards + alerts");
    println!("    8. Alerts + Monitors:");
    println!("       - Threshold + anomaly detection on datasets");
    println!("       - Slack, PagerDuty, Opsgenie, ServiceNow integrations");
    println!("  Snowflake architecture (the bet):");
    println!("    - Built on Snowflake (separate compute + storage)");
    println!("    - Decouples ingestion cost from query cost");
    println!("    - Unlimited retention without index bloat");
    println!("    - Backed by Sutter Hill (Snowflake's incubator) — deep architectural ties");
    println!("    - Big bet: Snowflake's elasticity is the right substrate for observability");
    println!("    - Counter-bet to Datadog's purpose-built TSDB + index architecture");
    println!("  Integrations:");
    println!("    - OpenTelemetry-native (logs, metrics, traces)");
    println!("    - FluentBit + Filebeat + Logstash + Vector + syslog");
    println!("    - AWS CloudWatch + CloudTrail + GuardDuty + VPC Flow");
    println!("    - Kubernetes (deep), Docker, ECS, Fargate");
    println!("    - DBs: PostgreSQL, MySQL, Redis, MongoDB built-in metrics");
    println!("    - Prometheus scrape endpoints supported");
    println!("    - Alerting: PagerDuty, Opsgenie, Slack, Teams, ServiceNow, Jira");
    println!("    - SSO: Okta, Azure AD, Google Workspace, SAML");
    println!("  Observe CLI usage:");
    println!("    observe login --tenant my-org");
    println!("    observe dataset list --filter resource");
    println!("    observe opal run @query.opal --dataset my-app-logs");
    println!("    observe app install --name kubernetes");
    println!("    observe dashboard import --file dashboard.json");
    println!("    observe monitor create --name 'High Error Rate' --query @alert.opal");
    println!("    observe o11ai chat --dataset my-app-logs --query 'why is the checkout service slow today?'");
    println!("  Customers (~150+):");
    println!("    - Cloud-native + data-savvy enterprises");
    println!("    - DoorDash, Roblox, RingCentral, Atlassian (some teams)");
    println!("    - Sweet spot: AWS-heavy SaaS startups + mid-market");
    println!("    - Growth driver: Snowflake customers cross-sold (architectural fit)");
    println!("    - Small but premium customer base in 2024");
    println!("  Critique: Snowflake compute can be expensive for high-throughput queries");
    println!("           young company (2018) — battle-tested at scale less than Datadog/Splunk");
    println!("           OPAL learning curve for teams without SPL/Kusto background");
    println!("           customer count modest vs Datadog's 28K+ paying base");
    println!("           Datadog brand awareness + sales engine huge advantage in deals");
    println!("           O11.ai feels follower-not-leader vs Datadog Bits / Dynatrace Davis CoPilot");
    println!("           dependent on Snowflake pricing + roadmap — not fully in control of cost basis");
    println!("           private funding modest ($112M) vs Datadog ($300M+ pre-IPO)");
    println!("  Differentiator: Snowflake-native architecture (separate compute + storage = pay-per-query, not pay-per-ingest) + 'datasets + OPAL' typed observability model + Sutter Hill incubation (Snowflake's investor firm) + unlimited retention via cold tiers + O11.ai LLM copilot + GameDay incident management + Jeremy Burton's enterprise sales credibility — the observability cloud for teams that want Snowflake's elasticity applied to operational telemetry instead of Datadog's purpose-built but rigid pricing");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "observe".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_observe(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
