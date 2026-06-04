#![deny(clippy::all)]

//! rivery-cli — OurOS Rivery (Israeli all-in-one ELT + orchestration platform)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rivery(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rivery [OPTIONS]");
        println!("Rivery (OurOS) — ELT + transformation + reverse-ETL + orchestration in one platform");
        println!();
        println!("Options:");
        println!("  --starter              Starter — from $599/mo (5 sources, basic ELT)");
        println!("  --professional         Professional — $1,499/mo (more sources, transformations)");
        println!("  --enterprise           Enterprise — custom (typically $30K-$200K+/yr)");
        println!("  --logic                Rivery Logic (orchestration + dbt + Python)");
        println!("  --reverse-etl          Action Rivers (reverse ETL to SaaS apps)");
        println!("  --data-catalog         Data Catalog + Lineage");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rivery 2024 (OurOS)"); return 0; }
    println!("Rivery 2024 (OurOS)");
    println!("  Vendor: Rivery Ltd. (Tel Aviv, Israel — private)");
    println!("  Founders: Itamar Ben Hemo (CEO), Aviv Noy (CTO), Eldad Stern, Asaf Yigal, 2019");
    println!("          founders had previously sold Logz.io (ELK log management) — repeat data infrastructure operators");
    println!("          Yigal was Logz.io co-founder, brought log-management ops mindset to data pipelines");
    println!("  Founded: 2019 in Tel Aviv (operates in Tel Aviv + NYC)");
    println!("          raised ~$45M total (State of Mind Ventures, JVP, Tiger Global)");
    println!("          ~$20M+ ARR estimated (private)");
    println!("          ~150 employees in Israel + remote globally");
    println!("  Strategic position: 'all-in-one data integration' vs unbundled stack (Fivetran + dbt + Airflow + Hightouch):");
    println!("                    primary competitor: Fivetran (ELT), Matillion (transformation), Hightouch + Census (reverse ETL)");
    println!("                    Rivery pitch: 'why buy 4 tools when you can use 1?' — unified ELT + transformation + orchestration + reverse-ETL");
    println!("                    targets mid-market: companies wanting modern data stack but with limited platform team");
    println!("                    differentiator: pricing per RPU (Rivery Pricing Unit) — single budget across all data flows");
    println!("                    Israeli engineering quality, B2B mid-market sales motion");
    println!("  Pricing (transparent — RPU-based + tier):");
    println!("    Starter — $599/mo (5 data sources, ~150 RPUs/mo)");
    println!("    Professional — $1,499/mo (more sources, more RPUs, transformations)");
    println!("    Enterprise — custom (~$30K-$200K+/yr, unlimited sources, RPU pool)");
    println!("    RPU = unit measuring compute used by ELT runs + transformations + reverse-ETL syncs");
    println!("    typically simpler to budget vs Fivetran's MAR (Monthly Active Rows) which scales unpredictably");
    println!("  Core architecture (the all-in-one):");
    println!("    - ELT: 200+ source connectors (databases, SaaS apps, files, APIs)");
    println!("    - Transformation: SQL + Python in-warehouse, dbt-compatible");
    println!("    - Orchestration: 'Rivery Logic' DAGs with conditional + parallel execution");
    println!("    - Reverse ETL: 'Action Rivers' to send warehouse data to SaaS apps");
    println!("    - File ingestion: S3, GCS, Azure Blob, SFTP, FTP");
    println!("    - Streaming: Kafka, Pub/Sub, Kinesis ingestion to warehouse");
    println!("    - Custom Python steps in pipelines (run your own code)");
    println!("    - Webhook triggers for event-driven pipelines");
    println!("    - All in one cloud-hosted platform (no agents to deploy)");
    println!("  Connectors (200+ sources):");
    println!("    - Databases: Postgres, MySQL, MSSQL, Oracle, MongoDB, DynamoDB, Cosmos DB, Snowflake");
    println!("    - CRM: Salesforce, HubSpot, Microsoft Dynamics, Pipedrive, Zoho");
    println!("    - Marketing: Marketo, Klaviyo, Iterable, Braze, Mailchimp, ActiveCampaign");
    println!("    - Ads: Google Ads, Meta, LinkedIn, TikTok, Bing Ads, Amazon Ads");
    println!("    - Analytics: GA4, Mixpanel, Amplitude, Heap, Snowplow");
    println!("    - Support: Zendesk, Intercom, Freshdesk, Help Scout");
    println!("    - Finance: Stripe, NetSuite, QuickBooks, Xero");
    println!("    - HR: Workday, BambooHR, Greenhouse, Lever");
    println!("  Rivery Logic (orchestration — Rivery's secret weapon):");
    println!("    - DAG editor for pipeline dependencies");
    println!("    - Conditional branching: if upstream succeeded → run transformation; else → notify");
    println!("    - Parallel execution + retry policies");
    println!("    - Webhook + cron + manual triggers");
    println!("    - SQL + Python steps mixed in same DAG");
    println!("    - dbt jobs as Logic steps");
    println!("    - 'Airflow without the ops burden' positioning");
    println!("  Reverse ETL ('Action Rivers'):");
    println!("    - 100+ reverse-ETL destinations");
    println!("    - Salesforce + HubSpot + Marketo + ads + CRM + support");
    println!("    - Less polished UI vs Hightouch + Census but bundled with rest of platform");
    println!("    - Same RPU budget covers reverse-ETL — no separate license");
    println!("  Templates + Kits (the differentiator for mid-market):");
    println!("    - 'Pre-built Data Models' — Salesforce → warehouse schema with transformations included");
    println!("    - 'Kits' for common verticals: SaaS metrics, e-commerce, marketing attribution");
    println!("    - Templates reduce time-to-value vs Fivetran-only + dbt-only stack assembly");
    println!("  Rivery AI (Riverbot, 2024):");
    println!("    - Chat with your data pipelines (NL2SQL on pipeline metadata)");
    println!("    - Auto-suggest transformations + joins between sources");
    println!("    - Generate dbt models from prompts");
    println!("    - Catching up to Fivetran's AI + Coalesce AI features");
    println!("  Data Catalog + Lineage:");
    println!("    - Auto-discover schema across sources + destinations");
    println!("    - Visualize end-to-end pipeline lineage");
    println!("    - Column-level lineage (which transformation impacts which downstream field)");
    println!("    - Lighter than dedicated data catalogs (Atlan, Collibra) — bundled with platform");
    println!("  Customers: ~300+ paying customers");
    println!("            Ladbrokes, Vibes, eToro (Israeli fintech), Trax, Wibbitz, Reali, Stash, Cybereason");
    println!("            Lemonade, Wix (yes, Wix uses Rivery for internal data), Sodexo, Sysdig");
    println!("            sweet spot: mid-market companies $50M-$1B revenue with data needs but small data teams");
    println!("            especially strong: Israeli tech ecosystem, EU mid-market, fintech, mar-tech analytics");
    println!("  Critique: less mature ecosystem than Fivetran (smaller connector library, fewer integrations)");
    println!("           reverse-ETL features lag Hightouch + Census in destination coverage + polish");
    println!("           dbt integration good but most teams keep dbt Cloud separate for advanced cases");
    println!("           RPU pricing simpler than Fivetran MAR but still scales unpredictably at enterprise");
    println!("           AI features behind Fivetran + Coalesce in 2024 marketing");
    println!("           brand awareness much lower than Fivetran or dbt Labs");
    println!("  Differentiator: all-in-one ELT + transformation + orchestration + reverse-ETL on single platform + RPU pricing simplicity + pre-built data model kits — for mid-market companies wanting modern data stack without buying 4 tools");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rivery".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rivery(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rivery};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rivery"), "rivery");
        assert_eq!(basename(r"C:\bin\rivery.exe"), "rivery.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rivery.exe"), "rivery");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rivery(&["--help".to_string()], "rivery"), 0);
        assert_eq!(run_rivery(&["-h".to_string()], "rivery"), 0);
        let _ = run_rivery(&["--version".to_string()], "rivery");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rivery(&[], "rivery");
    }
}
