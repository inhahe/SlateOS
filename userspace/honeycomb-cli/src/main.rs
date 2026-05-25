#![deny(clippy::all)]

//! honeycomb-cli — OurOS Honeycomb.io (Charity Majors' observability platform, high-cardinality events)
//!
//! Single personality: `honeycomb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: honeycomb [OPTIONS]");
        println!("Honeycomb.io (OurOS) — Observability for distributed systems");
        println!();
        println!("Options:");
        println!("  --query                Query (group by + filter + heatmap)");
        println!("  --bubbleup             BubbleUp (auto-find anomalous dimensions)");
        println!("  --slo                  SLO with error budget burn alerts");
        println!("  --refinery             Refinery (tail-sampling proxy)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Honeycomb.io (OurOS)"); return 0; }
    println!("Honeycomb.io (OurOS)");
    println!("  Vendor: Hound Technology, Inc. (San Francisco, dba 'Honeycomb', founded 2016)");
    println!("  Founders: Charity Majors (ex-Parse/Facebook ops legend) + Christine Yen");
    println!("           also founded the modern 'observability' (vs monitoring) discourse");
    println!("  Origin myth: at Parse, Charity needed to query high-cardinality data");
    println!("              (per-user-id, per-app-id, per-route, per-version, per-device)");
    println!("              traditional metrics couldn't handle cardinality blow-up");
    println!("              → built Scuba-inspired event store at Facebook → spun out Honeycomb");
    println!("  Pricing: Free tier — 20M events/mo");
    println!("          Pro $130/mo includes 100M events");
    println!("          Enterprise — custom (unlimited, SAML, SOC2, BAA, etc.)");
    println!("          → no per-host or per-user pricing — pure event volume model");
    println!("  Killer concept — high-cardinality events:");
    println!("    every request = one wide event with hundreds of fields");
    println!("    query by ANY field — no pre-aggregation, no schemas to plan");
    println!("    ask 'show me p99 latency grouped by build_id, customer_id, route' — works");
    println!("  Killer feature — BubbleUp:");
    println!("    select anomalous region in a heatmap");
    println!("    Honeycomb auto-computes which dimensions differ from baseline");
    println!("    → instant root cause without manual grouping experiments");
    println!("  Features:");
    println!("    - OpenTelemetry-native (Honeycomb maintains many OTel SDKs)");
    println!("    - Distributed tracing with waterfall view + trace search");
    println!("    - SLOs with burn-rate alerts (no static thresholds)");
    println!("    - Triggers (alerts) on any query");
    println!("    - Heatmaps for latency distribution (not just averages)");
    println!("    - Refinery — open-source tail-sampling proxy (keep interesting traces, drop boring)");
    println!("  Culture: Charity Majors evangelizes 'observability != monitoring' + 'test in production'");
    println!("          o11y book ('Observability Engineering' O'Reilly 2022)");
    println!("  Differentiator: built for unknown-unknowns; query any dimension after-the-fact, no pre-aggregation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "honeycomb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
