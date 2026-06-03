#![deny(clippy::all)]

//! soda-cli — OurOS Soda (open-source data quality, Belgium)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_soda(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: soda [OPTIONS]");
        println!("Soda (OurOS) — open-source data quality (SodaCL + Soda Cloud)");
        println!();
        println!("Options:");
        println!("  scan                   Run quality scan (Soda Core)");
        println!("  --sodacl PATH          SodaCL YAML checks file");
        println!("  --data-source NAME     Connection to scan");
        println!("  --soda-cloud           Push results to Soda Cloud");
        println!("  --contracts            Data contracts feature");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Soda 3.3 (OurOS)"); return 0; }
    println!("Soda 3.3 (OurOS) — Open-Source Data Quality");
    println!("  Vendor: Soda Data NV (Brussels, Belgium + Amsterdam)");
    println!("  Founders: Tom Baeyens (CTO) + Maarten Masschelein (CEO), 2019");
    println!("          Tom: former founder of Process Engine (jBPM/Activiti, Camunda predecessor) — JBoss veteran");
    println!("          Maarten: ex-Collibra (lead Belgian data catalog company)");
    println!("          headquartered in Belgium — heavy EU presence");
    println!("  Funding: ~$31M total through Series B (2022)");
    println!("         Series B 2022: $25M led by Singular");
    println!("         Series A 2021: ~$11.5M led by HV Capital");
    println!("         seed: Hummingbird, Point Nine, others");
    println!("  Strategic position: open-source-first data quality (vs proprietary observability):");
    println!("                    pitch: 'data contracts written as code, owned by data producers'");
    println!("                    target: data engineers wanting open-source + Python-friendly tooling");
    println!("                    primary competitor: Great Expectations (OSS), Monte Carlo + Anomalo (closed)");
    println!("                    secondary: dbt tests + Elementary (dbt-native quality)");
    println!("                    Soda's wedge: SodaCL DSL + open-source core + cheaper cloud tier than competitors");
    println!("                    EU-data-residency story strong for European enterprise");
    println!("  Pricing (tiered, with FREE OSS core):");
    println!("    Soda Core — FREE, Apache 2.0 (CLI + Python library)");
    println!("    Soda Cloud Developer — FREE for individuals (limited scans)");
    println!("    Soda Cloud Team — ~$1.5K-5K/mo (team collaboration + alerting)");
    println!("    Soda Cloud Enterprise — $50K-300K+/yr (SSO, RBAC, audit, EU residency)");
    println!("    significantly cheaper than Monte Carlo / Anomalo at comparable scale");
    println!("  SodaCL (Soda Check Language) — the DSL:");
    println!("    - YAML-based check definitions");
    println!("    - Example: 'missing_count(email) = 0' or 'duplicate_count(order_id) = 0'");
    println!("    - Schema checks: 'schema: name in [string], age in [int]'");
    println!("    - Anomaly detection checks (built-in)");
    println!("    - Reference checks (cross-table consistency)");
    println!("    - Distribution checks (drift detection)");
    println!("    - User-defined SQL checks");
    println!("    - 200+ pre-built check types");
    println!("  Soda Core (OSS):");
    println!("    - Python library + CLI: `pip install soda-core`");
    println!("    - Runs scans locally or in CI/CD pipelines");
    println!("    - Connectors: 25+ warehouses including Snowflake/BigQuery/Databricks/Postgres/MySQL/Redshift");
    println!("    - Integrates into Airflow/Dagster/Prefect as a task");
    println!("    - Outputs JSON results or pushes to Soda Cloud");
    println!("  Soda Cloud (SaaS layer):");
    println!("    - Dashboard for check history + incident management");
    println!("    - Alert routing (Slack, PagerDuty, MS Teams, webhook)");
    println!("    - Anomaly detection ML models (Cloud-only)");
    println!("    - Data dictionary + business context");
    println!("    - Multi-team collaboration + RBAC");
    println!("    - Compliance: SOC 2 Type 2 + GDPR + EU residency option");
    println!("  Data Contracts (2024 push):");
    println!("    - Schema + freshness + quality SLAs codified in YAML");
    println!("    - Producer signs the contract; consumer verifies");
    println!("    - Versioned contracts in Git");
    println!("    - Soda is one of the most active data-contract evangelists");
    println!("    - Compete with: Gable.ai, dbt model contracts");
    println!("  Integrations:");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres, MySQL, MSSQL, Trino");
    println!("    - dbt: SodaCL checks runnable as dbt tests via integration");
    println!("    - Orchestration: Airflow, Dagster, Prefect");
    println!("    - Catalog: Atlan, DataHub, Collibra (push check results)");
    println!("    - Alerts: Slack, MS Teams, PagerDuty, Opsgenie, Jira, ServiceNow");
    println!("  Soda CLI usage:");
    println!("    soda scan -d snowflake_prod -c soda-config.yml checks.yml");
    println!("    soda test-connection -d snowflake_prod -c soda-config.yml");
    println!("    soda cloud login");
    println!("    soda cloud upload-historic-scans");
    println!("    soda contracts verify --contract orders.contract.yml");
    println!("  Customers (~200+ paying + thousands OSS):");
    println!("    - Disney+, Hello Fresh, JustEat Takeaway, Air France-KLM");
    println!("    - Wise (Transferwise), ABN AMRO, ING, KBC (EU financial)");
    println!("    - Belfius, Daimler/Mercedes, EDF, BBVA, Bolt");
    println!("    - sweet spot: European enterprises with data sovereignty needs");
    println!("    - heavy in: EU financial services, automotive, telco, retail");
    println!("  Soda OSS ecosystem:");
    println!("    - 5K+ GitHub stars on soda-core");
    println!("    - 200+ community contributors");
    println!("    - SodaCL becoming a de-facto check spec for some shops");
    println!("    - Belgium/EU data-engineering community heavily involved");
    println!("  Critique: Soda Cloud less polished than Monte Carlo UX");
    println!("           narrower ML/auto-monitoring than Anomalo");
    println!("           SodaCL is yet another DSL to learn (vs SQL or Python)");
    println!("           lineage features behind Monte Carlo + Atlan");
    println!("           OSS Core is good but most teams need Cloud for collaboration");
    println!("           Great Expectations (more mature OSS) still strong competitor");
    println!("           data contracts category still emerging — no clear winner yet");
    println!("  Differentiator: open-source-first + SodaCL declarative DSL + EU data residency + active data contracts evangelist + cheaper than US competitors — the data quality platform of choice for European enterprises and Python-friendly teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "soda".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_soda(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_soda};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/soda"), "soda");
        assert_eq!(basename(r"C:\bin\soda.exe"), "soda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("soda.exe"), "soda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_soda(&["--help".to_string()], "soda"), 0);
        assert_eq!(run_soda(&["-h".to_string()], "soda"), 0);
        assert_eq!(run_soda(&["--version".to_string()], "soda"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_soda(&[], "soda"), 0);
    }
}
