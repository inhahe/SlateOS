#![deny(clippy::all)]

//! confluent-cli — OurOS Confluent (commercial Apache Kafka, NASDAQ:CFLT)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cflt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: confluent [OPTIONS]");
        println!("Confluent (OurOS) — Apache Kafka cloud + enterprise platform (NASDAQ:CFLT)");
        println!();
        println!("Options:");
        println!("  --cloud                Confluent Cloud (managed Kafka SaaS)");
        println!("  --platform             Confluent Platform (self-hosted enterprise)");
        println!("  --flink                Apache Flink stream processing (managed, Immerok-based)");
        println!("  --connect              Kafka Connect (200+ source/sink connectors)");
        println!("  --schema-registry      Schema Registry (Avro/Protobuf/JSON schema versioning)");
        println!("  --ksqldb               ksqlDB / Flink SQL streaming queries");
        println!("  --tableflow            Tableflow (Kafka topics → Iceberg lakehouse, 2024)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Confluent 2024 (OurOS)"); return 0; }
    println!("Confluent 2024 (OurOS)");
    println!("  Vendor: Confluent, Inc. (Mountain View, CA — NASDAQ:CFLT)");
    println!("  Founders: Jay Kreps (CEO), Jun Rao, Neha Narkhede, 2014");
    println!("          all three co-created Apache Kafka at LinkedIn (2010-2014)");
    println!("          Kreps + Rao + Narkhede left LinkedIn together to form Confluent");
    println!("          one of the great 'open-source founders → commercial venture' stories");
    println!("          Narkhede left 2020, founded Oscilar (AI risk decisioning)");
    println!("          Kreps still CEO — published 'I Heart Logs' (free O'Reilly book)");
    println!("  Founded: 2014 in Palo Alto/Mountain View");
    println!("          IPO Jun 2021 NASDAQ:CFLT at $36 (~$11B valuation)");
    println!("          peaked ~$93 late 2021, dropped to $20-30 range 2022-2024");
    println!("          FY2024 revenue ~$960M (+24% YoY), still operating losses but improving");
    println!("          ~3,500 employees");
    println!("          ~4,800 paying customers");
    println!("  Strategic position: 'event-streaming platform' — Kafka + everything that needs Kafka:");
    println!("                    primary competitor: AWS MSK (Managed Kafka), Aiven, Cloudera, Redpanda (BYOL alternative)");
    println!("                    Streaming alternatives: Apache Pulsar, Amazon Kinesis, Google Pub/Sub, Azure Event Hubs");
    println!("                    Confluent's wedge: original Kafka creators + Schema Registry + Connectors + Flink + ksqlDB");
    println!("                    pitch: 'data in motion' — every enterprise needs real-time pipelines + Kafka is the dominant standard");
    println!("                    enterprise sales motion + heavy investment in cloud-managed offering");
    println!("                    open-source Kafka governance still at Apache Software Foundation");
    println!("  Pricing (consumption + tier — Confluent Cloud is the growth driver):");
    println!("    Confluent Cloud Free — limited to 1 cluster, low throughput, dev/test only");
    println!("    Confluent Cloud Basic — pay-as-you-go ~$0.11/GB ingress, no SLA");
    println!("    Confluent Cloud Standard — SLA, regional, ~$0.12/GB + storage");
    println!("    Confluent Cloud Enterprise — private networking, premium SLA, ~$1K-50K+/mo typical");
    println!("    Confluent Cloud Dedicated — your own cluster, $$ pricing for max performance");
    println!("    Confluent Platform — annual license, $50K-$5M+/yr enterprise self-hosted");
    println!("    typical enterprise deals: $200K-$10M+/yr (some Fortune 500 over $20M ARR)");
    println!("  Core Kafka:");
    println!("    - Distributed append-only log: producers → topics → consumers");
    println!("    - Partitioned topics with replication for durability");
    println!("    - Exactly-once semantics (since 0.11, mature in 2.x)");
    println!("    - KRaft (Kafka Raft) replacing ZooKeeper (default in Kafka 3.3+)");
    println!("    - Tiered storage (cold data offloaded to S3) — Confluent Cloud + KIP-405 since 3.6");
    println!("    - Massive throughput: millions of events/sec per cluster");
    println!("  Confluent Cloud (the managed SaaS):");
    println!("    - Provision Kafka cluster in any AWS/GCP/Azure region in minutes");
    println!("    - Auto-scaling brokers + storage");
    println!("    - 99.99% SLA on Standard/Enterprise/Dedicated");
    println!("    - Stream Governance: Schema Registry + Lineage + Data Catalog");
    println!("    - Stream Sharing: securely share topics across orgs (B2B data sharing)");
    println!("    - Multi-region clusters + Cluster Linking (geo-replication)");
    println!("    - Private Networking (VPC peering, PrivateLink)");
    println!("  Kafka Connect (200+ connectors):");
    println!("    - Sources: Postgres, MySQL CDC (Debezium), MongoDB, Salesforce, Stripe, Mailchimp");
    println!("    - Sinks: Snowflake, BigQuery, Databricks, Elasticsearch, MongoDB, S3, Iceberg, Postgres");
    println!("    - SAP integration + Oracle CDC + JDBC connectors");
    println!("    - Schema-aware: respects Schema Registry contracts");
    println!("    - Many maintained by Confluent + community");
    println!("  Schema Registry (the unsung hero):");
    println!("    - Centralized schema management for Avro + Protobuf + JSON Schema");
    println!("    - Schema versioning + evolution (backward/forward/full compatibility checks)");
    println!("    - Prevents 'breaking topic consumers' problem");
    println!("    - Essential for serious Kafka deployments");
    println!("    - One of Confluent's most differentiated assets");
    println!("  Stream Processing:");
    println!("    - Kafka Streams (Java library) — original streaming framework");
    println!("    - ksqlDB — SQL-like stream processing on Kafka topics");
    println!("    - Apache Flink (since acquiring Immerok Jan 2023 for ~$60M) — full state, complex event processing");
    println!("    - Confluent's bet: Flink becomes the default stream-processing layer (vs ksqlDB)");
    println!("    - Managed Flink in Confluent Cloud since 2024");
    println!("  Tableflow (2024 announcement):");
    println!("    - Auto-publish Kafka topics as Apache Iceberg tables in S3");
    println!("    - Streaming events become lakehouse tables queryable from any engine (Snowflake, Databricks, Athena, Trino)");
    println!("    - Confluent's response to 'Iceberg + streaming' converging");
    println!("    - Direct competitor to: Redpanda Iceberg topics, Snowflake Snowpipe Streaming");
    println!("  Confluent Connect + Stream Governance:");
    println!("    - Stream Catalog: discover topics + schemas across org");
    println!("    - Stream Lineage: visualize where events come from + go to");
    println!("    - Tag-based access control (RBAC)");
    println!("    - Stream Quality + alerts on schema drift");
    println!("  Confluent CLI usage:");
    println!("    confluent login");
    println!("    confluent environment list");
    println!("    confluent kafka cluster create my-cluster --cloud aws --region us-west-2 --type basic");
    println!("    confluent kafka topic create events --partitions 6");
    println!("    confluent kafka topic produce events --schema events.avsc");
    println!("    confluent kafka topic consume events --from-beginning");
    println!("  Connectors + integrations:");
    println!("    - Stream Sharing partners: Databricks, Snowflake, MongoDB, Atlas, Imply (Druid)");
    println!("    - SAP Datasphere integration (announced 2024)");
    println!("    - GCP/Azure Marketplace listings + AWS partnership");
    println!("  Customers: ~4,800 paying customers");
    println!("            Goldman Sachs, Citi, Capital One, Wells Fargo, JPMorgan (real-time payments, fraud)");
    println!("            Walmart, Target, eBay, Domino's (real-time inventory, ML personalization)");
    println!("            BMW, Volvo (connected car telemetry), Lufthansa, Sky, Disney+");
    println!("            sweet spot: Fortune 1000 with real-time data needs + microservices + event-driven architecture");
    println!("            heavy in: financial services, retail, telco, manufacturing, automotive");
    println!("  Critique: expensive at scale (consumption-based pricing surprises)");
    println!("           competition: AWS MSK + Aiven offer Kafka cheaper (no Confluent enterprise features)");
    println!("           Redpanda (Apache 2.0 Kafka alternative) growing fast, often 10x cheaper");
    println!("           Kafka itself complex to operate even managed — Schema Registry + Connect + Flink each operational");
    println!("           Apache Pulsar (Streamnative) + Materialize (real-time analytics) compete on adjacent needs");
    println!("           operating losses persistent — stock down from peak, growth slowing");
    println!("           Tableflow + Iceberg integration: behind Snowflake's Snowpipe Streaming in adoption");
    println!("           Apache Kafka itself is open-source — Confluent must justify premium vs free");
    println!("  Differentiator: original Kafka creators + Schema Registry + 200+ Connectors + managed Flink + Cloud-native experience + Stream Governance — the de-facto choice for enterprise event streaming");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "confluent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cflt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cflt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/confluent"), "confluent");
        assert_eq!(basename(r"C:\bin\confluent.exe"), "confluent.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("confluent.exe"), "confluent");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cflt(&["--help".to_string()], "confluent"), 0);
        assert_eq!(run_cflt(&["-h".to_string()], "confluent"), 0);
        assert_eq!(run_cflt(&["--version".to_string()], "confluent"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cflt(&[], "confluent"), 0);
    }
}
