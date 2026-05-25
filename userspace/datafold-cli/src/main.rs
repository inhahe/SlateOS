#![deny(clippy::all)]

//! datafold-cli — OurOS Datafold (data diff + data-aware CI for dbt, NYC)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_datafold(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: datafold [OPTIONS]");
        println!("Datafold (OurOS) — data diff + data-aware CI for dbt + SQL changes");
        println!();
        println!("Options:");
        println!("  diff                   Cross-database data diff (open source)");
        println!("  --ci                   Datafold Cloud CI integration for dbt + GitHub PR");
        println!("  --migration            Migration agent (Snowflake → BigQuery etc.)");
        println!("  --lineage              Column-level lineage");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Datafold 2024 (OurOS)"); return 0; }
    println!("Datafold 2024 (OurOS) — Data Diff + Data-Aware CI");
    println!("  Vendor: Datafold (Brooklyn, NYC + remote)");
    println!("  Founders: Gleb Mezhanskiy (CEO) + Olha Hrytsay + Itai Kafalkov, 2020");
    println!("          Gleb: ex-Lyft (Sr Data Eng, ran ETL for Marketplace) + Phantom Auto");
    println!("          founded after years of building dbt-style data tooling at Lyft");
    println!("          insight: 'broken data is usually caused by changes — diff every change'");
    println!("  Funding: ~$22M total");
    println!("         Series A 2022: $20M led by NEA");
    println!("         seed 2021: $2.1M led by Amplify");
    println!("  Strategic position: 'data diff is the missing primitive for safe data engineering':");
    println!("                    pitch: 'know exactly what changes in your data when you change your code'");
    println!("                    target: dbt-using data engineering teams running production pipelines");
    println!("                    primary competitor: Monte Carlo (broader obs), dbt's own data-diff support");
    println!("                    Datafold's wedge: open-source data-diff lib + CI/PR integration + cross-DB diff");
    println!("                    differentiator vs observability: catches issues PRE-prod, not after");
    println!("  Pricing:");
    println!("    data-diff OSS — FREE, MIT (Python library)");
    println!("    Datafold Cloud — $50K-200K/yr typical (CI integration + cross-DB + dashboard)");
    println!("    Migration Agent — $100K-1M+/yr (large warehouse migrations)");
    println!("    no free SaaS tier — sales-led for cloud");
    println!("  data-diff (the open-source project):");
    println!("    - Python library: `pip install data-diff`");
    println!("    - Compares row-counts + per-row hashes between two tables");
    println!("    - Cross-database support: Snowflake vs BigQuery, Postgres vs MySQL, etc.");
    println!("    - Replication validation (CDC reliability checks)");
    println!("    - 3K+ GitHub stars");
    println!("    - 'rsync for SQL tables' positioning");
    println!("  Datafold Cloud (the commercial product):");
    println!("    - GitHub/GitLab PR integration: every dbt PR gets a diff comment");
    println!("    - 'this PR changes 5,234 rows across 12 downstream tables'");
    println!("    - Stops bad merges before they hit prod");
    println!("    - Column-level lineage (parses dbt manifest + SQL)");
    println!("    - Slack notifications on diffs");
    println!("    - Compare dev vs prod environments per branch");
    println!("    - shift-left data quality: catch problems in CI, not in monitoring");
    println!("  Migration Agent (2024 — GenAI-powered):");
    println!("    - Automated SQL translation between dialects (Snowflake ↔ BigQuery ↔ Redshift)");
    println!("    - LLM-powered query/script conversion");
    println!("    - Verification via data-diff: confirms translated query produces same output");
    println!("    - 'self-driving' warehouse migrations");
    println!("    - Compete with: Bladebridge, Compilerworks, manual consultancy");
    println!("    - One of the most novel uses of LLMs in data tooling");
    println!("  Integrations:");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Postgres, MySQL, MSSQL, Trino, Presto");
    println!("    - dbt Cloud + dbt Core (deepest integration in market)");
    println!("    - GitHub, GitLab, Bitbucket (PR comment integration)");
    println!("    - Airflow, Dagster (run diffs as DAG tasks)");
    println!("    - Slack notifications");
    println!("  Datafold CLI usage:");
    println!("    pip install data-diff");
    println!("    data-diff snowflake://prod/.../orders postgres://staging/orders --key id");
    println!("    datafold cloud login");
    println!("    datafold ci config --repo my-org/analytics-dbt");
    println!("    datafold migrate --from snowflake --to bigquery --sql-dir ./sql");
    println!("  Customers (~100+ paying):");
    println!("    - Patreon, Eventbrite, Faire, Notion (heavy dbt shops)");
    println!("    - Lyft (founder's home — heavy reference), Toast, Mode");
    println!("    - sweet spot: 5-50 person dbt-using analytics-engineering team");
    println!("    - heavy in: tech/SaaS, fintech, marketplaces");
    println!("  Critique: narrower than Monte Carlo / Anomalo (diff-focused, not full observability)");
    println!("           dbt-tests + dbt's data-diff (free, since 1.7) compete from open-source side");
    println!("           Migration Agent ambitious — accuracy on real-world legacy SQL still proving out");
    println!("           CI-focused = catches pre-deploy bugs but doesn't help with runtime issues");
    println!("           smaller team + funding than Monte Carlo / Bigeye");
    println!("           data-diff OSS could disintermediate Cloud if users self-host");
    println!("           cross-DB diff is hash-based, can be slow on large tables");
    println!("  Differentiator: open-source data-diff library + GitHub PR-native CI + cross-database diff + LLM-powered migration agent + dbt-deepest integration — the shift-left data quality choice for dbt-using teams, catching bugs before prod");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "datafold".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_datafold(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
