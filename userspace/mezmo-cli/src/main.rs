#![deny(clippy::all)]

//! mezmo-cli — SlateOS Mezmo (was LogDNA, telemetry pipeline + log management, Mountain View CA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mezmo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mezmo [OPTIONS]");
        println!("Mezmo (Slate OS) — Telemetry Pipeline + log mgmt (was LogDNA, private)");
        println!();
        println!("Options:");
        println!("  --log-analysis         Log Analysis (the original LogDNA product)");
        println!("  --telemetry-pipeline   Telemetry Pipeline (data routing + transformation)");
        println!("  --processors           Pipeline processors (filter, reduce, redact, route)");
        println!("  --destinations         Destinations (S3, Datadog, Splunk, etc.)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mezmo 2024 (Slate OS)"); return 0; }
    println!("Mezmo 2024 (Slate OS) — Telemetry Pipeline + Log Analysis");
    println!("  Vendor: Mezmo, Inc. (Mountain View, CA + Vancouver — private)");
    println!("  History: Founded as LogDNA in 2015 by Chris Nguyen + Lee Liu");
    println!("          'LogDNA' = 'DNA of logs' branding for log management");
    println!("          Rebranded to 'Mezmo' Feb 2022 to reflect broader pipeline + observability vision");
    println!("          Chris Nguyen: long-time CEO");
    println!("          'Mezmo' = phonetic spelling of 'mesmo' (Portuguese 'same' / 'self')");
    println!("  Private funding:");
    println!("         Series D Mar 2022: $50M at $700M valuation (Anthos Capital, Initialized)");
    println!("         total raised: ~$95M");
    println!("         Initialized, Microsoft M12, Salesforce Ventures, Hercules Capital backers");
    println!("         estimated $50-80M ARR (private)");
    println!("  Strategic position: 'telemetry pipeline — control + route observability data':");
    println!("                    pitch: 'reduce, redact, route — pay only for the observability data you need'");
    println!("                    target: cost-conscious observability + compliance-aware enterprises");
    println!("                    primary competitor: Cribl (head-to-head telemetry pipeline), Datadog, Splunk, Sumo Logic");
    println!("                    secondary: Edge Delta, Vector (open-source), Fluentd, Logz.io");
    println!("                    Mezmo's wedge: telemetry pipeline + log analysis in single platform + IBM Cloud Logging OEM");
    println!("                    pivot 2022: from log management to telemetry pipeline (Cribl's category) + log mgmt");
    println!("  Pricing:");
    println!("    Log Analysis: $0.80/GB/month for 7-day retention (volume-tiered)");
    println!("    Telemetry Pipeline: per-GB processed pricing");
    println!("    Enterprise: $50K-$1M+/yr typical");
    println!("    Free tier: 30-day trial");
    println!("    typically 30-50% cheaper than Splunk/Datadog for similar volumes");
    println!("  Product portfolio:");
    println!("    1. Log Analysis (the LogDNA heritage):");
    println!("       - Real-time log search + analytics");
    println!("       - Live tail (streaming logs as they arrive)");
    println!("       - Pre-built parsers + alerts");
    println!("       - Kubernetes + Docker auto-detection");
    println!("    2. Telemetry Pipeline (the Mezmo pivot product):");
    println!("       - Stream-processing pipeline for logs + metrics + traces");
    println!("       - Visual pipeline builder");
    println!("       - Route to multiple destinations from one ingestion point");
    println!("       - Compete with: Cribl Stream, Edge Delta, Vector (open-source)");
    println!("    3. Pipeline Processors:");
    println!("       - Filter: drop unwanted events");
    println!("       - Reduce: aggregate metrics from logs");
    println!("       - Redact: PII removal + masking");
    println!("       - Route: send subsets to different destinations");
    println!("       - Encrypt: end-to-end encryption for sensitive data");
    println!("       - Sample: stochastic sampling for volume control");
    println!("       - Mask: data anonymization");
    println!("       - Throttle: rate limiting");
    println!("       - Decode: JSON, CEF, syslog, etc. parsers");
    println!("    4. Destinations:");
    println!("       - Mezmo Log Analysis (self-destination)");
    println!("       - S3, GCS, Azure Blob (cold storage)");
    println!("       - Datadog, Splunk, New Relic, Elastic (other SIEMs)");
    println!("       - Kafka, Kinesis (streaming sinks)");
    println!("       - Snowflake, BigQuery, Redshift (data warehouses)");
    println!("    5. Live Tail + Search:");
    println!("       - Real-time log streaming with filters");
    println!("       - Saved searches + shareable URLs");
    println!("    6. Alerts:");
    println!("       - Threshold-based alerts on log patterns");
    println!("       - PagerDuty, Slack, Teams, Opsgenie, ServiceNow, email");
    println!("    7. Compliance + Audit Logs:");
    println!("       - HIPAA-eligible, PCI-DSS, SOC 2 Type II");
    println!("       - Audit trail for searches + access");
    println!("    8. Templates Library:");
    println!("       - Pre-built pipeline templates for common transforms");
    println!("       - Kubernetes audit log filtering, AWS CloudTrail noise reduction, etc.");
    println!("  Telemetry Pipeline strategy (the 2022 pivot):");
    println!("    - Recognized that 50-80% of log volume is noise");
    println!("    - Pipeline-first reduces downstream cost (Datadog/Splunk bills shrink)");
    println!("    - Customers run Mezmo as 'pre-filter' for Splunk/Datadog");
    println!("    - Compete directly with Cribl Stream (the category leader)");
    println!("    - Differentiator: integrated log analysis backend in same platform");
    println!("  IBM Cloud Logging partnership:");
    println!("    - LogDNA was IBM Cloud's official log management offering ('IBM Log Analysis')");
    println!("    - OEM partnership generated significant revenue");
    println!("    - IBM Log Analysis powered by Mezmo backend");
    println!("    - Material revenue contributor + sales channel");
    println!("  Integrations:");
    println!("    - Sources: Kubernetes, Docker, AWS, Azure, GCP, syslog, FluentBit, Filebeat");
    println!("    - OpenTelemetry (ingest support)");
    println!("    - Cloud Logs: CloudWatch, GuardDuty, VPC Flow, CloudTrail, Azure Activity, GCP Cloud Logging");
    println!("    - SIEM destinations: Splunk, Datadog, New Relic, Sumo Logic, Elastic");
    println!("    - Cold storage: S3, GCS, Azure Blob (with rehydration)");
    println!("    - Streaming: Kafka, Kinesis, Pub/Sub, Event Hubs");
    println!("    - Alerts: PagerDuty, Opsgenie, Slack, Teams, ServiceNow, Webhooks");
    println!("    - SSO: Okta, OneLogin, Azure AD, Google Workspace, SAML, OIDC");
    println!("  Mezmo CLI usage:");
    println!("    mezmo login --tenant my-org");
    println!("    mezmo log search --query 'level:error' --from -1h --tail");
    println!("    mezmo pipeline create --name 'k8s-prod-noise-reduction' --source k8s --dest splunk");
    println!("    mezmo processor add --pipeline 'k8s-prod' --type redact --pattern 'email|ssn'");
    println!("    mezmo destination create --type s3 --bucket logs-cold-tier --format parquet");
    println!("    mezmo alert create --name 'High Error Rate' --condition 'count > 100'");
    println!("    mezmo template browse --category kubernetes");
    println!("  Customers (~3,000+ direct + many via IBM):");
    println!("    - DevOps + SRE teams cost-conscious about observability bills");
    println!("    - IBM Cloud customers (huge OEM channel)");
    println!("    - Asana, Instacart, Shopify (some teams), HashiCorp (formerly), Coinbase");
    println!("    - International: significant via IBM Cloud global");
    println!("    - sweet spot: $50M-$2B revenue SaaS + tech with K8s adoption");
    println!("  Critique: Cribl Stream dominates the telemetry pipeline category");
    println!("           rebrand from LogDNA to Mezmo confused some legacy customers");
    println!("           log analysis backend less feature-rich than Datadog Logs / Splunk");
    println!("           IBM Cloud OEM dependency = revenue concentration risk");
    println!("           tracing + metrics support thinner than dedicated platforms");
    println!("           Edge Delta + Vector (Datadog acquisition) compress competitive space");
    println!("           growth slower than peer observability vendors in 2023-2024");
    println!("           AI features minimal vs Datadog / Dynatrace");
    println!("  Differentiator: telemetry pipeline + log analysis in single platform (rare combination — Cribl is pipeline-only) + IBM Cloud Logging OEM partnership (material revenue + global distribution) + visual pipeline builder with rich processors (redact, reduce, route, encrypt) + LogDNA heritage with sub-second log search + 30-50% cost reduction for downstream SIEMs by filtering noise — the dual-purpose telemetry pipeline + log management platform that customers use to shape their observability data while keeping logs analyzable in the same place");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mezmo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mezmo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mezmo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mezmo"), "mezmo");
        assert_eq!(basename(r"C:\bin\mezmo.exe"), "mezmo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mezmo.exe"), "mezmo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mezmo(&["--help".to_string()], "mezmo"), 0);
        assert_eq!(run_mezmo(&["-h".to_string()], "mezmo"), 0);
        let _ = run_mezmo(&["--version".to_string()], "mezmo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mezmo(&[], "mezmo");
    }
}
