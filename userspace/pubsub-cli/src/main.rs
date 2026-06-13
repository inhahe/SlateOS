#![deny(clippy::all)]

//! pubsub-cli — Slate OS Google Cloud Pub/Sub (Google's planet-scale messaging, built on Borg)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pubsub(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pubsub [OPTIONS]");
        println!("Google Cloud Pub/Sub (Slate OS) — planet-scale managed pub/sub (built on Google Borg)");
        println!();
        println!("Options:");
        println!("  --topic                Topic management (publisher side)");
        println!("  --subscription         Subscription management (consumer side)");
        println!("  --pull                 Pull subscription (consumer polls)");
        println!("  --push                 Push subscription (HTTP endpoint)");
        println!("  --lite                 Pub/Sub Lite (lower-cost, zonal, partitioned)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Google Cloud Pub/Sub 2024 (Slate OS) — gcloud pubsub CLI"); return 0; }
    println!("Google Cloud Pub/Sub 2024 (Slate OS) — Planet-Scale Managed Messaging (Google heritage)");
    println!("  Vendor: Google LLC, a subsidiary of Alphabet (Mountain View, CA — NASDAQ: GOOG)");
    println!("  History (the Google internal heritage):");
    println!("    - Internal predecessor: Google's PubSub2 (PubSub v2) — used internally for years");
    println!("    - Used internally for: Gmail event fanout, Search index updates, Ads pipelines");
    println!("    - Externalized as GCP Pub/Sub Mar 2015");
    println!("    - Built on Google's Borg + Spanner + Colossus infrastructure");
    println!("    - 'Same system that powers Gmail's billions of events/day'");
    println!("    - Pub/Sub Lite added 2020 (lower-cost partitioned variant)");
    println!("    - Pub/Sub Lite Kafka compatibility added 2022");
    println!("    - 'The Google-scale pub/sub' positioning");
    println!("  Strategic position: 'planet-scale pub/sub with exactly-once + global topics':");
    println!("                    pitch: 'one topic, global delivery, exactly-once semantics'");
    println!("                    target: real-time analytics, event-driven architectures, IoT, GCP-shop");
    println!("                    primary competitor: AWS SNS+SQS, AWS Kinesis, Azure Event Hubs, Confluent");
    println!("                    secondary: Apache Kafka (self-managed), RabbitMQ");
    println!("                    Pub/Sub wedge: global topics by default (single endpoint, multi-region)");
    println!("                    GCP-native (deep BigQuery, Dataflow, GCS integration)");
    println!("                    exactly-once delivery option (2021+)");
    println!("                    'The pub/sub Google uses internally for Gmail + Search'");
    println!("  Pricing (volume-based):");
    println!("    Pub/Sub: $40 per TiB of data ingested + delivered (first 10 GiB/month free)");
    println!("    Snapshot storage: $0.27/GiB/month");
    println!("    Pub/Sub Lite: $0.40/MiB/s/month for throughput + storage extra");
    println!("    BigQuery subscription: free (only BigQuery insert charges apply)");
    println!("    Cloud Storage subscription: free");
    println!("    notably charges per BYTE not per MESSAGE (Kinesis + SNS charge per-message)");
    println!("    Pub/Sub Lite ~10x cheaper for known steady throughput");
    println!("    'Free tier covers most hobby + small workloads'");
    println!("  Architecture (the Borg + Spanner backbone):");
    println!("    - Backed by Google's internal Borg cluster manager");
    println!("    - Spanner for global consistency");
    println!("    - Colossus for storage (Google's distributed FS, GFS successor)");
    println!("    - Global topics (single endpoint, automatic multi-region failover)");
    println!("    - Topics: publish point");
    println!("    - Subscriptions: pull, push (HTTP webhook), BigQuery, Cloud Storage, Lite");
    println!("    - At-least-once by default; exactly-once opt-in (2021+)");
    println!("    - 7-day default retention (extendable to 31 days)");
    println!("    - Ordering keys for partition-style ordering");
    println!("    - Message Filtering (subscriber filter expression)");
    println!("    - Dead-letter topics");
    println!("  Product portfolio:");
    println!("    1. Pub/Sub (the planet-scale flagship):");
    println!("       - Global topics with multi-region durability");
    println!("       - At-least-once delivery (default)");
    println!("       - Exactly-once delivery (opt-in, 2021+)");
    println!("       - Up to GB/s throughput per topic");
    println!("       - 7-31 day retention");
    println!("       - Used internally by Gmail, Search, Ads, Analytics");
    println!("    2. Pub/Sub Lite (lower-cost variant, 2020+):");
    println!("       - Zonal (not global) — lower cost trade-off");
    println!("       - Partitioned (like Kafka)");
    println!("       - 10x cheaper for steady, known throughput");
    println!("       - Kafka API compatible (2022+) — drop-in for Kafka workloads");
    println!("       - Used for: high-volume telemetry, log ingestion");
    println!("    3. Push subscriptions (the webhook model):");
    println!("       - Subscription delivers via HTTP POST to your endpoint");
    println!("       - Cloud Run + App Engine + Cloud Functions integration");
    println!("       - Subscriber-side concurrency control");
    println!("       - OIDC token auth for endpoints");
    println!("    4. Pull subscriptions (the polling model):");
    println!("       - Subscriber polls + ack");
    println!("       - StreamingPull (gRPC streaming) for low-latency");
    println!("       - Better for high-throughput consumers");
    println!("    5. BigQuery subscriptions (2022+):");
    println!("       - Pub/Sub → BigQuery direct (no Dataflow needed)");
    println!("       - Schema-aware writes");
    println!("       - Eliminates the canonical Pub/Sub → Dataflow → BigQuery pipeline for simple cases");
    println!("    6. Cloud Storage subscriptions (2023+):");
    println!("       - Pub/Sub → GCS object writes");
    println!("       - Time + size based batching");
    println!("       - Avro, JSON, text formats");
    println!("    7. Schema registry:");
    println!("       - Topic-level schemas (Avro, Protocol Buffers)");
    println!("       - Type-safe publish + consume");
    println!("       - Schema evolution rules");
    println!("    8. Ordering keys (2021+):");
    println!("       - Per-key ordering (like Kafka partition keys)");
    println!("       - Combined with exactly-once = strong semantics");
    println!("    9. Message Filtering (2020+):");
    println!("       - Subscription-level filter expression");
    println!("       - Reduces downstream cost");
    println!("       - Filter on attributes");
    println!("    10. Snapshots + seek:");
    println!("       - Snapshot a subscription's state");
    println!("       - Seek back in time (replay messages within retention)");
    println!("       - For testing + reprocessing");
    println!("  The 'global topic' angle (vs AWS regional Kinesis/SNS):");
    println!("    - AWS Kinesis: regional, you choose which region");
    println!("    - AWS SNS: regional with cross-region delivery option");
    println!("    - GCP Pub/Sub: global by default — single endpoint, automatic multi-region");
    println!("    - Publish anywhere, deliver anywhere");
    println!("    - No region selection in topic ARN — Google handles");
    println!("    - Critical for multi-region apps + DR");
    println!("    - Trade-off: pricing per-byte vs per-message means high-fanout workloads cost more");
    println!("  The Gmail heritage:");
    println!("    - Pub/Sub's internal twin processes Gmail's billions of events/day");
    println!("    - Search index updates flow through internal Pub/Sub");
    println!("    - Ads serving fanout uses it");
    println!("    - 'Same system that runs Google itself'");
    println!("    - Comparable scale claim: 'planet-scale, multi-trillion messages/day internally'");
    println!("  Integrations:");
    println!("    - gcloud CLI (primary)");
    println!("    - Client libraries: Python, Java, Go, Node, C#, Ruby, PHP, C++");
    println!("    - Cloud Functions trigger (event source)");
    println!("    - Cloud Run trigger (push subscription)");
    println!("    - Dataflow (Apache Beam) — canonical streaming processor");
    println!("    - BigQuery direct subscription (no Dataflow needed for simple sink)");
    println!("    - Cloud Storage subscription");
    println!("    - Eventarc (event routing layer, similar to EventBridge)");
    println!("    - Kafka API for Pub/Sub Lite (drop-in Kafka client compatibility)");
    println!("    - Cloud Monitoring metrics");
    println!("    - DataDog, Splunk, third-party observability");
    println!("  gcloud usage:");
    println!("    gcloud pubsub topics create my-topic");
    println!("    gcloud pubsub topics list");
    println!("    gcloud pubsub subscriptions create my-sub --topic=my-topic");
    println!("    gcloud pubsub subscriptions create my-push-sub --topic=my-topic --push-endpoint=https://example.com/webhook");
    println!("    gcloud pubsub topics publish my-topic --message='hello'");
    println!("    gcloud pubsub topics publish my-topic --message='hello' --ordering-key=user-123");
    println!("    gcloud pubsub subscriptions pull my-sub --auto-ack --limit=10");
    println!("    gcloud pubsub subscriptions create my-bq-sub --topic=my-topic --bigquery-table=project:dataset.table");
    println!("    gcloud pubsub subscriptions create my-gcs-sub --topic=my-topic --cloud-storage-bucket=my-bucket");
    println!("    gcloud pubsub snapshots create my-snap --subscription=my-sub");
    println!("    gcloud pubsub subscriptions seek my-sub --snapshot=my-snap");
    println!("  Customers (GCP-native event-driven architectures):");
    println!("    - Google itself (Gmail, Search, Ads, Analytics, YouTube)");
    println!("    - Spotify (heavy GCP user — analytics pipelines)");
    println!("    - PayPal (event-driven payments)");
    println!("    - Twitter/X (some GCP workloads)");
    println!("    - Snap (Snapchat — backend on GCP)");
    println!("    - Niantic (Pokemon GO — Spanner + Pub/Sub)");
    println!("    - Home Depot, Target, Best Buy (retail GCP shops)");
    println!("    - 'Anyone GCP-native uses Pub/Sub by default'");
    println!("  Critique: per-byte pricing surprises high-fanout workloads (vs per-message)");
    println!("           exactly-once is opt-in + 1.5x latency vs at-least-once");
    println!("           Pub/Sub Lite zonal = no automatic multi-region failover");
    println!("           push subscriptions retry behavior tricky (HTTP 5xx + 4xx handling)");
    println!("           pull subscriber complexity (StreamingPull, flow control, ack deadline mgmt)");
    println!("           GCP lock-in — migrating to Kafka requires significant rework");
    println!("           no message-level priority");
    println!("           7-31 day retention max (use Pub/Sub Lite for longer log-style)");
    println!("           Kafka API on Pub/Sub Lite is partial (not 100% Kafka surface)");
    println!("  Differentiator: Google Cloud planet-scale pub/sub (March 2015 externalization of internal Google system used for Gmail + Search + Ads + Analytics for years prior) + global topics by default (single endpoint, automatic multi-region failover, no region selection needed) + at-least-once + exactly-once delivery modes (exactly-once opt-in 2021+) + Pub/Sub Lite (10x cheaper zonal partitioned variant with Kafka API compatibility 2022+) + push subscriptions (HTTP webhook with OIDC auth, Cloud Run/Functions/App Engine integration) + pull + StreamingPull subscriptions + BigQuery direct subscriptions (2022, no Dataflow needed) + Cloud Storage subscriptions (2023) + schema registry (Avro + Protobuf) + ordering keys + message filtering + dead-letter topics + snapshots + seek for replay + 7-31 day retention + Eventarc event routing + Dataflow canonical processor + Cloud Functions trigger + Spotify/PayPal/Snap/Niantic-proven + 'planet-scale, multi-trillion messages/day internally at Google' + $40/TiB pricing + Borg + Spanner + Colossus backbone — the planet-scale managed messaging service backed by the same infrastructure that runs Gmail and Google Search, the default pub/sub for GCP-native architectures");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pubsub".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pubsub(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pubsub};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pubsub"), "pubsub");
        assert_eq!(basename(r"C:\bin\pubsub.exe"), "pubsub.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pubsub.exe"), "pubsub");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pubsub(&["--help".to_string()], "pubsub"), 0);
        assert_eq!(run_pubsub(&["-h".to_string()], "pubsub"), 0);
        let _ = run_pubsub(&["--version".to_string()], "pubsub");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pubsub(&[], "pubsub");
    }
}
