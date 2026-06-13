#![deny(clippy::all)]

//! heap-cli — Slate OS Heap (auto-capture analytics — never miss an event)
//!
//! Single personality: `heap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_heap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: heap [OPTIONS]");
        println!("Heap (Slate OS) — auto-capture digital insights (Contentsquare since 2024)");
        println!();
        println!("Options:");
        println!("  --free                 Free tier (10K sessions/mo)");
        println!("  --growth               Growth (custom, ~$3,600/yr starter)");
        println!("  --pro                  Pro (mid-market)");
        println!("  --premier              Premier (enterprise)");
        println!("  --sessions             Session replay (now bundled, ex-Auryc)");
        println!("  --illuminate           Illuminate AI — auto-surface friction insights");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Heap 2024 (Slate OS)"); return 0; }
    println!("Heap 2024 (Slate OS)");
    println!("  Vendor: Heap, Inc. — acquired by Contentsquare Sep 2023 for ~$400M");
    println!("        now part of Contentsquare's 'digital experience analytics' empire");
    println!("        Heap brand still active as Contentsquare's product analytics SKU");
    println!("  Founders: Matin Movassate (CEO until 2022), Ravi Parikh, Dan Robinson, 2013");
    println!("          all three Stanford CS — Movassate ex-Facebook PM");
    println!("          ICONIQ, NEA, Menlo Ventures, Y Combinator W13");
    println!("  Founded: 2013, YC Winter 2013");
    println!("  Acquisition timeline:");
    println!("    - Heap raised ~$200M total, last private valuation ~$960M (2021 Series D)");
    println!("    - Acquired by Contentsquare Sep 2023 for reported $400M (down round to acquisition)");
    println!("    - Contentsquare = Paris-based session replay + UX analytics (~$1.4B valuation)");
    println!("    - combined entity covers product analytics (Heap) + session replay (CS) + voice-of-customer");
    println!("  Defining feature — Auto-Capture:");
    println!("    - Drop ONE snippet on your site/app");
    println!("    - Heap records EVERY click, pageview, form submit, input change");
    println!("    - Define 'events' retroactively after the fact — no instrumenting code in advance");
    println!("    - Contrast with Amplitude/Mixpanel: those require explicit track() calls per event");
    println!("    - Tradeoff: more data noise, more storage cost, but never miss an event");
    println!("  Pricing:");
    println!("    Free — 10K sessions/mo (basic auto-capture, limited reports)");
    println!("    Growth — custom (typically $300-500/mo starter, scales with sessions)");
    println!("    Pro / Premier — custom (mid-market to enterprise, four-six figure deals)");
    println!("    pricing famously opaque — must talk to sales for serious quotes");
    println!("  Core features:");
    println!("    - Event Visualizer — point-and-click to define events from raw auto-capture data");
    println!("    - Funnels with auto-discovered conversion paths");
    println!("    - Retention curves + cohort comparison");
    println!("    - Paths (sankey diagrams)");
    println!("    - Segments (user/account cohorts)");
    println!("    - Effortless tracking — backfill historical data when new events defined");
    println!("    - Heap Connect — sync events to data warehouse (Snowflake, BigQuery, Redshift)");
    println!("  Illuminate AI (post-acquisition flagship):");
    println!("    - Auto-surface 'friction points' — drop-offs the user didn't define a funnel for");
    println!("    - Anomaly detection across metrics");
    println!("    - Conversion impact ranking (what behaviors correlate with retention)");
    println!("    - Natural language Q&A over Heap data");
    println!("  Session replay (post-Contentsquare merger):");
    println!("    - Watch individual user sessions tied to Heap events");
    println!("    - Frustration signals (rage clicks, dead clicks, U-turns)");
    println!("    - Heat maps + zone analysis");
    println!("    - Privacy-safe (masking sensitive fields)");
    println!("  Data warehouse strategy:");
    println!("    - Heap Connect for export to Snowflake/BigQuery/Redshift");
    println!("    - Reverse ETL for syncing audiences back");
    println!("    - 'You own your data' positioning");
    println!("  Integrations: 50+ destinations");
    println!("              Snowflake, BigQuery, Redshift, Databricks");
    println!("              Salesforce, Hubspot, Marketo");
    println!("              Slack, Tableau, Looker");
    println!("              Segment as both upstream + downstream");
    println!("  Customers: ~7,000+ paying companies");
    println!("            Twilio (early flagship), Asana, Microsoft (parts), AppLovin, Zendesk (parts)");
    println!("            Pluralsight, Toast, Stitch Fix, Northwestern Mutual");
    println!("            sweet spot: B2C + B2B SaaS with high event diversity + small data eng teams");
    println!("  Critique: storage costs add up — auto-capture means you pay for everything captured");
    println!("           sometimes auto-captured events lack the semantic meaning manual events have");
    println!("           query latency on large datasets can be slow vs Amplitude");
    println!("           Contentsquare merger integration still ongoing — UX consistency lagging");
    println!("  Differentiator: only major product analytics tool with TRUE retroactive event definition (auto-capture from day 1)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "heap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_heap(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_heap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/heap"), "heap");
        assert_eq!(basename(r"C:\bin\heap.exe"), "heap.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("heap.exe"), "heap");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_heap(&["--help".to_string()], "heap"), 0);
        assert_eq!(run_heap(&["-h".to_string()], "heap"), 0);
        let _ = run_heap(&["--version".to_string()], "heap");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_heap(&[], "heap");
    }
}
