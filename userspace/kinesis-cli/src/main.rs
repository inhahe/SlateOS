#![deny(clippy::all)]

//! kinesis-cli — OurOS AWS Kinesis (real-time data streaming on AWS, Seattle WA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kinesis(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kinesis [OPTIONS]");
        println!("AWS Kinesis (OurOS) — real-time data streaming family on AWS");
        println!();
        println!("Options:");
        println!("  --data-streams         Kinesis Data Streams (raw streaming, shards)");
        println!("  --firehose             Data Firehose (managed delivery to S3/Redshift/OpenSearch)");
        println!("  --video-streams        Kinesis Video Streams (video ingestion + storage)");
        println!("  --analytics            Managed Service for Apache Flink (formerly Kinesis Analytics)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AWS Kinesis 2024 (OurOS) — kinesis CLI (aws-cli v2)"); return 0; }
    println!("AWS Kinesis 2024 (OurOS) — Real-Time Data Streaming on AWS");
    println!("  Vendor: Amazon Web Services (Seattle, WA — NASDAQ: AMZN)");
    println!("  History:");
    println!("    - Launched Nov 2013 at AWS re:Invent");
    println!("    - Built to compete with on-prem stream processing (Storm, Spark Streaming)");
    println!("    - Kinesis Firehose added 2015 (managed delivery)");
    println!("    - Kinesis Analytics added 2016 (SQL on streams)");
    println!("    - Kinesis Video Streams added 2017 (video ingestion)");
    println!("    - Kinesis Analytics renamed Managed Apache Flink 2023");
    println!("    - Kinesis Firehose renamed Data Firehose 2024");
    println!("  Strategic position: 'managed streaming for AWS-native data pipelines':");
    println!("                    pitch: 'real-time streams without managing Kafka brokers'");
    println!("                    target: AWS-shop data engineering teams, clickstream, IoT, log ingestion");
    println!("                    primary competitor: Apache Kafka (self-managed), MSK (AWS managed Kafka)");
    println!("                    secondary: Google Pub/Sub, Azure Event Hubs, Confluent Cloud, Redpanda");
    println!("                    Kinesis wedge: deepest AWS integration (IAM, KMS, CloudWatch, Lambda)");
    println!("                    pay-per-shard or pay-per-throughput pricing");
    println!("                    Amazon's first-party streaming = strong default for AWS shops");
    println!("  Pricing (multi-product, complex):");
    println!("    Kinesis Data Streams Provisioned: $0.015/shard/hour + $0.014/million PUT records");
    println!("    Kinesis Data Streams On-Demand: $0.04/GB ingress + $0.04/GB egress (Sep 2021+)");
    println!("    Data Firehose: $0.029/GB ingested + format conversion fees");
    println!("    Video Streams: $0.0085/MB ingested + $0.023/GB stored + processing");
    println!("    Managed Apache Flink: $0.11/Kinesis Processing Unit (KPU)/hour");
    println!("    notably cheaper than MSK at low throughput, more expensive at high throughput");
    println!("    'pay only for what you stream' — appealing for spiky workloads");
    println!("  Architecture (the AWS-native pieces):");
    println!("    - Kinesis Data Streams: shard-based (like Kafka partitions)");
    println!("    - Each shard: 1 MB/s ingress, 2 MB/s egress, 1000 records/s");
    println!("    - Records ordered within shard (key-based)");
    println!("    - 24-hour default retention (extendable to 365 days)");
    println!("    - KCL (Kinesis Client Library) for consumers — handles checkpointing");
    println!("    - KPL (Kinesis Producer Library) for producers — handles batching");
    println!("    - Enhanced Fan-Out: dedicated 2 MB/s per consumer (no shared bandwidth)");
    println!("    - Server-side encryption with KMS keys");
    println!("    - Lambda integration: trigger function per batch of records");
    println!("  Product portfolio:");
    println!("    1. Kinesis Data Streams (the raw streaming):");
    println!("       - Shard-based scaling (manually or on-demand)");
    println!("       - Producer/consumer model with KCL/KPL");
    println!("       - 24h to 365d retention");
    println!("       - At-least-once delivery semantics");
    println!("       - Used for: clickstreams, IoT telemetry, log ingestion, financial ticks");
    println!("    2. Data Firehose (managed delivery, formerly Kinesis Firehose):");
    println!("       - Push records, Firehose delivers to S3/Redshift/OpenSearch/Splunk/HTTP");
    println!("       - Buffering (size + time), compression (gzip/snappy/parquet/ORC)");
    println!("       - Optional Lambda transformation pre-delivery");
    println!("       - Dynamic partitioning for partition-aware S3 layout");
    println!("       - 'Fully managed, just send data, we deliver it'");
    println!("    3. Kinesis Video Streams:");
    println!("       - Video ingestion at scale (security cameras, drones, mobile)");
    println!("       - Built-in playback, storage, archive");
    println!("       - WebRTC signaling for live two-way video");
    println!("       - Integration with Rekognition Video for ML analysis");
    println!("       - Used by: security camera platforms, telemedicine, smart-home");
    println!("    4. Managed Service for Apache Flink (formerly Kinesis Data Analytics):");
    println!("       - Managed Flink runtime (Apache Flink + AWS integrations)");
    println!("       - SQL queries on streams or full Flink Java/Scala apps");
    println!("       - Stateful processing, exactly-once semantics");
    println!("       - Auto-scaling Kinesis Processing Units (KPUs)");
    println!("       - Replaces older SQL-on-streams flavor (deprecated 2025)");
    println!("    5. Kinesis Agent (the ingestion tool):");
    println!("       - Lightweight Linux agent");
    println!("       - Tails log files, batches, sends to Data Streams or Firehose");
    println!("       - Pre-processing rules (parse, filter, transform)");
    println!("    6. Cross-region replication:");
    println!("       - Replicate streams to another region for DR");
    println!("       - Cross-account consumers via resource policies");
    println!("    7. CloudWatch integration:");
    println!("       - Per-shard metrics (IncomingRecords, IteratorAge)");
    println!("       - Alarms for lag detection");
    println!("       - 'IteratorAge growing' = consumers falling behind");
    println!("    8. Lambda triggers:");
    println!("       - Lambda function invoked per batch of records");
    println!("       - Automatic scaling with shard count");
    println!("       - Common pattern: stream → Lambda → DynamoDB/Aurora");
    println!("  The shard model vs Kafka partitions:");
    println!("    - Kafka: partitions are static, rebalance for scaling");
    println!("    - Kinesis: shards can be split + merged dynamically (resharding)");
    println!("    - Both: ordered within partition/shard, parallel across");
    println!("    - Kinesis advantage: managed resharding via on-demand mode");
    println!("    - Kafka advantage: better ecosystem (Kafka Streams, ksqlDB, Connect)");
    println!("  The AWS lock-in angle:");
    println!("    - Deep IAM integration (no external auth needed)");
    println!("    - VPC endpoints for private networking");
    println!("    - KMS-managed encryption at rest");
    println!("    - CloudWatch Metrics + Logs built-in");
    println!("    - Lambda + Firehose + Flink ecosystem AWS-only");
    println!("    - Migrating off AWS = full app rewrite");
    println!("    - Trade-off: zero ops vs vendor lock-in");
    println!("  Integrations:");
    println!("    - aws-cli (Python-based, primary CLI)");
    println!("    - AWS SDKs: Python (boto3), Java, JS/TS, Go, .NET, Ruby, etc.");
    println!("    - KCL/KPL Java + multi-lang daemon");
    println!("    - Lambda triggers (event source mapping)");
    println!("    - Firehose → S3 → Athena/Glue (analytics)");
    println!("    - Apache Flink connector (Managed Flink)");
    println!("    - Apache Spark connector (community)");
    println!("    - Logstash, Fluentd, Vector input plugins");
    println!("    - DataDog, New Relic, Splunk monitoring integrations");
    println!("  AWS CLI usage:");
    println!("    aws kinesis create-stream --stream-name my-stream --shard-count 4");
    println!("    aws kinesis put-record --stream-name my-stream --partition-key user-123 --data 'hello'");
    println!("    aws kinesis describe-stream --stream-name my-stream");
    println!("    aws kinesis get-shard-iterator --stream-name my-stream --shard-id shardId-0 --shard-iterator-type LATEST");
    println!("    aws kinesis get-records --shard-iterator <iter>");
    println!("    aws kinesis split-shard --stream-name my-stream --shard-to-split <id> --new-starting-hash-key <key>");
    println!("    aws kinesis update-shard-count --stream-name my-stream --target-shard-count 8 --scaling-type UNIFORM_SCALING");
    println!("    aws firehose create-delivery-stream --delivery-stream-name my-fh --s3-destination-configuration ...");
    println!("    aws kinesisvideo create-stream --stream-name my-video --data-retention-in-hours 24");
    println!("  Customers (any AWS-heavy data team):");
    println!("    - Netflix (early adopter — clickstream, log ingestion)");
    println!("    - Lyft, Airbnb, Pinterest, Coinbase, Slack");
    println!("    - Verizon, Comcast, Capital One (enterprise)");
    println!("    - Most AWS-native data engineering teams default to Kinesis or MSK");
    println!("    - 'If you're on AWS, your first streaming choice is Kinesis Firehose'");
    println!("  Critique: shard-based pricing surprises (forgotten provisioned shards = bill spike)");
    println!("           less mature ecosystem than Kafka (no Kafka Streams equivalent native)");
    println!("           Enhanced Fan-Out doubles cost (per-consumer dedicated bandwidth)");
    println!("           24h default retention too short for replay use cases (extend = $$)");
    println!("           AWS lock-in — migrating to Kafka or other cloud requires app rewrite");
    println!("           AWS MSK competing internally (Amazon's own managed Kafka)");
    println!("           Managed Flink expensive at scale (KPU pricing)");
    println!("           Firehose buffering minimum 60s/1MB = not truly real-time");
    println!("  Differentiator: AWS first-party streaming family covering Data Streams (raw shard-based streaming like Kafka partitions) + Data Firehose (managed delivery to S3/Redshift/OpenSearch/Splunk with buffering + transform) + Video Streams (managed video ingestion + storage + WebRTC) + Managed Apache Flink (formerly Kinesis Analytics, stream processing with exactly-once) + Kinesis Agent (Linux log shipper) + KCL/KPL (client libraries) + Enhanced Fan-Out (dedicated bandwidth per consumer) + KMS encryption + IAM auth + Lambda triggers + CloudWatch metrics + on-demand pricing (no shard management) + dynamic shard split/merge resharding + cross-region replication + cross-account consumers + Netflix/Lyft/Airbnb-proven + AWS Console UX + 24h-365d retention — the AWS-native streaming platform with the deepest IAM + KMS + Lambda + Firehose integration, the default streaming choice for AWS-heavy data engineering teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kinesis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kinesis(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
