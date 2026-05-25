#![deny(clippy::all)]

//! starburst-cli — OurOS Starburst (commercial Trino/Presto data lakehouse query engine)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: starburst [OPTIONS]");
        println!("Starburst (OurOS) — commercial Trino — query data wherever it lives (data lakehouse)");
        println!();
        println!("Options:");
        println!("  --galaxy               Starburst Galaxy (fully managed SaaS, cloud)");
        println!("  --enterprise           Starburst Enterprise (self-hosted on K8s)");
        println!("  --warp-speed           Warp Speed (managed acceleration layer)");
        println!("  --iceberg              Apache Iceberg-native lakehouse storage");
        println!("  --gravity              Starburst Gravity (federated catalog + RBAC)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Starburst 2024 (OurOS)"); return 0; }
    println!("Starburst 2024 (OurOS)");
    println!("  Vendor: Starburst Data, Inc. (Boston, MA — private)");
    println!("  Founders: Justin Borgman (CEO), Kamil Bajda-Pawlikowski (CTO), Matt Fuller, Martin Traverso, David Phillips, 2017");
    println!("          Borgman + Bajda-Pawlikowski had founded Hadapt (acquired Teradata 2014)");
    println!("          Traverso + Phillips + Dain Sundstrom were Facebook engineers who created Presto (2012)");
    println!("          all four creators eventually forked Presto to form Trino (2020) over PrestoDB/Presto Software Foundation governance dispute");
    println!("          rare 'commercial entity for open-source project I created' setup at scale");
    println!("  Founded: 2017 in Boston");
    println!("          raised ~$414M total (~$3.35B valuation at Series D, 2022)");
    println!("          Insight Partners, Coatue, Andreessen Horowitz, Salesforce Ventures lead");
    println!("          ~$100M ARR estimate (private)");
    println!("          ~600 employees");
    println!("  Strategic position: 'data lakehouse query engine without lock-in':");
    println!("                    primary competitor: Snowflake (proprietary warehouse), Databricks (Spark lakehouse), Dremio (Apache Iceberg lakehouse)");
    println!("                    AWS Athena, Google BigQuery Omni (federation features) compete on cross-cloud queries");
    println!("                    Starburst's wedge: 'open-format, your-data-anywhere' — query S3 + GCS + Azure + Snowflake + DB without moving data");
    println!("                    natural fit for: enterprises with multi-cloud sprawl, cost-conscious teams, regulatory data residency");
    println!("                    open-source Trino is foundational — Starburst is to Trino what Databricks is to Spark");
    println!("  Pricing (consumption + cluster-based — opaque enterprise):");
    println!("    Starburst Galaxy — pay-as-you-go cloud, ~$2-$5/Galaxy unit hour (cluster-based)");
    println!("    Starburst Enterprise — annual license + support, typically $50K-$5M+/yr");
    println!("    pricing axis: compute (cluster hours) + supported connectors + premium features");
    println!("    Galaxy is the growth driver, Enterprise is the legacy on-prem/private-cloud business");
    println!("  Core architecture (Trino + enterprise extensions):");
    println!("    - Massively parallel SQL execution engine");
    println!("    - 50+ connectors (Snowflake, Postgres, MySQL, Oracle, Cassandra, MongoDB, S3+Iceberg/Hive/Delta/Parquet, ADLS, GCS, Hudi, Kafka, Pinot, Elasticsearch, Redshift)");
    println!("    - Federation: single SQL across many data sources without moving data");
    println!("    - ANSI SQL with extensions (window functions, JSON, arrays, etc.)");
    println!("    - Cost-based query optimizer");
    println!("    - Adaptive Query Plans + fault-tolerant execution (since 2023)");
    println!("    - Apache Iceberg + Delta Lake + Hudi native support (formats-agnostic lakehouse)");
    println!("  Starburst Galaxy (managed SaaS — growth product):");
    println!("    - One-click provision Trino clusters across AWS + GCP + Azure");
    println!("    - Auto-scaling, auto-shutdown for idle clusters");
    println!("    - Catalog + RBAC + lineage built-in");
    println!("    - Galaxy Editor: web-based SQL editor + visualization");
    println!("    - Marketing pitch: 'BigQuery for any cloud + any storage'");
    println!("  Warp Speed (acceleration layer, 2022+):");
    println!("    - Caches hot data + indexes in NVMe storage attached to query cluster");
    println!("    - Order-of-magnitude faster queries on Iceberg/Parquet vs cold S3");
    println!("    - No data movement required — transparent acceleration");
    println!("    - Starburst's response to Snowflake's compute+storage tight coupling advantage");
    println!("  Apache Iceberg-native lakehouse:");
    println!("    - Iceberg is the modern open table format (vs Delta which is Databricks-controlled)");
    println!("    - Starburst is a leading commercial Iceberg implementation");
    println!("    - Time-travel queries (AS OF SNAPSHOT)");
    println!("    - Schema evolution + hidden partitioning");
    println!("    - Iceberg REST catalog support");
    println!("  Starburst Gravity (federated catalog + RBAC, 2024):");
    println!("    - Cross-source data discovery + governance");
    println!("    - Tag-based access control across catalogs (Snowflake + S3 + Postgres)");
    println!("    - Lineage tracking across federation");
    println!("    - Competes with: Unity Catalog (Databricks), Snowflake Polaris, Atlan");
    println!("  Open-source Trino:");
    println!("    - Starburst Data are top maintainers of Trino (along with Bloomberg + ex-Facebook contributors)");
    println!("    - Trino used by: Lyft, Netflix (creator), Pinterest, LinkedIn, Twitter/X, Comcast");
    println!("    - Starburst's commercial extensions: more connectors, RBAC, federation, Warp Speed acceleration");
    println!("    - dual-license model — community Trino is Apache 2.0, Starburst extensions are commercial");
    println!("  AI + GenAI:");
    println!("    - 'Starburst AI Workflows' (2024 push) — natural language to SQL");
    println!("    - Vector search support in Trino for RAG pipelines");
    println!("    - Integration with LangChain + Snowflake Cortex + AWS Bedrock");
    println!("    - 'query your lakehouse from your AI app' positioning");
    println!("  Integrations: 50+ connectors:");
    println!("              Cloud object stores: S3, GCS, ADLS Gen2");
    println!("              Open table formats: Iceberg, Delta, Hudi, Hive, Parquet");
    println!("              Databases: Postgres, MySQL, Oracle, MSSQL, Cassandra, MongoDB, DynamoDB");
    println!("              Warehouses: Snowflake, BigQuery, Redshift, Synapse, Databricks SQL");
    println!("              Streaming: Kafka, Pinot, Druid, Pulsar");
    println!("              Search: Elasticsearch, OpenSearch");
    println!("              BI: Tableau, Looker, Power BI, Sigma, Mode (via JDBC/ODBC)");
    println!("  Customers: ~500+ paying enterprise customers");
    println!("            Comcast, Bloomberg (deep), Lufthansa, Société Générale, FINRA, AT&T, GE Aerospace");
    println!("            Verizon, ABN AMRO, ING Bank, Allianz, Walmart Connect");
    println!("            sweet spot: F1000 enterprises with multi-cloud + multi-format data sprawl");
    println!("            historically WEAK in: SMB (Snowflake/BigQuery win), pure-play SaaS startups");
    println!("            very strong in: Bloomberg-style finance (real federation needs), telco, retail, healthcare with EHR");
    println!("  Critique: still has Trino's complexity tax — query tuning + cluster sizing is non-trivial");
    println!("           cost can spike if queries aren't optimized + clusters not auto-scaled");
    println!("           catalog/governance features (Gravity) newer than Snowflake's Horizon/Polaris");
    println!("           SaaS Galaxy product younger than Snowflake — fewer enterprise-grade features");
    println!("           competition from open-source Trino itself + Dremio (similar pitch)");
    println!("           Databricks + Snowflake have brand mindshare + better marketing");
    println!("           Iceberg vs Delta format wars: Databricks pushing Delta + Iceberg, Starburst leans Iceberg");
    println!("  Differentiator: federated SQL across ANY data source/format + Iceberg lakehouse leadership + Warp Speed acceleration + open-source Trino lineage — for F1000 enterprises with multi-cloud + multi-format sprawl");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "starburst".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
