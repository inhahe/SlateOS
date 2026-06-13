#![deny(clippy::all)]

//! sqs-cli — Slate OS AWS SQS (the original cloud queue — Amazon's first-ever AWS service, 2004)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sqs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sqs [OPTIONS]");
        println!("AWS SQS (Slate OS) — Simple Queue Service, Amazon's first-ever cloud service (2004)");
        println!();
        println!("Options:");
        println!("  --standard             Standard queue (at-least-once, best-effort ordering)");
        println!("  --fifo                 FIFO queue (exactly-once, strict ordering)");
        println!("  --dlq                  Dead-letter queue (failed messages)");
        println!("  --batching             Batched send/receive/delete");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AWS SQS 2024 (Slate OS) — sqs CLI (aws-cli v2)"); return 0; }
    println!("AWS SQS 2024 (Slate OS) — Simple Queue Service (the original cloud queue)");
    println!("  Vendor: Amazon Web Services (Seattle, WA — NASDAQ: AMZN)");
    println!("  History (the AWS origin story):");
    println!("    - Launched 9 Nov 2004 — Amazon's FIRST-EVER cloud service");
    println!("    - Predates S3 (2006), EC2 (2006), even the AWS brand itself");
    println!("    - Original use case: decouple Amazon's own internal services");
    println!("    - 2006: re-launched as part of new 'AWS' product family");
    println!("    - 2016: FIFO queues added (exactly-once + strict ordering)");
    println!("    - 2017: SSE-KMS encryption added");
    println!("    - 2022: SQS to Lambda short polling improvements");
    println!("    - 'If AWS has a founding service, it's SQS'");
    println!("  Strategic position: 'the simplest possible cloud queue at infinite scale':");
    println!("                    pitch: 'send/receive messages, no servers, infinite throughput'");
    println!("                    target: any decoupled async workload, microservice architectures");
    println!("                    primary competitor: RabbitMQ (self-managed), Azure Service Bus, GCP Pub/Sub");
    println!("                    secondary: ActiveMQ, NATS, internal queue libs");
    println!("                    SQS wedge: simplest API in cloud queuing (4 main ops)");
    println!("                    pay-per-million-requests pricing");
    println!("                    1M requests/month free tier forever");
    println!("                    'The queue that just works' — boring tech reliability");
    println!("  Pricing (extremely cheap):");
    println!("    First 1M requests/month: free (forever tier)");
    println!("    Standard queue requests: $0.40 per million");
    println!("    FIFO queue requests: $0.50 per million");
    println!("    Data transfer in: free");
    println!("    Data transfer out: standard AWS egress rates");
    println!("    notably the cheapest AWS service at scale (billing in millions of requests)");
    println!("    'For most workloads, SQS is functionally free'");
    println!("  Architecture (the proven AWS scale):");
    println!("    - Massively distributed (hundreds of thousands of servers globally)");
    println!("    - Each queue replicated across multiple AZs");
    println!("    - Standard queues: at-least-once, best-effort FIFO");
    println!("    - FIFO queues: exactly-once, strict ordering by message group");
    println!("    - Messages: up to 256KB (or up to 2GB via S3 reference + Extended Library)");
    println!("    - Retention: 1 minute to 14 days (default 4 days)");
    println!("    - Visibility timeout: 0s to 12h (default 30s)");
    println!("    - Long polling: up to 20 seconds (reduces empty-receive costs)");
    println!("    - Dead-letter queue for unprocessable messages");
    println!("  Product portfolio:");
    println!("    1. Standard queues (the default):");
    println!("       - Unlimited throughput (truly elastic)");
    println!("       - At-least-once delivery (occasional duplicates)");
    println!("       - Best-effort ordering (mostly FIFO, not guaranteed)");
    println!("       - The right choice for 95% of workloads");
    println!("       - Used by: web app job queues, microservice decoupling");
    println!("    2. FIFO queues (the strict version, 2016+):");
    println!("       - Exactly-once delivery");
    println!("       - Strict ordering within MessageGroupId");
    println!("       - Up to 3000 messages/second per group (with batching)");
    println!("       - Up to 300 messages/second without batching");
    println!("       - Used for: financial transactions, deduplication-critical workflows");
    println!("    3. Dead-Letter Queues (DLQ):");
    println!("       - Capture messages that fail processing N times");
    println!("       - Standard DLQ for standard, FIFO DLQ for FIFO");
    println!("       - Redrive policy: replay from DLQ back to source");
    println!("       - Critical for operational triage");
    println!("    4. SQS Extended Client Library:");
    println!("       - Send messages >256KB by storing payload in S3");
    println!("       - SQS holds the S3 reference");
    println!("       - Consumer auto-fetches from S3");
    println!("    5. Server-Side Encryption (SSE):");
    println!("       - SSE-SQS: AWS-managed keys");
    println!("       - SSE-KMS: customer-managed KMS keys");
    println!("       - Encryption at rest by default since 2022");
    println!("    6. Lambda triggers (event source mapping):");
    println!("       - Lambda polls SQS, invokes function per batch");
    println!("       - Automatic scaling (concurrency adjusts to queue depth)");
    println!("       - The canonical serverless pattern: SQS → Lambda → DynamoDB");
    println!("    7. CloudWatch metrics:");
    println!("       - ApproximateNumberOfMessagesVisible");
    println!("       - ApproximateAgeOfOldestMessage (lag indicator)");
    println!("       - NumberOfMessagesSent/Received/Deleted");
    println!("    8. VPC endpoints (PrivateLink):");
    println!("       - SQS accessible without public internet");
    println!("       - Required for many compliance regimes");
    println!("    9. Tags + cost allocation:");
    println!("       - Tag queues for billing breakdown");
    println!("       - Per-tag CloudWatch metrics");
    println!("    10. Cross-account access:");
    println!("       - Resource policies for cross-account producers/consumers");
    println!("       - Common pattern: central data team queue, multiple producers");
    println!("  The 'boring infrastructure' angle:");
    println!("    - SQS has been operating continuously since 2004 — 20+ years");
    println!("    - No major outages affecting SQS itself in recent memory");
    println!("    - 'The most boring AWS service' — and that's the highest compliment");
    println!("    - When SQS goes down, half the internet goes down (it's that fundamental)");
    println!("    - Documented at trillions of requests/day across all AWS customers");
    println!("  The polling model vs push:");
    println!("    - SQS is pull-based: consumers receive messages by calling ReceiveMessage");
    println!("    - Long polling: up to 20s wait — reduces empty-receive costs");
    println!("    - Lambda integration makes it feel push-based (Lambda polls for you)");
    println!("    - SNS+SQS combo: SNS pushes to SQS for true fanout");
    println!("  Integrations:");
    println!("    - aws-cli (Python-based, primary CLI)");
    println!("    - AWS SDKs: every language Amazon supports");
    println!("    - Lambda event source (canonical pattern)");
    println!("    - SNS → SQS fanout (very common)");
    println!("    - EventBridge → SQS (event routing)");
    println!("    - Step Functions integration (async tasks)");
    println!("    - Celery (Python), Sidekiq (Ruby), Bull (Node) all support SQS backend");
    println!("    - DataDog, New Relic monitoring");
    println!("  AWS CLI usage:");
    println!("    aws sqs create-queue --queue-name my-queue");
    println!("    aws sqs create-queue --queue-name my-fifo-queue.fifo --attributes FifoQueue=true,ContentBasedDeduplication=true");
    println!("    aws sqs send-message --queue-url https://sqs.us-east-1.amazonaws.com/123/my-queue --message-body 'hello'");
    println!("    aws sqs send-message-batch --queue-url <url> --entries '[{{\"Id\":\"1\",\"MessageBody\":\"a\"}}]'");
    println!("    aws sqs receive-message --queue-url <url> --max-number-of-messages 10 --wait-time-seconds 20");
    println!("    aws sqs delete-message --queue-url <url> --receipt-handle <rh>");
    println!("    aws sqs get-queue-attributes --queue-url <url> --attribute-names All");
    println!("    aws sqs set-queue-attributes --queue-url <url> --attributes RedrivePolicy='{{\"deadLetterTargetArn\":\"<arn>\",\"maxReceiveCount\":\"5\"}}'");
    println!("    aws sqs purge-queue --queue-url <url>                  # nuke all messages");
    println!("  Customers (essentially everyone on AWS):");
    println!("    - Netflix (early adopter — internal service decoupling)");
    println!("    - Airbnb, Slack, Lyft, Pinterest, Coinbase, Stripe");
    println!("    - Capital One, Goldman Sachs, NASDAQ (financial)");
    println!("    - NASA JPL (Mars rover telemetry buffering)");
    println!("    - 'Trillions of SQS messages per day across AWS' — official AWS figure");
    println!("    - Default queue for any AWS-native architecture");
    println!("  Critique: pull-based polling = higher latency than push systems");
    println!("           Standard queue duplicates = consumer must be idempotent");
    println!("           FIFO 3000 msg/s/group limit = bottleneck at high throughput");
    println!("           256KB message size limit (Extended Client = S3 hop)");
    println!("           14-day max retention = not a durable log (use Kafka/Kinesis for replay)");
    println!("           no built-in priority queues");
    println!("           no scheduled messages (use DelaySeconds up to 15 min, or Step Functions)");
    println!("           AWS lock-in — migrating to RabbitMQ requires significant changes");
    println!("           visibility timeout debugging is tricky (long-running consumers)");
    println!("  Differentiator: Amazon's FIRST-EVER cloud service (Nov 2004, predates S3 + EC2 + AWS brand) + 20+ years of continuous operation + trillions of requests/day across AWS + Standard queues (unlimited throughput, at-least-once, best-effort FIFO) + FIFO queues (exactly-once, strict ordering by MessageGroupId, 3000 msg/s/group) + Dead-Letter Queues + Lambda event source mapping (canonical serverless pattern) + SQS Extended Library (S3 for messages >256KB) + SSE-KMS encryption + VPC endpoints + 1M requests/month free forever tier + $0.40-$0.50/M requests beyond + cross-account resource policies + SNS fanout pattern + CloudWatch metrics (ApproximateAgeOfOldestMessage) + EventBridge integration + Netflix/Airbnb/Slack/Capital One/Goldman/NASDAQ-proven + simplest API in cloud queuing — the boring-by-design queue that powers most AWS-native architectures, the foundation under which the rest of AWS was built");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sqs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sqs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sqs"), "sqs");
        assert_eq!(basename(r"C:\bin\sqs.exe"), "sqs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sqs.exe"), "sqs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sqs(&["--help".to_string()], "sqs"), 0);
        assert_eq!(run_sqs(&["-h".to_string()], "sqs"), 0);
        let _ = run_sqs(&["--version".to_string()], "sqs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sqs(&[], "sqs");
    }
}
