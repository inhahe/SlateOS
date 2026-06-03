#![deny(clippy::all)]

//! alation-cli — OurOS Alation (data catalog category leader, Redwood City)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_alation(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alation [OPTIONS]");
        println!("Alation (OurOS) — enterprise data catalog (the category creator)");
        println!();
        println!("Options:");
        println!("  --catalog              Browse cataloged data assets");
        println!("  --search               Natural language search");
        println!("  --query-log            Query log ingestion for popularity scoring");
        println!("  --governance           Alation Data Governance App");
        println!("  --intelligence         Alation Anywhere AI assistant");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Alation 2024.3 (OurOS)"); return 0; }
    println!("Alation 2024.3 (OurOS) — Enterprise Data Catalog");
    println!("  Vendor: Alation, Inc. (Redwood City, CA)");
    println!("  Founders: Satyen Sangani (CEO) + Aaron Kalb + Feng Niu + Venky Ganti, 2012");
    println!("          Satyen: ex-Oracle, ex-McKinsey + Stanford CS + Goldman Sachs analyst");
    println!("          Aaron: ex-Apple Siri team");
    println!("          credit for popularizing the term 'data catalog' as a software category");
    println!("          Stanford research roots: machine learning on query logs to surface relevant data");
    println!("  Funding: ~$340M total through Series E (2022)");
    println!("         Series E Aug 2022: $123M led by Thoma Bravo + Sanabil + Riverwood + Costanoa");
    println!("         valuation ~$1.7B post-Series E");
    println!("         Series D Jun 2019: $50M led by Salesforce Ventures");
    println!("         earlier: Costanoa, Icon Ventures, Sapphire Ventures, Andreessen Horowitz, Data Collective");
    println!("  ARR: estimated $150M+ (private)");
    println!("  Strategic position: 'data catalog 2.0 — query-log-driven' (Alation's wedge):");
    println!("                    pitch: 'people use data they trust — Alation builds trust'");
    println!("                    contrast: 1st-gen Informatica/IBM (top-down, manual)");
    println!("                                              Alation (query-log-driven, learns from usage)");
    println!("                    primary competitor: Collibra (more governance-heavy), Atlan (modern stack)");
    println!("                    secondary: data.world, Microsoft Purview, Google Dataplex, AWS DataZone");
    println!("                    Gartner Magic Quadrant: consistently in the Leader quadrant for catalogs");
    println!("                    Forrester Wave Leader for data governance solutions");
    println!("  Pricing (enterprise sales-led, no free tier):");
    println!("    Starter — ~$50K/yr (small enterprise, limited sources)");
    println!("    Standard — $100K-300K/yr (mid-market)");
    println!("    Enterprise — $250K-2M+/yr (large enterprise, SSO/audit/multi-region)");
    println!("    Cloud — Alation Cloud Service (managed)");
    println!("    on-prem option still available (banks + government)");
    println!("  Core product (the catalog):");
    println!("    - Auto-discovers tables, columns, dashboards, queries, users");
    println!("    - Query log ingestion: 'who queries what, when, how often'");
    println!("    - Popularity scoring: 'this table is used by 47 people daily' — trust signal");
    println!("    - TrustCheck: warnings on stale/deprecated/uncertified data");
    println!("    - Crowdsourced annotations + business glossary");
    println!("    - Stewardship workflows + ownership routing");
    println!("  Alation Compose (SQL editor):");
    println!("    - In-catalog SQL editor with auto-complete + lineage hints");
    println!("    - Helps analysts write queries using catalog context");
    println!("    - Differentiator vs pure catalogs — Alation is also a workspace");
    println!("  Alation Anywhere (AI assistant, 2024):");
    println!("    - GenAI-powered natural language search over catalog");
    println!("    - Auto-generated table/column descriptions via LLM");
    println!("    - Snowflake Cortex + GPT-4 + Anthropic Claude options");
    println!("    - 'AI-ready data' positioning for enterprise GenAI initiatives");
    println!("  Data Governance App (separate paid module):");
    println!("    - Policy management + access requests");
    println!("    - Data domain ownership + stewardship workflows");
    println!("    - Regulatory mapping (GDPR, CCPA, HIPAA, SOX)");
    println!("    - Compete head-on with Collibra's strength area");
    println!("  Data Quality App (2023):");
    println!("    - Built-in data quality monitoring");
    println!("    - Compete with Monte Carlo + Anomalo + Soda");
    println!("    - Acquired some quality tech, building rest in-house");
    println!("  Connectors (100+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Oracle, Teradata, Netezza");
    println!("    - BI: Tableau, Power BI, Looker, Qlik, MicroStrategy, Cognos");
    println!("    - Hadoop: Cloudera, Hortonworks (still has banking customers on these)");
    println!("    - Streaming: Kafka, Confluent (limited)");
    println!("    - Cloud: AWS (Glue catalog ingestion), Azure, GCP catalog integration");
    println!("    - ETL: Informatica, Talend, dbt, Airflow, Fivetran");
    println!("  Alation CLI usage:");
    println!("    alation login --server alation.company.com");
    println!("    alation search 'customer revenue'");
    println!("    alation table get prod.snowflake.fact_orders");
    println!("    alation steward assign --table prod.fact_orders --user alice");
    println!("    alation policy apply --domain pii --policy mask-email");
    println!("  Customers (450+ paying):");
    println!("    - Pfizer, AstraZeneca, Mizuho, NASDAQ, Daimler, Deutsche Telekom");
    println!("    - eBay, Cisco, Salesforce, American Family Insurance");
    println!("    - Fortune 500 dominant: 40%+ of customer base is Fortune 1000");
    println!("    - heavy in: financial services, pharma, manufacturing, government");
    println!("    - long sales cycles (6-12 months) but high stickiness");
    println!("  Alation vs Collibra (the catalog war):");
    println!("    - Both 12+ year-old enterprise catalog vendors");
    println!("    - Alation: stronger UX, query-log-driven, more analyst-friendly");
    println!("    - Collibra: deeper governance + workflow, more compliance-heavy");
    println!("    - Both sell to similar Fortune 500 buyers — frequently compete head-to-head");
    println!("  Critique: slow, heavy enterprise software — months to deploy");
    println!("           UX dated vs modern catalogs (Atlan, Select Star, Castor)");
    println!("           Atlan winning modern-data-stack greenfield deals");
    println!("           Snowflake Horizon + Databricks Unity Catalog + cloud-native catalogs threaten from below");
    println!("           pricing high ($100K+ floor) — locks out mid-market");
    println!("           on-prem deployments require significant infrastructure");
    println!("           GenAI race: Alation Anywhere good but Atlan AI + Collibra AI competing");
    println!("           IPO repeatedly delayed — private valuation under pressure post-2022");
    println!("  Differentiator: original 'data catalog' category creator + query-log-driven trust scoring + Compose SQL editor + Fortune 500 install base + Alation Anywhere AI — the enterprise catalog of choice for regulated industries");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alation".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_alation(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_alation};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/alation"), "alation");
        assert_eq!(basename(r"C:\bin\alation.exe"), "alation.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("alation.exe"), "alation");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_alation(&["--help".to_string()], "alation"), 0);
        assert_eq!(run_alation(&["-h".to_string()], "alation"), 0);
        assert_eq!(run_alation(&["--version".to_string()], "alation"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_alation(&[], "alation"), 0);
    }
}
