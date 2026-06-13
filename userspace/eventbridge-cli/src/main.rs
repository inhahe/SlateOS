#![deny(clippy::all)]

//! eventbridge-cli — SlateOS AWS EventBridge (serverless event bus, formerly CloudWatch Events, 2019)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eventbridge [OPTIONS]");
        println!("AWS EventBridge (Slate OS) — serverless event bus (formerly CloudWatch Events, 2019)");
        println!();
        println!("Options:");
        println!("  --bus                  Custom event bus or default bus");
        println!("  --rule                 Event pattern matching rule");
        println!("  --schedule             Scheduled events (cron + rate expressions)");
        println!("  --pipes                EventBridge Pipes (point-to-point with filter + transform)");
        println!("  --schemas              Schema Registry (event schema discovery)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("AWS EventBridge 2024 (Slate OS) — eventbridge CLI (aws-cli v2)"); return 0; }
    println!("AWS EventBridge 2024 (Slate OS) — Serverless Event Bus + Routing + Schedule + Pipes");
    println!("  Vendor: Amazon Web Services (Seattle, WA — NASDAQ: AMZN)");
    println!("  History (the CloudWatch Events evolution):");
    println!("    - Started as CloudWatch Events (2016) — cron + event routing inside CloudWatch");
    println!("    - Rebranded EventBridge Jul 2019 with major expansion");
    println!("    - SaaS event sources added 2019 (Zendesk, PagerDuty, Datadog, etc. push events)");
    println!("    - Schema Registry added Nov 2019");
    println!("    - Archive + Replay added 2020");
    println!("    - API Destinations (outbound webhooks) added 2021");
    println!("    - EventBridge Pipes added Dec 2022 (point-to-point routing)");
    println!("    - EventBridge Scheduler added Nov 2022 (better cron service)");
    println!("    - 'AWS's answer to event-driven serverless architectures'");
    println!("  Strategic position: 'the serverless event router for AWS event-driven systems':");
    println!("                    pitch: 'route events from anywhere to anywhere with pattern matching'");
    println!("                    target: event-driven serverless architectures, SaaS integrations, scheduled tasks");
    println!("                    primary competitor: GCP Eventarc, Azure Event Grid, Zapier (lo-code)");
    println!("                    secondary: Apache Kafka (with Connect), Workato, Tray.io");
    println!("                    EventBridge wedge: pattern matching + 200+ AWS service event sources + SaaS partners");
    println!("                    'one service for routing, scheduling, point-to-point, and outbound webhooks'");
    println!("                    pay-per-event pricing (no servers)");
    println!("                    Lambda + Step Functions + EventBridge = the canonical AWS event-driven trio");
    println!("  Pricing (per-event, multi-component):");
    println!("    Custom events: $1.00 per million events");
    println!("    AWS service events: free");
    println!("    SaaS partner events: $1.00 per million");
    println!("    Cross-account/cross-region: $1.00 per million");
    println!("    Schema Registry discovery: $0.10 per million events processed");
    println!("    EventBridge Pipes: $0.40 per million invocations");
    println!("    Scheduler: $1.00 per million invocations (better than legacy CW Events Schedule)");
    println!("    Archive: storage charges + replay charges");
    println!("    notably cheap per-event; cost adds up at high volume");
    println!("  Architecture (event-driven serverless):");
    println!("    - Event buses (logical event routers)");
    println!("    - Default bus: receives AWS service events");
    println!("    - Custom buses: for app events");
    println!("    - Partner buses: for SaaS integrations (per partner)");
    println!("    - Rules: event pattern + targets (up to 5 targets per rule)");
    println!("    - Targets: Lambda, Step Functions, SNS, SQS, Kinesis, ECS, API Destinations, etc.");
    println!("    - Pattern matching: JSON-based, supports prefix, suffix, anything-but, numeric ranges");
    println!("    - Schema Registry: auto-discover schemas from events");
    println!("    - DLQ for failed deliveries");
    println!("  Product portfolio:");
    println!("    1. Event Buses (the core router):");
    println!("       - Default bus: AWS service events arrive here automatically");
    println!("       - Custom buses: for your app's events");
    println!("       - Resource policies for cross-account event sharing");
    println!("       - PutEvents API for publishing custom events");
    println!("    2. Rules (the pattern matchers):");
    println!("       - JSON event patterns (rich matching: prefix, suffix, numeric, exists)");
    println!("       - Up to 5 targets per rule (Lambda, SQS, SNS, Kinesis, Step Functions, etc.)");
    println!("       - Input transformer (reshape events before delivery)");
    println!("       - Dead-letter queue per target");
    println!("       - Retry policy customization");
    println!("    3. AWS service event sources (200+ services):");
    println!("       - EC2 state changes, S3 object creation, Lambda invocations");
    println!("       - CodeBuild, CodePipeline status events");
    println!("       - GuardDuty findings, Config rule violations");
    println!("       - Health Dashboard events, EBS volume events");
    println!("       - Auto-published to default bus, no setup required");
    println!("    4. SaaS partner event sources (~40 partners):");
    println!("       - Zendesk, PagerDuty, Datadog, Auth0, MongoDB Atlas");
    println!("       - Stripe, Shopify, GitHub (some via API Destinations)");
    println!("       - Partner pushes events to your partner bus");
    println!("       - No polling, no webhooks to manage");
    println!("    5. API Destinations (outbound webhooks, 2021+):");
    println!("       - Route events to external HTTP endpoints");
    println!("       - Connection objects manage auth (API key, OAuth, Basic)");
    println!("       - Used for: notifying SaaS tools, custom webhook integration");
    println!("    6. EventBridge Pipes (point-to-point, Dec 2022+):");
    println!("       - Source (e.g. SQS, Kinesis, DynamoDB Streams)");
    println!("       - Optional filter (EventBridge pattern syntax)");
    println!("       - Optional enrichment (Lambda, Step Functions)");
    println!("       - Target (e.g. Lambda, SNS, EventBus)");
    println!("       - Replaces a lot of glue Lambda code");
    println!("    7. EventBridge Scheduler (Nov 2022+):");
    println!("       - One-time + recurring schedules");
    println!("       - Cron + rate expressions");
    println!("       - At-least-once or at-most-once delivery");
    println!("       - 1M+ schedules per account (vs CloudWatch Events 300)");
    println!("       - Flexible time windows + schedule groups");
    println!("       - Replaces legacy CloudWatch Events scheduled rules");
    println!("    8. Schema Registry:");
    println!("       - Auto-discover schemas from events flowing through buses");
    println!("       - OpenAPI + JSON Schema format");
    println!("       - Code bindings generation (Java, Python, TypeScript, Go)");
    println!("       - Schema versioning");
    println!("    9. Archive + Replay:");
    println!("       - Archive events (compressed storage)");
    println!("       - Replay events later (e.g. backfill new consumers, test event handlers)");
    println!("       - Critical for testing + reprocessing");
    println!("    10. Global endpoints (2022+):");
    println!("       - Multi-region event bus failover");
    println!("       - For active-active multi-region apps");
    println!("  The EventBridge vs SNS distinction:");
    println!("    - SNS: pub/sub fanout — one publish, N subscribers, no routing logic");
    println!("    - EventBridge: event router with pattern matching — one publish, route to multiple based on content");
    println!("    - SNS: cheap per-message ($0.50/M), simple");
    println!("    - EventBridge: 2x cost ($1.00/M), but smart routing + AWS service integration");
    println!("    - Common pattern: EventBridge → SNS → SQS (when you need both)");
    println!("    - 'Use EventBridge for routing, SNS for fanout'");
    println!("  The EventBridge Pipes story:");
    println!("    - Pre-2022: needed Lambda for SQS → enrich → SNS routing");
    println!("    - With Pipes: declarative source + filter + enrich + target, no Lambda glue");
    println!("    - Reduces code, increases reliability");
    println!("    - 'The serverless ETL for streams'");
    println!("  Integrations:");
    println!("    - aws-cli (Python-based, primary CLI)");
    println!("    - AWS SDKs: every language Amazon supports");
    println!("    - 200+ AWS service event sources (auto-published)");
    println!("    - ~40 SaaS partner sources (Zendesk, PagerDuty, Datadog, Stripe, etc.)");
    println!("    - Lambda + Step Functions + SQS + SNS + Kinesis as targets");
    println!("    - API Destinations for external webhooks");
    println!("    - CloudWatch Logs as target (auditing)");
    println!("    - DataDog, Splunk for observability");
    println!("    - CloudFormation, Terraform, AWS CDK for IaC");
    println!("  AWS CLI usage:");
    println!("    aws events list-event-buses");
    println!("    aws events create-event-bus --name my-bus");
    println!("    aws events put-rule --name s3-create-rule --event-pattern '{{\"source\":[\"aws.s3\"],\"detail-type\":[\"Object Created\"]}}'");
    println!("    aws events put-targets --rule s3-create-rule --targets 'Id=1,Arn=<lambda-arn>'");
    println!("    aws events put-events --entries 'Source=my.app,DetailType=order.placed,Detail=\"{{\\\"orderId\\\":\\\"123\\\"}}\"'");
    println!("    aws events list-rules --event-bus-name my-bus");
    println!("    # EventBridge Pipes:");
    println!("    aws pipes create-pipe --name my-pipe --source <sqs-arn> --target <lambda-arn> --role-arn <iam-role>");
    println!("    # EventBridge Scheduler:");
    println!("    aws scheduler create-schedule --name daily-job --schedule-expression 'cron(0 12 * * ? *)' --target '{{\"Arn\":\"<lambda-arn>\",\"RoleArn\":\"<role>\"}}' --flexible-time-window '{{\"Mode\":\"OFF\"}}'");
    println!("    # Schema Registry:");
    println!("    aws schemas list-discoverers");
    println!("    aws schemas start-discoverer --discoverer-id <id>");
    println!("  Customers (every AWS event-driven shop):");
    println!("    - Capital One (event-driven banking on AWS)");
    println!("    - Liberty Mutual, Travelers (insurance event processing)");
    println!("    - Coca-Cola, Pearson (large enterprise)");
    println!("    - Many SaaS partners: Datadog, PagerDuty, Auth0, Shopify, Stripe");
    println!("    - Used by: serverless apps, multi-account orgs, SaaS integrations");
    println!("    - 'If you do event-driven on AWS, EventBridge is the default'");
    println!("  Critique: per-event cost adds up at scale (2x SNS pricing)");
    println!("           debugging event patterns can be tedious (no test framework built-in)");
    println!("           Schema Registry discovery is per-event charged");
    println!("           pattern matching has subtle gotchas (case sensitivity, type matching)");
    println!("           target retry behavior + DLQ semantics confusing per target");
    println!("           Pipes limited to single source → single target (not arbitrary routing)");
    println!("           Scheduler replaces CW Events Schedules — migration friction");
    println!("           cross-region event delivery costs +1x events");
    println!("           AWS lock-in — equivalent on Azure (Event Grid) + GCP (Eventarc) differ");
    println!("  Differentiator: AWS serverless event bus + routing service (formerly CloudWatch Events 2016, rebranded EventBridge Jul 2019) + Event Buses (default + custom + partner) + Rules with JSON pattern matching (prefix/suffix/numeric/anything-but) + 200+ AWS service event sources (auto-published, no setup) + ~40 SaaS partner sources (Zendesk, PagerDuty, Datadog, Stripe, Shopify, Auth0, MongoDB Atlas) + API Destinations (outbound webhooks with OAuth + API key + Basic auth) + EventBridge Pipes (point-to-point with filter + enrich + target, replaces Lambda glue, Dec 2022+) + EventBridge Scheduler (1M+ schedules per account, replaces CW Events Scheduled, Nov 2022+) + Schema Registry (auto-discovery + code bindings) + Archive + Replay (compress + reprocess) + global endpoints (multi-region failover) + Input transformer (reshape events) + Dead-Letter Queues per target + cross-account event sharing + Lambda/Step Functions/SQS/SNS/Kinesis/ECS targets + Capital One/Liberty Mutual/Coca-Cola/Pearson-proven + $1/M custom events pricing — the AWS serverless event router for event-driven architectures, replacing tens of thousands of Lambda glue functions with declarative routing");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eventbridge".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/eventbridge"), "eventbridge");
        assert_eq!(basename(r"C:\bin\eventbridge.exe"), "eventbridge.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("eventbridge.exe"), "eventbridge");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_eb(&["--help".to_string()], "eventbridge"), 0);
        assert_eq!(run_eb(&["-h".to_string()], "eventbridge"), 0);
        let _ = run_eb(&["--version".to_string()], "eventbridge");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_eb(&[], "eventbridge");
    }
}
