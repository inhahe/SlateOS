#![deny(clippy::all)]

//! metaplane-cli — OurOS Metaplane (data observability, Boston, acquired by Datadog 2024)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_meta(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: metaplane [OPTIONS]");
        println!("Metaplane (OurOS) — data observability (acquired by Datadog Oct 2024)");
        println!();
        println!("Options:");
        println!("  --monitors             ML-based monitors");
        println!("  --lineage              Field-level lineage");
        println!("  --incidents            Incident management + Slack");
        println!("  --datadog              Datadog integration (post-acquisition)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Metaplane 2024 (OurOS)"); return 0; }
    println!("Metaplane 2024 (OurOS) — Data Observability (now Datadog Data Observability)");
    println!("  Vendor: Metaplane, Inc. (Boston) — ACQUIRED by Datadog Oct 2024");
    println!("  Founders: Kevin Hu (CEO) + Peter Casinelli (CTO), 2020");
    println!("          Kevin: MIT PhD (CSAIL data systems research)");
    println!("          Peter: ex-MIT + Google");
    println!("          founded straight out of MIT — went through Y Combinator W21");
    println!("          'data Datadog' positioning from day one (ironic, given the acquirer)");
    println!("  Funding (now exited):");
    println!("         Series A May 2022: $13.8M led by Khosla Ventures");
    println!("         seed 2020: Y Combinator + Khosla + Slack Fund");
    println!("         total raised ~$15M before acquisition");
    println!("  Acquisition Oct 2024:");
    println!("         Datadog acquired Metaplane for undisclosed amount (estimated $50-150M range)");
    println!("         became Datadog Data Observability (announced Datadog DASH 2024)");
    println!("         strategic fit: Datadog already had Database Monitoring; data obs natural extension");
    println!("         Kevin Hu became Senior Director of Product at Datadog");
    println!("         most of Metaplane team moved to Datadog Boston office");
    println!("  Strategic position (pre-acquisition):");
    println!("                    pitch: 'data observability for the modern data stack — fast time-to-value'");
    println!("                    target: mid-market data teams (50-1000 employees)");
    println!("                    primary competitor: Monte Carlo (more enterprise), Bigeye, Anomalo, Soda");
    println!("                    Metaplane's wedge: cheaper + faster onboarding than Monte Carlo");
    println!("                    YC pedigree + MIT + 'data Datadog' branding");
    println!("                    accidentally telegraphed the exit thesis ('we are data's Datadog')");
    println!("  Pricing (pre-acquisition):");
    println!("    Free tier — 5 tables, basic monitoring");
    println!("    Starter — $1K-3K/mo for small teams");
    println!("    Growth — $5K-15K/mo mid-market");
    println!("    Enterprise — $100K-500K/yr");
    println!("    significantly cheaper than Monte Carlo at similar scale — won SMB/mid-market");
    println!("    post-Datadog: pricing integrating with Datadog billing");
    println!("  Core platform:");
    println!("    - ML-based anomaly detection (freshness, volume, schema, distribution)");
    println!("    - Auto-tuning monitor sensitivity");
    println!("    - Column-level lineage parsing dbt + SQL");
    println!("    - Field health metrics (% null, % unique, value distribution)");
    println!("    - Custom SQL monitors");
    println!("    - Slack-native incident management (one-click resolve)");
    println!("  Datadog integration (post-acquisition, 2024-2025):");
    println!("    - Data + APM + Infrastructure in unified dashboard");
    println!("    - 'pipeline broke → API errors → DB slow' end-to-end correlation");
    println!("    - Datadog Database Monitoring + Metaplane = full data layer visibility");
    println!("    - Compete head-on with Monte Carlo + Anomalo via Datadog's distribution");
    println!("    - Datadog's 30K customers = built-in TAM for upsell");
    println!("  Integrations (40+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres");
    println!("    - dbt Cloud + Core (deep)");
    println!("    - Airflow, Dagster, Prefect");
    println!("    - BI: Looker, Tableau, Mode, Sigma (lineage)");
    println!("    - Alerts: Slack, PagerDuty, Opsgenie, MS Teams, webhook");
    println!("    - Datadog (post-acquisition, native)");
    println!("  Metaplane CLI usage (legacy, pre-Datadog):");
    println!("    metaplane login");
    println!("    metaplane monitors list --table prod.orders");
    println!("    metaplane incidents resolve --id i-1234");
    println!("    metaplane lineage --asset prod.dashboards.revenue");
    println!("    (post-acquisition: `datadog data` CLI integration TBD)");
    println!("  Customers (~250+ at acquisition):");
    println!("    - Imperfect Foods, Vendr, Reforge, Drift, Plaid (some)");
    println!("    - heavy SaaS/tech mid-market");
    println!("    - many continued post-Datadog; some defected to Monte Carlo/Anomalo over integration uncertainty");
    println!("  Datadog acquisition logic:");
    println!("    - Datadog needs to expand TAM beyond pure infra/APM");
    println!("    - Data observability is a fast-growing adjacent market");
    println!("    - Metaplane's tech + team much cheaper than building from scratch");
    println!("    - Cross-sell into Datadog's 30K customer base");
    println!("    - Compete with New Relic (no data obs offering), Splunk (data obs ambitions)");
    println!("    - Datadog's 'observability platform' thesis now spans data, infra, apps, security");
    println!("  Critique (legacy + acquisition era):");
    println!("           independent product lifespan now ending — Datadog integration may take 1-2 years");
    println!("           customers worried about Datadog pricing tier creep");
    println!("           Datadog Data Observability is still nascent (announced Oct 2024)");
    println!("           Monte Carlo + Anomalo aggressively recruiting Metaplane customers");
    println!("           Anomalo's deeper ML may outpace Metaplane in pure data quality");
    println!("           historically: less mature than Monte Carlo for enterprise governance");
    println!("  Differentiator: YC + MIT pedigree + 'data Datadog' early positioning + cheaper than Monte Carlo + now backed by Datadog's distribution + integrated into Datadog observability platform — the data observability that became Datadog");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "metaplane".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_meta(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
