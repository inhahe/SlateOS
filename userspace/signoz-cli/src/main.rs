#![deny(clippy::all)]

//! signoz-cli — SlateOS SigNoz (open-source OpenTelemetry-native observability, Bangalore + SF)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_signoz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: signoz [OPTIONS]");
        println!("SigNoz (Slate OS) — open-source OpenTelemetry-native observability (YC W21)");
        println!();
        println!("Options:");
        println!("  --traces               Distributed tracing (OpenTelemetry-native)");
        println!("  --metrics              Metrics (ClickHouse + PromQL-compatible)");
        println!("  --logs                 Logs (ClickHouse-based)");
        println!("  --apm                  Application Performance Monitoring");
        println!("  --alerts               Alerts + notifications");
        println!("  --self-hosted          Self-hosted Docker / Helm chart");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SigNoz 2024 (Slate OS) — v0.45 community + cloud"); return 0; }
    println!("SigNoz 2024 (Slate OS) — Open-Source OpenTelemetry-Native Observability");
    println!("  Vendor: SigNoz, Inc. (Bangalore + San Francisco — private, YC W21)");
    println!("  Founders: Pranay Prateek + Ankit Anand, 2021 (Y Combinator W21)");
    println!("          founded with thesis: 'open-source Datadog alternative on OpenTelemetry'");
    println!("          'SigNoz' = 'signal + noise' — extract signal from telemetry noise");
    println!("          Indian-founded, dual HQ Bangalore + SF");
    println!("          Pranay Prateek: CEO, ex-VWO product");
    println!("          Ankit Anand: CTO, ex-Microsoft");
    println!("  Private funding:");
    println!("         Series A Mar 2024: $6.5M (Nexus Venture Partners, YC, etc.)");
    println!("         total raised: ~$16M (modest by observability standards)");
    println!("         Nexus Venture Partners, Bessemer, Y Combinator backers");
    println!("         estimated $5-15M ARR (private — early stage, growing fast)");
    println!("         GitHub: 18K+ stars (one of the most-starred observability OSS projects)");
    println!("  Strategic position: 'OpenTelemetry-native observability — open-source, self-hosted, no lock-in':");
    println!("                    pitch: 'one tool for logs + metrics + traces — built on the OTel spec'");
    println!("                    target: developers + cost-conscious teams + open-source-aligned shops");
    println!("                    primary competitor: Datadog (sticker shock alternative), Grafana, Jaeger + Prometheus");
    println!("                    secondary: New Relic, Honeycomb, Logz.io, Elastic APM");
    println!("                    SigNoz's wedge: 100% open-source + OpenTelemetry-native + ClickHouse backend = self-hostable");
    println!("                    'OSS-first' = mirrors Grafana's growth strategy at younger stage");
    println!("  Pricing:");
    println!("    Community (self-hosted): FREE forever, full features");
    println!("    Cloud Standard: $20-$200/month per host or per-GB ingestion");
    println!("    Cloud Enterprise: SLA, SSO, audit logs, premium support");
    println!("    typically 50-80% cheaper than Datadog for similar volumes");
    println!("    free tier on Cloud: 30 days trial");
    println!("  Product portfolio:");
    println!("    1. SigNoz Tracing:");
    println!("       - OpenTelemetry-native ingestion");
    println!("       - Distributed trace visualization (flame graphs + service maps)");
    println!("       - ClickHouse backend for fast queries");
    println!("       - Service-level dependency analysis");
    println!("    2. SigNoz Metrics:");
    println!("       - PromQL-compatible queries");
    println!("       - ClickHouse storage (different from Prometheus TSDB)");
    println!("       - OpenTelemetry metrics ingestion");
    println!("    3. SigNoz Logs:");
    println!("       - ClickHouse-based log search + filtering");
    println!("       - Saved searches + log dashboards");
    println!("       - Log-to-trace correlation");
    println!("    4. APM:");
    println!("       - Auto-instrumentation guidance for OTel languages");
    println!("       - Service overview + endpoint performance");
    println!("       - Error tracking + slow query identification");
    println!("    5. Alerts:");
    println!("       - PromQL + ClickHouse query-based alerts");
    println!("       - Slack, PagerDuty, Opsgenie, MS Teams, Webhook integrations");
    println!("    6. Dashboards:");
    println!("       - Custom dashboard builder");
    println!("       - Grafana dashboard import (basic)");
    println!("    7. Exceptions:");
    println!("       - Auto-aggregated exception tracking");
    println!("       - Compete with: Sentry, Bugsnag, Rollbar");
    println!("    8. Anomaly Detection (early — ML on metrics):");
    println!("       - Threshold-free anomaly detection");
    println!("       - Compete with: Datadog Watchdog, Dynatrace Davis");
    println!("  Architecture (the technical bet):");
    println!("    - OpenTelemetry Collector for ingestion (no vendor agent)");
    println!("    - ClickHouse for storage (columnar, fast aggregations)");
    println!("    - React-based UI");
    println!("    - Single Docker Compose / Helm chart deployment");
    println!("    - K8s-native architecture");
    println!("    - Aligns with CNCF observability standards (OTel + Prometheus exposition)");
    println!("  Open-source approach:");
    println!("    - MIT licensed");
    println!("    - 18K+ GitHub stars");
    println!("    - 200+ contributors");
    println!("    - Active Slack community (10K+ members)");
    println!("    - Docker pulls + Helm installs growing rapidly");
    println!("    - Strategy mirrors successful OSS observability vendors (Grafana, Elastic in early days)");
    println!("  OpenTelemetry champion:");
    println!("    - All ingestion through OTel Collector or OTLP protocol");
    println!("    - No proprietary agent or SDK");
    println!("    - Customers can switch to/from SigNoz without re-instrumenting");
    println!("    - Direct contrast to Datadog tracer + New Relic agent lock-in");
    println!("    - 'OpenTelemetry is the future' = strategic bet shared with CNCF");
    println!("  Integrations:");
    println!("    - OpenTelemetry Collector (native — primary ingestion)");
    println!("    - Auto-instrumentation: Java, Python, Node, Go, Ruby, .NET via OTel SDKs");
    println!("    - Container: Kubernetes, Docker, EKS, GKE, AKS");
    println!("    - Cloud: AWS, Azure, GCP (via OTel resource detection)");
    println!("    - Alerts: PagerDuty, Slack, Teams, Opsgenie, ServiceNow, Webhooks");
    println!("    - Logs sources: Fluentd, FluentBit, Vector, Filebeat, OTel logs");
    println!("    - DBs: PostgreSQL, MySQL, Redis, MongoDB metrics + APM");
    println!("    - SSO: SAML, OIDC, Google, GitHub, Microsoft sign-in");
    println!("  SigNoz CLI usage:");
    println!("    signoz auth login --endpoint https://my-org.signoz.io --token $SIGNOZ_TOKEN");
    println!("    signoz trace search --service checkout --duration '>1s' --from -1h");
    println!("    signoz metrics query 'avg(latency)' --service checkout --range 1h");
    println!("    signoz logs query --service checkout --level error --from -30m");
    println!("    signoz dashboard import --file dashboard.json");
    println!("    signoz alert create --name 'High Error Rate' --query @query.signoz --threshold 100");
    println!("    signoz install --self-hosted --target docker-compose");
    println!("    signoz install --self-hosted --target kubernetes --namespace signoz");
    println!("  Customers (community + paid):");
    println!("    - Modest paid customer count (~150-300 estimated)");
    println!("    - Larger community user base (10K+ self-hosted deployments)");
    println!("    - Startups + scale-ups + open-source-friendly teams");
    println!("    - Indian + Southeast Asian tech scene strong");
    println!("    - Growing US + Europe presence");
    println!("    - Sweet spot: technical teams that want OTel without Datadog cost");
    println!("  Critique: young company (founded 2021) — battle-tested at scale less than peers");
    println!("           ClickHouse self-hosting ops burden non-trivial (no managed open-source ClickHouse)");
    println!("           feature parity with Datadog still maturing (especially RUM + synthetics)");
    println!("           community-to-paid conversion rate uncertain");
    println!("           Grafana Cloud's OSS-friendly bundling competes for similar buyers");
    println!("           AI features minimal vs leading vendors");
    println!("           sales engine + brand awareness modest vs Datadog/Splunk");
    println!("           dual HQ Bangalore + SF good for talent but less Western enterprise muscle");
    println!("  Differentiator: 100% open-source MIT-licensed + OpenTelemetry-native (no proprietary protocols or agents) + ClickHouse backend (sub-second queries on logs/metrics/traces) + 18K+ GitHub stars (one of the most-starred OSS observability projects) + self-hostable for full data sovereignty + 50-80% cheaper than Datadog Cloud + Y Combinator W21 + Indian + US dual HQ + early bet on OTel as the future standard — the open-source observability platform for developers who want OpenTelemetry without vendor lock-in and Datadog without the enterprise bill");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "signoz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_signoz(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_signoz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/signoz"), "signoz");
        assert_eq!(basename(r"C:\bin\signoz.exe"), "signoz.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("signoz.exe"), "signoz");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_signoz(&["--help".to_string()], "signoz"), 0);
        assert_eq!(run_signoz(&["-h".to_string()], "signoz"), 0);
        let _ = run_signoz(&["--version".to_string()], "signoz");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_signoz(&[], "signoz");
    }
}
