#![deny(clippy::all)]

//! dremio-cli — OurOS Dremio (Apache Arrow + Iceberg lakehouse query engine)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dremio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dremio [OPTIONS]");
        println!("Dremio (OurOS) — open data lakehouse platform (Apache Arrow + Iceberg)");
        println!();
        println!("Options:");
        println!("  --cloud                Dremio Cloud (managed SaaS, free standard tier)");
        println!("  --enterprise           Dremio Enterprise (self-hosted)");
        println!("  --arctic               Dremio Arctic (Iceberg catalog with Git-like branching)");
        println!("  --reflections          Data Reflections (materialized acceleration layer)");
        println!("  --sonar                Dremio Sonar (query engine)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dremio 2024 (OurOS)"); return 0; }
    println!("Dremio 2024 (OurOS)");
    println!("  Vendor: Dremio Corp. (Santa Clara, CA — private)");
    println!("  Founders: Tomer Shiran (CPO), Jacques Nadeau (ex-CTO), Kelly Stirman, 2015");
    println!("          Shiran + Nadeau were former MapR engineers + creators of Apache Arrow");
    println!("          Apache Arrow (started 2016, Drill-derived) is one of the most important data formats today");
    println!("          Wendy Stirman + Kelly Stirman early team — early data engineering veterans");
    println!("          Nadeau left 2023 to join Sundeck (founded by other ex-Dremio folks)");
    println!("  Founded: 2015 in Mountain View");
    println!("          raised ~$410M total (~$2B valuation at Series E, 2022)");
    println!("          Sapphire, Adams Street, Lightspeed, Sequoia lead investors");
    println!("          ~$70M+ ARR estimate (private)");
    println!("          ~500 employees");
    println!("  Strategic position: 'lakehouse without lock-in — Iceberg + Arrow + open formats':");
    println!("                    primary competitor: Snowflake (closed format), Databricks (Delta/Spark), Starburst (Trino-based)");
    println!("                    Dremio's wedge: 'lakehouse from the people who created Arrow' — purpose-built for open formats");
    println!("                    smaller than Snowflake/Databricks but technically deep — Arrow + Sabot + Iceberg expertise");
    println!("                    Apache Arrow underlies Dremio Sonar (query engine) — vectorized + columnar execution");
    println!("                    enterprise positioning: 'cheaper than Snowflake by 50-80%' — typical price comparison pitch");
    println!("  Pricing (cloud-consumption + enterprise):");
    println!("    Dremio Cloud Standard — FREE (1 small project, basic features)");
    println!("    Dremio Cloud Enterprise — usage-based, typically $25K-$1M+/yr");
    println!("    Dremio Software (self-hosted) — annual license + support, $100K-$2M+/yr enterprise");
    println!("    pricing axis: compute (DCU = Dremio Compute Units) + storage");
    println!("    common pitch: 'lower compute cost than Snowflake for same workload'");
    println!("  Core architecture (Sonar query engine):");
    println!("    - Built on Apache Arrow (in-memory columnar format) — native vectorization");
    println!("    - Sabot kernel: C++ execution layer for compute push-down + vectorized SIMD");
    println!("    - Massively parallel SQL execution engine");
    println!("    - Native Apache Iceberg + Delta Lake + Parquet support");
    println!("    - Data Reflections: smart materialized acceleration layer (transparent to query)");
    println!("    - ANSI SQL with extensions, JDBC + ODBC + Arrow Flight SQL drivers");
    println!("    - Self-service semantic layer (Spaces + Virtual Datasets)");
    println!("  Data Reflections (the killer feature):");
    println!("    - Pre-computed aggregations + raw-data subsets stored in fast columnar format");
    println!("    - Optimizer transparently routes queries to fastest reflection without user knowing");
    println!("    - Similar concept to: BigQuery materialized views, Snowflake search optimization");
    println!("    - Dremio: 'pretend everything is a reflection — never write CREATE INDEX again'");
    println!("    - Auto-recommend reflections based on query history");
    println!("  Dremio Arctic (Iceberg-native catalog, 2022+):");
    println!("    - Iceberg catalog with Git-like branching + merging + tagging");
    println!("    - Time-travel queries (AS OF SNAPSHOT)");
    println!("    - Multiple data 'branches' for experimentation + staging before production merge");
    println!("    - Open-source: based on Apache Polaris (also adopted by Snowflake)");
    println!("    - Direct competitor: Nessie (open-source by Project Nessie / Dremio's own contribution)");
    println!("  Dremio Cloud (managed SaaS, the growth product):");
    println!("    - Provision in minutes on AWS + Azure");
    println!("    - Auto-scaling engines per workload type");
    println!("    - Standard tier free forever for development");
    println!("    - Enterprise tier with SSO + RBAC + audit logs + private connectivity");
    println!("    - Marketing pitch: 'BigQuery + Iceberg + 50% cheaper than Snowflake'");
    println!("  Semantic Layer + Virtual Datasets:");
    println!("    - Self-service: business users define 'Spaces' with virtual datasets");
    println!("    - SQL views or no-code transformations on top of raw lakehouse data");
    println!("    - Lineage + dependency tracking");
    println!("    - Tag-based access control + row-level security");
    println!("    - Cuts out: separate BI semantic layer (Looker LookML, Cube.dev)");
    println!("  Apache Arrow + Arrow Flight SQL:");
    println!("    - Native Arrow over the wire — no SerDe overhead");
    println!("    - Arrow Flight SQL: 10-100x faster than JDBC/ODBC for large result sets");
    println!("    - Direct integration with Pandas + Polars + DuckDB for fast Python data science");
    println!("    - Dremio Sonar is one of the fastest engines for Iceberg + Parquet workloads");
    println!("  AI + GenAI features (2024):");
    println!("    - Dremio Text-to-SQL");
    println!("    - Vector search support in Iceberg tables");
    println!("    - RAG integration with embeddings stored in lakehouse");
    println!("    - Sundeck partnership (founded by Dremio alums) for AI-augmented workflows");
    println!("  Integrations: 30+ data sources:");
    println!("              Lakehouse storage: S3, ADLS, GCS (Iceberg + Delta + Parquet + Hudi)");
    println!("              Databases: Postgres, MySQL, MSSQL, Oracle, MongoDB, Elasticsearch");
    println!("              Warehouses: Snowflake (federation), Redshift, Synapse, BigQuery");
    println!("              Streaming + NoSQL: HBase, Cassandra");
    println!("              BI: Tableau, Power BI, Looker, Mode, Sigma, Hex (JDBC/ODBC/Arrow Flight)");
    println!("              Data science: dbt, Pandas, Polars, Apache Superset, Jupyter");
    println!("  Customers: ~600 paying customers");
    println!("            UBS, S&P Global, NCR, Capital One (some teams), Royal Caribbean, Diageo, Ford Direct");
    println!("            Maersk, US Department of Defense (Lakehouse Architecture, Mission Partner Environment)");
    println!("            sweet spot: F1000 enterprises with S3/ADLS lakes + Iceberg adoption + multi-cloud");
    println!("            cost-sensitive vs Snowflake — Dremio Cloud Standard FREE for dev/staging");
    println!("            US Federal + DoD strong (sovereignty + open-source angle)");
    println!("  Critique: smaller than Snowflake + Databricks — fewer connectors + fewer integrations");
    println!("           ecosystem of partners + marketplace smaller than Snowflake's");
    println!("           market positioning between Starburst (also lakehouse) + Snowflake (warehouse) sometimes confusing");
    println!("           Snowflake's Polaris (open Iceberg catalog) launched 2024 — directly threatens Arctic differentiator");
    println!("           AI/GenAI features behind Snowflake Cortex + Databricks AI in marketing");
    println!("           Reflections sometimes mysterious to non-experts (acceleration without user control)");
    println!("           Nadeau departure 2023 raised technical leadership questions");
    println!("  Differentiator: Apache Arrow native + Iceberg-leading + Reflections acceleration + 50% cheaper than Snowflake + Dremio Arctic Git-like catalog — for enterprises seeking Iceberg-native lakehouse without lock-in");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dremio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dremio(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dremio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dremio"), "dremio");
        assert_eq!(basename(r"C:\bin\dremio.exe"), "dremio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dremio.exe"), "dremio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dremio(&["--help".to_string()], "dremio"), 0);
        assert_eq!(run_dremio(&["-h".to_string()], "dremio"), 0);
        let _ = run_dremio(&["--version".to_string()], "dremio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dremio(&[], "dremio");
    }
}
