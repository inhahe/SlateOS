#![deny(clippy::all)]
//! launchdarkly-cli — personality CLI for LaunchDarkly, the category-defining
//! feature management platform.
//!
//! Founded 2014 in Oakland, CA by Edith Harbaugh and John Kodumal (ex-Atlassian).
//! Coined "feature management" as a category, sitting atop the older "feature
//! flag" idea by adding targeting, percentage rollouts, experimentation, and
//! audit. Raised $200M Series D in Aug 2021 at a $3B valuation (Lightspeed,
//! Bessemer, Redpoint, Threshold). Acquired Highlight.io in 2024 to add
//! session-replay observability to the feature-flag platform.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — LaunchDarkly feature management personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Oakland, category creation");
    println!("    flags         Boolean, multivariate, JSON, number flags");
    println!("    targeting     Rules, segments, percentage rollouts");
    println!("    experiments   Built-in experimentation engine");
    println!("    observability Highlight.io acquisition, session replay");
    println!("    architecture  Streaming SDKs, Relay Proxy, edge offerings");
    println!("    pricing       Per-MAU + per-seat tiers");
    println!("    customers     IBM, Atlassian, NBC, CarMax, TrueCar, Square");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("launchdarkly-cli 0.1.0 (Feature Management personality build)"); }

fn run_about() {
    println!("LaunchDarkly, Inc.");
    println!("  Founded:      2014");
    println!("  Founders:     Edith Harbaugh (CEO), John Kodumal (CTO)");
    println!("  HQ:           Oakland, California");
    println!("  Latest round: $200M Series D, Aug 2021 at $3B valuation");
    println!("  Investors:    Lightspeed, Bessemer, Redpoint, Threshold,");
    println!("                Vertex Ventures, Bloomberg Beta.");
    println!("  Category:     'Feature Management' — a term LaunchDarkly");
    println!("                coined to distinguish from raw feature flags.");
}

fn run_flags() {
    println!("Flag types:");
    println!("  Boolean       on/off, the classic feature toggle.");
    println!("  Multivariate  any number of named variations.");
    println!("  Number        numeric variations (config tuning).");
    println!("  String        string variations (e.g. copy text).");
    println!("  JSON          arbitrary JSON for complex config payloads.");
    println!();
    println!("Each flag carries:");
    println!("  - Targeting rules per environment.");
    println!("  - Default variation, off variation.");
    println!("  - Maintainer, tags, description.");
    println!("  - Audit log of every change.");
}

fn run_targeting() {
    println!("Targeting model:");
    println!("  User context     attributes: key, email, country, plan, etc.");
    println!("  Multi-context    LD-6 multi-context: user + org + device.");
    println!("  Rules            'if email ends with @acme.com -> variation B'");
    println!("  Segments         saved cohorts reusable across flags.");
    println!("  Rollouts         percentage by hashed key for stickiness.");
    println!("  Holdouts         experiment-level exclusion sets.");
}

fn run_experiments() {
    println!("Experimentation engine:");
    println!("  Random assignment via hashed targeting attribute.");
    println!("  Frequentist + Bayesian analysis modes.");
    println!("  Sample ratio mismatch (SRM) detection.");
    println!("  Built-in metrics (conversion, count, numeric, duration).");
    println!("  Integrations with Segment/Snowflake/BigQuery for custom metrics.");
    println!("  Funnel analysis across multi-step user journeys.");
}

fn run_observability() {
    println!("Observability — via Highlight.io acquisition (2024).");
    println!("  Session replay capturing DOM mutations + console + network.");
    println!("  Errors with stack traces and source maps.");
    println!("  Logs and traces (OpenTelemetry).");
    println!("  Tied back to flag changes: 'this error spike correlates with");
    println!("    the flag flip that occurred 4 minutes earlier'.");
}

fn run_architecture() {
    println!("Architecture:");
    println!("  Streaming SDK    server-side SDKs hold an in-memory flag store");
    println!("                   updated via Server-Sent Events.");
    println!("  Client SDKs      web/mobile SDKs evaluate the same flag set.");
    println!("  Relay Proxy      self-hostable cache for SDKs behind firewalls.");
    println!("  Edge SDKs        Cloudflare Workers / Vercel Edge / Akamai");
    println!("                   evaluate flags at the CDN edge.");
    println!("  Flag Delivery    typical end-to-end fan-out: <200ms p99.");
    println!("Reliability target: multi-region active-active with 99.99% SLA.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Starter      free up to a small MAU cap, 2 environments.");
    println!("  Foundation   per-seat + per-MAU.");
    println!("  Enterprise   custom contract, includes audit, SSO, advanced");
    println!("               approvals, custom roles, Federal options.");
    println!("Cost driver: client-side Monthly Active Contexts (MAU/MAC).");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  IBM            internal continuous delivery across business units");
    println!("  Atlassian      Jira/Confluence safe-rollout backbone");
    println!("  NBC Universal  peacock streaming product gating");
    println!("  CarMax         online auto-buying funnel experiments");
    println!("  TrueCar        pricing-engine A/B");
    println!("  Square         merchant features gradual rollout");
    println!("  TripAdvisor    booking funnel experimentation");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "launchdarkly-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flags" => run_flags(),
        "targeting" => run_targeting(),
        "experiments" => run_experiments(),
        "observability" => run_observability(),
        "architecture" => run_architecture(),
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
        run_targeting();
        run_experiments();
        run_observability();
        run_architecture();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("launchdarkly-cli");
        print_version();
    }
}
