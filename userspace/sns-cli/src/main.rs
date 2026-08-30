#![deny(clippy::all)]

//! sns-cli — Slate OS AWS SNS (Simple Notification Service — pub/sub + SMS + email + push, 2010)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sns(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sns [OPTIONS]");
        println!("AWS SNS (Slate OS) — Simple Notification Service (pub/sub + SMS + email + mobile push)");
        println!();
        println!("Options:");
        println!("  --topic                Standard or FIFO topic (pub/sub)");
        println!("  --sms                  SMS delivery (global)");
        println!("  --email                Email delivery (subscriber-confirmed)");
        println!("  --push                 Mobile push (APNs, FCM, ADM, Baidu)");
        println!("  --fanout               SNS → SQS fanout pattern");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AWS SNS 2024 (Slate OS) — sns CLI (aws-cli v2)"); return 0; }
    println!("AWS SNS 2024 (Slate OS) — Simple Notification Service (pub/sub + multi-protocol delivery)");
    println!("  Vendor: Amazon Web Services (Seattle, WA — NASDAQ: AMZN)");
    println!("  History:");
    println!("    - Launched Apr 2010 — fifth core AWS service");
    println!("    - Built to complement SQS: SQS is pull, SNS is push");
    println!("    - SMS delivery added 2013");
    println!("    - Mobile push (APNs/GCM/ADM) added 2013");
    println!("    - FIFO topics added 2020 (strict ordering)");
    println!("    - Message Filtering added 2018 (subscriber filter policies)");
    println!("    - Cross-region delivery + global SMS expansion");
    println!("    - 'SNS = push, SQS = pull' — the two pillars of AWS messaging");
    println!("  Strategic position: 'cloud pub/sub with multi-protocol delivery':");
    println!("                    pitch: 'fan out events to anywhere — SQS, Lambda, SMS, email, HTTP, mobile push'");
    println!("                    target: event-driven architectures, alerts, notifications, A2A integration");
    println!("                    primary competitor: GCP Pub/Sub, Azure Event Grid, Apache Kafka topics");
    println!("                    secondary: Twilio (SMS), SendGrid (email), Firebase (mobile push)");
    println!("                    SNS wedge: pub/sub + multi-protocol delivery in one service");
    println!("                    pay-per-million-publishes pricing");
    println!("                    'Send a message, AWS fans it out everywhere'");
    println!("  Pricing (cheap unless using SMS):");
    println!("    Standard topic publishes: $0.50 per million");
    println!("    FIFO topic publishes: $0.30 per million + $0.017/GB processed");
    println!("    HTTP/HTTPS deliveries: $0.60 per million");
    println!("    SQS deliveries: free");
    println!("    Lambda deliveries: free");
    println!("    Email deliveries: $2.00 per 100K emails");
    println!("    SMS deliveries: variable by country ($0.00645+/SMS US, up to $0.50+/SMS some intl)");
    println!("    Mobile push (APNs/FCM): $0.50 per million");
    println!("    SMS pricing is THE main SNS cost driver — verify rates per country");
    println!("  Architecture (durable pub/sub):");
    println!("    - Topic-based pub/sub (one publisher, many subscribers)");
    println!("    - Multi-AZ replication for durability");
    println!("    - Standard topics: at-least-once delivery, best-effort ordering");
    println!("    - FIFO topics: exactly-once, strict ordering by group");
    println!("    - Message Filtering: subscribers receive only matching messages");
    println!("    - Cross-region delivery (publish in region A, deliver to region B)");
    println!("    - Server-side encryption with KMS");
    println!("    - Dead-letter queues for failed deliveries (since 2019)");
    println!("    - Retry policy customizable per subscription");
    println!("  Product portfolio:");
    println!("    1. Standard topics (the workhorse):");
    println!("       - Unlimited throughput");
    println!("       - At-least-once delivery");
    println!("       - Best-effort ordering");
    println!("       - The default choice for fanout");
    println!("    2. FIFO topics (2020+):");
    println!("       - Exactly-once, strict ordering");
    println!("       - 300 messages/second/topic (up to 3000 with batching)");
    println!("       - Pairs with FIFO SQS subscribers");
    println!("       - For order-critical workflows");
    println!("    3. SMS delivery (the big differentiator):");
    println!("       - Send SMS to 200+ countries");
    println!("       - Transactional + promotional tiers");
    println!("       - Sender ID, origination number support");
    println!("       - Delivery receipts");
    println!("       - Cost varies wildly by country ($0.006-$0.50+)");
    println!("    4. Email delivery (subscriber-confirmed):");
    println!("       - Subscribers confirm via email click");
    println!("       - JSON + plain text formats");
    println!("       - Note: for transactional email, use SES (better deliverability)");
    println!("    5. Mobile push (APNs/FCM/ADM/Baidu):");
    println!("       - iOS (APNs)");
    println!("       - Android (FCM, formerly GCM)");
    println!("       - Kindle (ADM — Amazon Device Messaging)");
    println!("       - Baidu (China)");
    println!("       - Platform endpoints + device tokens");
    println!("    6. SQS subscription (the fanout pattern):");
    println!("       - SNS topic → multiple SQS queues");
    println!("       - Each queue gets its own copy of every message");
    println!("       - Canonical AWS fanout pattern (used by half of AWS)");
    println!("    7. Lambda subscription:");
    println!("       - Lambda function invoked per message");
    println!("       - Async + sync invocation modes");
    println!("       - Common with EventBridge for event routing");
    println!("    8. HTTP/HTTPS subscription:");
    println!("       - POST message to webhook URL");
    println!("       - Built-in retry with exponential backoff");
    println!("       - Subscription confirmation via HTTP");
    println!("    9. Message Filtering (2018+):");
    println!("       - JSON filter policy per subscription");
    println!("       - Subscriber receives only matching messages");
    println!("       - Reduces downstream processing cost");
    println!("       - Filter on MessageAttributes or message body (since 2023)");
    println!("    10. SNS Mobile (the rebrand attempt):");
    println!("       - Specifically for mobile push at scale");
    println!("       - Endpoint management for millions of devices");
    println!("       - Used by: mobile games, news apps, ride-hailing");
    println!("  The SNS → SQS fanout pattern (the canonical AWS architecture):");
    println!("    - Publisher sends one message to SNS topic");
    println!("    - SNS delivers a copy to each subscribed SQS queue");
    println!("    - Each consumer processes from its own queue independently");
    println!("    - Decouples publishers from consumer count + processing speed");
    println!("    - Used everywhere: order events → inventory + email + analytics queues");
    println!("    - Replaces Kafka topics in many AWS-native architectures");
    println!("  The SMS angle (SNS as Twilio competitor):");
    println!("    - SNS SMS competes with Twilio for transactional SMS");
    println!("    - Strengths: cheap, integrated with AWS auth + billing, global reach");
    println!("    - Weaknesses: no two-way SMS, no Programmable Messaging API features");
    println!("    - AWS End User Messaging (formerly Pinpoint SMS) added more features 2024");
    println!("    - For complex SMS workflows, use End User Messaging; for simple alerts, use SNS");
    println!("  Integrations:");
    println!("    - aws-cli (Python-based, primary CLI)");
    println!("    - AWS SDKs: every language Amazon supports");
    println!("    - SQS (the canonical fanout target)");
    println!("    - Lambda (event-driven processing)");
    println!("    - EventBridge (event routing layer)");
    println!("    - CloudWatch Alarms → SNS → SMS/email/Slack (alerts)");
    println!("    - Step Functions (workflow notifications)");
    println!("    - Kinesis Data Firehose subscription (Aug 2021+)");
    println!("    - HTTP/HTTPS for webhooks (Slack, Discord, custom apps)");
    println!("    - APNs/FCM/ADM for mobile push");
    println!("  AWS CLI usage:");
    println!("    aws sns create-topic --name my-topic");
    println!("    aws sns create-topic --name my-fifo-topic.fifo --attributes FifoTopic=true");
    println!("    aws sns subscribe --topic-arn <arn> --protocol sqs --notification-endpoint <sqs-arn>");
    println!("    aws sns subscribe --topic-arn <arn> --protocol email --notification-endpoint user@example.com");
    println!("    aws sns subscribe --topic-arn <arn> --protocol sms --notification-endpoint +15551234567");
    println!("    aws sns publish --topic-arn <arn> --message 'hello world'");
    println!("    aws sns publish --phone-number +15551234567 --message 'OTP: 123456'   # direct SMS");
    println!("    aws sns publish --topic-arn <arn> --message-attributes '{{\"priority\":{{\"DataType\":\"String\",\"StringValue\":\"high\"}}}}'");
    println!("    aws sns set-subscription-attributes --subscription-arn <arn> --attribute-name FilterPolicy --attribute-value '{{\"priority\":[\"high\"]}}'");
    println!("    aws sns list-topics");
    println!("    aws sns list-subscriptions-by-topic --topic-arn <arn>");
    println!("  Customers (every AWS customer using event-driven architecture):");
    println!("    - Netflix (early SNS user — internal event fanout)");
    println!("    - Airbnb (booking events fanout)");
    println!("    - Lyft, Pinterest, Coinbase, Stripe");
    println!("    - PagerDuty, Splunk (CloudWatch → SNS → 3rd-party alerting)");
    println!("    - Capital One, Goldman, NASDAQ (compliance + alerts)");
    println!("    - Mobile games + news apps (push notifications)");
    println!("    - 'Every CloudWatch alarm in production uses SNS' — common saying");
    println!("  Critique: SMS pricing per-country surprises (international SMS = expensive)");
    println!("           no native two-way SMS (use End User Messaging instead)");
    println!("           SNS email deliverability inferior to SES (use SES for marketing/transactional)");
    println!("           Standard topic duplicates = subscribers must be idempotent");
    println!("           FIFO 300 msg/s limit = bottleneck for high-throughput pub/sub");
    println!("           message size 256KB max (extended library for larger)");
    println!("           AWS lock-in — moving to Kafka requires architectural rework");
    println!("           no message retention (delivery only, no replay) — use Kinesis for that");
    println!("           filter policies have complexity limits");
    println!("  Differentiator: AWS pub/sub fanout service (April 2010, fifth core AWS service) + Standard topics (unlimited throughput, at-least-once) + FIFO topics (exactly-once, strict ordering, 300 msg/s) + multi-protocol delivery (SQS + Lambda + HTTP/HTTPS + email + SMS + mobile push APNs/FCM/ADM/Baidu) + the canonical SNS→SQS fanout pattern (one publish, N queue copies) + Message Filtering (subscribers see only matching messages via JSON filter policy) + Dead-Letter Queues for failed deliveries + cross-region delivery + KMS server-side encryption + 200+ countries SMS reach + transactional + promotional SMS tiers + Kinesis Data Firehose subscription + EventBridge integration + Netflix/Airbnb/Lyft/Pinterest/Coinbase-proven + 'every CloudWatch alarm uses SNS' + $0.50/M publishes pricing — the AWS push-based messaging service that complements SQS, the foundation under which event-driven AWS architectures are built");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sns".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sns(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sns};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sns"), "sns");
        assert_eq!(basename(r"C:\bin\sns.exe"), "sns.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sns.exe"), "sns");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sns(&["--help".to_string()], "sns"), 0);
        assert_eq!(run_sns(&["-h".to_string()], "sns"), 0);
        let _ = run_sns(&["--version".to_string()], "sns");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sns(&[], "sns");
    }
}
