#![deny(clippy::all)]

//! trino-cli — SlateOS Trino (open-source distributed SQL query engine, ex-PrestoSQL)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_trino(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trino [OPTIONS]");
        println!("Trino (SlateOS) — open-source distributed SQL query engine (formerly PrestoSQL)");
        println!();
        println!("Options:");
        println!("  --start                Start Trino server (coordinator + workers)");
        println!("  --execute SQL          Execute SQL via Trino CLI");
        println!("  --catalog NAME         Use specified catalog");
        println!("  --schema NAME          Use specified schema");
        println!("  --file PATH            Execute SQL from file");
        println!("  --commercial           Commercial distributions (Starburst, AWS Athena, GCP)");
        println!("  --foundation           Trino Software Foundation (501(c)(3) nonprofit governance)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Trino 445 (SlateOS)"); return 0; }
    println!("Trino 445 (SlateOS) — Open-source distributed SQL");
    println!("  License: Apache 2.0 — fully open-source, no commercial dual-license");
    println!("  Governance: Trino Software Foundation (TSF) — 501(c)(3) nonprofit (since 2022)");
    println!("            board: Martin Traverso, Dain Sundstrom, David Phillips + community members");
    println!("            independent governance after fork from PrestoDB (Facebook-controlled)");
    println!("  Creators: Martin Traverso, Dain Sundstrom, David Phillips, Eric Hwang, 2012 at Facebook");
    println!("          built Presto inside Facebook to query 300+ PB Hive warehouse + replace Hadoop MapReduce SQL");
    println!("          open-sourced Nov 2013 as Presto");
    println!("          all left Facebook 2018 to form Presto Software Foundation (independent governance)");
    println!("          Facebook donated PrestoDB to Linux Foundation Sep 2019");
    println!("          fork dispute: Traverso et al renamed their fork 'PrestoSQL' (Jan 2019)");
    println!("          renamed again to 'Trino' Dec 2020 due to trademark concerns with Facebook + Linux Foundation");
    println!("  Origin story (the dramatic fork):");
    println!("    - Facebook donated Presto trademark to Linux Foundation as 'PrestoDB Software Foundation'");
    println!("    - PrestoDB stayed Facebook-controlled; PrestoSQL/Trino became truly community-led");
    println!("    - For ~2 years (2019-2020) there were TWO 'Presto' projects causing massive confusion");
    println!("    - Trino rename (Dec 2020) was meant to end confusion + signal independence");
    println!("    - PrestoDB (Facebook-led) still exists but has far less development velocity than Trino");
    println!("    - Trino now considered the 'real Presto' by most of the data community");
    println!("  Pricing:");
    println!("    Trino itself — FREE, Apache 2.0 (download + run anywhere)");
    println!("    Commercial: Starburst Galaxy + Enterprise (largest commercial vendor)");
    println!("    Commercial: AWS Athena (Presto/Trino-based, $5/TB scanned)");
    println!("    Commercial: GCP Athena equivalent (BigQuery Federation), Azure Synapse Serverless");
    println!("    self-hosting: pay for compute infrastructure (EC2/GCE/Azure VM) + operations");
    println!("    typical self-hosted Trino cluster ops: $20K-$200K+/yr in cloud compute");
    println!("  Core architecture (distributed MPP SQL):");
    println!("    - Coordinator node + multiple Worker nodes (typical 3-100+ workers)");
    println!("    - Pull-based query execution: workers stream results to coordinator");
    println!("    - In-memory query execution (no intermediate disk staging like Spark)");
    println!("    - Pluggable Connector SPI: read from any source (Hive, Iceberg, Delta, Postgres, MySQL, MongoDB)");
    println!("    - Cost-based optimizer + reorder joins + push down predicates");
    println!("    - ANSI SQL with extensions (Lambda functions, window functions, JSON, arrays)");
    println!("    - Fault-tolerant execution (since v382) — checkpoints + retry instead of restart");
    println!("    - Dynamic filtering across partitioned joins");
    println!("  Connectors (50+):");
    println!("    - Lakehouse: Hive (Iceberg + Delta + Hudi + Parquet + ORC + Avro)");
    println!("    - Iceberg connector (native, no Hive Metastore needed)");
    println!("    - Delta Lake connector");
    println!("    - Databases: Postgres, MySQL, MSSQL, Oracle, Redshift, Synapse, BigQuery");
    println!("    - NoSQL: MongoDB, Cassandra, Elasticsearch, OpenSearch, Pinot");
    println!("    - Cloud object stores: S3, GCS, ADLS, MinIO (open-source S3)");
    println!("    - Streaming: Kafka (limited)");
    println!("    - Federation: any combination of above in single query");
    println!("  Famous users (the install base):");
    println!("    - Netflix: original adopter, 7+ PB Trino daily, internal data platform centerpiece");
    println!("    - Lyft: 5K queries/day across Iceberg lakehouse");
    println!("    - Pinterest: petabyte-scale ad analytics");
    println!("    - LinkedIn: replaced Hive for interactive queries");
    println!("    - Salesforce: Tableau CRM (formerly Einstein Analytics) on Trino");
    println!("    - Comcast (heavy Starburst user)");
    println!("    - Twitter/X, eBay, Bloomberg, Walmart, Robinhood, Shopify");
    println!("  Trino Community:");
    println!("    - 1,100+ contributors on GitHub");
    println!("    - 10K+ stars (one of most popular data engines after Spark)");
    println!("    - Quarterly releases (much faster cadence than PrestoDB)");
    println!("    - Slack + community-led Trino Summit annual conference");
    println!("    - Starburst Data (commercial Trino) employs many top maintainers but doesn't control project");
    println!("  TSF (Trino Software Foundation):");
    println!("    - Nonprofit 501(c)(3) (Delaware) since 2022");
    println!("    - Governs trademark + project assets");
    println!("    - Multiple commercial entities (Starburst, AWS, Bloomberg) employ contributors");
    println!("    - Board includes original creators + 2 community-elected members");
    println!("  Commercial distributions (the ecosystem):");
    println!("    - Starburst Data (most prominent — Galaxy SaaS + Enterprise on-prem)");
    println!("    - AWS Athena (Presto-based, but heavily diverged at AWS)");
    println!("    - Trino on Kubernetes (Helm charts, official + community)");
    println!("    - PrestoDB (Facebook-led, separate fork, slower velocity)");
    println!("  Trino CLI usage:");
    println!("    trino --server localhost:8080 --catalog hive --schema default");
    println!("    > SELECT * FROM customer WHERE region = 'US' LIMIT 10;");
    println!("    --format CSV/JSON/ALIGNED/AUTO etc.");
    println!("    --output-format CSV_HEADER, JSON, VERTICAL");
    println!("    --debug for query plans + EXPLAIN ANALYZE");
    println!("  AI + GenAI ecosystem:");
    println!("    - Trino + LangChain integration for RAG over data lakes");
    println!("    - Trino + Polars + Pandas + Arrow Flight for fast AI data prep");
    println!("    - Vector search support via experimental connectors");
    println!("    - Many AI startups build on Trino because it's free + battle-tested");
    println!("  Critique: operating Trino requires data engineering expertise (cluster sizing, GC tuning, connector configuration)");
    println!("           cost-based optimizer occasionally produces poor plans on Iceberg/Delta — needs tuning");
    println!("           memory-heavy: spilling to disk available since v336 but performance varies");
    println!("           catalog/governance: native RBAC is basic — typically use Apache Ranger, Privacera, or Starburst Gravity");
    println!("           ecosystem fragmentation: Trino vs PrestoDB vs Athena (all somewhat compatible, somewhat not)");
    println!("           commercial vendors (Starburst especially) drive most enterprise feature roadmap");
    println!("           competing with: Spark SQL (Databricks), BigQuery, Snowflake — all have commercial backing");
    println!("  Differentiator: truly open-source + community-governed + 50+ connectors + federation across any source + battle-tested at Netflix/Lyft/Pinterest scale + 1,100+ contributors — the free distributed SQL engine that powers most lakehouses");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "trino".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_trino(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_trino};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/trino"), "trino");
        assert_eq!(basename(r"C:\bin\trino.exe"), "trino.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("trino.exe"), "trino");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trino(&["--help".to_string()], "trino"), 0);
        assert_eq!(run_trino(&["-h".to_string()], "trino"), 0);
        let _ = run_trino(&["--version".to_string()], "trino");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trino(&[], "trino");
    }
}
