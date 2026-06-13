#![deny(clippy::all)]

//! atlan-cli — SlateOS Atlan (modern active metadata + data catalog, Bangalore)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_atlan(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: atlan [OPTIONS]");
        println!("Atlan (SlateOS) — modern data catalog + active metadata platform");
        println!();
        println!("Options:");
        println!("  --catalog              Browse data catalog (tables, dashboards, ML models)");
        println!("  --lineage              Column-level lineage across stack");
        println!("  --glossary             Business glossary + data contracts");
        println!("  --governance           Access controls + PII tagging");
        println!("  --dbt                  dbt integration (models, tests, docs)");
        println!("  --snowflake            Snowflake-native cataloging");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Atlan 2024 (SlateOS)"); return 0; }
    println!("Atlan 2024 (SlateOS) — Modern Data Catalog");
    println!("  Vendor: Atlan, Inc. (Delaware HQ, engineering Bangalore + remote-first)");
    println!("  Founders: Prukalpa Sankar + Varun Banka, 2018-2020");
    println!("          previously ran SocialCops (data-for-good consultancy, 2012)");
    println!("          built internal data tooling for UN SDG project + Indian gov reports");
    println!("          spun those tools out as Atlan in 2020 — 'data team productivity platform'");
    println!("          Prukalpa: World Economic Forum Young Global Leader, prolific data Substack");
    println!("          remote-first since founding — team across 15+ countries");
    println!("  Funding: ~$206M total raised through Series C (May 2024)");
    println!("         Series C May 2024: $105M led by Meritech + GIC, ~$750M valuation");
    println!("         Series B 2022: $50M led by Insight Partners + Salesforce Ventures");
    println!("         Series A 2021: $16M led by Insight Partners");
    println!("         earlier: Sequoia India, Waterbridge, angels (Postman + Freshworks founders)");
    println!("  ARR: estimated $50M-100M+ ARR (growing fast, not public)");
    println!("  Strategic position: 'data catalog 3.0' — third wave after Informatica + Collibra/Alation:");
    println!("                    1st gen: Informatica/IBM/SAP (enterprise on-prem, $$$)");
    println!("                    2nd gen: Collibra + Alation (2010s, cloud-aware, still enterprise-heavy)");
    println!("                    3rd gen: Atlan + Castor + Select Star (modern data stack-native, fast onboarding)");
    println!("                    Atlan's wedge: 'data team product' UX + dbt/Snowflake/Looker-native + asset-based pricing");
    println!("                    Galaxy-style canvas + Notion-style docs + Slack-style collaboration");
    println!("                    bet: every modern data team needs a catalog, and old vendors are unusable");
    println!("  Pricing (consumption + tier):");
    println!("    Free trial — 14 days full access");
    println!("    Starter — ~$5K-15K/yr for small teams (per-user pricing)");
    println!("    Pro — $50K-200K/yr typical mid-market");
    println!("    Enterprise — $250K-2M+/yr (SSO, SCIM, on-prem option, audit logs, dedicated CSM)");
    println!("    asset-based + user-based hybrid pricing model");
    println!("  Core platform:");
    println!("    - Active metadata: bidirectional sync with all data tools (not just passive scraping)");
    println!("    - Open API + webhooks: query metadata, push annotations from anywhere");
    println!("    - Column-level lineage across Snowflake/BigQuery/Redshift/Databricks → dbt → BI tools");
    println!("    - Asset 360 view: SQL, owners, freshness, quality, lineage, business context all in one page");
    println!("    - Slack + Teams integration: data context follows the conversation");
    println!("    - Atlan Playbooks: no-code automation (auto-tag PII, alert on schema drift, propagate ownership)");
    println!("  Connectors (60+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, ClickHouse, Postgres");
    println!("    - Transformation: dbt Cloud + Core (deep integration — Atlan was an early dbt partner)");
    println!("    - BI: Looker, Tableau, Power BI, Mode, Sigma, Hex, Metabase, Superset");
    println!("    - Orchestration: Airflow, Dagster, Prefect (capture pipeline lineage)");
    println!("    - ML/AI: MLflow, Weights & Biases (track model→data dependencies)");
    println!("    - Streaming: Kafka, Confluent (track topic schemas + consumers)");
    println!("    - Quality: Monte Carlo, Soda, Great Expectations (surface test results inline)");
    println!("    - Reverse ETL: Hightouch, Census (audience activation lineage)");
    println!("  dbt integration (the strongest in market):");
    println!("    - Auto-import all dbt models + tests + docs + exposures");
    println!("    - dbt model page: SQL + lineage + tests + docs + owners + downstream BI");
    println!("    - Push back to dbt: ownership + tags + descriptions sync to dbt YAML");
    println!("    - dbt Mesh + multi-project lineage support");
    println!("    - dbt Labs (Tristan Handy + team) publicly recommends Atlan as catalog of choice");
    println!("  Snowflake-native:");
    println!("    - Snowflake Partner Network: Snowflake Ready Technology + Powered by Snowflake");
    println!("    - Snowflake Horizon governance integration (2024)");
    println!("    - Snowflake Native App available (run Atlan inside customer Snowflake)");
    println!("    - Account_usage view ingestion: query history, access patterns, popularity scores");
    println!("  Governance + Compliance:");
    println!("    - PII auto-classification (ML-based scanning of column samples)");
    println!("    - Data contracts (formal producer→consumer agreements)");
    println!("    - Tag-based access policies (push to Snowflake row/column policies)");
    println!("    - SOC 2 Type 2 + GDPR + HIPAA + ISO 27001");
    println!("    - Audit logs + change history");
    println!("    - SCIM provisioning + SSO (Okta, Azure AD, Google)");
    println!("  Active metadata thesis:");
    println!("    - Gartner coined 'Active Metadata' category in 2021 — Atlan leans into it");
    println!("    - vs passive catalogs (Collibra/Alation): metadata is read + written bidirectionally");
    println!("    - Atlan annotations flow back to dbt YAML, Looker LookML, Snowflake comments");
    println!("    - 'metadata at the speed of the modern data stack' — Prukalpa's talking point");
    println!("  Atlan CLI usage:");
    println!("    atlan login");
    println!("    atlan asset list --type Table --connection snowflake");
    println!("    atlan asset get --qualified-name 'default/snowflake/prod/raw/customers'");
    println!("    atlan asset annotate --qualified-name '...' --description 'PII: contains email'");
    println!("    atlan lineage --asset 'customers' --depth 3");
    println!("    atlan playbook run --name 'pii-classifier'");
    println!("  Customers (~500+ paying):");
    println!("    - Postman (early reference customer — fellow Bangalore unicorn)");
    println!("    - Plaid, Cisco, Autodesk, Unilever, Nasdaq, Aviva, Ralph Lauren");
    println!("    - North, Tide, Klarna (heavy EU presence)");
    println!("    - HubSpot, Disney+ Hotstar, Heineken, WeWork, Western Digital");
    println!("    - typical buyer: data platform/engineering lead at company with 20+ data team");
    println!("    - sweet spot: dbt + Snowflake/Databricks + Looker shop with growing data org");
    println!("  Atlan vs competitors:");
    println!("    - vs Collibra: Atlan faster onboarding (hours vs months), better dbt/Snowflake-native UX");
    println!("    - vs Alation: similar — Atlan's UX is the bet, Alation has more enterprise feature breadth");
    println!("    - vs data.world: data.world more semantic/knowledge graph-leaning, Atlan more workflow");
    println!("    - vs Select Star: similar generation, Atlan larger + more enterprise traction");
    println!("    - vs Castor (now CastorDoc, acquired by Coalesce 2024): direct competitor, Atlan bigger");
    println!("    - vs DataHub (LinkedIn OSS + Acryl commercial): DataHub more eng-first, Atlan more business-friendly");
    println!("    - vs Microsoft Purview / Google Dataplex / AWS DataZone: cloud-native catalogs, free w/ cloud — Atlan justifies premium via UX + cross-cloud");
    println!("  Recent moves:");
    println!("    - AtlanAI (2024): natural language metadata search + auto-documentation via LLM");
    println!("    - Iceberg cataloging support (2024)");
    println!("    - Atlan for Apache Airflow expanded integration");
    println!("    - Asia-Pacific expansion + EU data residency (Frankfurt region)");
    println!("  Critique: still defining 'active metadata' category — must educate market");
    println!("           expensive at enterprise scale ($250K+/yr typical floor)");
    println!("           overlap with dbt Cloud's own catalog features (dbt Explorer) — dbt could disintermediate");
    println!("           Snowflake Horizon governance + Databricks Unity Catalog compete from the warehouse side");
    println!("           catalog category historically slow ROI — buyers struggle to justify");
    println!("           less mature than Collibra for highly-regulated industries (banking, pharma)");
    println!("           remote-first culture + India HQ: occasional time-zone friction for US enterprise sales");
    println!("  Differentiator: dbt-native + Snowflake-native + active metadata thesis + Notion-style UX + remote-first culture — the modern-data-stack catalog of choice, beating legacy Collibra/Alation on speed-to-value");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "atlan".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_atlan(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_atlan};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/atlan"), "atlan");
        assert_eq!(basename(r"C:\bin\atlan.exe"), "atlan.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("atlan.exe"), "atlan");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_atlan(&["--help".to_string()], "atlan"), 0);
        assert_eq!(run_atlan(&["-h".to_string()], "atlan"), 0);
        let _ = run_atlan(&["--version".to_string()], "atlan");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_atlan(&[], "atlan");
    }
}
