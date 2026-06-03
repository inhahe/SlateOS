#![deny(clippy::all)]

//! lightup-cli — OurOS Lightup Data (no-code data quality, SF, acquired by Acceldata 2024)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lightup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightup [OPTIONS]");
        println!("Lightup (OurOS) — no-code AI data quality (acquired by Acceldata Apr 2024)");
        println!();
        println!("Options:");
        println!("  --pulse                Pulse — quality monitor (proactive)");
        println!("  --indicator            Indicator — drill-down metric exploration");
        println!("  --no-code              Pre-built monitors, no SQL required");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lightup 2024 (OurOS)"); return 0; }
    println!("Lightup 2024 (OurOS) — No-Code AI Data Quality");
    println!("  Vendor: Lightup Data, Inc. (Mountain View) — ACQUIRED by Acceldata Apr 2024");
    println!("  Founders: Manu Bansal (CEO) + Vasudev Vikram + others, 2019");
    println!("          Manu: Stanford PhD (EE), ex-Uhana (ML for telco, sold to VMware)");
    println!("          founded to bring no-code ML-based data quality to enterprises");
    println!("          pitch: 'data quality without writing rules'");
    println!("  Funding (pre-acquisition):");
    println!("         Series A 2021: $15M led by A.Capital + Spider Capital");
    println!("         total raised ~$20M");
    println!("  Acquisition Apr 2024:");
    println!("         Acceldata acquired Lightup for undisclosed amount");
    println!("         strategic fit: Lightup's no-code UX + Acceldata's multi-layered platform");
    println!("         Manu Bansal joined Acceldata as Chief Product Officer");
    println!("         Lightup brand sunsetting; tech being integrated into Acceldata Torch module");
    println!("  Strategic position (pre-acquisition):");
    println!("                    pitch: 'pre-built data quality monitors — turn on, ship insights'");
    println!("                    target: non-technical data quality teams (analysts, stewards)");
    println!("                    primary competitor (pre-acq): Monte Carlo, Anomalo, Bigeye, Soda");
    println!("                    Lightup's wedge: no-code UI + pre-built monitor library + ML auto-thresholding");
    println!("                    differentiator: 'data analysts can use it, not just data engineers'");
    println!("                    Stanford PhD + ML credibility");
    println!("  Pricing (pre-acquisition):");
    println!("    Lightup Cloud — $30K-150K/yr typical");
    println!("    no free tier — enterprise sales-led");
    println!("    cheaper than Monte Carlo / Anomalo at comparable scale");
    println!("    post-Acceldata: bundled into Acceldata enterprise pricing tiers");
    println!("  Core platform:");
    println!("    - Pulse: continuous quality monitoring (freshness, volume, distribution)");
    println!("    - Indicator: drill-down exploration (where did the anomaly come from?)");
    println!("    - Pre-built monitor library: null rate, uniqueness, range, regex, business-rule, etc.");
    println!("    - No-code monitor creation: point-and-click in UI");
    println!("    - ML auto-thresholding: thresholds learned from history, not hand-set");
    println!("    - Anomaly attribution: identifies which segment (region, product, customer cohort) is off");
    println!("  Integrations (30+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Postgres");
    println!("    - dbt + Airflow + Dagster");
    println!("    - BI: Looker, Tableau (lineage)");
    println!("    - Alerts: Slack, PagerDuty, MS Teams, email, webhook");
    println!("    - Acceldata Torch (post-acquisition, deeply integrated)");
    println!("  Lightup CLI usage (legacy, pre-Acceldata):");
    println!("    lightup login");
    println!("    lightup monitors list --source snowflake-prod");
    println!("    lightup monitor create --type null-rate --column orders.email");
    println!("    lightup indicator drill --metric orders_count --segment region");
    println!("    (post-acquisition: `acceldata torch ...` CLI replaces these)");
    println!("  Customers (pre-acquisition, ~75+ paying):");
    println!("    - PG&E, Forrester, Hewlett Packard Enterprise");
    println!("    - several Fortune 500 retail + financial services");
    println!("    - sweet spot: mid-market data quality teams without dedicated data engineering");
    println!("    - heavy in: utilities, retail, financial services");
    println!("    - most customers continued under Acceldata post-acquisition");
    println!("  Critique (legacy + acquisition era):");
    println!("           smaller than Monte Carlo / Anomalo in installed base");
    println!("           independent product lifespan ending — full Acceldata integration mid-2025");
    println!("           no-code UX = limited customization vs SQL-first competitors");
    println!("           pricing pressure from cheaper Soda / Metaplane alternatives");
    println!("           Acceldata multi-pillar story = potential UX dilution as Lightup absorbed");
    println!("  Acquisition rationale (Acceldata's view):");
    println!("           Acceldata's Torch (quality) needed UX polish — Lightup brought that");
    println!("           consolidation in observability space: ~5+ similar startups, market shaking out");
    println!("           Acceldata bulks up for IPO narrative (2025-2026 target)");
    println!("           Lightup investors get partial liquidity in down-round-era exit");
    println!("           pattern: Datadog/Metaplane, Coalesce/Castor, Acceldata/Lightup — observability consolidation 2024");
    println!("  Differentiator: pre-built no-code monitor library + ML auto-thresholding + analyst-friendly UI + Stanford-PhD-led product + now backed by Acceldata's multi-pillar platform — the no-code data quality choice for non-engineering data teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lightup(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lightup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightup"), "lightup");
        assert_eq!(basename(r"C:\bin\lightup.exe"), "lightup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightup.exe"), "lightup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lightup(&["--help".to_string()], "lightup"), 0);
        assert_eq!(run_lightup(&["-h".to_string()], "lightup"), 0);
        assert_eq!(run_lightup(&["--version".to_string()], "lightup"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lightup(&[], "lightup"), 0);
    }
}
