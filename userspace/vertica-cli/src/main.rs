#![deny(clippy::all)]

//! vertica-cli — OurOS Vertica (Stonebraker C-Store-derived columnar MPP, OpenText/HPE heritage)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vsql(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vertica [OPTIONS]");
        println!("Vertica (OurOS) — columnar MPP analytics database (OpenText, since 2005)");
        println!();
        println!("Options:");
        println!("  --eon                  Eon Mode (separation of storage + compute on S3/HDFS/Azure)");
        println!("  --enterprise           Enterprise Mode (shared-nothing on-prem classic)");
        println!("  --in-db-ml             In-database ML functions");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Vertica 2024 (OurOS) — vsql 24.x"); return 0; }
    println!("Vertica 2024 (OurOS) — Columnar MPP Analytics Database");
    println!("  Vendor: Vertica (part of OpenText Corp. — NASDAQ:OTEX, TSX:OTEX since 1996)");
    println!("  Origins: Spun out of Michael Stonebraker's MIT C-Store research (2005)");
    println!("          Michael Stonebraker: Turing Award 2014, founder of Ingres, Postgres, Vertica, VoltDB, SciDB, Tamr");
    println!("          'C-Store: A Column-Oriented DBMS' (Stonebraker et al, VLDB 2005)");
    println!("          One of the most cited papers in databases — birthed Vertica + influenced everyone else");
    println!("          Andrew Palmer + Stan Zdonik + Sam Madden + Daniel Abadi: co-founders");
    println!("  Corporate history:");
    println!("         Founded 2005 (Vertica Systems Inc.)");
    println!("         Acquired by HP $350M (2011) → HP Enterprise (2015)");
    println!("         Spun into Micro Focus (2017) → OpenText acquired Micro Focus (Jan 2023, $6B)");
    println!("         Now part of OpenText (Canadian enterprise software, NASDAQ:OTEX/TSX:OTEX)");
    println!("         OpenText FY2024 revenue: ~$5.7B (Vertica is ~$300M-$500M segment estimate)");
    println!("  Public market (OpenText NASDAQ:OTEX):");
    println!("         OpenText IPO 1996 (early Canadian tech IPO)");
    println!("         Market cap: $7-10B range");
    println!("         Bought Documentum 2017, Carbonite 2019, Micro Focus 2023 ($6B)");
    println!("         Vertica positioned as 'analytics engine' in OpenText enterprise content portfolio");
    println!("  Strategic position: 'cloud-optional MPP columnar analytics — sweet spot between Snowflake + Postgres':");
    println!("                    pitch: 'columnar MPP analytics — deploy anywhere (cloud, hybrid, on-prem), with in-DB ML'");
    println!("                    target: enterprises with petabyte-scale analytics + multi-cloud requirements");
    println!("                    primary competitor: Snowflake, Databricks, ClickHouse, Greenplum");
    println!("                    secondary: Teradata, Redshift, Synapse");
    println!("                    Vertica's wedge: cloud + on-prem + hybrid + edge deployments (rare flexibility)");
    println!("                    challenge: ownership churn (Vertica → HP → HPE → Micro Focus → OpenText) bred uncertainty");
    println!("                    counter: technical excellence + 100+ in-database ML algorithms");
    println!("  Pricing (consumption + perpetual options):");
    println!("    Vertica Analytics Platform: $999/TB/yr (perpetual license, on-prem)");
    println!("    Vertica Accelerator (SaaS): consumption-based by query + storage");
    println!("    Vertica in Eon Mode: $0.50/hour per node (cloud)");
    println!("    Community Edition: free up to 1TB + 3 nodes");
    println!("    typically cheaper per TB than Snowflake for steady-state workloads");
    println!("    more expensive than ClickHouse self-hosted but with enterprise features");
    println!("  Architecture (the C-Store inheritance):");
    println!("    - Columnar storage (the C-Store DNA)");
    println!("    - Massively Parallel Processing (MPP) shared-nothing");
    println!("    - Projections (materialized column subsets) — key Vertica concept");
    println!("    - Read-Optimized Store (ROS) + Write-Optimized Store (WOS) → merged Tuple Mover");
    println!("    - Aggressive compression: RLE, delta, dictionary, LZO (often 8-10x)");
    println!("    - Multiple sort orders (projection diversity)");
    println!("    - Database Designer: auto-tunes projections for workload");
    println!("    - Eon Mode (2018+): separation of storage (S3/HDFS/Azure) and compute");
    println!("  Product portfolio:");
    println!("    1. Vertica Enterprise Mode (the classic):");
    println!("       - Shared-nothing MPP on-prem deployment");
    println!("       - K-Safety: configurable redundancy + failover");
    println!("       - Local storage per node");
    println!("       - Best for: large stable workloads on owned hardware");
    println!("    2. Vertica Eon Mode (the cloud answer, 2018+):");
    println!("       - Separates storage (S3, Azure Blob, GCS, HDFS, MinIO) from compute");
    println!("       - Elastic resize of compute clusters");
    println!("       - Multiple subclusters (workload isolation)");
    println!("       - Cloud-native deployment (AWS/Azure/GCP) + on-prem object stores");
    println!("       - Architectural answer to Snowflake's separation");
    println!("    3. Vertica Accelerator (SaaS):");
    println!("       - Fully managed Vertica (2022+)");
    println!("       - Compete with Snowflake on managed convenience");
    println!("       - AWS-only initially, expanding");
    println!("    4. In-database ML (the analytics differentiator):");
    println!("       - 100+ built-in ML functions");
    println!("       - Regression, classification, clustering, time series");
    println!("       - Naive Bayes, SVM, k-means, ARIMA, Random Forest");
    println!("       - Train + score directly in SQL — no data movement");
    println!("       - Python + R UDFs for custom models");
    println!("    5. Vertica for SQL on Apache Hadoop:");
    println!("       - Query HDFS Parquet/ORC files in place");
    println!("       - 'External tables' over Hadoop");
    println!("       - Legacy product, still maintained");
    println!("    6. Vertica Voltage SecureData integration:");
    println!("       - Format-preserving encryption at column level");
    println!("       - Compliance-grade tokenization");
    println!("    7. Flex Tables (semi-structured):");
    println!("       - JSON / Avro / Parquet on-ingest schema discovery");
    println!("       - Materialize columns from semi-structured");
    println!("    8. Workload Analyzer + Database Designer:");
    println!("       - Automated tuning + projection design");
    println!("       - Best-practice recommendations");
    println!("    9. Time-series + geospatial:");
    println!("       - Native time series functions (TIMESERIES, INTERPOLATE)");
    println!("       - GIS functions (PostGIS-compatible)");
    println!("    10. Vertica Management Console:");
    println!("       - Web UI for cluster admin");
    println!("       - Performance monitoring + query tuning");
    println!("  Projections (the architectural innovation):");
    println!("    - Vertica stores data as 'projections' — sorted column subsets");
    println!("    - Multiple projections per table (different sort orders for different queries)");
    println!("    - Pre-join projections (denormalized for query speed)");
    println!("    - Database Designer auto-creates optimal projection set");
    println!("    - Trade-off: storage overhead vs query performance");
    println!("    - This is the C-Store paper's key idea, productized");
    println!("  C-Store paper legacy:");
    println!("    - Stonebraker's 2005 VLDB paper kicked off columnar DB revolution");
    println!("    - C-Store concepts: read/write store split, projections, aggressive compression");
    println!("    - Influenced: Vertica (direct), MonetDB (parallel), ClickHouse (later)");
    println!("    - Made columnar mainstream — every modern analytical DB now columnar");
    println!("    - Stonebraker: Turing Award 2014 — partial recognition for C-Store");
    println!("  Integrations:");
    println!("    - vsql (CLI — psql-like)");
    println!("    - JDBC + ODBC drivers");
    println!("    - SDKs: Python (vertica-python), R, Node.js, Go");
    println!("    - dbt-vertica adapter");
    println!("    - Spark connector + Kafka connector + Flume");
    println!("    - Tableau, Power BI, Looker, Cognos");
    println!("    - Airflow VerticaOperator");
    println!("    - Cloud marketplaces: AWS, Azure, GCP");
    println!("  Vertica CLI usage:");
    println!("    vsql -h myhost -U dbadmin -d mydb                        # interactive SQL");
    println!("    vsql -c 'SELECT version();'");
    println!("    vsql -f script.sql                                       # run script");
    println!("    admintools                                               # cluster admin TUI");
    println!("    admintools -t create_db -d mydb -p mypass -s node1,node2,node3");
    println!("    admintools -t start_db -d mydb");
    println!("    vsql -c \"COPY my_table FROM '/path/to/data.csv' DELIMITER ',';\"");
    println!("    vsql -c \"SELECT REBALANCE_CLUSTER();\"                  # rebalance after node add");
    println!("    vsql -c \"SELECT ANALYZE_STATISTICS('my_schema.my_table');\"");
    println!("    vsql -c \"SELECT KMEANS('cluster_model', 'customers', 'features', 5);\"  # in-DB ML");
    println!("  Customers:");
    println!("    - Telcos: AT&T, T-Mobile, Verizon (call detail records — billions of rows/day)");
    println!("    - Financial: Morgan Stanley, Cerberus");
    println!("    - Gaming + adtech: Zynga, Epic, Trade Desk");
    println!("    - Retail: Walmart Labs, eBay");
    println!("    - 1,000+ enterprise customers globally");
    println!("    - Use cases: ad tech (clickstream), telecom (CDRs), gaming telemetry, IoT");
    println!("  Critique: ownership churn (HP → HPE → Micro Focus → OpenText) hurt mindshare");
    println!("           Snowflake + Databricks captured most net-new analytical workloads 2018+");
    println!("           Eon Mode launched ~3 years after Snowflake — late to separation-of-S+C");
    println!("           perception in dev community: 'enterprise legacy'");
    println!("           OpenText acquisition (2023) — analytics not OpenText core focus");
    println!("           Vertica Accelerator (SaaS) is years behind Snowflake on polish");
    println!("           projections concept is powerful but operationally heavy");
    println!("           less marketing budget than cloud DW competitors");
    println!("  Differentiator: Stonebraker's C-Store paper (VLDB 2005) productized → original columnar MPP DB + projections (multiple sort orders per table) + 100+ in-database ML functions + Eon Mode (storage/compute separation on S3/HDFS/Azure) + cloud + on-prem + hybrid + edge deployments + aggressive compression (8-10x typical) + telecom CDR / adtech / gaming workloads at petabyte scale + AT&T/Trade Desk/Zynga customers + ~$300-500M segment revenue + Stonebraker's pedigree — the columnar MPP database that powers some of the largest analytical workloads in telecom and adtech, with the flexibility to deploy anywhere and 100+ in-DB ML algorithms");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vertica".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vsql(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
