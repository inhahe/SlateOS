#![deny(clippy::all)]

//! logzio-cli — OurOS Logz.io (open-source-based observability, Tel Aviv + Boston, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logzio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logzio [OPTIONS]");
        println!("Logz.io (OurOS) — Open 360 observability (ELK + Prometheus + Jaeger SaaS)");
        println!();
        println!("Options:");
        println!("  --logs                 Log Management (managed ELK/OpenSearch)");
        println!("  --metrics              Metrics (managed Prometheus)");
        println!("  --traces               Distributed Tracing (managed Jaeger/OpenTelemetry)");
        println!("  --siem                 Cloud SIEM (managed ELK + threat intel)");
        println!("  --kibana               Kibana / OpenSearch Dashboards");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Logz.io 2024 (OurOS) — Open 360 Platform"); return 0; }
    println!("Logz.io 2024 (OurOS) — Open 360 Observability Platform");
    println!("  Vendor: Logz.io, Inc. (Tel Aviv, Israel + Boston, MA — private)");
    println!("  Founders: Tomer Levy + Asaf Yigal, 2014");
    println!("          founded with thesis: 'managed open-source observability — ELK + Prometheus + Jaeger as SaaS'");
    println!("          Tomer Levy: long-time CEO + ex-Intel/Sears Israel security veteran");
    println!("          Asaf Yigal: VP Product, technical co-founder");
    println!("          early open-source champion — major contributor to Logstash + Beats");
    println!("  Private funding:");
    println!("         Series D Dec 2020: $52M at ~$300M valuation (OpenView, 83North, Giza Venture)");
    println!("         total raised: ~$120M");
    println!("         OpenView, Vintage, 83North, Giza Venture, Greenfield backers");
    println!("         estimated $60-80M ARR (private)");
    println!("         IPO discussed but not imminent");
    println!("  Strategic position: 'open-source-based observability — no vendor lock-in':");
    println!("                    pitch: 'unified ELK + Prometheus + Jaeger + SIEM — managed, scaled, integrated'");
    println!("                    target: cloud-native + DevOps teams who like open standards");
    println!("                    primary competitor: Datadog, New Relic, Elastic Cloud, Splunk, Sumo Logic");
    println!("                    secondary: Grafana Cloud, Chronosphere, Mezmo, Honeycomb");
    println!("                    Logz.io's wedge: 100% open-source-based + no proprietary agent lock-in");
    println!("                    Migration story: 'come from ELK, stay on ELK, but managed'");
    println!("  Pricing (per-GB ingestion):");
    println!("    Build: free trial / starter");
    println!("    Pro Logs: $0.74 per GB/day for 7-day retention");
    println!("    Pro Tracing: $0.40 per 1M spans");
    println!("    Pro Metrics: $0.50 per 1M data points");
    println!("    Cloud SIEM: from $1/GB ingestion + retention extras");
    println!("    typically 30-50% cheaper than Datadog for equivalent volumes");
    println!("  Product portfolio (Open 360):");
    println!("    1. Log Management (the original product):");
    println!("       - Managed Elasticsearch / OpenSearch backend");
    println!("       - Kibana / OpenSearch Dashboards UI");
    println!("       - Filebeat + Fluentd + native shippers");
    println!("       - Auto-parsing for common formats (NGINX, Apache, JSON, etc.)");
    println!("       - 'Cognitive Insights' = ML-driven log anomaly detection");
    println!("       - 'Smart Tier' = warm/cold tiering for cost optimization");
    println!("    2. Infrastructure Monitoring (Prometheus-based):");
    println!("       - Managed Prometheus + Thanos for HA + long retention");
    println!("       - Grafana UI for visualization");
    println!("       - 1,300+ pre-built dashboards");
    println!("    3. Distributed Tracing (Jaeger-based):");
    println!("       - OpenTelemetry-native ingestion");
    println!("       - Jaeger backend for trace storage + UI");
    println!("       - Service maps + dependency graphs");
    println!("    4. Cloud SIEM:");
    println!("       - Cloud SIEM built on ELK + threat intel feeds");
    println!("       - MITRE ATT&CK mappings");
    println!("       - Detection rules library + custom rule editor");
    println!("       - Compete with: Sumo Logic, Splunk, Microsoft Sentinel, Elastic Security");
    println!("    5. App 360 (APM):");
    println!("       - APM based on OpenTelemetry + traces + RUM");
    println!("       - Compete with: Datadog APM, New Relic, Dynatrace");
    println!("    6. Telemetry Collector:");
    println!("       - Managed OpenTelemetry collector");
    println!("       - 'Send everything to Logz.io with one agent'");
    println!("    7. Service Health (built on traces):");
    println!("       - Service-level health dashboards");
    println!("       - SLO + SLI tracking");
    println!("  Open-source commitment:");
    println!("    - Major contributor to: Logstash, Beats, OpenTelemetry, Jaeger");
    println!("    - 'Open Observability' open-source movement (with Grafana, etc.)");
    println!("    - 'Logz.io for Open Source' — free tier for OSS projects");
    println!("    - Maintained the OSS 'ELK' forks during the Elastic license drama (2021)");
    println!("    - Migrated to OpenSearch (Amazon-backed Elasticsearch fork) for compatibility");
    println!("  Integrations:");
    println!("    - Cloud: AWS (CloudWatch, GuardDuty, VPC Flow Logs), Azure, GCP");
    println!("    - Container: Kubernetes (deep), Docker, ECS, Fargate, GKE, AKS");
    println!("    - Shippers: Filebeat, Metricbeat, Auditbeat, Fluentd, FluentBit, OpenTelemetry");
    println!("    - Alerts: PagerDuty, Opsgenie, VictorOps, Slack, Teams, ServiceNow, Jira");
    println!("    - AWS-native: Lambda, CloudWatch Logs, VPC Flow Logs, AWS Config");
    println!("    - APM: OpenTelemetry-native, also supports Datadog tracer + Jaeger client");
    println!("  Logz.io CLI usage:");
    println!("    logzio login --region us-east-1 --account my-org");
    println!("    logzio logs search --query 'level:error' --from -1h");
    println!("    logzio dashboard import --file dashboard.json");
    println!("    logzio alert create --name 'High Error Rate' --condition 'count > 100'");
    println!("    logzio telemetry-collector deploy --target kubernetes");
    println!("    logzio siem rule list --severity high");
    println!("    logzio apm service list --env production");
    println!("  Customers (~10,000+):");
    println!("    - Cloud-native + DevOps-led mid-market sweet spot");
    println!("    - Schneider Electric, ZF Group, Unity, Akamai, ironSource, Soluto");
    println!("    - International: significant European + Israeli enterprise");
    println!("    - Strong in: SaaS startups, gaming, fintech, cloud-native infra teams");
    println!("  Critique: Datadog's marketing + acquisitions dominate share growth");
    println!("           per-GB pricing can still surprise at scale (vs Chronosphere's metric-cost focus)");
    println!("           App 360 (APM) less mature than Datadog APM in distributed-trace UX");
    println!("           Elastic Cloud (vendor of ELK) competes head-to-head with marketing budget");
    println!("           Grafana Cloud's all-OSS bundling attracts similar 'no lock-in' buyers");
    println!("           Cloud SIEM relatively young vs Splunk + Sumo Logic in enterprise SOC");
    println!("           growth slower than top-tier observability vendors in 2023-2024");
    println!("  Differentiator: 100% open-source-based observability platform (ELK + Prometheus + Jaeger + OpenTelemetry) + Cloud SIEM on same stack + 'Open 360' unified UI + managed OpenSearch (post-Elastic-license-drama) + 'Cognitive Insights' ML log analysis + 10K+ customers in cloud-native + Israeli + Boston dual HQ — the no-lock-in observability platform for teams that want managed ELK/Prometheus/Jaeger without running it themselves");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logzio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logzio(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logzio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logzio"), "logzio");
        assert_eq!(basename(r"C:\bin\logzio.exe"), "logzio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logzio.exe"), "logzio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_logzio(&["--help".to_string()], "logzio"), 0);
        assert_eq!(run_logzio(&["-h".to_string()], "logzio"), 0);
        assert_eq!(run_logzio(&["--version".to_string()], "logzio"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_logzio(&[], "logzio"), 0);
    }
}
