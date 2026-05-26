#![deny(clippy::all)]
//! statsig-cli — personality CLI for Statsig, the Bellevue feature-management
//! + product-analytics platform built by ex-Facebook engineers.
//!
//! Founded 2020 in Bellevue, WA by Vijaye Raji (ex-Facebook VP of product
//! engineering, who built and ran Facebook's internal experimentation
//! platform Gatekeeper / Quick Experiments). Series B Sequoia-led $43M in
//! May 2023 at ~$1B valuation. Famous for an aggressive free tier (1M events
//! free) and for being adopted by OpenAI, Notion, Figma, Atlassian. Combines
//! feature flags, experiments, product analytics, session replay, and a
//! warehouse-native pillar all under one platform — a much wider product
//! surface than the typical feature-management vendor.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Statsig product platform personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founder, Bellevue, Facebook origins");
    println!("    flags         Feature gates (Statsig's term for flags)");
    println!("    experiments   Layered experiments, sequential, CUPED");
    println!("    analytics     Product analytics pillar");
    println!("    replay        Session replay add-on");
    println!("    warehouse     Warehouse-native via Cloud Cost product");
    println!("    pricing       1M events free tier and beyond");
    println!("    customers     OpenAI, Notion, Figma, Atlassian, ...");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("statsig-cli 0.1.0 (Gatekeeper-lineage personality build)"); }

fn run_about() {
    println!("Statsig, Inc.");
    println!("  Founded:    2020");
    println!("  Founder:    Vijaye Raji (CEO).");
    println!("              Ex-VP product engineering at Facebook, ran the");
    println!("              internal experimentation stack (Gatekeeper / QE)");
    println!("              that gated every Facebook product change.");
    println!("  HQ:         Bellevue, Washington.");
    println!("  Latest:     ~$43M Series B led by Sequoia, May 2023, ~$1B val.");
    println!("  Headcount:  ~150-250 (growing fast).");
    println!("  Pitch:      'The modern product platform' — make every team");
    println!("              ship like a Facebook product team.");
}

fn run_flags() {
    println!("Feature Gates — Statsig's term for feature flags.");
    println!("  Boolean gates, dynamic configs, layered experiments.");
    println!("  Targeting via user attributes + saved Segments.");
    println!("  Holdouts (cross-experiment exclusions).");
    println!("  Mutual exclusion groups.");
    println!("  Built-in approval workflows and audit log.");
}

fn run_experiments() {
    println!("Experiments:");
    println!("  Layered experiments — Facebook-style 'parameter slots' across");
    println!("                        multiple concurrent tests share the same");
    println!("                        user randomisation seed without coupling.");
    println!("  Sequential testing — peek-safe early stopping.");
    println!("  CUPED variance reduction.");
    println!("  Pulse — exposure-event live monitoring during a launch.");
    println!("  Auto-Tune — multi-armed bandit mode for marketing variants.");
    println!("  Powered by an in-house analytics engine, not a third-party.");
}

fn run_analytics() {
    println!("Product Analytics pillar.");
    println!("  Same SDK that fires exposures fires analytics events.");
    println!("  Funnels, retention, paths, dashboards.");
    println!("  Cohort builder and saved views.");
    println!("  Aimed at displacing Amplitude/Mixpanel inside Statsig customers.");
    println!("  Tight loop: a launch automatically generates analytics views");
    println!("  with the launched gate as a property filter.");
}

fn run_replay() {
    println!("Session Replay (add-on).");
    println!("  DOM-mutation capture + console + network.");
    println!("  Sampled by gate exposure so you can find replays of users");
    println!("  who hit a specific variation.");
    println!("  Privacy redaction by default for sensitive selectors.");
}

fn run_warehouse() {
    println!("Warehouse-native option.");
    println!("  Statsig can compute metrics against the customer's warehouse");
    println!("  (Snowflake, BigQuery, Redshift, Databricks) without copying");
    println!("  raw events out. Same UI as the cloud product.");
    println!("  Aimed at large customers whose data already lives in the lake.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free               1M events/month, unlimited users + flags,");
    println!("                     all features (real free tier, no asterisks).");
    println!("  Pro                per-event past the free band.");
    println!("  Enterprise         custom contract, warehouse-native option,");
    println!("                     SSO/SCIM, dedicated support.");
    println!("Free tier is the public moat — Statsig is unusually generous.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  OpenAI         experimentation on ChatGPT product launches");
    println!("  Microsoft      embedded across multiple product teams");
    println!("  Notion         feature gating + analytics");
    println!("  Figma          design-product launches");
    println!("  Atlassian      product analytics");
    println!("  Brex           B2B finance product");
    println!("  Plaid          financial-data products");
    println!("  Rippling       HR/payroll product launches");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "statsig-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flags" => run_flags(),
        "experiments" => run_experiments(),
        "analytics" => run_analytics(),
        "replay" => run_replay(),
        "warehouse" => run_warehouse(),
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
        run_flags();
        run_experiments();
        run_analytics();
        run_replay();
        run_warehouse();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("statsig-cli");
        print_version();
    }
}
