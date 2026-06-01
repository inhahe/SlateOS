#![deny(clippy::all)]
//! split-cli — personality CLI for Split Software, the feature delivery
//! platform that pioneered "Feature Data Platform" framing, now part of
//! Harness.
//!
//! Founded 2015 in Redwood City, CA by Patricio Echague and Trevor Stuart
//! (both ex-LinkedIn). Raised across Series A/B/C/D rounds (most recently
//! $50M Series D Jun 2021). Acquired by Harness in late 2024 (announced
//! Sep 2024) and folded into Harness's Software Delivery Platform alongside
//! Drone CI, Cloud Cost Management, and Chaos Engineering. Split's
//! differentiator: tight binding between feature flag and metric impact,
//! plus an automated 'Monitor' that flags ship regressions.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Split (a Harness company) personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, LinkedIn lineage, Harness acquisition");
    println!("    splits        Split's term for a feature flag");
    println!("    metrics       Impact analysis on every release");
    println!("    monitor       Automated guardrail metric watch");
    println!("    treatments    Multivariate treatments + targeting");
    println!("    sdk           Server, client, edge SDKs");
    println!("    harness       The acquisition and the unified platform");
    println!("    customers     Twilio, BlueApron, WePay, LendingTree");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("split-cli 0.1.0 (Feature Data Platform personality build)"); }

fn run_about() {
    println!("Split Software, Inc. (a Harness company since 2024)");
    println!("  Founded:  2015");
    println!("  Founders: Patricio Echague (CTO), Trevor Stuart (CEO).");
    println!("            Both ex-LinkedIn engineers.");
    println!("  HQ:       Redwood City, California.");
    println!("  Funding:  Sapphire-led across multiple rounds; ~$50M Series D");
    println!("            in Jun 2021. Total raised >$110M.");
    println!("  Acquired: by Harness, announced Sep 2024, closed late 2024.");
    println!("  Pitch:    'Feature Delivery Platform' — flag + measure together.");
}

fn run_splits() {
    println!("Splits — the unit of configuration.");
    println!("  A Split is a named decision point with treatments (variants).");
    println!("  Each environment has its own targeting rules.");
    println!("  Default rule + matchers + percentage allocations.");
    println!("  Treatments may be boolean, string, or carry JSON config.");
}

fn run_metrics() {
    println!("Metrics — Split's product differentiator.");
    println!("  Every Split can be associated with metrics.");
    println!("  Metrics ingested via the Events API or a warehouse connector.");
    println!("  When a Split changes targeting, Split automatically runs a");
    println!("  statistical impact analysis comparing the new variation cohort");
    println!("  to the baseline cohort using the customer's own metrics.");
    println!("  Result: see ship impact next to the ship itself.");
}

fn run_monitor() {
    println!("Monitor — automated guardrail.");
    println!("  Pick a guardrail metric (latency, errors, conversion).");
    println!("  Split watches for statistical degradation after any change.");
    println!("  Alerts via Slack/email and can suggest automatic rollback.");
    println!("  Aimed at making the safe-rollout story self-driving.");
}

fn run_treatments() {
    println!("Targeting model:");
    println!("  Attribute-based matchers: equals, in_segment, between, regex,");
    println!("                            contains, semver_*.");
    println!("  Combinators: AND across conditions, ordered rule list.");
    println!("  Allocation: percentage rollout within a rule, sticky hash.");
    println!("  Audiences/Segments: saved cohorts reusable across Splits.");
    println!("  Dynamic Configurations: treatments carry JSON payload.");
}

fn run_sdk() {
    println!("SDK matrix:");
    println!("  Server   Node, Java, Go, .NET, Python, PHP, Ruby.");
    println!("  Client   JavaScript, React, Vue, Angular.");
    println!("  Mobile   iOS, Android, React Native, Flutter.");
    println!("  Edge     Cloudflare Workers, Fastly Compute, Akamai EdgeWorkers.");
    println!("  All SDKs evaluate locally from a cached rules payload;");
    println!("  impressions stream back to Split for analytics.");
}

fn run_harness() {
    println!("Harness acquisition.");
    println!("  Harness is the AI-powered Software Delivery Platform from");
    println!("  Jyoti Bansal (also founder of AppDynamics).");
    println!("  Split slots in as the Feature Management pillar alongside");
    println!("  CD pipelines, Cloud Cost Management, Chaos Engineering, and");
    println!("  the Drone CI engine.");
    println!("  Branding: 'Split (a Harness company)' during integration.");
}

fn run_customers() {
    println!("Selected customers (pre-acquisition):");
    println!("  Twilio          internal feature delivery");
    println!("  Blue Apron      meal-kit web/app experimentation");
    println!("  LendingTree     financial-product gating");
    println!("  WePay (Chase)   payments rollouts");
    println!("  Experian        credit-product experimentation");
    println!("  Mulesoft        SaaS product gating");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "split-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "splits" => run_splits(),
        "metrics" => run_metrics(),
        "monitor" => run_monitor(),
        "treatments" => run_treatments(),
        "sdk" => run_sdk(),
        "harness" => run_harness(),
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
        run_splits();
        run_metrics();
        run_monitor();
        run_treatments();
        run_sdk();
        run_harness();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("split-cli");
        print_version();
    }
}
