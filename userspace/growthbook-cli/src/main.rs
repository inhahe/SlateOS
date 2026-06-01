#![deny(clippy::all)]
//! growthbook-cli — personality CLI for GrowthBook, the open-source
//! warehouse-native feature flag and experimentation platform.
//!
//! Founded 2020 by Jeremy Dorn and Graham Mcneilly, YC W22 batch. MIT-licensed
//! OSS core plus a managed cloud and an Enterprise tier. The differentiator
//! is "warehouse-native": metric computation runs in the customer's data
//! warehouse (Snowflake, BigQuery, Redshift, Databricks, ClickHouse, Postgres,
//! Athena) so raw event data never leaves the warehouse. Implements Bayesian
//! statistics + CUPED variance reduction by default.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — GrowthBook OSS feature flag + experimentation CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, YC W22, MIT license");
    println!("    warehouse     Warehouse-native experiment engine");
    println!("    sdks          SDK languages and edge integrations");
    println!("    flags         Flag types and targeting model");
    println!("    bayesian      Default analysis mode + CUPED");
    println!("    selfhost      Self-host vs Cloud vs Enterprise");
    println!("    integrations  Segment, Snowflake, Mixpanel, GA, etc.");
    println!("    pricing       OSS free, Cloud tiers, Enterprise");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("growthbook-cli 0.1.0 (Warehouse-Native personality build)"); }

fn run_about() {
    println!("GrowthBook (Growth Book, Inc.)");
    println!("  Founded:   2020");
    println!("  Founders:  Jeremy Dorn, Graham Mcneilly");
    println!("  YC batch:  W22");
    println!("  License:   MIT (server + SDKs).");
    println!("  Source:    github.com/growthbook/growthbook");
    println!("  Tagline:   'The open source feature flagging and");
    println!("             A/B testing platform.'");
}

fn run_warehouse() {
    println!("Warehouse-native experiment analysis.");
    println!("  Metrics are SQL definitions stored in GrowthBook.");
    println!("  Queries run on the customer's warehouse, results are summarised");
    println!("  and only those summaries (means, variances, counts) are sent");
    println!("  back to the GrowthBook control plane.");
    println!("  Result: no per-event tax, raw PII never copied, and experiments");
    println!("  reuse the same business metrics that the BI team already trusts.");
    println!("  Supported warehouses: Snowflake, BigQuery, Redshift, Databricks,");
    println!("  ClickHouse, Postgres, MySQL, MS SQL Server, Athena.");
}

fn run_sdks() {
    println!("SDK matrix:");
    println!("  Server     JavaScript/Node, Python, Ruby, PHP, Go, Java,");
    println!("             Kotlin, C#, Elixir.");
    println!("  Client     React, React Native, Vue, Angular, vanilla JS.");
    println!("  Mobile     iOS (Swift), Android (Kotlin), Flutter.");
    println!("  Edge       Cloudflare Workers, Fastly Compute, Vercel Edge,");
    println!("             AWS Lambda@Edge, Akamai EdgeWorkers.");
    println!("  All SDKs evaluate flags locally from a 'features' payload");
    println!("  fetched once and updated via SSE.");
}

fn run_flags() {
    println!("Flag types: boolean, string, number, JSON.");
    println!("Targeting:");
    println!("  Attribute conditions (eq, in, regex, semver, etc.).");
    println!("  Saved groups (reusable cohort lists).");
    println!("  Pre-requisites (gate flag on another flag's value).");
    println!("  Schedules (auto-toggle at time T).");
    println!("  Rollout rules with sticky-bucket hashing.");
    println!("  Experiment rules — same flag drives an experiment variation.");
}

fn run_bayesian() {
    println!("Default analysis: Bayesian.");
    println!("  Chance to Beat Control (CTBC) as the headline number.");
    println!("  Credible intervals, no p-values.");
    println!("  Adjustable priors per metric.");
    println!("Variance reduction:");
    println!("  CUPED (Controlled-experiment Using Pre-Experiment Data)");
    println!("  ratio metrics with delta method.");
    println!("Frequentist mode also supported, with sequential testing.");
}

fn run_selfhost() {
    println!("Deployment options:");
    println!("  Self-hosted OSS  Docker Compose, Kubernetes Helm chart, ");
    println!("                   single-binary. MIT licensed, run it yourself.");
    println!("  Cloud Free       managed, generous free tier.");
    println!("  Cloud Pro        managed, more environments + SSO + SCIM.");
    println!("  Enterprise       on-prem with vendor support, custom SLAs,");
    println!("                   SOC 2 / HIPAA features.");
}

fn run_integrations() {
    println!("Integrations:");
    println!("  Event sources       Segment, Rudderstack, Mixpanel, Amplitude,");
    println!("                      GA4, Heap, PostHog (raw events flow to");
    println!("                      warehouse where GrowthBook reads them).");
    println!("  Warehouses          Snowflake, BigQuery, Redshift, Databricks,");
    println!("                      ClickHouse, Postgres, Athena, MS SQL.");
    println!("  Notifications       Slack, Discord, generic webhooks.");
    println!("  Code references     Github action scans for flag usage in code.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  OSS              free forever, MIT, self-hosted.");
    println!("  Cloud Free       up to 3 team members, basic features.");
    println!("  Cloud Pro        per-seat, includes Encrypted SDK endpoints,");
    println!("                   Audit log, custom roles.");
    println!("  Enterprise       custom contract, SSO/SAML, dedicated support,");
    println!("                   on-prem option with vendor-managed updates.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "growthbook-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "warehouse" => run_warehouse(),
        "sdks" => run_sdks(),
        "flags" => run_flags(),
        "bayesian" => run_bayesian(),
        "selfhost" => run_selfhost(),
        "integrations" => run_integrations(),
        "pricing" => run_pricing(),
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
        run_warehouse();
        run_sdks();
        run_flags();
        run_bayesian();
        run_selfhost();
        run_integrations();
        run_pricing();
    }

    #[test]
    fn help_and_version() {
        print_help("growthbook-cli");
        print_version();
    }
}
