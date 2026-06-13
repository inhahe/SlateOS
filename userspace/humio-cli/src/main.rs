#![deny(clippy::all)]

//! humio-cli — SlateOS Humio / CrowdStrike Falcon LogScale (log-management, Aarhus DK / Sunnyvale CA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_humio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: humio [OPTIONS]");
        println!("Humio (now Falcon LogScale) (SlateOS) — index-free log management (CrowdStrike subsidiary)");
        println!();
        println!("Options:");
        println!("  --logscale             Falcon LogScale (the rebranded product)");
        println!("  --repositories         Log repositories");
        println!("  --queries              LogScale Query Language (LQL)");
        println!("  --dashboards           Dashboards + visualizations");
        println!("  --alerts               Alert rules");
        println!("  --falcon-integration   Falcon platform / NG-SIEM integration");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Falcon LogScale 2024 (was Humio, SlateOS)"); return 0; }
    println!("Humio / Falcon LogScale 2024 (SlateOS) — Index-Free Log Management");
    println!("  Vendor: CrowdStrike Holdings (Falcon LogScale division, integrated since 2021)");
    println!("  Original Humio: Humio ApS — Aarhus, Denmark + London (founded 2016)");
    println!("  Founders: Geeta Schmidt (CEO) + Kresten Krab Thorup (CTO) + Christian Hvitved (Chief Architect)");
    println!("          founded with thesis: 'index-free architecture for cheap, fast log ingest at any scale'");
    println!("          Kresten Krab Thorup: ex-Trifork CTO + Erlang expert (Humio is Java/Scala)");
    println!("          unique architectural approach: 'stream + scan' instead of 'index + look up'");
    println!("  Acquisition: CrowdStrike bought Humio Feb 2021 for ~$400M");
    println!("              Strategic: CrowdStrike's wedge into log management + SIEM expansion");
    println!("              Renamed to 'Falcon LogScale' in 2022");
    println!("              Now anchors CrowdStrike's NG-SIEM product (announced 2023)");
    println!("              Geeta Schmidt + co-founders remained at CrowdStrike");
    println!("  Strategic position: 'index-free log management — modern, fast, affordable at any volume':");
    println!("                    pitch: 'logs at any scale without breaking the bank or the search latency'");
    println!("                    target: enterprise security + observability (especially CrowdStrike Falcon customers)");
    println!("                    primary competitor: Splunk (head-to-head), Datadog Logs, Elastic, Sumo Logic");
    println!("                    secondary: Logz.io, Graylog, Mezmo, Cribl (log pipelines)");
    println!("                    Humio's wedge: 1TB/day at ~$1K-$2K with sub-second search latency (vs Splunk's $50K+)");
    println!("                    'Sub-second search at terabyte/petabyte scale' is the marketing claim");
    println!("  Pricing:");
    println!("    Falcon LogScale Self-Hosted: $20K-$5M+/yr (volume-tiered)");
    println!("    Falcon LogScale Cloud: $30-$150/GB/year (depending on retention)");
    println!("    Community Edition: free up to 16 GB/day ingestion, 7 days retention");
    println!("    Falcon NG-SIEM bundle: $30K-$5M+/yr with Falcon Insight EDR");
    println!("    typically 5-10x cheaper than Splunk per TB/day for similar query latency");
    println!("  Architecture (the 'index-free' bet):");
    println!("    - No traditional inverted-index per term (vs Splunk + Elasticsearch)");
    println!("    - Compresses logs aggressively + stores efficiently");
    println!("    - Queries scan compressed data in parallel across nodes");
    println!("    - Modern CPUs + NVMe make 'scan' competitive with 'index'");
    println!("    - Saves on storage cost + ingestion cost (no index build)");
    println!("    - Trade-off: deep historical queries slower than indexed lookup");
    println!("  Product portfolio (Falcon LogScale):");
    println!("    1. Falcon LogScale (the engine):");
    println!("       - Index-free log storage + search");
    println!("       - LQL (LogScale Query Language) — pipe-syntax DSL like Splunk SPL");
    println!("       - Real-time streaming queries (continuous queries trigger alerts)");
    println!("       - PB-scale capable single tenant");
    println!("    2. LogScale Repositories:");
    println!("       - Logical partitions for log data");
    println!("       - Per-repo retention + access control");
    println!("       - Compressed + columnar storage");
    println!("    3. Dashboards + Widgets:");
    println!("       - Charts, tables, gauges, heatmaps");
    println!("       - Saved searches as 'widgets'");
    println!("       - Hand-build or import from community");
    println!("    4. Real-time Alerts:");
    println!("       - Continuous queries fire alerts on threshold breaches");
    println!("       - PagerDuty, Slack, Teams, OpsGenie, ServiceNow, Email integrations");
    println!("    5. Falcon Next-Gen SIEM (2023 — built on LogScale):");
    println!("       - Anchors CrowdStrike's SIEM offering");
    println!("       - Combines Falcon Insight EDR data + LogScale storage + Charlotte AI analytics");
    println!("       - Competes head-to-head with Splunk ES, Microsoft Sentinel, Sumo Logic CSE");
    println!("    6. LogScale Connectors + Forwarders:");
    println!("       - Native shippers: Falcon LogScale Collector, Falcon Forwarder");
    println!("       - Supports: Fluentd, Filebeat, OpenTelemetry, Logstash, Vector, syslog");
    println!("       - Cloud: AWS, Azure, GCP CloudWatch/Activity Log/Cloud Logging");
    println!("    7. LogScale Marketplace:");
    println!("       - Pre-built dashboards + parsers + detection rules");
    println!("       - CrowdStrike-built + community-contributed");
    println!("    8. LogScale Cloud (managed SaaS):");
    println!("       - Multi-tenant managed offering");
    println!("       - GovCloud + EU residency options");
    println!("  CrowdStrike Falcon integration:");
    println!("    - Tight integration with Falcon EDR data (every endpoint detection ships to LogScale)");
    println!("    - Charlotte AI (CrowdStrike LLM) can query LogScale conversationally");
    println!("    - 'Falcon NG-SIEM' = LogScale + Charlotte AI + Falcon Insight + Falcon Identity + Falcon Cloud");
    println!("    - End-to-end SOC platform with single agent");
    println!("    - Big-ticket displacement of Splunk in CrowdStrike installed base");
    println!("  Integrations:");
    println!("    - CrowdStrike: Falcon Insight, Falcon Identity, Falcon Cloud, Falcon for ICS");
    println!("    - Open: OpenTelemetry, Vector, Fluentd, Filebeat, Logstash, syslog");
    println!("    - Cloud: AWS CloudWatch + GuardDuty + VPC Flow + S3, Azure Activity, GCP Cloud Logging");
    println!("    - Network: Cisco ASA, Palo Alto, Fortinet, Zscaler, F5");
    println!("    - Identity: Okta, Microsoft Entra ID, Ping, Duo, Auth0");
    println!("    - Email: Microsoft 365, Google Workspace, Proofpoint, Mimecast");
    println!("    - Notifications: PagerDuty, Opsgenie, Slack, Teams, ServiceNow, Jira");
    println!("  LogScale CLI usage:");
    println!("    humio login --tenant my-org --token $LOGSCALE_TOKEN");
    println!("    humio repo list --filter active");
    println!("    humio query run 'level=error | groupBy(service) | count() | sort(count, desc)' --repo logs-prod");
    println!("    humio dashboard import --repo logs-prod --file dashboard.json");
    println!("    humio alert create --name 'Crash Rate Spike' --query 'level=fatal' --threshold 10 --window 5m");
    println!("    humio parser create --name custom-nginx --script @parser.lql");
    println!("    humio repo retention set --repo logs-prod --days 90");
    println!("  Customers (~3,000+ since CrowdStrike acquisition):");
    println!("    - CrowdStrike customer base (significant — 70K+ EDR customers cross-sold)");
    println!("    - Microsoft (used Humio for own internal logging pre-acquisition)");
    println!("    - Comcast, Bloomberg, Lockheed Martin, AIB Bank");
    println!("    - U.S. federal: significant DoD + civilian agency presence");
    println!("    - Original European footprint: Maersk, LEGO, Volvo, Saxo Bank");
    println!("    - Growth driver: Falcon NG-SIEM replacing Splunk + ELK in enterprise SOCs");
    println!("  Critique: rebrand to 'Falcon LogScale' lost some standalone Humio brand equity");
    println!("           CrowdStrike sales motion = often bundled, harder to buy standalone");
    println!("           index-free architecture trade-off: deep historical queries slower than indexed");
    println!("           Splunk SPL community much larger than LQL — onboarding cost for new users");
    println!("           NG-SIEM bake-off with Splunk in deep CrowdStrike accounts material");
    println!("           outside CrowdStrike ecosystem, brand awareness lower vs Splunk/Datadog");
    println!("           OpenTelemetry support continues catching up to native shippers");
    println!("           Aarhus Denmark dev hub talented but small vs giant US observability teams");
    println!("  Differentiator: index-free architecture (5-10x cheaper per TB/day than Splunk for similar latency) + LogScale Query Language (LQL) + sub-second search at PB scale + CrowdStrike Falcon NG-SIEM anchor (huge cross-sell into 70K+ EDR customers) + integration with Charlotte AI for conversational queries + Danish + UK engineering heritage — the log management platform that CrowdStrike is using to displace Splunk in its huge EDR install base");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "humio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_humio(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_humio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/humio"), "humio");
        assert_eq!(basename(r"C:\bin\humio.exe"), "humio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("humio.exe"), "humio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_humio(&["--help".to_string()], "humio"), 0);
        assert_eq!(run_humio(&["-h".to_string()], "humio"), 0);
        let _ = run_humio(&["--version".to_string()], "humio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_humio(&[], "humio");
    }
}
