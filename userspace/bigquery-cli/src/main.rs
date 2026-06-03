#![deny(clippy::all)]

//! bigquery-cli — OurOS Google BigQuery (serverless petabyte-scale data warehouse, part of GCP, NASDAQ:GOOGL)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bigquery [OPTIONS]");
        println!("BigQuery (OurOS) — serverless petabyte-scale data warehouse (part of GCP)");
        println!();
        println!("Options:");
        println!("  --omni                 BigQuery Omni (run queries on AWS S3 + Azure Blob)");
        println!("  --ml                   BigQuery ML (SQL-driven ML models)");
        println!("  --bi-engine            BI Engine (in-memory analysis acceleration)");
        println!("  --data-transfer        Data Transfer Service");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BigQuery 2024 (OurOS) — bq CLI 2.x (gcloud bigquery)"); return 0; }
    println!("BigQuery 2024 (OurOS) — Serverless Petabyte-Scale Data Warehouse");
    println!("  Vendor: Google Cloud (part of Alphabet/Google — NASDAQ:GOOGL/GOOG since 2004)");
    println!("  Origins: Dremel internal paper (Google 2010) → BigQuery launched 2010 → GA 2012");
    println!("          Dremel paper: 'Dremel: Interactive Analysis of Web-Scale Datasets' (Sergey Melnik et al, 2010)");
    println!("          One of the most influential database papers of the 2010s");
    println!("          Designed for: thousands of nodes scanning trillions of rows in seconds");
    println!("          Now part of Google Cloud Platform (GCP) — Thomas Kurian (CEO since 2018)");
    println!("  Public market (NASDAQ:GOOGL — Alphabet parent):");
    println!("         Alphabet FY2024 revenue: ~$350B+ (+12-15% YoY)");
    println!("         Google Cloud revenue: ~$43B+ FY2024 (+30-35% YoY)");
    println!("         GCP profitable from 2023 onwards (after years of losses)");
    println!("         BigQuery: estimated ~$5-10B annual revenue inside GCP");
    println!("         BigQuery is GCP's strongest/most-loved product");
    println!("         Alphabet market cap: ~$2T+ range");
    println!("  Strategic position: 'serverless analytics — separation of storage + compute, scale to petabytes':");
    println!("                    pitch: 'no clusters to manage, pay for what you scan, query petabytes in seconds'");
    println!("                    target: enterprises + dev-teams running analytics workloads of any size");
    println!("                    primary competitor: Snowflake, Databricks SQL, Redshift, Synapse");
    println!("                    secondary: Athena (AWS), Trino/Presto self-hosted");
    println!("                    BigQuery's wedge: truly serverless (no warehouse sizing), built on Google's Borg + Colossus");
    println!("                    'Dremel + Colossus' architectural foundation = hard for competitors to replicate");
    println!("                    GA4 Analytics adoption => widely-adopted BigQuery on-ramp");
    println!("  Pricing (on-demand or flat-rate):");
    println!("    On-demand: $5/TB scanned (first 1 TB/month free)");
    println!("    Storage: $0.02/GB-month (active) / $0.01/GB-month (long-term)");
    println!("    Editions (flat-rate, 2023+): Standard $0.04/slot-hr, Enterprise $0.06, Enterprise Plus $0.10");
    println!("    Streaming inserts: $0.01 per 200MB");
    println!("    BigQuery ML training: scanned bytes + slot consumption");
    println!("    BI Engine: $0.04/GB-hour");
    println!("    typically cheaper than Snowflake for sporadic/spiky workloads");
    println!("    typically more expensive than Snowflake for sustained large workloads (without editions)");
    println!("  Architecture (the Dremel inheritance):");
    println!("    - Columnar storage (Capacitor format)");
    println!("    - Storage on Colossus (Google's distributed file system)");
    println!("    - Compute via Dremel-derived query engine (Borg-orchestrated)");
    println!("    - Separation of storage + compute = scale independently");
    println!("    - Tree-distribution query plan (Dremel pattern)");
    println!("    - Network: Jupiter (Google's data center fabric, Petabit/s capacity)");
    println!("    - Underlying infra is same as Google Search / YouTube");
    println!("  Product portfolio:");
    println!("    1. BigQuery (the core):");
    println!("       - SQL:2011-compatible standard SQL");
    println!("       - JSON + nested + repeated types (BigQuery legacy)");
    println!("       - Geographic / GIS functions");
    println!("       - Time-travel queries (7 days, point-in-time recovery)");
    println!("       - Multi-region datasets (US, EU)");
    println!("    2. BigQuery ML (the SQL-ML approach):");
    println!("       - Train ML models with SQL (CREATE MODEL ...)");
    println!("       - Linear/logistic regression, k-means, ARIMA, DNNs, boosted trees, AutoML");
    println!("       - Imported TensorFlow / Vertex AI integration");
    println!("       - Gemini integration for SQL-based prompts (2024)");
    println!("    3. BigQuery Omni (multi-cloud):");
    println!("       - Query data in AWS S3 + Azure Blob without moving");
    println!("       - Uses BigQuery interface on Anthos clusters in AWS/Azure");
    println!("       - Reduces egress costs for multi-cloud");
    println!("    4. BI Engine (in-memory acceleration):");
    println!("       - Sub-second BI queries with caching");
    println!("       - Looker + Looker Studio + Tableau acceleration");
    println!("    5. Data Transfer Service:");
    println!("       - Scheduled imports from GA, YouTube, Salesforce, S3, etc.");
    println!("       - 100+ source connectors");
    println!("    6. Streaming insertions + Storage Write API:");
    println!("       - Real-time row-level inserts");
    println!("       - Storage Write API (newer, cheaper, exactly-once semantics)");
    println!("    7. BigQuery DataFrames + Python notebooks:");
    println!("       - Pandas-like API backed by BigQuery");
    println!("       - Run Python ML at BigQuery scale");
    println!("    8. Data clean rooms (privacy + secure sharing):");
    println!("       - Cross-org secure analytics without data movement");
    println!("       - SQL-based privacy-preserving joins");
    println!("    9. BigLake (lakehouse):");
    println!("       - Query Iceberg/Hudi/Delta tables on Cloud Storage");
    println!("       - Open-format lakehouse with BigQuery semantics");
    println!("    10. Search indexes + vector search:");
    println!("       - Text search (CONTAINS_SUBSTR)");
    println!("       - VECTOR_SEARCH for embedding-based RAG (2024)");
    println!("  GA4 + BigQuery (the on-ramp):");
    println!("    - Google Analytics 4 free export to BigQuery (universal in 2024)");
    println!("    - Brought millions of marketing teams to BigQuery");
    println!("    - 'GA4 + BQ + Looker Studio' = the new marketing analytics stack");
    println!("    - Replaced Universal Analytics (sunset July 2023)");
    println!("  Gemini integration (2024 AI bet):");
    println!("    - SQL generation from natural language");
    println!("    - Insights generation from query results");
    println!("    - 'Duet AI for BigQuery' / 'Gemini for BigQuery'");
    println!("    - ML.GENERATE_TEXT functions calling Gemini from SQL");
    println!("    - Vertex AI integration for embeddings + RAG");
    println!("  Integrations:");
    println!("    - bq CLI (legacy) + gcloud bigquery (modern)");
    println!("    - SDKs: Python, JS, Go, Java, .NET, Ruby, PHP, C#");
    println!("    - dbt-bigquery (dbt adapter, very popular)");
    println!("    - Airflow BigQueryOperator");
    println!("    - Looker + Looker Studio (Google-owned BI)");
    println!("    - Tableau, Power BI, Mode, Hex via JDBC/ODBC");
    println!("    - Dataflow (Beam) + Dataproc (Spark) bidirectional");
    println!("    - Pub/Sub for streaming ingestion");
    println!("  BigQuery CLI usage:");
    println!("    bq query --nouse_legacy_sql 'SELECT COUNT(*) FROM `project.dataset.table`'");
    println!("    bq load --source_format=PARQUET dataset.table gs://bucket/data.parquet");
    println!("    bq extract --destination_format=AVRO dataset.table gs://bucket/out.avro");
    println!("    bq mk -d --location=US my_dataset");
    println!("    bq mk -t my_dataset.my_table schema.json");
    println!("    bq ls -j --max_results=10                                # list recent jobs");
    println!("    bq show --schema dataset.table");
    println!("    bq cp dataset.source_table dataset.dest_table");
    println!("    bq ml predict my_model 'SELECT * FROM data'");
    println!("    gcloud bigquery jobs list");
    println!("  Customers (massive scale):");
    println!("    - Spotify (recommendation logs)");
    println!("    - Twitter (back when public)");
    println!("    - The New York Times");
    println!("    - HSBC, BBVA, Banco Santander");
    println!("    - Wayfair, Home Depot, Walmart (some)");
    println!("    - Snap, Pinterest");
    println!("    - 5,000+ paying customers");
    println!("    - 10+ exabytes scanned daily across all customers");
    println!("  Critique: Snowflake's multi-cloud + better SQL semantics challenge BigQuery");
    println!("           on-demand $5/TB pricing surprises customers on large scans");
    println!("           Editions model (2023) addressed cost predictability");
    println!("           lock-in concern: deeply tied to GCP, harder to migrate off");
    println!("           Standard SQL transition (2018) was disruptive — legacy SQL still lingers in docs");
    println!("           BI Engine limits per project can be restrictive");
    println!("           streaming insertion legacy expensive; Storage Write API addresses");
    println!("           Google's customer support reputation weaker than Snowflake's");
    println!("  Differentiator: Dremel paper architecture (Google internal since 2006) + Colossus distributed storage + Jupiter petabit/s network = truly serverless petabyte-scale analytics + BigQuery ML (SQL-trained models) + Gemini/Duet AI integration + BigQuery Omni multi-cloud query + GA4 free export brought millions of marketing teams + BigLake open-format lakehouse + Vertex AI ML integration + part of GCP with $43B+ revenue +30-35% growth — the original serverless cloud data warehouse, built on the same infrastructure as Google Search, used at petabyte scale by Spotify/Twitter/NYTimes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bigquery".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bq(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bigquery"), "bigquery");
        assert_eq!(basename(r"C:\bin\bigquery.exe"), "bigquery.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bigquery.exe"), "bigquery");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bq(&["--help".to_string()], "bigquery"), 0);
        assert_eq!(run_bq(&["-h".to_string()], "bigquery"), 0);
        assert_eq!(run_bq(&["--version".to_string()], "bigquery"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bq(&[], "bigquery"), 0);
    }
}
