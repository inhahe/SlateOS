#![deny(clippy::all)]
//! planetscale-cli — personality CLI for PlanetScale, the serverless MySQL
//! database built on Vitess by the team that ran YouTube's MySQL fleet.
//!
//! Founded 2018 in SF by Sam Lambert (CEO, ex-GitHub VP eng), Jiten Vaidya,
//! and Sugu Sougoumarane (the latter two are co-creators of Vitess at
//! YouTube). $50M Series C in May 2021 led by Kleiner Perkins at >$1B
//! valuation. Famous for the 'branch and deploy schema' developer workflow
//! ('database branching like git') and for non-blocking online schema
//! changes via Vitess. Discontinued the free Hobby tier in April 2024.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — PlanetScale serverless MySQL personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Vitess lineage, YouTube heritage");
    println!("    vitess        Open source database sharding layer");
    println!("    branching     Database branches like git branches");
    println!("    deploy        Deploy requests + revert window");
    println!("    insights      Query insights and slow query analyzer");
    println!("    boost         PlanetScale Boost query caching");
    println!("    pricing       Free tier retirement, current bands");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("planetscale-cli 0.1.0 (Vitess-lineage personality build)"); }

fn run_about() {
    println!("PlanetScale, Inc.");
    println!("  Founded:    2018, San Francisco.");
    println!("  Founders:   Sam Lambert (CEO, ex-GitHub VP eng),");
    println!("              Jiten Vaidya, Sugu Sougoumarane.");
    println!("  Heritage:   Vaidya and Sougoumarane co-built Vitess at YouTube");
    println!("              to scale YouTube's MySQL fleet to billions of qps.");
    println!("  Funding:    ~$105M total. $50M Series C May 2021 at >$1B,");
    println!("              led by Kleiner Perkins.");
    println!("  Pitch:      Serverless MySQL with the developer experience of");
    println!("              git branching for schema changes.");
}

fn run_vitess() {
    println!("Vitess — the open source backbone.");
    println!("  Distributed MySQL sharding layer originally built at YouTube.");
    println!("  CNCF graduated project.");
    println!("  VTGate proxy fans out queries to per-shard MySQL.");
    println!("  Online schema changes via shadow tables + cutover.");
    println!("  Connection-pool multiplexing dramatically reduces conn count.");
    println!("  PlanetScale ships a hosted, hardened, productized Vitess.");
}

fn run_branching() {
    println!("Database Branching — the developer headline feature.");
    println!("  Every database has a main branch and any number of dev branches.");
    println!("  A branch is a fast clone of the schema (and optionally data).");
    println!("  Devs run schema changes on a branch, exercise the app, iterate.");
    println!("  Production is shielded: you cannot DDL directly against main.");
    println!("  All schema changes ship via Deploy Requests.");
}

fn run_deploy() {
    println!("Deploy Requests + Revert Window.");
    println!("  A Deploy Request is the PR for a schema change.");
    println!("  Review the diff, run a non-blocking online schema change,");
    println!("  cutover atomically when ready.");
    println!("  Revert Window: keep the old schema available for ~30 minutes");
    println!("  so a botched change can be rolled back without data loss.");
    println!("  This workflow is the moat: schema changes feel like deploys.");
}

fn run_insights() {
    println!("Insights — query observability.");
    println!("  Per-query latency, throughput, row-read counts, error rates.");
    println!("  Aggregated by query fingerprint (parameter-stripped).");
    println!("  Slow query log + recommendations.");
    println!("  Helps customers find the N+1 in production before it bills.");
}

fn run_boost() {
    println!("PlanetScale Boost (in 2023-era branding).");
    println!("  Per-query result caching layer that knows the database schema.");
    println!("  Cache invalidation triggered by upstream writes, not TTL.");
    println!("  Designed for read-heavy hot paths where a CDN would be wrong");
    println!("  (per-user data) but raw MySQL is too slow.");
}

fn run_pricing() {
    println!("Pricing history:");
    println!("  Hobby (free tier) was retired in April 2024.");
    println!("  Current entry-level Scaler tier is paid ($39/mo at announcement).");
    println!("  Scaler -> Scaler Pro -> Team -> Enterprise.");
    println!("  Pricing scales on row-reads, storage, and number of branches.");
    println!("  The retirement of free Hobby drove visible community churn");
    println!("  toward Neon, Turso, and self-hosted MySQL alternatives.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Square, Cash App     payments at scale");
    println!("  Slack                team-data workloads");
    println!("  GitHub               various services");
    println!("  Figma                design data");
    println!("  Etsy                 marketplace data");
    println!("  Intercom             customer messaging");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "planetscale-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "vitess" => run_vitess(),
        "branching" => run_branching(),
        "deploy" => run_deploy(),
        "insights" => run_insights(),
        "boost" => run_boost(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_vitess();
        run_branching();
        run_deploy();
        run_insights();
        run_boost();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("planetscale-cli");
        print_version();
    }
}
