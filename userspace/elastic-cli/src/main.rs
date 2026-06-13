#![deny(clippy::all)]

//! elastic-cli — SlateOS Elastic (Elasticsearch + Kibana + ELK Stack, Mountain View + Amsterdam, NYSE:ESTC)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_elastic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: elastic [OPTIONS]");
        println!("Elastic (SlateOS) — Elasticsearch + Kibana + Beats (NYSE:ESTC, search + observability + security)");
        println!();
        println!("Options:");
        println!("  --search               Elasticsearch (the search engine)");
        println!("  --kibana               Kibana (visualization + analytics)");
        println!("  --observability        Elastic Observability (logs + metrics + APM + uptime)");
        println!("  --security             Elastic Security (SIEM + endpoint)");
        println!("  --enterprise-search    Enterprise Search (Workplace + App + Site Search)");
        println!("  --machine-learning     ML for anomaly detection + AIOps + vector search");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Elastic 2024 (SlateOS) — Elasticsearch 8.x"); return 0; }
    println!("Elastic 2024 (SlateOS) — Search + Observability + Security Platform");
    println!("  Vendor: Elastic N.V. (Mountain View, CA + Amsterdam — NYSE:ESTC since 2018)");
    println!("  Founder: Shay Banon, 2012 (Elasticsearch BV — then 'Elastic')");
    println!("          Shay built Compass (search lib, 2004) → Elasticsearch (2010 — open-source)");
    println!("          Elasticsearch first public release 2010, became dominant Lucene-based search engine");
    println!("          ELK Stack: Elasticsearch + Logstash + Kibana — the open-source observability standard 2014-2020");
    println!("          Shay Banon stepped down as CEO Aug 2022, replaced by Ash Kulkarni");
    println!("          remains as engineering leader + on board");
    println!("  Public market (NYSE:ESTC):");
    println!("         IPO Oct 2018 at $36/share, opened $70 (one of best IPO pops)");
    println!("         peak ~$190 in late 2021");
    println!("         settled $80-130 range 2023-2024");
    println!("         FY2024 revenue: ~$1.27B (+18% YoY)");
    println!("         Market cap: ~$10-13B range");
    println!("         Profitability improving — focus on operational discipline");
    println!("  Strategic position: '3 use cases, one platform — search, observability, security':");
    println!("                    pitch: 'one platform, three solutions, the speed of relevance'");
    println!("                    target: every developer + every enterprise (broadest base of any observability vendor)");
    println!("                    primary competitor (observability): Datadog, Splunk, Logz.io, Sumo Logic");
    println!("                    primary competitor (search): OpenSearch (Amazon fork), Algolia, Solr");
    println!("                    primary competitor (security): Splunk, Microsoft Sentinel, CrowdStrike");
    println!("                    Elastic's wedge: Elasticsearch ubiquity + open-source community + 3-product bundling");
    println!("                    'Search is the foundation of all data problems' positioning");
    println!("  Pricing:");
    println!("    Elastic Cloud (SaaS):");
    println!("       Standard: from $95/mo (small clusters)");
    println!("       Gold: $0.31/hr per resource (small dev/prod)");
    println!("       Enterprise: $0.62/hr + cross-cluster replication");
    println!("    Self-managed: free tier + paid for security/ML/advanced features");
    println!("    Enterprise Search: $5K-$500K/yr based on documents");
    println!("    Endpoint Security (Elastic Defend): $4-12/endpoint/month");
    println!("    Observability bundles: $0.10-$0.50/GB ingestion typical");
    println!("  License history (controversial):");
    println!("    - Apache 2.0 since 2010");
    println!("    - Elastic License (SSPL+ELv2 dual) Jan 2021 — to prevent AWS competition");
    println!("    - Triggered the OpenSearch fork by AWS (Apache 2.0)");
    println!("    - Sep 2024: re-licensed Elasticsearch + Kibana under AGPLv3 (in addition to Elastic License)");
    println!("    - 'Elastic is back to open source' — Shay Banon blog");
    println!("  Product portfolio (3 Solutions):");
    println!("    1. Elasticsearch (the search engine):");
    println!("       - Distributed Lucene-based search + analytics engine");
    println!("       - JSON document store with rich queries");
    println!("       - Vector search (k-NN) for ML + RAG use cases (2022+)");
    println!("       - Used by 50%+ of Fortune 500 somewhere in their stack");
    println!("    2. Kibana (visualization + UI):");
    println!("       - Dashboards, queries, alerting UI");
    println!("       - 'Lens' visualization builder");
    println!("       - Discover (raw log browsing), Maps, Canvas");
    println!("    3. Beats + Logstash + Elastic Agent (ingestion):");
    println!("       - Filebeat, Metricbeat, Heartbeat, Auditbeat, Winlogbeat, Packetbeat");
    println!("       - Logstash (heavyweight ETL pipeline)");
    println!("       - Elastic Agent (unified next-gen agent)");
    println!("    4. Elastic Observability:");
    println!("       - Logs, metrics, APM, uptime, RUM");
    println!("       - OpenTelemetry-native");
    println!("       - Compete with: Datadog, Splunk, New Relic, Logz.io");
    println!("    5. Elastic Security (SIEM + Endpoint):");
    println!("       - Cloud SIEM on Elasticsearch");
    println!("       - Endpoint security (acquired Endgame 2019)");
    println!("       - MITRE ATT&CK mapping, detection rules");
    println!("       - Compete with: Splunk, Microsoft Sentinel, CrowdStrike");
    println!("    6. Enterprise Search:");
    println!("       - Workplace Search (intranet/SharePoint search)");
    println!("       - App Search (in-app search-as-a-service)");
    println!("       - Site Search (website search)");
    println!("       - Compete with: Algolia, Glean, Coveo");
    println!("    7. Elasticsearch Relevance Engine (ESRE):");
    println!("       - Vector search + hybrid (BM25 + ANN) ranking");
    println!("       - LLM integration (RAG patterns)");
    println!("       - Compete with: Pinecone, Weaviate, Qdrant, Milvus");
    println!("    8. Machine Learning + AIOps:");
    println!("       - Anomaly detection on time-series");
    println!("       - Classification, regression, embeddings");
    println!("       - Built-in NLP models");
    println!("    9. Elastic AI Assistant (2024):");
    println!("       - LLM-powered ChatOps for observability + security");
    println!("       - 'Ask Elastic' natural language to data queries");
    println!("  ELK Stack heritage (still the OSS standard):");
    println!("    - Elasticsearch — dominant search engine + analytics DB");
    println!("    - Logstash — log shipping + processing");
    println!("    - Kibana — visualization");
    println!("    - 300M+ Docker pulls (Elasticsearch alone)");
    println!("    - 50,000+ companies use Elasticsearch in production");
    println!("    - 100,000+ Kibana dashboards in active use");
    println!("  Integrations:");
    println!("    - Beats family + Logstash + Elastic Agent (Elastic's own shippers)");
    println!("    - OpenTelemetry (full ingestion support)");
    println!("    - Filebeat connectors: AWS, Azure, GCP services, Kubernetes, syslog, NGINX, Apache, etc.");
    println!("    - Cloud: AWS (deep), Azure, GCP (Elastic Cloud runs on all 3)");
    println!("    - Alerts: PagerDuty, Slack, Teams, ServiceNow, Jira");
    println!("    - SDK: Java, Python, Go, .NET, Ruby, PHP, JavaScript native clients");
    println!("    - Hadoop: HDFS, Spark, Hive (Elasticsearch-Hadoop connector)");
    println!("    - SIEM ingest: 300+ pre-built integrations for security data sources");
    println!("  Elastic CLI usage:");
    println!("    elastic auth login --cloud-id my-cluster --api-key XXXXX");
    println!("    elastic search --index logs-* --query 'level:error' --size 50");
    println!("    elastic kibana dashboard import --file my-dashboard.ndjson");
    println!("    elastic alert create --name 'High Error Rate' --condition 'count > 100'");
    println!("    elastic apm service list --env production");
    println!("    elastic security rule enable --rule-id detection-rule-12345");
    println!("    elastic ml job create --type anomaly-detection --bucket-span 15m");
    println!("    elastic enterprise-search workplace add-content-source --type sharepoint");
    println!("  Customers:");
    println!("    - 50%+ of Fortune 500 use Elasticsearch somewhere");
    println!("    - 17,000+ paying Elastic Cloud customers");
    println!("    - Netflix, Adobe, Cisco, Walmart, Uber (massive search use cases)");
    println!("    - U.S. federal: NSA, DoD, IRS (Elastic Federal Cloud)");
    println!("    - GitHub: Elasticsearch powers GitHub code search at massive scale");
    println!("    - Stack Overflow: Q&A search on Elasticsearch");
    println!("    - international: heavy in EMEA + APAC");
    println!("  Critique: 3-solution positioning can confuse buyers (search vs observability vs security)");
    println!("           OpenSearch (AWS-led fork) divides community + erodes mindshare");
    println!("           Elasticsearch Kubernetes deployment notoriously complex");
    println!("           Elastic Cloud pricing surprises at scale (especially for high-cardinality data)");
    println!("           Datadog growing observability share faster (better UX + simpler model)");
    println!("           ML/AIOps features less polished than Dynatrace Davis or DataDog Bits");
    println!("           Vector search competing with focused vendors (Pinecone) for RAG use cases");
    println!("           Endgame endpoint integration with Security Solution still maturing");
    println!("  Differentiator: Elasticsearch ubiquity (50%+ Fortune 500 install base) + 3-in-one platform (search + observability + security) + OpenTelemetry-native + vector search + ELK Stack open-source heritage + AGPLv3 + Elastic License dual-license return to OSS (2024) + ESRE (Elasticsearch Relevance Engine) for RAG/LLM + $1.27B revenue — the search-foundation observability + security platform that nearly every Fortune 500 has Elasticsearch running somewhere in their stack");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "elastic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_elastic(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_elastic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/elastic"), "elastic");
        assert_eq!(basename(r"C:\bin\elastic.exe"), "elastic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("elastic.exe"), "elastic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_elastic(&["--help".to_string()], "elastic"), 0);
        assert_eq!(run_elastic(&["-h".to_string()], "elastic"), 0);
        let _ = run_elastic(&["--version".to_string()], "elastic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_elastic(&[], "elastic");
    }
}
