#![deny(clippy::all)]

//! chronosphere-cli — SlateOS Chronosphere (cloud-native observability for K8s, NYC, private unicorn)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chronosphere(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chronosphere [OPTIONS]");
        println!("Chronosphere (SlateOS) — cloud-native observability built on M3 + OpenTelemetry");
        println!();
        println!("Options:");
        println!("  --metrics              Metrics platform (M3-based, Prometheus-compatible)");
        println!("  --logs                 Logs (Calyptia-acquired Fluent Bit pipeline)");
        println!("  --traces               Distributed Tracing (OpenTelemetry-native)");
        println!("  --control-plane        Control Plane (governance + cost mgmt)");
        println!("  --differential-diagnosis Differential Diagnosis (AI root-cause)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Chronosphere 2024 (SlateOS) — M3 v1.x"); return 0; }
    println!("Chronosphere 2024 (SlateOS) — Cloud-Native Observability Platform");
    println!("  Vendor: Chronosphere, Inc. (New York City, NY — private unicorn)");
    println!("  Founders: Martin Mao (CEO) + Rob Skillington (CTO), 2019");
    println!("          Both ex-Uber engineers who built M3 (Uber's Prometheus-scale TSDB)");
    println!("          M3 = scalable distributed time-series database open-sourced by Uber 2018");
    println!("          founded to commercialize M3 for high-cardinality observability");
    println!("          Focus on Kubernetes + microservices observability at scale");
    println!("  Private funding:");
    println!("         Series D Apr 2024: $115M at $1.6B valuation (Greylock Founders Fund)");
    println!("         total raised: ~$450M");
    println!("         Greylock, General Atlantic, Bessemer, Lux Capital, Founders Fund, Glynn backers");
    println!("         estimated $150M+ ARR (private, growing rapidly)");
    println!("         IPO discussed 2025-2026 (depends on market)");
    println!("  Strategic position: 'cloud-native observability that doesn't kill your budget':");
    println!("                    pitch: 'observability for Kubernetes + microservices — predictable cost + 100% OSS-compatible'");
    println!("                    target: cloud-native enterprises (K8s + microservices heavy)");
    println!("                    primary competitor: Datadog, Dynatrace, New Relic, Grafana Cloud");
    println!("                    secondary: Splunk, Logz.io, Honeycomb, Coralogix");
    println!("                    Chronosphere's wedge: M3 cardinality + Control Plane cost mgmt + 100% OSS-API-compatible");
    println!("                    'Predictable observability cost' is the killer talking point");
    println!("                    Datadog cardinality bill horror stories drive sales");
    println!("  Pricing:");
    println!("    Enterprise: $100K-$5M+/yr based on metric volume + retention");
    println!("    Calyptia / Logs: $0.05-$0.50/GB based on retention");
    println!("    Traces: per-span pricing");
    println!("    Control Plane (cost mgmt) is differentiator: customers self-shape costs");
    println!("    typically 30-60% cheaper than Datadog for similar scale (per customer testimonials)");
    println!("  Product portfolio:");
    println!("    1. Chronosphere Metrics (M3-based):");
    println!("       - PromQL-compatible — drop-in for Prometheus");
    println!("       - Distributed TSDB built for billions of unique metric series");
    println!("       - Long-term retention (~13 months default) at affordable cost");
    println!("       - Aggregation rules + downsampling at ingest time");
    println!("    2. Chronosphere Logs (acquired Calyptia 2024):");
    println!("       - Built on Fluent Bit pipeline (Calyptia is the Fluent Bit creators' company)");
    println!("       - Cost-optimized log management with shaping");
    println!("       - Strategic 2024 acquisition to compete with Splunk + Datadog Logs");
    println!("    3. Chronosphere Tracing (OpenTelemetry-native):");
    println!("       - Distributed traces at high cardinality");
    println!("       - Service maps + dependency graphs");
    println!("       - Trace sampling + retention policies");
    println!("    4. Control Plane (the differentiator):");
    println!("       - Real-time cost + cardinality governance");
    println!("       - Rules to shape, drop, or aggregate high-cardinality metrics");
    println!("       - 'Quota policy' enforcement before hitting bill surprises");
    println!("       - Visibility into who is driving cost");
    println!("       - Unique among observability vendors (most expose cost as opaque bill)");
    println!("    5. Differential Diagnosis (AI root-cause, 2023+):");
    println!("       - Automated incident triage");
    println!("       - 'What changed?' analysis across metrics + traces + deploys");
    println!("       - Compete with: Datadog Watchdog, Dynatrace Davis");
    println!("    6. Alerts + Dashboards:");
    println!("       - PromQL-based alerts");
    println!("       - Grafana-compatible dashboard import");
    println!("    7. Lens (the new query UI, 2024):");
    println!("       - Visual query builder over PromQL");
    println!("       - More accessible than raw PromQL for non-SREs");
    println!("    8. Service-level Objectives (SLOs):");
    println!("       - SLI/SLO/error-budget tracking");
    println!("       - Cross-team reliability reporting");
    println!("  M3 architecture (the engineering foundation):");
    println!("    - Built at Uber to handle billions of metrics across microservices");
    println!("    - Distributed sharded storage (M3DB)");
    println!("    - Aggregation tier (M3 Aggregator)");
    println!("    - Coordinator for PromQL query routing");
    println!("    - Open-sourced 2018 (Apache 2.0)");
    println!("    - Chronosphere = M3 + control plane + UI + commercial support");
    println!("  Calyptia acquisition (2024):");
    println!("    - Calyptia = company founded by Fluent Bit + Fluentd creators");
    println!("    - Acquired May 2024 to expand into logs market");
    println!("    - Strategic: matches Datadog's logs + metrics + traces bundle");
    println!("    - Fluent Bit (used by ~95% of Kubernetes deployments) becomes Chronosphere advantage");
    println!("  Open-source commitment:");
    println!("    - M3 (Apache 2.0) — public open-source TSDB");
    println!("    - Fluent Bit + Fluentd (post-Calyptia, CNCF projects)");
    println!("    - OpenTelemetry contributors");
    println!("    - PromQL + OpenMetrics compatibility — 'no proprietary protocols'");
    println!("    - Cloud Native Computing Foundation (CNCF) end-user member");
    println!("  Integrations:");
    println!("    - OpenTelemetry-native + OpenMetrics + Prometheus exposition");
    println!("    - Kubernetes (deep, the core use case)");
    println!("    - Fluent Bit + Fluentd (post-Calyptia native)");
    println!("    - Grafana dashboard import (read PromQL/Grafana JSON)");
    println!("    - AWS, Azure, GCP cloud metrics");
    println!("    - Alerts: PagerDuty, Opsgenie, Slack, Teams, ServiceNow");
    println!("    - SSO: Okta, Azure AD, Google Workspace, SAML");
    println!("    - APIs: CRUD APIs for everything (Infrastructure-as-Code-friendly)");
    println!("  Chronosphere CLI usage:");
    println!("    chronosphere login --tenant my-org");
    println!("    chronosphere metrics query 'rate(http_requests_total[5m])' --range 1h");
    println!("    chronosphere shaping-rule create --target 'k8s_pod_*' --action aggregate --interval 1m");
    println!("    chronosphere alert create --query @alert.promql --severity critical");
    println!("    chronosphere dashboard import --file grafana-dashboard.json");
    println!("    chronosphere slo create --name 'API Availability' --target 99.95");
    println!("    chronosphere logs query 'level=error AND service=checkout' --from -1h");
    println!("    chronosphere differential-diagnosis trigger --incident-id INC-12345");
    println!("  Customers (~400+ enterprise):");
    println!("    - Cloud-native + microservices-heavy companies");
    println!("    - DoorDash, Robinhood, Tessian (Proofpoint), Snap, Affirm, Carvana");
    println!("    - International: heavy in fintech + SaaS scale-ups");
    println!("    - 90%+ retention with high net revenue retention (~140%)");
    println!("    - sweet spot: Kubernetes-native SaaS companies migrating off Datadog");
    println!("  Critique: M3 architecture trade-offs (eventually consistent, complex ops)");
    println!("           customer count modest vs Datadog 28K+ paying base");
    println!("           tracing + logs less mature than metrics (M3 heritage)");
    println!("           logs market entry late vs Splunk/Datadog");
    println!("           Calyptia integration ongoing — strategic but execution risk");
    println!("           AI features early stage vs Datadog Bits AI + Dynatrace Davis");
    println!("           PromQL learning curve for non-Prometheus shops");
    println!("           Datadog brand awareness + sales engine still dominant");
    println!("           IPO timing uncertain in current market");
    println!("  Differentiator: M3-based metrics platform (built at Uber for billions of high-cardinality time-series) + Control Plane (the cost-shaping governance layer — unique among observability vendors) + 100% OpenTelemetry + Prometheus + Grafana compatibility (no lock-in) + Calyptia acquisition (Fluent Bit creators) + Kubernetes-native focus + $1.6B unicorn valuation + ex-Uber engineering pedigree — the observability platform that ex-Datadog customers migrate to when their Datadog bill becomes unbearable and they need predictable cardinality cost");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chronosphere".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chronosphere(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chronosphere};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chronosphere"), "chronosphere");
        assert_eq!(basename(r"C:\bin\chronosphere.exe"), "chronosphere.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chronosphere.exe"), "chronosphere");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_chronosphere(&["--help".to_string()], "chronosphere"), 0);
        assert_eq!(run_chronosphere(&["-h".to_string()], "chronosphere"), 0);
        let _ = run_chronosphere(&["--version".to_string()], "chronosphere");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_chronosphere(&[], "chronosphere");
    }
}
