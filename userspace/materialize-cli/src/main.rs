#![deny(clippy::all)]

//! materialize-cli — Slate OS Materialize (streaming SQL via incremental view maintenance, NYC, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: materialize [OPTIONS]");
        println!("Materialize (Slate OS) — streaming SQL database with incremental view maintenance");
        println!();
        println!("Options:");
        println!("  --materialized-views   Always-fresh materialized views (key innovation)");
        println!("  --sources              Kafka, Redpanda, Postgres CDC, S3 sources");
        println!("  --differential         Differential Dataflow (the timely-flow engine)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Materialize 2024 (Slate OS) — mz CLI 2.x"); return 0; }
    println!("Materialize 2024 (Slate OS) — Streaming SQL Database (Incremental View Maintenance)");
    println!("  Vendor: Materialize, Inc. (New York, NY — private)");
    println!("  Founders: Arjun Narayan (CEO) + Frank McSherry + Nikhil Benesch, 2019");
    println!("          Frank McSherry: researcher behind Timely Dataflow + Differential Dataflow");
    println!("          McSherry: 'Naiad' MSR paper (2013) → Timely Dataflow → Differential Dataflow → Materialize");
    println!("          Arjun Narayan: ex-Cockroach Labs, ex-MSR");
    println!("          Nikhil Benesch: ex-Cockroach Labs");
    println!("          All Cornell PhD lineage (database systems research pedigree)");
    println!("  Funding:");
    println!("         Total raised: ~$148M");
    println!("         Series C Aug 2022: $60M at $750M valuation");
    println!("         Investors: Kleiner Perkins, Lightspeed, Redpoint, Foundation Capital");
    println!("         Layoffs 2023 (industry-wide cooling)");
    println!("  Strategic position: 'streaming SQL that's actually SQL — incremental view maintenance for real-time':");
    println!("                    pitch: 'PostgreSQL-compatible streaming SQL — write a query, it stays fresh forever'");
    println!("                    target: real-time apps + dashboards + operational analytics");
    println!("                    primary competitor: Flink SQL, Spark Streaming, ksqlDB, Rising Wave");
    println!("                    secondary: Snowflake Snowpipe Streaming, Pinot/Druid (different model)");
    println!("                    Materialize's wedge: true incremental view maintenance, not micro-batches");
    println!("                    Frank McSherry's Timely + Differential Dataflow = unique technical foundation");
    println!("                    'A streaming database that thinks like a database' — strong DBA + DE positioning");
    println!("  Pricing (cloud, consumption-based):");
    println!("    Free trial: $400 credit + free starter tier");
    println!("    Standard: from ~$0.10/credit/hr (XS cluster) up to $20+/hr (XL cluster)");
    println!("    Per-cluster pricing (each cluster = isolated compute)");
    println!("    Storage: $0.10/GB-month");
    println!("    typically more expensive per TB than batch DWs but for fundamentally different workloads");
    println!("  Architecture (the McSherry contribution):");
    println!("    - Built on Timely Dataflow (Rust, open-source) — McSherry's research engine");
    println!("    - Differential Dataflow: incremental computation over collections");
    println!("    - 'Differential' = automatically computes diffs when input changes");
    println!("    - SQL queries compile to differential dataflow programs");
    println!("    - Sources: Kafka, Redpanda, Postgres CDC, MySQL CDC, S3");
    println!("    - Storage: 'persist' layer on S3 + replication");
    println!("    - Cluster-based isolation (each cluster = independent compute)");
    println!("    - PostgreSQL wire protocol");
    println!("  Product portfolio:");
    println!("    1. Materialized Views (the always-fresh innovation):");
    println!("       - CREATE MATERIALIZED VIEW = view that stays continuously fresh");
    println!("       - When source data changes, view updates incrementally");
    println!("       - Latency: typically milliseconds to seconds");
    println!("       - Eliminate the cron-job ETL pattern");
    println!("    2. Sources (input data):");
    println!("       - Kafka + Redpanda (the primary source type)");
    println!("       - Postgres CDC via logical replication");
    println!("       - MySQL CDC");
    println!("       - S3 (Parquet, CSV, JSON files)");
    println!("       - Webhooks (HTTP POST)");
    println!("    3. Sinks (output):");
    println!("       - Kafka topic emission");
    println!("       - Continuous tail-able results (SUBSCRIBE)");
    println!("       - Direct query (psql + JDBC)");
    println!("    4. Clusters (compute isolation):");
    println!("       - Each cluster runs subset of views/queries");
    println!("       - Independent scaling per cluster");
    println!("       - 'Production' vs 'Dev' clusters typical pattern");
    println!("    5. Catalog + schema management:");
    println!("       - Standard SQL DDL (CREATE TABLE, CREATE VIEW)");
    println!("       - Cluster + replica management");
    println!("       - Source/sink registries");
    println!("    6. Time-windowed queries:");
    println!("       - HOPPING / TUMBLING / SLIDING windows");
    println!("       - Event-time + processing-time semantics");
    println!("       - Watermarks for late-arriving data");
    println!("    7. SQL features:");
    println!("       - JOINs (incremental on changing inputs)");
    println!("       - Aggregations (GROUP BY incremental)");
    println!("       - Subqueries + CTEs");
    println!("       - Complex expressions (similar to Postgres SQL)");
    println!("    8. dbt-materialize adapter:");
    println!("       - First-class dbt integration");
    println!("       - 'Real-time dbt' pattern");
    println!("    9. Tail / SUBSCRIBE (streaming results):");
    println!("       - SUBSCRIBE to view = streaming changes back to client");
    println!("       - Build real-time dashboards or push to clients");
    println!("    10. Source/sink connectors marketplace:");
    println!("       - Standard Kafka Connect-style connectors");
    println!("       - 30+ integrations");
    println!("  Differential Dataflow (the academic foundation):");
    println!("    - Frank McSherry's PhD + MSR research (2010s)");
    println!("    - 'Naiad' (2013 MSR paper) introduced timely dataflow");
    println!("    - Differential Dataflow generalizes: computes diffs over collections");
    println!("    - Cited by ~1000+ research papers");
    println!("    - Materialize commercializes McSherry's research → unique IP foundation");
    println!("    - Open-source: github.com/MaterializeInc/timely-dataflow + differential-dataflow");
    println!("    - Used by other projects: ClickHouse (some inspiration), Risingwave (competitor inspired by)");
    println!("  The 'streaming SQL is the future' bet:");
    println!("    - Traditional ETL: batch transforms run on cron (15min, hourly, daily)");
    println!("    - Streaming SQL: transforms always-fresh, real-time");
    println!("    - Use cases: operational dashboards, fraud detection, real-time personalization, alerting");
    println!("    - Competitors: Flink SQL (open-source, complex), ksqlDB (Confluent), RisingWave (open-source MZ competitor)");
    println!("    - Materialize's bet: SQL-first + incrementality + true correctness wins");
    println!("  Integrations:");
    println!("    - mz CLI (Rust, open source)");
    println!("    - PostgreSQL JDBC/ODBC (wire-compatible)");
    println!("    - SDKs: any PostgreSQL driver (Python, JS, Go, etc.)");
    println!("    - dbt-materialize adapter");
    println!("    - Airflow operators");
    println!("    - Tableau, Power BI, Metabase, Grafana (live dashboards via SUBSCRIBE)");
    println!("    - Kafka + Redpanda + Postgres CDC native sources");
    println!("    - Hightouch, Census reverse-ETL");
    println!("  Materialize CLI usage:");
    println!("    mz profile init --profile=prod                           # auth + region");
    println!("    mz app-password create my-token");
    println!("    psql 'host=region.materialize.cloud port=6875 user=me password=token dbname=materialize'");
    println!("    CREATE CLUSTER my_cluster SIZE = 'xsmall';");
    println!("    CREATE SOURCE my_kafka_src FROM KAFKA CONNECTION my_kafka_conn (TOPIC 'orders') FORMAT JSON;");
    println!("    CREATE MATERIALIZED VIEW order_totals_by_region AS");
    println!("        SELECT region, COUNT(*) AS n, SUM(amount) AS total");
    println!("        FROM orders GROUP BY region;");
    println!("    SELECT * FROM order_totals_by_region;                    # always-fresh result");
    println!("    SUBSCRIBE TO order_totals_by_region;                     # streaming results");
    println!("    CREATE SINK my_sink FROM order_totals_by_region INTO KAFKA CONNECTION my_kafka_conn (TOPIC 'aggregates');");
    println!("    mz region list");
    println!("    mz cluster list");
    println!("  Customers (real-time ops):");
    println!("    - Various fintech + adtech + SaaS real-time use cases");
    println!("    - Operational dashboard / fraud detection / personalization customers");
    println!("    - Customer count private but small (likely <500 enterprise as of 2024)");
    println!("  Critique: Flink SQL is open-source and more mature");
    println!("           streaming SQL still small market vs batch SQL");
    println!("           customers can run Timely+Differential Dataflow open source themselves");
    println!("           RisingWave + ksqlDB compete on similar incremental view value prop");
    println!("           customer education needed — 'streaming SQL DB' is novel category");
    println!("           2023 layoffs reflected challenging fundraising environment");
    println!("           cluster sizing/cost can surprise teams used to serverless");
    println!("           pure 'incremental view maintenance' is brilliant but may be overkill for some use cases");
    println!("  Differentiator: Frank McSherry's Timely + Differential Dataflow (academic IP, MSR research) → only true incremental view maintenance database + always-fresh materialized views + PostgreSQL wire protocol + Kafka + Postgres CDC + S3 sources + cluster-based isolation + SUBSCRIBE for streaming dashboards + Arjun Narayan + Frank McSherry + Nikhil Benesch founders (Cornell + Cockroach Labs + MSR pedigree) + $148M raised + $750M valuation + Rust-built engine + open-source dataflow primitives — the streaming SQL database that uses incremental view maintenance (not micro-batches) so your dashboards and pipelines stay always-fresh with millisecond latency");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "materialize".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mz(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/materialize"), "materialize");
        assert_eq!(basename(r"C:\bin\materialize.exe"), "materialize.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("materialize.exe"), "materialize");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mz(&["--help".to_string()], "materialize"), 0);
        assert_eq!(run_mz(&["-h".to_string()], "materialize"), 0);
        let _ = run_mz(&["--version".to_string()], "materialize");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mz(&[], "materialize");
    }
}
