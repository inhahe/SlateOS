#![deny(clippy::all)]

//! solace-cli — OurOS Solace PubSub+ (Canadian event broker + event mesh, Kanata ON, founded 2001)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_solace(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: solace [OPTIONS]");
        println!("Solace (OurOS) — PubSub+ event broker + event mesh + event portal (Kanata, Ontario)");
        println!();
        println!("Options:");
        println!("  --event-broker         PubSub+ Event Broker (multi-protocol broker)");
        println!("  --event-mesh           PubSub+ Event Mesh (federated broker network)");
        println!("  --event-portal         PubSub+ Event Portal (governance + catalog)");
        println!("  --hardware             PubSub+ Appliance (purpose-built network hardware)");
        println!("  --cloud                PubSub+ Cloud (managed SaaS)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Solace PubSub+ 2024 (OurOS) — solace CLI 10.x"); return 0; }
    println!("Solace PubSub+ 2024 (OurOS) — Event Broker + Event Mesh + Event Portal");
    println!("  Vendor: Solace Corporation (Kanata, Ontario, Canada — private)");
    println!("  Founders: Larry Neumann + Craig Betts + Greg Hyatt + others, 2001");
    println!("          Originally founded as Solace Systems for financial-services hardware messaging");
    println!("          Spun off from Nortel R&D in Ottawa-area (Kanata is the Silicon Valley North hub)");
    println!("          Started with purpose-built hardware appliances for low-latency messaging");
    println!("          Pivoted to software + cloud (PubSub+ branding) ~2015");
    println!("          Renamed Solace Corporation (dropped 'Systems')");
    println!("          One of the few profitable, growing infrastructure companies in Canada");
    println!("  Funding:");
    println!("         Founded 2001, raised total ~$25M early venture (Bessemer, OAK)");
    println!("         Acquired Bridge Growth Partners investment 2016");
    println!("         Acquired by Hg Capital (UK private equity) Jan 2024 majority stake");
    println!("         Revenue private — believed >$100M ARR (financial services + logistics enterprise)");
    println!("         '23 years of building event-driven infrastructure'");
    println!("  Strategic position: 'event mesh — federated brokers across cloud + on-prem + edge':");
    println!("                    pitch: 'one event mesh spanning multi-cloud, on-prem, IoT, partner systems'");
    println!("                    target: large enterprises with hybrid cloud, banks, airlines, logistics");
    println!("                    primary competitor: Confluent (Kafka), IBM MQ, TIBCO EMS, Software AG webMethods");
    println!("                    secondary: AWS SQS+SNS, Azure Service Bus, RabbitMQ");
    println!("                    Solace's wedge: multi-protocol (AMQP + MQTT + JMS + Kafka + REST + WebSocket) in ONE broker");
    println!("                    + event mesh (federated brokers form a global event fabric)");
    println!("                    + Event Portal (governance, schema catalog)");
    println!("                    + Canadian engineering pedigree (Ottawa-area telecom heritage)");
    println!("                    'Better than Kafka for financial-services hybrid + edge'");
    println!("  Pricing (enterprise, opaque):");
    println!("    PubSub+ Standard Edition: free (community broker, single instance)");
    println!("    PubSub+ Enterprise: per-broker subscription (broker count + features)");
    println!("    PubSub+ Cloud: consumption-based (Service Class — Dev/Production/Enterprise)");
    println!("    PubSub+ Cloud Dev plan: free (limited connections + throughput)");
    println!("    PubSub+ Appliance: hardware capex + support contract");
    println!("    typically 6-figure annual contracts for large deployments");
    println!("    Hg's PE ownership = pricing remains enterprise-style");
    println!("  Architecture (the multi-protocol broker + mesh):");
    println!("    - Single broker supports: AMQP 1.0, MQTT 3.1.1/5.0, JMS 1.1/2.0, REST, WebSocket, Solace SMF");
    println!("    - Topic hierarchy with wildcard routing");
    println!("    - Topic-to-queue mapping (subscribe queues to topics)");
    println!("    - Direct messaging (low-latency, no persistence)");
    println!("    - Guaranteed messaging (persistence, retry, DLQ)");
    println!("    - Persistence: spool to disk + replication");
    println!("    - Event mesh: brokers federated via bridges, automatic topology");
    println!("    - C-based broker engine (high perf)");
    println!("    - Originally appliance (hardware), now mostly software + cloud");
    println!("  Product portfolio:");
    println!("    1. PubSub+ Event Broker (the core):");
    println!("       - Multi-protocol single broker (the big differentiator)");
    println!("       - AMQP + MQTT + JMS + REST + WebSocket + Kafka API + SMF native");
    println!("       - Topic-based pub/sub with wildcards");
    println!("       - Topic-to-queue subscription mapping");
    println!("       - Guaranteed messaging (persistence)");
    println!("       - Direct messaging (low-latency, no persistence)");
    println!("    2. PubSub+ Event Mesh (the federation, the unique value):");
    println!("       - Brokers federated via bridges across clouds + on-prem + edge");
    println!("       - Dynamic message routing across mesh");
    println!("       - Single logical event fabric");
    println!("       - Used for: multi-cloud, hybrid cloud, branch + HQ");
    println!("       - 'One topic published once reaches every broker in the mesh'");
    println!("    3. PubSub+ Event Portal (the governance, 2020+):");
    println!("       - Event catalog (browse all event types)");
    println!("       - Event modeling (design before implementation)");
    println!("       - Schema management (Avro + JSON Schema + Protobuf)");
    println!("       - Lineage + impact analysis");
    println!("       - 'Like a data catalog but for events'");
    println!("    4. PubSub+ Cloud (managed SaaS):");
    println!("       - Multi-cloud (AWS, Azure, GCP)");
    println!("       - Service Classes: Dev (free) / Production / Enterprise");
    println!("       - Multi-region clusters");
    println!("       - Auto-scaling broker capacity");
    println!("    5. PubSub+ Appliance (the heritage hardware):");
    println!("       - Purpose-built network hardware");
    println!("       - Used by: NYSE, NASDAQ, options exchanges for low-latency trading");
    println!("       - Sub-millisecond latency, deterministic");
    println!("       - 'The most extreme messaging hardware in production'");
    println!("    6. PubSub+ Insights (observability):");
    println!("       - Broker metrics dashboards");
    println!("       - Event flow visualization");
    println!("       - SLA monitoring");
    println!("    7. PubSub+ Mission Control (admin):");
    println!("       - Cluster management UI");
    println!("       - Capacity planning + scaling");
    println!("    8. PubSub+ Kafka Bridge:");
    println!("       - Bidirectional Kafka <-> Solace");
    println!("       - Migrate or coexist with Kafka workloads");
    println!("    9. PubSub+ for Microservices:");
    println!("       - Spring Cloud Stream binder");
    println!("       - Quarkus connector");
    println!("       - Standard JMS API");
    println!("    10. Replay Log:");
    println!("       - Optional persistent log for events");
    println!("       - Replay messages after restart");
    println!("       - Combines pub/sub + log semantics");
    println!("  The event mesh angle (vs Kafka):");
    println!("    - Kafka: monolithic cluster, federation via MirrorMaker is fragile + manual");
    println!("    - Solace: brokers federated automatically via bridges, single mesh");
    println!("    - One mesh can span: AWS us-east + AWS eu-west + on-prem DC + branch + AWS IoT");
    println!("    - One topic publish reaches every interested subscriber across the mesh");
    println!("    - Dynamic routing — no manual MirrorMaker setup");
    println!("    - Better for: financial-services hybrid, airline IT, logistics");
    println!("  The multi-protocol angle (vs single-protocol brokers):");
    println!("    - RabbitMQ: AMQP-focused + STOMP + MQTT plugin (limited multi-proto)");
    println!("    - Kafka: Kafka protocol only (gRPC, REST proxies are bolt-on)");
    println!("    - Solace: AMQP + MQTT + JMS + REST + WebSocket + Kafka API + SMF — all native");
    println!("    - Trade-off: more complex to operate, but one broker for diverse clients");
    println!("    - Use case: IoT (MQTT) + Java apps (JMS) + Kafka apps (Kafka API) on one broker");
    println!("  The financial services heritage:");
    println!("    - Solace appliances used by NYSE, NASDAQ, options exchanges");
    println!("    - Sub-millisecond deterministic latency");
    println!("    - 'When milliseconds matter, hardware messaging matters'");
    println!("    - That heritage informs Solace's enterprise reliability + performance");
    println!("  Integrations:");
    println!("    - solace CLI (Python-based + SolAdmin GUI)");
    println!("    - Client APIs: Java (JCSMP), JS/TS, Python, C, C#, Ruby, Node");
    println!("    - JMS for Java apps");
    println!("    - MQTT for IoT (Eclipse Mosquitto compat)");
    println!("    - AMQP 1.0 for cross-broker bridging");
    println!("    - REST messaging API");
    println!("    - Spring Cloud Stream binder");
    println!("    - Quarkus connector");
    println!("    - Kafka API for Kafka client compatibility");
    println!("    - DataDog, Splunk, Prometheus integrations");
    println!("    - Apache Camel routes");
    println!("    - Salesforce Event Bus connector");
    println!("  Solace CLI usage:");
    println!("    solace cloud login                                       # PubSub+ Cloud auth");
    println!("    solace cloud services list                               # list managed services");
    println!("    solace cloud services create --name my-broker --service-class production");
    println!("    # SolAdmin CLI (broker-side):");
    println!("    enable                                                   # enter privileged mode");
    println!("    configure                                                # configuration mode");
    println!("    message-vpn my-vpn                                       # select VPN");
    println!("    create queue my-queue                                    # create queue");
    println!("    subscription topic 'orders/*/created' to queue my-queue");
    println!("    # Solace Try-Me-Now (browser CLI):");
    println!("    # publish to: tutorial/topic");
    println!("    # subscribe to: tutorial/>");
    println!("  Customers (financial + airline + logistics):");
    println!("    - NYSE, NASDAQ, CBOE, ICE (financial exchanges — hardware appliances)");
    println!("    - JPMorgan Chase, Barclays, RBC (large banks)");
    println!("    - SAP, Microsoft Industry Cloud (partner)");
    println!("    - Lufthansa, Singapore Airlines, Korean Air (airline ops + departure control)");
    println!("    - FedEx, DHL, Maersk (logistics tracking)");
    println!("    - Caterpillar, John Deere (industrial IoT + telematics)");
    println!("    - 'Used by every major options exchange in the world'");
    println!("  Critique: opaque enterprise pricing (no public price list)");
    println!("           less mainstream than Kafka — smaller community");
    println!("           steeper learning curve (multi-protocol = more concepts)");
    println!("           dev community + ecosystem smaller than Kafka");
    println!("           Hg PE ownership (Jan 2024) = enterprise-only focus suspected");
    println!("           Event Portal less mature than data catalogs (Collibra, Atlan)");
    println!("           appliance heritage = some legacy concepts in newer cloud product");
    println!("           Canadian timezone for support (some intl customer friction)");
    println!("  Differentiator: 23+ years (founded 2001 in Kanata Ontario, Nortel telecom heritage) + multi-protocol single broker (AMQP + MQTT + JMS + REST + WebSocket + Kafka API + Solace SMF native in one engine, no protocol-specific clusters) + Event Mesh (brokers federated via bridges into single global event fabric spanning AWS + Azure + GCP + on-prem + edge + IoT, automatic dynamic routing, no MirrorMaker required) + Event Portal (event catalog + modeling + schema management + lineage + impact analysis, like data catalog for events) + PubSub+ Appliance (purpose-built hardware used by NYSE/NASDAQ/CBOE/ICE for sub-millisecond deterministic latency) + Replay Log + topic-to-queue mapping + wildcard topic routing + JCSMP native API + Spring Cloud Stream binder + Quarkus connector + Salesforce Event Bus connector + Lufthansa/Singapore Airlines/JPMorgan/FedEx/DHL/Maersk-proven + Hg Capital PE backed (Jan 2024) + Service Class managed cloud tier + 'every major options exchange in the world uses Solace' — the enterprise event mesh platform for hybrid + multi-cloud + edge financial-services / airline / logistics workloads, the only broker that natively speaks every major messaging protocol");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "solace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_solace(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_solace};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/solace"), "solace");
        assert_eq!(basename(r"C:\bin\solace.exe"), "solace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("solace.exe"), "solace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_solace(&["--help".to_string()], "solace"), 0);
        assert_eq!(run_solace(&["-h".to_string()], "solace"), 0);
        assert_eq!(run_solace(&["--version".to_string()], "solace"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_solace(&[], "solace"), 0);
    }
}
