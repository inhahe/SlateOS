#![deny(clippy::all)]

//! servicebus-cli — SlateOS Azure Service Bus (Microsoft's enterprise broker, AMQP 1.0, Redmond WA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: servicebus [OPTIONS]");
        println!("Azure Service Bus (SlateOS) — Microsoft's enterprise messaging broker on Azure");
        println!();
        println!("Options:");
        println!("  --namespace            Service Bus namespace (the broker)");
        println!("  --queue                Queue (point-to-point messaging)");
        println!("  --topic                Topic (pub/sub with subscriptions)");
        println!("  --session              Session-aware ordered messaging");
        println!("  --premium              Premium tier (dedicated CU + JetStream-like)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Azure Service Bus 2024 (SlateOS) — servicebus CLI (az + Service Bus Explorer)"); return 0; }
    println!("Azure Service Bus 2024 (SlateOS) — Enterprise Messaging on Azure (AMQP 1.0-native)");
    println!("  Vendor: Microsoft Corporation (Redmond, WA — NASDAQ: MSFT)");
    println!("  History:");
    println!("    - Launched as 'Service Bus' on Windows Azure 2010");
    println!("    - Original product name: 'AppFabric Service Bus' (.NET Services era)");
    println!("    - Rebranded Azure Service Bus around 2012");
    println!("    - Premium tier (dedicated capacity) added 2015");
    println!("    - JMS 2.0 over AMQP added 2021");
    println!("    - One of Azure's foundational integration services");
    println!("    - Designed for: enterprise integration, not just web-scale streaming");
    println!("    - Contrasts with Event Hubs (Azure's Kafka-like, log-based)");
    println!("  Strategic position: 'enterprise messaging on Azure with full broker semantics':");
    println!("                    pitch: 'queues + topics + sessions + transactions + DLQ + scheduling — on Azure managed'");
    println!("                    target: enterprise .NET shops, Azure-native, JMS migrators");
    println!("                    primary competitor: AWS SQS+SNS, GCP Pub/Sub, IBM MQ");
    println!("                    secondary: RabbitMQ, ActiveMQ, Solace");
    println!("                    Service Bus wedge: AMQP 1.0 native + advanced features (sessions, transactions, scheduling, DLQ)");
    println!("                    + Premium tier for dedicated isolation");
    println!("                    + JMS 2.0 for Java migration paths");
    println!("                    + integration with all Azure services (Functions, Logic Apps, Event Grid)");
    println!("                    'For Azure shops what SQS+SNS+EventBridge are for AWS'");
    println!("  Pricing (multi-tier):");
    println!("    Basic tier: $0.05 per million operations (queues only, no topics, smaller features)");
    println!("    Standard tier: $9.81/month base + $0.80 per million operations (full features)");
    println!("    Premium tier: $0.667/hour per Messaging Unit (MU) — dedicated capacity");
    println!("        Premium Small: 1 MU = ~$486/month");
    println!("        Premium scales 1-16 MUs");
    println!("    transaction extras: $0.80/M for operations");
    println!("    Premium gets: predictable performance, geo-DR, JetStream-like features");
    println!("    notably Premium dedicates the broker (no noisy neighbor)");
    println!("  Architecture (the broker + namespace model):");
    println!("    - Namespace = the logical container (DNS endpoint)");
    println!("    - Each namespace hosts queues + topics");
    println!("    - Queues: point-to-point");
    println!("    - Topics + Subscriptions: pub/sub (subscription is a virtual queue with filters)");
    println!("    - AMQP 1.0 native protocol");
    println!("    - REST API for management + data plane");
    println!("    - Sessions: ordered FIFO grouped by SessionId");
    println!("    - Transactions: atomic send+receive across queues");
    println!("    - Auto-forwarding (topic → queue → another queue)");
    println!("    - Dead-letter sub-queues for poisoned messages");
    println!("    - Auto-delete on idle");
    println!("    - Duplicate detection windows");
    println!("    - Scheduled enqueue (deliver later)");
    println!("  Product portfolio (rich for an enterprise broker):");
    println!("    1. Queues (point-to-point):");
    println!("       - At-least-once or at-most-once delivery");
    println!("       - Optional sessions (FIFO ordering by SessionId)");
    println!("       - Duplicate detection (configurable window)");
    println!("       - Message TTL + auto-delete");
    println!("       - Dead-letter sub-queue (DLQ built-in, not separate resource)");
    println!("       - Peek, Receive, ReceiveAndDelete modes");
    println!("    2. Topics + Subscriptions (pub/sub):");
    println!("       - Topics publish; subscriptions consume");
    println!("       - Each subscription is a virtual queue with filters");
    println!("       - SQL filters (SELECT-like): UserProperty='premium' AND Region IN ('US','EU')");
    println!("       - Correlation filters (exact match)");
    println!("       - True/False (no filter)");
    println!("       - SQL actions to transform messages on subscription");
    println!("    3. Sessions (ordered groups):");
    println!("       - SessionId groups messages with strict FIFO ordering");
    println!("       - Session state storage (key-value per session)");
    println!("       - Lock per session per consumer");
    println!("       - Critical for: shopping cart, chat threads, ordered workflows");
    println!("    4. Transactions (atomic groups):");
    println!("       - Send + Complete + Defer atomically");
    println!("       - Cross-entity transactions within namespace");
    println!("       - 'Receive from queue A, send to queue B, all-or-nothing'");
    println!("       - Rare in cloud brokers (most don't support transactions)");
    println!("    5. Scheduled enqueue:");
    println!("       - Send a message, deliver it at time T");
    println!("       - Built-in (no Scheduler workaround)");
    println!("       - Used for: reminders, retries, delayed workflows");
    println!("    6. Duplicate detection:");
    println!("       - MessageId-based deduplication");
    println!("       - Configurable window (1 min - 7 days)");
    println!("       - 'Idempotency at the broker level'");
    println!("    7. Auto-forwarding:");
    println!("       - Forward messages from queue/subscription → another queue/topic");
    println!("       - Chain: subscription → queue → another topic → ...");
    println!("       - No code, no Function needed");
    println!("    8. Premium tier (dedicated):");
    println!("       - Dedicated Messaging Units (MUs)");
    println!("       - Predictable latency + throughput");
    println!("       - JMS 2.0 support (Java migration path)");
    println!("       - Geo-DR pairing (active/passive replication)");
    println!("       - Customer-managed key encryption");
    println!("    9. Dead-letter queues (DLQ):");
    println!("       - Built-in sub-queue per entity (queue or subscription)");
    println!("       - Receive max-delivery-count exceeded messages");
    println!("       - Manually move messages back (resubmit pattern)");
    println!("    10. JMS 2.0 over AMQP:");
    println!("       - Standard Java JMS API works against Service Bus");
    println!("       - Migration path from on-prem JMS (WebLogic, JBoss, ActiveMQ)");
    println!("       - Apache Qpid JMS client");
    println!("  The Premium tier 'dedicated cluster' angle:");
    println!("    - Standard tier = multi-tenant (noisy neighbor possible)");
    println!("    - Premium tier = dedicated MUs (predictable, isolated)");
    println!("    - 1 MU ~ 1000 msg/sec sustained (peak much higher)");
    println!("    - Scale 1-16 MUs in a namespace");
    println!("    - Geo-DR + VNet integration premium-only");
    println!("    - 'Enterprise-grade SLA' lives in Premium");
    println!("  Service Bus vs Event Hubs (Azure's two messaging products):");
    println!("    - Service Bus: broker-style (queues + topics + sessions + transactions + DLQ)");
    println!("    - Event Hubs: log-style (Kafka-compatible partitioned stream)");
    println!("    - Service Bus: 'each message processed once, decoupled microservices'");
    println!("    - Event Hubs: 'all consumers see all events, log replay, streaming analytics'");
    println!("    - Many Azure architectures use both (Event Hubs for telemetry + Service Bus for workflows)");
    println!("  Integrations:");
    println!("    - Azure CLI (az servicebus subcommands)");
    println!("    - Service Bus Explorer (community + Microsoft tool)");
    println!("    - Azure SDKs: .NET (canonical), Java, Python, JS/TS, Go");
    println!("    - JMS 2.0 over AMQP (Apache Qpid JMS)");
    println!("    - Azure Functions trigger (Service Bus binding)");
    println!("    - Azure Logic Apps (Service Bus connectors)");
    println!("    - Event Grid + Service Bus bridge");
    println!("    - Azure App Configuration + Key Vault for connection strings");
    println!("    - Spring Cloud Azure binder");
    println!("    - Quarkus connector");
    println!("    - Confluent Kafka Connect (community)");
    println!("    - DataDog, New Relic, Application Insights for monitoring");
    println!("  Azure CLI usage:");
    println!("    az servicebus namespace create --resource-group my-rg --name my-sb-ns --location eastus --sku Standard");
    println!("    az servicebus queue create --resource-group my-rg --namespace-name my-sb-ns --name my-queue");
    println!("    az servicebus queue create -g my-rg --namespace-name my-sb-ns --name my-q --requires-session true --enable-dead-lettering-on-message-expiration true");
    println!("    az servicebus topic create -g my-rg --namespace-name my-sb-ns --name my-topic");
    println!("    az servicebus topic subscription create -g my-rg --namespace-name my-sb-ns --topic-name my-topic --name my-sub");
    println!("    az servicebus topic subscription rule create -g my-rg --namespace-name my-sb-ns --topic-name my-topic --subscription-name my-sub --name rule1 --filter-sql-expression \"priority='high'\"");
    println!("    az servicebus namespace list-keys -g my-rg --name my-sb-ns");
    println!("    # Premium tier:");
    println!("    az servicebus namespace create -g my-rg -n my-prem-ns --sku Premium --capacity 1");
    println!("    # Service Bus Explorer (GUI):");
    println!("    # Connect with connection string, browse + send + peek + dead-letter");
    println!("  Customers (Microsoft + Azure-heavy enterprises):");
    println!("    - Microsoft itself (Xbox Live, Azure DevOps, Office 365 backend events)");
    println!("    - Mercedes-Benz, BMW, Bosch (automotive Azure shops)");
    println!("    - GE Healthcare, Bayer (pharma + medical)");
    println!("    - Walmart (massive Azure deployments)");
    println!("    - HSBC, Allianz (financial services with Azure)");
    println!("    - Bosch IoT (large industrial IoT)");
    println!("    - Many SAP-on-Azure customers (Service Bus for SAP integration events)");
    println!("    - 'Default broker for Azure-native .NET enterprise apps'");
    println!("  Critique: Standard tier multi-tenant has variable latency under noisy neighbors");
    println!("           Premium tier pricing high vs SQS/Pub/Sub (Premium MU $486/mo+)");
    println!("           JMS over AMQP works but quirks vs native broker JMS");
    println!("           AMQP 1.0 client interop varies by SDK quality");
    println!("           less popular than SQS in cloud-native architectures");
    println!("           Topic subscription model with separate sub queues = some confusion");
    println!("           filter SQL syntax learning curve");
    println!("           connection string + SAS token sprawl vs IAM-style auth");
    println!("           geo-DR Premium-only locks small customers out of DR");
    println!("           az CLI service bus commands inconsistent verbosity");
    println!("  Differentiator: Microsoft's enterprise broker for Azure (since 2010, originally 'AppFabric Service Bus' in .NET Services era) + AMQP 1.0 native protocol + Queues (point-to-point with DLQ sub-queue) + Topics + Subscriptions (pub/sub with SQL + correlation filters per subscription + SQL actions to transform on subscription) + Sessions (ordered FIFO by SessionId + per-session state + per-session lock) + Transactions (atomic send+receive across entities within namespace, rare in cloud brokers) + Scheduled enqueue (built-in delayed delivery) + Duplicate detection (MessageId-based dedup with configurable window) + Auto-forwarding (chain queues + topics without code) + Premium tier (dedicated Messaging Units, geo-DR active/passive replication, VNet integration, customer-managed key encryption, JMS 2.0 support) + JMS 2.0 over AMQP for Java migration paths + Azure Functions trigger + Logic Apps connector + Event Grid bridge + Mercedes/BMW/Bosch/GE Healthcare/Bayer/Walmart/HSBC/Allianz-proven + 'For Azure shops what SQS+SNS+EventBridge are for AWS' + Standard/Premium pricing tiers + Service Bus Explorer GUI tool + .NET SDK canonical + Microsoft's own Xbox Live + Azure DevOps + Office 365 backend uses it — the broker with the richest enterprise feature set in any major cloud (sessions + transactions + scheduling + dedup + auto-forward all built-in), the default messaging for Azure-native .NET enterprise applications");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "servicebus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/servicebus"), "servicebus");
        assert_eq!(basename(r"C:\bin\servicebus.exe"), "servicebus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("servicebus.exe"), "servicebus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sb(&["--help".to_string()], "servicebus"), 0);
        assert_eq!(run_sb(&["-h".to_string()], "servicebus"), 0);
        let _ = run_sb(&["--version".to_string()], "servicebus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sb(&[], "servicebus");
    }
}
