#![deny(clippy::all)]

//! imply-cli — Slate OS Imply (Apache Druid commercial — Polaris cloud, real-time analytics, Burlingame, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_imply(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: imply [OPTIONS]");
        println!("Imply (Slate OS) — Apache Druid commercial platform (real-time + sub-second analytics)");
        println!();
        println!("Options:");
        println!("  --polaris              Polaris (fully-managed Druid on AWS)");
        println!("  --enterprise           Imply Enterprise (self-managed Druid with HA/security)");
        println!("  --pivot                Pivot (BI + dashboards for Druid)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Imply 2024 (Slate OS) — imply CLI 2.x + Druid 30.x"); return 0; }
    println!("Imply 2024 (Slate OS) — Apache Druid Commercial (Real-Time Analytics)");
    println!("  Vendor: Imply Data, Inc. (Burlingame, CA — private)");
    println!("  Founders: Fangjin Yang + Gian Merlino + Vadim Ogievetsky, 2015");
    println!("          Fangjin Yang: co-creator of Druid at Metamarkets (2011)");
    println!("          Gian Merlino: co-creator of Druid, key contributor still active");
    println!("          Vadim Ogievetsky: co-creator of Druid + Plywood (visualization)");
    println!("          'Imply' founded to commercialize Druid after Metamarkets acquired by Snap (2017)");
    println!("          Apache Druid: open source database, ASF top-level project since 2018");
    println!("  Funding:");
    println!("         Total raised: ~$215M");
    println!("         Series D Jul 2022: $100M at $1.1B+ valuation (unicorn)");
    println!("         Investors: Andreessen Horowitz, Bessemer, Khosla Ventures");
    println!("         Layoffs 2023 (industry-wide cooling)");
    println!("  Strategic position: 'real-time analytics database — interactive sub-second on streaming + historical':");
    println!("                    pitch: 'sub-second queries on real-time + historical data — for observability, IoT, gaming, ads, security'");
    println!("                    target: real-time analytical apps (vs batch DWs like Snowflake)");
    println!("                    primary competitor: ClickHouse, StarRocks, Pinot, Rockset, Firebolt");
    println!("                    secondary: SingleStore, Snowflake Snowpipe Streaming");
    println!("                    Imply's wedge: Druid open source + commercial Polaris cloud + Pivot BI");
    println!("                    Druid sweet spot: time-series + dimensional analytics over high-cardinality data");
    println!("                    used at: Netflix, Confluent, Salesforce, Lyft, Airbnb (all open-source Druid)");
    println!("  Pricing (cloud + enterprise tiers):");
    println!("    Polaris Free: free starter cluster");
    println!("    Polaris (managed): consumption-based by compute + storage");
    println!("    Imply Enterprise (self-managed): per-node licensing, custom quotes");
    println!("    Apache Druid: free (open source) — Imply contributes ~70% of commits");
    println!("    typically positioned cheaper than Snowflake for real-time use cases");
    println!("  Architecture (Druid + Imply commercial):");
    println!("    - Druid: columnar + time-partitioned + bitmap-indexed");
    println!("    - Segments: immutable shards by time (typically per-hour or per-day)");
    println!("    - Bitmap indexes on string columns (fast filter)");
    println!("    - Roaring bitmaps for high-cardinality");
    println!("    - Stream ingest: Kafka, Kinesis, Pulsar (exactly-once)");
    println!("    - Batch ingest: S3, HDFS, GCS (Parquet, ORC, CSV, JSON)");
    println!("    - Process nodes: Coordinator, Overlord, Broker, MiddleManager, Historical");
    println!("    - Deep storage: S3/HDFS/Azure Blob/GCS");
    println!("    - Native query language + SQL");
    println!("  Product portfolio:");
    println!("    1. Apache Druid (the open-source core):");
    println!("       - Time-partitioned columnar OLAP database");
    println!("       - Sub-second queries on TB-PB datasets");
    println!("       - High concurrency (1000s of QPS)");
    println!("       - Real-time + historical unified queries");
    println!("       - 30+ release, 14K+ GitHub stars");
    println!("    2. Polaris (the fully-managed cloud, 2022+):");
    println!("       - SaaS Druid on AWS (Azure/GCP planned)");
    println!("       - Auto-scaling, zero ops");
    println!("       - Integrated ingestion + query + monitoring");
    println!("       - 'Druid without the operational burden'");
    println!("    3. Imply Enterprise (self-managed Druid):");
    println!("       - HA configurations + automated failover");
    println!("       - Security: LDAP/OIDC SSO, RBAC, encryption");
    println!("       - Imply Manager (GUI cluster management)");
    println!("       - 24/7 support + indemnification");
    println!("    4. Pivot (BI + visualization):");
    println!("       - Druid-native dashboard tool");
    println!("       - Time-series + funnel + cohort analysis built-in");
    println!("       - 'BI for real-time' positioning");
    println!("       - Bundled with Imply Enterprise + Polaris");
    println!("    5. Druid SQL:");
    println!("       - ANSI SQL with time-series extensions");
    println!("       - APPROX_COUNT_DISTINCT (HLL), APPROX_QUANTILE (T-Digest)");
    println!("       - LATEST + EARLIEST aggregators");
    println!("       - TIME_FLOOR / TIME_SHIFT functions");
    println!("    6. Multi-stage query engine (MSQ — 2022+):");
    println!("       - Druid SQL via async distributed engine");
    println!("       - Bridge to batch ETL workloads");
    println!("       - INSERT INTO + REPLACE INTO via SQL");
    println!("    7. Async queries:");
    println!("       - Long-running queries (10min+)");
    println!("       - Result storage + retrieval");
    println!("       - Use case: scheduled reports, data exports");
    println!("    8. Streaming + batch ingestion:");
    println!("       - Kafka, Kinesis, Pulsar streaming sources");
    println!("       - S3, GCS, Azure batch + auto-compaction");
    println!("       - Exactly-once + late-arriving data handling");
    println!("    9. Imply Connect (CDC + integrations):");
    println!("       - Postgres CDC, MySQL CDC (newer)");
    println!("       - Fivetran-like managed connectors");
    println!("  Druid's sweet spot:");
    println!("    - Event/time-series data: clickstreams, ad impressions, security events, IoT");
    println!("    - High cardinality: millions of distinct user IDs, products, etc.");
    println!("    - Interactive analytics: dashboards with sub-second response");
    println!("    - High concurrency: 1000s of QPS supported");
    println!("    - Time-bounded queries: 'last 24h', 'last 7d', 'last 90d'");
    println!("    - Not great for: ad-hoc SQL across long time windows, joins of arbitrary tables");
    println!("  The Metamarkets → Imply story:");
    println!("    - Metamarkets (2010-2017): ad analytics SaaS, built Druid as internal engine");
    println!("    - Open-sourced Druid 2012 (Apache 2.0)");
    println!("    - Snap acquired Metamarkets 2017 (~$200M) for adtech tech");
    println!("    - Yang + Merlino + Ogievetsky departed → founded Imply 2015 (parallel)");
    println!("    - Imply became primary commercial steward of Druid project");
    println!("    - Apache Druid graduated to top-level ASF project Feb 2019");
    println!("  Integrations:");
    println!("    - imply CLI (Polaris management)");
    println!("    - Druid HTTP API + SQL JDBC/ODBC");
    println!("    - SDKs: Java native, Python (pydruid), JS clients");
    println!("    - dbt-druid adapter");
    println!("    - Airflow operators (DruidOperator)");
    println!("    - Tableau, Power BI, Apache Superset, Pivot");
    println!("    - Kafka, Kinesis, Pulsar (native streaming sources)");
    println!("    - S3, GCS, Azure Blob (batch ingest + deep storage)");
    println!("    - Grafana data source (popular for observability)");
    println!("  Imply CLI usage:");
    println!("    imply auth login                                         # Polaris auth");
    println!("    imply polaris cluster create --name=my-cluster --tier=development");
    println!("    imply polaris ingestion-job submit --spec=ingestion.json");
    println!("    imply polaris query --cluster=my-cluster --sql='SELECT COUNT(*) FROM events WHERE __time >= CURRENT_TIMESTAMP - INTERVAL 1 HOUR'");
    println!("    druid post-index-task --file ingestion-spec.json --url http://overlord:8090");
    println!("    druid sql-cli                                            # interactive Druid SQL");
    println!("    SELECT TIME_FLOOR(__time, 'PT1H') AS hr, COUNT(*) FROM events GROUP BY 1 ORDER BY 1;");
    println!("    SELECT APPROX_COUNT_DISTINCT(user_id) FROM events WHERE __time >= CURRENT_TIMESTAMP - INTERVAL 1 DAY;");
    println!("    INSERT INTO events SELECT * FROM TABLE(EXTERN('{{\"type\":\"s3\"...}}','{{...}}','...'));");
    println!("    imply pivot connect --url=druid-broker --user=admin");
    println!("  Customers (open-source Druid + Imply commercial):");
    println!("    - Open-source Druid: Netflix, Lyft, Airbnb, Confluent, Salesforce, Yahoo");
    println!("    - Imply commercial: Reddit, Twitch, NTT Docomo, Visa (some), various ad networks");
    println!("    - Use cases: ad analytics, observability, gaming telemetry, security analytics, IoT");
    println!("    - 100s of Imply commercial customers (private metrics)");
    println!("  Critique: ClickHouse + StarRocks open-source competition is fierce");
    println!("           Druid operational complexity (5 process types) intimidating");
    println!("           join support historically weak (improved with MSQ)");
    println!("           Pinot (Apache, LinkedIn) similar use case + active competitor");
    println!("           Polaris cloud GA in 2022 — later than ClickHouse Cloud");
    println!("           limited multi-cloud (Polaris AWS-only initially)");
    println!("           Rockset acquired by OpenAI Jun 2024 = real-time analytics market signal");
    println!("           pure 'real-time analytics' market overshadowed by Snowflake/BigQuery convergence");
    println!("           need for separate BI tool (Pivot vs Superset/Looker) operational overhead");
    println!("  Differentiator: Apache Druid commercial steward (Imply contributes ~70% of Druid commits) + Druid co-creators founded Imply (Fangjin Yang + Gian Merlino + Vadim Ogievetsky 2015) + sub-second queries on TB-PB datasets + high-cardinality + high-concurrency support (1000s QPS) + time-partitioned segments + bitmap indexes + Roaring bitmaps + streaming + historical unified queries + Pivot BI + Polaris managed cloud + Imply Enterprise self-managed + $215M raised + $1.1B+ valuation + Netflix/Lyft/Airbnb/Confluent open-source users + Reddit/Twitch commercial customers — the commercial backer of Apache Druid for real-time analytics workloads where sub-second response on streaming + historical data matters (observability, IoT, gaming, adtech, security)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "imply".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_imply(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_imply};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/imply"), "imply");
        assert_eq!(basename(r"C:\bin\imply.exe"), "imply.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("imply.exe"), "imply");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_imply(&["--help".to_string()], "imply"), 0);
        assert_eq!(run_imply(&["-h".to_string()], "imply"), 0);
        let _ = run_imply(&["--version".to_string()], "imply");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_imply(&[], "imply");
    }
}
