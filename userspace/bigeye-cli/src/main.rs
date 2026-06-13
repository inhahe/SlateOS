#![deny(clippy::all)]

//! bigeye-cli — SlateOS Bigeye (data observability, Uber data team alumni)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bigeye(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bigeye [OPTIONS]");
        println!("Bigeye (Slate OS) — data observability for enterprise data teams");
        println!();
        println!("Options:");
        println!("  --metrics              200+ pre-built quality metrics");
        println!("  --monitor              Autometrics + custom monitors");
        println!("  --lineage              Column-level lineage");
        println!("  --slack                Slack-native incident management");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bigeye 2024 (Slate OS)"); return 0; }
    println!("Bigeye 2024 (Slate OS) — Data Observability");
    println!("  Vendor: Bigeye Inc. (San Francisco)");
    println!("  Founders: Kyle Kirwan (CEO) + Egor Gryaznov (CTO), 2019");
    println!("          both: ex-Uber data team — built Databook, Uber's internal data catalog (2017-2019)");
    println!("          Databook influence: deep understanding of metadata-at-scale problems");
    println!("          Kyle: also ex-Yahoo data team");
    println!("          founded Bigeye to take Uber's data observability ideas public");
    println!("  Funding: ~$66M total through Series B");
    println!("         Series B Mar 2022: $45M led by Coatue + Sequoia");
    println!("         Series A Jul 2021: $17M led by Sequoia");
    println!("         seed 2020: Costanoa Ventures, Sequoia angels");
    println!("  Strategic position: 'observability for data engineers, by data engineers':");
    println!("                    pitch: 'Bigeye finds the metrics that matter and watches them automatically'");
    println!("                    target: data engineering teams at mid-large enterprise");
    println!("                    primary competitor: Monte Carlo (broader), Anomalo (ML focus), Soda (OSS)");
    println!("                    Bigeye's wedge: 200+ pre-built metrics library + Autometrics ML + Slack UX");
    println!("                    Uber-engineering-veteran credibility for selling to data eng leads");
    println!("  Pricing: enterprise sales-led, $50K-$500K+/yr typical");
    println!("         free trial / pilot then annual contract");
    println!("         priced per table/asset monitored");
    println!("  Core platform:");
    println!("    - 200+ pre-built data quality metrics (Bigeye's open-source 'Toretto' framework)");
    println!("    - Examples: percent_null, percent_unique, mean, p99, count_distinct, regex_match");
    println!("    - Apply any metric to any column with one click — no SQL needed");
    println!("    - Thresholds auto-set via ML (Autometrics) or manual");
    println!("    - Schema drift detection + freshness + volume monitoring");
    println!("  Autometrics (ML-driven monitoring):");
    println!("    - Anomaly detection trained per-metric on history");
    println!("    - Detects: outliers, trend shifts, seasonality breaks, missing data points");
    println!("    - Confidence scoring on alerts (reduces false-positive noise)");
    println!("    - Compete head-on with Anomalo's ML approach");
    println!("  Column-level lineage:");
    println!("    - Parses SQL across Snowflake/BigQuery/Databricks/Redshift");
    println!("    - dbt-aware: ingests dbt manifest for richer lineage");
    println!("    - Impact analysis: 'which downstream BI/ML is broken?'");
    println!("    - Lineage powers incident root cause + ownership routing");
    println!("  Slack-native experience:");
    println!("    - Issues raised + triaged in Slack threads");
    println!("    - Acknowledge / resolve / mute from Slack");
    println!("    - Engineers don't leave the chat tool");
    println!("    - One of Bigeye's UX bets — less context-switching than Monte Carlo dashboard");
    println!("  Integrations (40+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres, MySQL");
    println!("    - Lakehouse: Iceberg, Delta (2024)");
    println!("    - dbt + Airflow + Dagster + Prefect");
    println!("    - BI: Looker, Tableau, Mode, Sigma (lineage)");
    println!("    - Alert routing: Slack, PagerDuty, Opsgenie, MS Teams, email, webhook");
    println!("    - SSO/SCIM: Okta, Azure AD, Google");
    println!("  Bigeye CLI usage:");
    println!("    bigeye login");
    println!("    bigeye metric apply --table prod.orders --metric percent_null");
    println!("    bigeye autometrics enable --table prod.orders");
    println!("    bigeye issues list --status open");
    println!("    bigeye lineage --asset prod.dashboards.revenue");
    println!("  Customers (~150+ paying):");
    println!("    - Instacart, Riot Games, Octopus Energy");
    println!("    - Clearcover, Confluent (yes, Confluent uses Bigeye), Phreesia");
    println!("    - sweet spot: 50-200 person data orgs at mid-large companies");
    println!("    - heavy in: tech/SaaS, gaming, energy");
    println!("  Open-source contributions:");
    println!("    - Toretto: 200+ data quality metrics, MIT license");
    println!("    - Active in data-engineering Slack communities + Locally Optimistic");
    println!("  Critique: smaller installed base than Monte Carlo");
    println!("           Anomalo's ML approach more sophisticated for some use cases");
    println!("           Snowflake Cortex AI competes for free from warehouse");
    println!("           growth slower than peak — data-observability category cooling slightly");
    println!("           less developed catalog/governance features than Atlan / Collibra");
    println!("           dbt-tests-as-baseline question (free) — must justify premium");
    println!("  Differentiator: Uber-data-team founders + 200+ pre-built metrics library + Slack-native incident UX + Autometrics ML + open-source Toretto library — the data observability platform built by engineers, for engineers");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bigeye".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bigeye(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bigeye};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bigeye"), "bigeye");
        assert_eq!(basename(r"C:\bin\bigeye.exe"), "bigeye.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bigeye.exe"), "bigeye");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bigeye(&["--help".to_string()], "bigeye"), 0);
        assert_eq!(run_bigeye(&["-h".to_string()], "bigeye"), 0);
        let _ = run_bigeye(&["--version".to_string()], "bigeye");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bigeye(&[], "bigeye");
    }
}
