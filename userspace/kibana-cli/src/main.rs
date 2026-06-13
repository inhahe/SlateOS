#![deny(clippy::all)]

//! kibana-cli — SlateOS Kibana (Elastic's visualization frontend, the 'K' in ELK)
//!
//! Single personality: `kibana`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kib(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kibana [OPTIONS]");
        println!("Kibana 8.16 (Slate OS) — Visualization for Elasticsearch");
        println!();
        println!("Options:");
        println!("  start                  Start Kibana server");
        println!("  --discover             Discover (ad-hoc log exploration)");
        println!("  --lens                 Lens (drag-and-drop visualizations)");
        println!("  --dashboard            Dashboard editor");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kibana 8.16.1 (Slate OS)"); return 0; }
    println!("Kibana 8.16.1 (Slate OS)");
    println!("  Vendor: Elastic N.V. (Mountain View / Amsterdam — NYSE:ESTC)");
    println!("  Original author: Rashid Khan (~2013, started as personal project to query Logstash data)");
    println!("                   joined Elastic, Kibana became official UI");
    println!("  Role in stack: the 'K' in ELK / Elastic Stack");
    println!("                 Elasticsearch (store/search) + Logstash (ingest) + Kibana (viz) + Beats (ship)");
    println!("  License: 2021 — Elasticsearch + Kibana relicensed from Apache 2.0 to dual SSPL/Elastic License");
    println!("          → AWS forked into OpenSearch + OpenSearch Dashboards");
    println!("          2024 — Elastic re-added AGPLv3 as third option (some upstream→OpenSearch flow back)");
    println!("  Pricing: Self-managed (free with restrictions) + Elastic Cloud subscription tiers");
    println!("          Standard, Gold, Platinum, Enterprise (per-resource pricing)");
    println!("  Core features:");
    println!("    - Discover: search + filter logs with KQL (Kibana Query Language)");
    println!("    - Lens: drag-and-drop multi-chart builder (line, bar, heatmap, donut, table)");
    println!("    - Dashboard: pin panels, add filters, drilldowns, embed in other apps");
    println!("    - Canvas: presentation-style infographic dashboards");
    println!("    - Maps: geospatial visualization on Elastic Maps Service");
    println!("    - Vega + Vega-Lite custom chart support");
    println!("  Solutions on top:");
    println!("    - Observability (logs + metrics + APM + uptime + RUM in one UI)");
    println!("    - Security (SIEM dashboards + alerts + cases + Endpoint Security)");
    println!("    - Enterprise Search (Workplace Search, App Search, Site Search)");
    println!("    - Machine Learning (anomaly detection, forecasting, classification)");
    println!("  Tech: Node.js server, React frontend, talks to Elasticsearch REST API");
    println!("  Customers: Walmart, Netflix, Salesforce, Adobe, US Census, NASA — anyone with ELK stack");
    println!("  Differentiator: best UX for ad-hoc log search at scale + tight Elasticsearch integration");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kibana".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kib(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kib};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kibana"), "kibana");
        assert_eq!(basename(r"C:\bin\kibana.exe"), "kibana.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kibana.exe"), "kibana");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kib(&["--help".to_string()], "kibana"), 0);
        assert_eq!(run_kib(&["-h".to_string()], "kibana"), 0);
        let _ = run_kib(&["--version".to_string()], "kibana");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kib(&[], "kibana");
    }
}
