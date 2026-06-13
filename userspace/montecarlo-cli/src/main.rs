#![deny(clippy::all)]

//! montecarlo-cli — SlateOS Monte Carlo Data (data observability category creator, SF)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: montecarlo [OPTIONS]");
        println!("Monte Carlo (SlateOS) — data observability platform (the category creator)");
        println!();
        println!("Options:");
        println!("  --incidents            View data incidents (freshness, volume, schema, distribution)");
        println!("  --lineage              Field-level lineage across stack");
        println!("  --monitors             ML-based + custom SQL monitors");
        println!("  --circuit-breakers     Halt downstream pipelines on bad data");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Monte Carlo 2024 (SlateOS)"); return 0; }
    println!("Monte Carlo 2024 (SlateOS) — Data Observability");
    println!("  Vendor: Monte Carlo Data, Inc. (San Francisco)");
    println!("  Founders: Barr Moses (CEO) + Lior Gavish (CTO), 2019");
    println!("          Barr: ex-Gainsight (VP Ops, ran a 60-person data team — coined 'data downtime')");
    println!("          Lior: ex-Barracuda (security infra) + serial founder");
    println!("          coined the phrase 'data observability' modeled on Datadog for infra ('data downtime' = 'when data is missing, wrong, or stale')");
    println!("  Funding: ~$236M total through Series D (May 2022 @ $1.6B valuation)");
    println!("         Series D May 2022: $135M led by IVP at unicorn $1.6B valuation");
    println!("         Series C Aug 2021: $60M led by ICONIQ Growth");
    println!("         Series B Feb 2021: $25M led by Redpoint");
    println!("         Series A Jul 2020: $16M led by Accel + GGV");
    println!("         seed 2019: Accel + many data-VC angels");
    println!("  ARR: estimated $50M+ (private, growing — slower in 2023 macro cooldown)");
    println!("  Strategic position: 'data downtime' = Datadog for data:");
    println!("                    pitch: 'your dashboards lie 30% of the time and you don't know it'");
    println!("                    target: data engineering + analytics teams running production pipelines");
    println!("                    primary competitor: Anomalo, Bigeye, Soda, Acceldata, Datafold, Metaplane");
    println!("                    secondary: dbt tests (free but coverage-limited), Great Expectations (OSS)");
    println!("                    cloud-native data observability — sits on warehouse + BI + transform tools");
    println!("                    moat: best ML auto-monitoring + biggest brand in the category");
    println!("  Pricing (asset + connector tier):");
    println!("    no free tier — sales-led only");
    println!("    Starter — $50K-100K/yr (small warehouse, few connectors)");
    println!("    Pro — $100K-300K/yr (mid-market, multi-warehouse)");
    println!("    Enterprise — $300K-2M+/yr (Fortune 500, custom integrations, SSO/SCIM/dedicated CSM)");
    println!("    pricing pegged to: # of data assets monitored + # of connector types");
    println!("  Core platform — 5 pillars of data observability:");
    println!("    1. Freshness — did data arrive on schedule?");
    println!("    2. Volume — is row count anomalous (too few/too many rows)?");
    println!("    3. Schema — did a column type change / get dropped?");
    println!("    4. Distribution — are values within expected range / null rate?");
    println!("    5. Lineage — which upstream/downstream assets are affected?");
    println!("  Auto-monitoring (the differentiator):");
    println!("    - ML models trained per-table on historical patterns");
    println!("    - Anomaly detection without manual rule writing");
    println!("    - Coverage: every table by default, no opt-in needed");
    println!("    - Severity scoring + auto-prioritization");
    println!("    - Incident IQ: root cause analysis suggesting the upstream issue");
    println!("  Custom monitors (SQL-based):");
    println!("    - Field health metrics (% null, % unique, % matches regex)");
    println!("    - Custom SQL queries with thresholds");
    println!("    - Comparison monitors (this run vs last run, this table vs that table)");
    println!("    - Schedule-based or event-triggered (run after dbt run finishes)");
    println!("  Lineage (column-level):");
    println!("    - Parses SQL across Snowflake/BigQuery/Redshift/Databricks/dbt");
    println!("    - Tracks column transformations through joins/CTEs/views");
    println!("    - Impact radius: 'this dashboard depends on this column'");
    println!("    - Domain ownership: who owns this asset?");
    println!("  Circuit Breakers (kill switches):");
    println!("    - Pre-built dbt/Airflow operators that fail the DAG on bad data");
    println!("    - Prevents bad data from cascading to BI/ML models");
    println!("    - Industry-leading feature (Anomalo + Bigeye copied this)");
    println!("  Performance Monitoring (2023+):");
    println!("    - Snowflake/BigQuery query cost + performance tracking");
    println!("    - Identify slow/expensive queries draining warehouse credits");
    println!("    - Compete with Capital One Slingshot + Bluesky Data");
    println!("  Apollo (GenAI agent, 2024):");
    println!("    - Natural language incident triage");
    println!("    - Auto-write incident summaries");
    println!("    - Suggest fixes via LLM analysis of pipeline + query history");
    println!("  Integrations (50+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres, MySQL");
    println!("    - Transformation: dbt Cloud + Core (every dbt run posts metadata)");
    println!("    - Orchestrators: Airflow, Dagster, Prefect");
    println!("    - BI: Looker, Tableau, Power BI, Mode, Sigma (lineage-aware)");
    println!("    - Alert routing: PagerDuty, Opsgenie, Slack, MS Teams, email, webhook");
    println!("    - Lakehouse: Iceberg, Delta, Hudi tables (2024 native support)");
    println!("  Monte Carlo CLI usage:");
    println!("    montecarlo login");
    println!("    montecarlo incidents list --status open");
    println!("    montecarlo monitors create --type freshness --table prod.orders");
    println!("    montecarlo lineage --asset 'prod.dashboards.revenue_dashboard'");
    println!("    montecarlo circuit-breaker enable --dag daily_etl");
    println!("  Customers (500+ paying):");
    println!("    - Fox, Vimeo, JetBlue, Mercari, AutoTrader UK, ThredUp");
    println!("    - PepsiCo, CNN, Roche, Affirm, SoFi");
    println!("    - Yotpo, Mindbody, Wayfair, Drift");
    println!("    - sweet spot: mid-market and enterprise data teams (5-200 data engineers)");
    println!("    - heavy in: e-commerce, media, fintech, SaaS");
    println!("  Critique: expensive ($100K+ starting price common)");
    println!("           dbt tests (free) cover 60-70% of common cases for free");
    println!("           Anomalo + Bigeye + Metaplane gaining traction at lower price points");
    println!("           Snowflake Cortex AI + Snowflake Trail (observability) compete from warehouse");
    println!("           Datadog Database Monitoring expanding into 'data observability' adjacent");
    println!("           ML auto-monitoring can produce false-positive noise (tuning required)");
    println!("           field-level lineage on legacy SQL parsing still imperfect (joins, recursive CTEs)");
    println!("           category growth slowing — many buyers question ROI vs cheaper alternatives");
    println!("  Differentiator: category creator brand + best-in-class auto-monitoring + 5-pillar framework + 50+ integrations + circuit breakers — the data observability platform most enterprise data teams default to evaluating first");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "montecarlo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/montecarlo"), "montecarlo");
        assert_eq!(basename(r"C:\bin\montecarlo.exe"), "montecarlo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("montecarlo.exe"), "montecarlo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mc(&["--help".to_string()], "montecarlo"), 0);
        assert_eq!(run_mc(&["-h".to_string()], "montecarlo"), 0);
        let _ = run_mc(&["--version".to_string()], "montecarlo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mc(&[], "montecarlo");
    }
}
