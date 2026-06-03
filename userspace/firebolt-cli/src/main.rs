#![deny(clippy::all)]

//! firebolt-cli — OurOS Firebolt (cloud DW for data-intensive apps + sub-second BI, Tel Aviv/NYC, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: firebolt [OPTIONS]");
        println!("Firebolt (OurOS) — cloud data warehouse for data-intensive apps + sub-second analytics");
        println!();
        println!("Options:");
        println!("  --engines              Compute engines (independent scalable per-workload)");
        println!("  --sparse-indexes       Sparse indexes + aggregating indexes");
        println!("  --semi-structured      JSON + arrays + nested types");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Firebolt 2024 (OurOS) — firebolt CLI 2.x"); return 0; }
    println!("Firebolt 2024 (OurOS) — Cloud Data Warehouse for Data-Intensive Apps");
    println!("  Vendor: Firebolt Analytics, Inc. (Tel Aviv, Israel + NYC — private)");
    println!("  Founders: Eldad Farkash + Saar Bitner + Ariel Yaroshevich + Eran Levy, 2019");
    println!("          Eldad Farkash: ex-CTO + co-founder Sisense (BI platform)");
    println!("          'Firebolt' = aimed at fast-as-lightning analytics + interactive apps");
    println!("          Initial vision: 'data warehouse fast enough for data-intensive apps'");
    println!("          Tel Aviv DB engineering heritage (similar to ScyllaDB, Snyk, Lightbits)");
    println!("  Funding:");
    println!("         Total raised: ~$269M");
    println!("         Series C Jan 2022: $100M at $1.4B valuation");
    println!("         Layoffs in 2023 (industry-wide cooling), refocused product");
    println!("         Investors: Sapphire Ventures, Zeev Ventures, Bessemer, Insight Partners");
    println!("  Strategic position: 'cloud DW for data-intensive apps + sub-second BI — Snowflake too slow for app backends':");
    println!("                    pitch: 'sub-second analytical queries for customer-facing dashboards + data products'");
    println!("                    target: SaaS companies building data products + customer dashboards");
    println!("                    primary competitor: Snowflake (positioned as 'too slow for app backends')");
    println!("                    secondary: ClickHouse, Druid, Pinot, Rockset, StarRocks");
    println!("                    Firebolt's wedge: sparse + aggregating indexes + sub-second p99 + per-workload engines");
    println!("                    target use case: power Mode dashboards, Looker embedded analytics, customer-facing analytics");
    println!("                    challenge: ClickHouse is open source and similarly fast for many workloads");
    println!("  Pricing (consumption + transparent):");
    println!("    Free trial: $200 credit + free tier");
    println!("    Engine pricing: $0.65-$10/hr per engine (varies by spec — XS to XL)");
    println!("    Storage: $0.023/GB-month (decoupled from compute)");
    println!("    Pause engines when idle (no compute charge)");
    println!("    typically positioned as cheaper than Snowflake for sustained interactive workloads");
    println!("    transparent engine sizing (XS/S/M/L/XL with vCPU/RAM spec disclosed)");
    println!("  Architecture (sub-second analytics):");
    println!("    - Decoupled storage + compute (S3-backed Tabular store)");
    println!("    - Multiple independent engines per database (workload isolation)");
    println!("    - Engines = compute clusters of various sizes");
    println!("    - Sparse indexes: covering subset of data, accelerate range scans");
    println!("    - Aggregating indexes: precomputed roll-ups, accelerate dashboards");
    println!("    - Join indexes: precomputed join keys");
    println!("    - Columnar storage with F3 file format (Firebolt-proprietary)");
    println!("    - Vectorized SIMD execution");
    println!("    - PostgreSQL-compatible SQL");
    println!("  Product portfolio:");
    println!("    1. Firebolt Cloud DW (the core):");
    println!("       - Multi-cloud (AWS, soon Azure/GCP)");
    println!("       - Engines per workload (BI, ETL, ad-hoc, real-time)");
    println!("       - Auto-pause idle engines (cost optimization)");
    println!("       - F3 columnar storage on S3");
    println!("    2. Index types (the differentiator):");
    println!("       - Primary index: data sort order (zone maps)");
    println!("       - Aggregating index: materialized roll-ups");
    println!("       - Sparse index: subset of values, range pruning");
    println!("       - Join index: precomputed join keys");
    println!("       - Combination: queries hit multiple indexes — sub-second result");
    println!("    3. Engines (independent compute):");
    println!("       - Read engines + write engines separate");
    println!("       - Multiple engines per database, sized independently");
    println!("       - Designed for: dashboards (read) + ETL (write) + ad-hoc (mixed)");
    println!("    4. Semi-structured data:");
    println!("       - JSON columns, arrays, nested types");
    println!("       - Lambda functions on arrays");
    println!("       - Tagged unions");
    println!("    5. Continuous ingestion:");
    println!("       - COPY FROM S3 (Parquet, CSV, JSON)");
    println!("       - External tables for federated queries");
    println!("       - Stream ingest via Kafka connector");
    println!("    6. User-Defined Functions (UDFs):");
    println!("       - SQL UDFs + remote UDFs (REST/HTTP)");
    println!("       - Python UDFs (preview 2024)");
    println!("    7. Workload management:");
    println!("       - Engines isolate noisy neighbors");
    println!("       - Cost predictable by engine size");
    println!("    8. PostgreSQL-compatible wire protocol:");
    println!("       - Use psql, JDBC, ODBC PostgreSQL drivers");
    println!("       - Mostly drop-in for BI tool connections");
    println!("    9. Embedded analytics integrations:");
    println!("       - Direct integrations with Cube.js, Mode, Hex, Looker");
    println!("       - 'Powering customer-facing dashboards' pitch");
    println!("  The 'data-intensive apps' positioning:");
    println!("    - Customer-facing dashboards need sub-second latency");
    println!("    - Snowflake/BigQuery latency typically 1-5s warm, much more cold");
    println!("    - Firebolt targets sub-200ms p99 for indexed queries");
    println!("    - Examples: SaaS analytics tabs, ecommerce reporting, marketing dashboards");
    println!("    - Compete with: ClickHouse (open source), Druid, Pinot, Rockset");
    println!("    - SaaS founders pick Firebolt when self-managed ClickHouse is too much work");
    println!("  Integrations:");
    println!("    - firebolt CLI (Python)");
    println!("    - SDKs: Python (firebolt-sdk), JS/TS, Java, Go");
    println!("    - PostgreSQL JDBC/ODBC drivers (wire-compatible)");
    println!("    - dbt-firebolt adapter");
    println!("    - Airflow + Dagster providers");
    println!("    - Tableau, Power BI, Looker, Metabase, Cube.js, Mode, Hex");
    println!("    - Spark connector");
    println!("    - Direct S3 / Parquet integration");
    println!("  Firebolt CLI usage:");
    println!("    firebolt configure                                       # set credentials");
    println!("    firebolt database create --name=my_db --region=us-east-1");
    println!("    firebolt engine create --name=my_engine --database=my_db --spec=S --scale=2");
    println!("    firebolt engine start --name=my_engine --database=my_db");
    println!("    psql 'host=hostname.firebolt.io port=5432 dbname=my_db user=me'");
    println!("    CREATE TABLE sales (id INT, region TEXT, amount DOUBLE);");
    println!("    CREATE AGGREGATING INDEX sales_agg ON sales (region, SUM(amount));");
    println!("    CREATE SPARSE INDEX sales_sparse ON sales (id, region);");
    println!("    COPY INTO sales FROM 's3://my-bucket/sales/*.parquet' WITH (TYPE=PARQUET);");
    println!("    SELECT region, SUM(amount) FROM sales WHERE id > 1000 GROUP BY region;");
    println!("    firebolt query --database=my_db --engine=my_engine --query='SELECT...'");
    println!("  Customers (SaaS + adtech + data products):");
    println!("    - SimilarWeb (web analytics)");
    println!("    - AppsFlyer (mobile attribution)");
    println!("    - Bigabid (adtech)");
    println!("    - Various SaaS + adtech analytics platforms");
    println!("    - Use case: powering customer-facing dashboards at SaaS companies");
    println!("    - 100s of customer accounts (private metrics)");
    println!("  Critique: ClickHouse free + nearly as fast = formidable competition");
    println!("           Snowflake's continued speed improvements close the gap");
    println!("           young company (founded 2019, 2023 layoffs)");
    println!("           limited multi-cloud (AWS-focused)");
    println!("           ecosystem smaller than Snowflake/BigQuery");
    println!("           AI/vector features behind newer competitors (SingleStore, Pinecone)");
    println!("           index management (sparse/aggregating) requires expertise");
    println!("           private + closed-source = no community contribution moat vs ClickHouse");
    println!("  Differentiator: cloud DW purpose-built for data-intensive apps + customer-facing dashboards + sub-200ms p99 query latency + sparse/aggregating/join indexes (unique combo) + multiple independent engines per database (workload isolation) + auto-pause idle engines + PostgreSQL wire-compatible + F3 columnar format + vectorized SIMD execution + ex-Sisense founders (Eldad Farkash et al) + $269M raised + $1.4B valuation + Tel Aviv DB engineering heritage + SimilarWeb/AppsFlyer customers — the cloud data warehouse for SaaS companies building customer-facing analytics where Snowflake/BigQuery are too slow for embedded dashboards");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "firebolt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/firebolt"), "firebolt");
        assert_eq!(basename(r"C:\bin\firebolt.exe"), "firebolt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("firebolt.exe"), "firebolt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fb(&["--help".to_string()], "firebolt"), 0);
        assert_eq!(run_fb(&["-h".to_string()], "firebolt"), 0);
        assert_eq!(run_fb(&["--version".to_string()], "firebolt"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fb(&[], "firebolt"), 0);
    }
}
