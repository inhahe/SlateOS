#![deny(clippy::all)]

//! anomalo-cli — OurOS Anomalo (ML-first data quality, SF)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_anomalo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anomalo [OPTIONS]");
        println!("Anomalo (OurOS) — ML-first data quality (Instacart founders)");
        println!();
        println!("Options:");
        println!("  --checks               View ML auto-checks + custom rules");
        println!("  --notify               Configure alert channels (Slack, PagerDuty, Email)");
        println!("  --root-cause           Auto root-cause analysis on incidents");
        println!("  --unstructured         Unstructured data quality (text, images via VLM)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Anomalo 2024 (OurOS)"); return 0; }
    println!("Anomalo 2024 (OurOS) — ML-first Data Quality");
    println!("  Vendor: Anomalo, Inc. (Palo Alto / San Francisco)");
    println!("  Founders: Elliot Shmukler (CEO) + Jeremy Stanley (CTO), 2018");
    println!("          Elliot: ex-Instacart VP Product, LinkedIn growth team");
    println!("          Jeremy: ex-Instacart Head of Data Science, Sailthru CTO");
    println!("          philosophy: 'data quality with ML — not 10K hand-written tests'");
    println!("          contrast to Monte Carlo: Anomalo pitches deeper ML, narrower scope");
    println!("  Funding: ~$72M total through Series B (Dec 2023)");
    println!("         Series B Dec 2023: $33M led by SignalFire + Foundation Capital");
    println!("         Series A Feb 2022: $33M led by Norwest Venture Partners");
    println!("         seed: Two Sigma Ventures, Foundation Capital");
    println!("  Strategic position: deep ML auto-quality (vs Monte Carlo's broad observability):");
    println!("                    pitch: 'Anomalo finds the issues your dbt tests can't catch'");
    println!("                    target: large-warehouse data quality teams (Fortune 500 data orgs)");
    println!("                    primary competitor: Monte Carlo (broader), Bigeye (similar ML focus)");
    println!("                    Anomalo's edge: ML model sophistication + unstructured data (2024 unique)");
    println!("                    typical buyer: head of data engineering at retail/fintech/healthcare");
    println!("  Pricing: enterprise sales-led, ~$100K-$1M+/yr typical");
    println!("         no free tier — proof-of-value pilot then annual contract");
    println!("         pricing pegged to # of tables monitored + warehouse credits consumed by checks");
    println!("  Core checks (auto-enabled on every table):");
    println!("    - Freshness: did data arrive on schedule?");
    println!("    - Volume: row count anomalies vs historical baseline");
    println!("    - Schema: column add/drop/type-change detection");
    println!("    - Field-level: null rate, uniqueness, distribution drift");
    println!("    - Time-series anomalies: seasonal + trend-aware modeling");
    println!("    - Correlation drift: when column relationships change");
    println!("  ML approach (the differentiator):");
    println!("    - Time-series models per metric (not just threshold-based)");
    println!("    - Handles seasonality (day-of-week, month, holiday patterns)");
    println!("    - Auto-tunes sensitivity based on noise level");
    println!("    - Confidence intervals on anomaly scores");
    println!("    - Bayesian change-point detection for sudden shifts");
    println!("    - Less false-positive prone than naive z-score approaches");
    println!("  Root cause analysis:");
    println!("    - When an anomaly fires, Anomalo back-traces to upstream cause");
    println!("    - Correlates schema changes, volume drops, freshness issues across pipeline");
    println!("    - Identifies which segment of data is anomalous (e.g., 'all NULLs in US-East-2 region')");
    println!("    - Hypothesis ranking: most likely root causes ranked by historical precedent");
    println!("  Unstructured Data Quality (2024 — first in market):");
    println!("    - Quality checks on text (PII detection, language drift, toxicity)");
    println!("    - Image QA via VLMs (blurriness, content drift, brand compliance)");
    println!("    - Document QA: extracted-field consistency for LLM RAG pipelines");
    println!("    - Critical for GenAI: input data quality determines model quality");
    println!("  Integrations (40+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse");
    println!("    - Lakehouse: Iceberg, Delta, Hudi (2024)");
    println!("    - Object stores: S3, GCS, ADLS for unstructured data");
    println!("    - dbt + Airflow + Dagster + Prefect (orchestration-aware)");
    println!("    - Notification: Slack, PagerDuty, Opsgenie, MS Teams, email, webhook");
    println!("    - SSO/SCIM: Okta, Azure AD, Google Workspace");
    println!("  Anomalo CLI usage:");
    println!("    anomalo login");
    println!("    anomalo checks list --table prod.orders");
    println!("    anomalo checks run --table prod.orders --check freshness");
    println!("    anomalo root-cause --incident i-1234");
    println!("    anomalo unstructured scan --bucket s3://reviews/ --check toxicity");
    println!("  Customers (~150+ paying):");
    println!("    - Block (Square), Discover Financial, Notion, Buzzfeed");
    println!("    - Domino's Pizza, Ovo Energy (UK), Etsy, AT&T");
    println!("    - heavy in: financial services, retail/e-commerce, media");
    println!("    - sweet spot: 500K+ rows/day + 100+ critical tables");
    println!("  Critique: expensive — $100K floor pricing puts off smaller teams");
    println!("           ML auto-checks can over-fit to recent data (need historical horizon tuning)");
    println!("           narrower than Monte Carlo: lacks full lineage + BI/ML integration depth");
    println!("           dbt tests (free) + manual SQL still cover many use cases");
    println!("           Snowflake Cortex AI + Databricks Genie compete from warehouse side");
    println!("           unstructured data quality nascent — VLM costs significant on large image corpora");
    println!("  Differentiator: deepest ML-driven anomaly detection + first-to-market unstructured data quality + Instacart-veteran founders + clean root-cause UX — the data quality choice for teams that want ML doing the work, not humans writing rules");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "anomalo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_anomalo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_anomalo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/anomalo"), "anomalo");
        assert_eq!(basename(r"C:\bin\anomalo.exe"), "anomalo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("anomalo.exe"), "anomalo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_anomalo(&["--help".to_string()], "anomalo"), 0);
        assert_eq!(run_anomalo(&["-h".to_string()], "anomalo"), 0);
        let _ = run_anomalo(&["--version".to_string()], "anomalo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_anomalo(&[], "anomalo");
    }
}
