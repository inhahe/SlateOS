#![deny(clippy::all)]

//! acceldata-cli — OurOS Acceldata (multi-layered data observability, Campbell CA + Bangalore)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_accel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: acceldata [OPTIONS]");
        println!("Acceldata (OurOS) — multi-layered data observability (data + pipeline + cost)");
        println!();
        println!("Options:");
        println!("  --torch                Torch — data reliability + quality monitoring");
        println!("  --pulse                Pulse — compute + cluster observability");
        println!("  --flow                 Flow — pipeline + event monitoring");
        println!("  --finops               FinOps — Snowflake/Databricks/Hadoop cost optimization");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Acceldata 2024 (OurOS)"); return 0; }
    println!("Acceldata 2024 (OurOS) — Multi-layered Data Observability");
    println!("  Vendor: Acceldata Inc. (Campbell, CA + Bangalore, India)");
    println!("  Founders: Rohit Choudhary (CEO) + Ashwin Rajeeva (CTO), 2018");
    println!("          Rohit: ex-Hortonworks (Hadoop distribution co, sold to Cloudera 2019)");
    println!("          Ashwin: ex-Hortonworks engineering");
    println!("          founded by Hadoop ecosystem veterans who saw operational cost+performance issues at scale");
    println!("  Funding: ~$95M total through Series C (2022)");
    println!("         Series C Sep 2022: $50M led by March Capital + Insight Partners");
    println!("         Series B Aug 2021: $35M led by Insight Partners");
    println!("         seed/A: Lightspeed Venture Partners, Sorenson");
    println!("  Strategic position: 'data + compute + cost observability in one platform':");
    println!("                    pitch: 'three observability layers for the data lake/warehouse era'");
    println!("                    contrast: Monte Carlo only watches data; Acceldata also watches compute + cost");
    println!("                    target: large enterprises with Snowflake + Databricks + on-prem Hadoop");
    println!("                    primary competitor: Monte Carlo, Bigeye (narrower scope), Unravel Data (compute-only)");
    println!("                    Indian heritage = strong APAC enterprise distribution");
    println!("                    bet: data observability + FinOps converge — Acceldata covers both");
    println!("  Pricing: enterprise sales-led, $100K-$2M+/yr typical");
    println!("         Indian customers get lower-tier pricing; US enterprise full sticker");
    println!("         priced per data source + node count");
    println!("  Three pillars (the multi-layered story):");
    println!("    Torch — Data Reliability:");
    println!("      - Quality monitoring (ML-based + custom SQL checks)");
    println!("      - Schema drift, freshness, distribution, completeness");
    println!("      - Lineage across pipelines");
    println!("      - Compares to: Monte Carlo, Anomalo, Bigeye");
    println!("    Pulse — Compute Observability:");
    println!("      - Cluster health for Hadoop, Spark, Snowflake, Databricks, EMR");
    println!("      - Query performance + bottleneck analysis");
    println!("      - Auto-tuning recommendations");
    println!("      - Compares to: Unravel Data, Pepperdata, Datadog DB monitoring");
    println!("    Flow — Pipeline Observability:");
    println!("      - Airflow/dbt/Spark/Kafka pipeline run tracking");
    println!("      - SLA breach detection");
    println!("      - Event-driven monitoring (Kafka topic health)");
    println!("      - Compares to: Datafold, Anomalo Flow");
    println!("  FinOps (the differentiator vs pure observability vendors):");
    println!("    - Snowflake credit usage forecasting + anomaly detection");
    println!("    - Databricks DBU cost optimization");
    println!("    - Identify expensive/redundant queries + unused tables");
    println!("    - Chargeback reporting (cost by team/workload/cluster)");
    println!("    - Pepperdata + Slingshot + Capital One Open Source FinOps tools compete");
    println!("    - Acceldata's strength: combined with quality data = full picture");
    println!("  Hadoop legacy strength:");
    println!("    - Founders' Hortonworks background = deep Hadoop expertise");
    println!("    - Still serves banks/telco with on-prem CDH/HDP clusters");
    println!("    - Few competitors care about legacy Hadoop — Acceldata owns this niche");
    println!("    - Migration assistance: helps customers move Hadoop → Snowflake/Databricks");
    println!("  Integrations (60+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse");
    println!("    - On-prem: Cloudera (CDH/CDP), Hortonworks (HDP), Apache Hadoop");
    println!("    - Compute: EMR, Dataproc, HDInsight, Spark on K8s");
    println!("    - Streaming: Kafka, Kinesis, Pulsar");
    println!("    - Object stores: S3, GCS, ADLS, HDFS, MapR-FS");
    println!("    - dbt + Airflow + Oozie (yes, Oozie — Hadoop heritage)");
    println!("  Acceldata CLI usage:");
    println!("    acceldata login");
    println!("    acceldata torch monitors list --asset prod.fact_orders");
    println!("    acceldata pulse cluster status --cluster snowflake-prod");
    println!("    acceldata flow pipeline runs --dag daily_etl --status failed");
    println!("    acceldata finops query-cost --warehouse snowflake-analytics --top 50");
    println!("  Customers (~150+ paying):");
    println!("    - PubMatic, Phonepe, Oracle, HPE");
    println!("    - Verisk, True Digital, T-Mobile (heavy Acceldata customer for Hadoop migration)");
    println!("    - Many Indian banks + telco (Reliance Jio, ICICI, HDFC)");
    println!("    - sweet spot: enterprises with Hadoop + Snowflake or Databricks (hybrid environments)");
    println!("    - heavy in: financial services, telco, retail (especially APAC + India)");
    println!("  Critique: multi-pillar = complex to deploy, longer sales cycles");
    println!("           Hadoop strength = legacy positioning (Hadoop is shrinking)");
    println!("           Monte Carlo + Anomalo more focused = often quicker to value for pure data obs");
    println!("           FinOps story competes with Snowflake's own Account Usage views (free)");
    println!("           pricing high for mid-market");
    println!("           less prominent in US tech-startup buyers than competitors");
    println!("  Differentiator: only platform combining data + compute + cost observability + deep Hadoop heritage + APAC enterprise distribution — the data observability choice for hybrid (cloud + on-prem) enterprises and cost-conscious data orgs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "acceldata".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_accel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_accel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/acceldata"), "acceldata");
        assert_eq!(basename(r"C:\bin\acceldata.exe"), "acceldata.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("acceldata.exe"), "acceldata");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_accel(&["--help".to_string()], "acceldata"), 0);
        assert_eq!(run_accel(&["-h".to_string()], "acceldata"), 0);
        assert_eq!(run_accel(&["--version".to_string()], "acceldata"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_accel(&[], "acceldata"), 0);
    }
}
