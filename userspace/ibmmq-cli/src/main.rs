#![deny(clippy::all)]

//! ibmmq-cli — OurOS IBM MQ (formerly MQSeries, the granddaddy of messaging, launched 1993)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ibmmq(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ibmmq [OPTIONS]");
        println!("IBM MQ (OurOS) — the original enterprise messaging system (formerly MQSeries, 1993+)");
        println!();
        println!("Options:");
        println!("  --queue-manager        Queue manager (the broker) management");
        println!("  --queue                Queue (point-to-point) management");
        println!("  --topic                Topic (pub/sub) management");
        println!("  --channel              Channel (broker-to-broker) management");
        println!("  --advanced             IBM MQ Advanced (replication, AMS, MFT)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IBM MQ 2024 (OurOS) — ibmmq CLI (runmqsc + dmpmqcfg)"); return 0; }
    println!("IBM MQ 2024 (OurOS) — Enterprise Messaging (formerly MQSeries, the original since 1993)");
    println!("  Vendor: IBM Corporation (Armonk, NY — NYSE: IBM)");
    println!("  History (the granddaddy of messaging):");
    println!("    - Announced Mar 1992, GA Dec 1993 as MQSeries v1");
    println!("    - First major commercial message broker product");
    println!("    - Originally for mainframe (MVS), then ported to AIX + Windows + Linux");
    println!("    - Renamed WebSphere MQ 2002 (under WebSphere brand)");
    println!("    - Renamed IBM MQ 2014 (dropped WebSphere)");
    println!("    - 'MQ' = Message Queuing — the term IBM popularized");
    println!("    - 30+ years of continuous evolution");
    println!("    - 'If your bank has a mainframe, it's running IBM MQ'");
    println!("    - JMS 2.0 + 3.0 compliance (Jakarta Messaging)");
    println!("    - IBM MQ on Cloud Pak for Integration (containers, 2018+)");
    println!("  Strategic position: 'enterprise messaging — assured delivery on every platform':");
    println!("                    pitch: 'guaranteed delivery + transactional + every-platform broker for big enterprises'");
    println!("                    target: Fortune 500, banks, insurance, government, mainframe shops");
    println!("                    primary competitor: TIBCO EMS, Solace, RabbitMQ (open-source), Kafka/Confluent");
    println!("                    secondary: Azure Service Bus, AWS MQ (managed IBM MQ rival), MuleSoft");
    println!("                    IBM MQ wedge: most platforms supported (z/OS + AIX + Windows + Linux + iSeries + macOS + Solaris + cloud)");
    println!("                    + transactional integrity (XA + 2PC support)");
    println!("                    + JMS 2.0 + 3.0 compliance");
    println!("                    + 30+ years of bank-grade reliability");
    println!("                    'AWS MQ literally hosts IBM MQ for you' — that's market dominance");
    println!("  Pricing (enterprise IBM-style):");
    println!("    IBM MQ Advanced for Developers: free (non-production)");
    println!("    IBM MQ Standard: per-PVU (Processor Value Unit) licensing");
    println!("    IBM MQ Advanced: adds AMS encryption + replication + MFT (more expensive)");
    println!("    IBM MQ on Cloud: $0.30-$1.50/hr per managed broker tier");
    println!("    AWS MQ for IBM MQ: AWS-billed managed");
    println!("    typically 6-7 figure annual deals at large banks");
    println!("    PVU-based pricing complex (charged per CPU core based on architecture)");
    println!("    'IBM enterprise pricing' = procurement-heavy");
    println!("  Architecture (the bank-grade engine):");
    println!("    - Queue Manager: the broker instance (a 'QMgr')");
    println!("    - Queues: persistent or non-persistent, local or remote");
    println!("    - Topics: hierarchical pub/sub (added later, queues came first)");
    println!("    - Channels: communication between QMgrs (sender + receiver, MCA processes)");
    println!("    - Cluster: multiple QMgrs that share namespace + workload balance");
    println!("    - Persistent messaging: spool to disk, transactional (XA/2PC)");
    println!("    - HA: Multi-Instance QMgr (shared filesystem) or RDQM (replicated, MQ Advanced)");
    println!("    - C-based broker, runs on every major platform");
    println!("    - JMS 2.0 + 3.0 compliant for Java apps");
    println!("    - MQI (Message Queue Interface) native API");
    println!("  Product portfolio:");
    println!("    1. IBM MQ Queue Manager (the broker):");
    println!("       - The central abstraction: a QMgr is one broker instance");
    println!("       - Multiple QMgrs federate via channels into clusters");
    println!("       - Per-platform binaries: z/OS, AIX, Windows, Linux, iSeries, macOS");
    println!("       - 'One broker, every platform' positioning");
    println!("    2. Queues (point-to-point):");
    println!("       - Local queues (on this QMgr)");
    println!("       - Remote queues (forwarded to another QMgr)");
    println!("       - Transmission queues (channel send buffers)");
    println!("       - Alias queues (indirection)");
    println!("       - Persistent + non-persistent + durable subscriptions");
    println!("       - 'Assured delivery' is the brand");
    println!("    3. Topics (pub/sub, added in MQ 7.0, 2008):");
    println!("       - Hierarchical topic strings (topic/sub/category)");
    println!("       - Wildcards in subscriptions");
    println!("       - Durable + non-durable subscriptions");
    println!("    4. Channels + Clusters:");
    println!("       - Channels: sender/receiver/server/requester/cluster-sender/cluster-receiver");
    println!("       - Clusters: workload-balanced across multiple QMgrs");
    println!("       - SSL/TLS-secured channels");
    println!("       - Bandwidth + heartbeat tuning");
    println!("    5. IBM MQ Advanced (the premium tier):");
    println!("       - Advanced Message Security (AMS): app-level message encryption + signing");
    println!("       - Replicated Data Queue Manager (RDQM): HA via synchronous replication");
    println!("       - Managed File Transfer (MFT): file transfer over MQ infrastructure");
    println!("       - High-Availability built-in (replaces 3rd-party HA software)");
    println!("    6. IBM MQ on Cloud Pak for Integration:");
    println!("       - Containerized MQ for Kubernetes + OpenShift");
    println!("       - Operator-based deployment");
    println!("       - Combined with App Connect, API Connect");
    println!("    7. IBM MQ Appliance:");
    println!("       - Hardware MQ broker (like Solace appliance)");
    println!("       - DataPower-derived hardware");
    println!("       - For air-gapped, ultra-secure, dedicated deployments");
    println!("    8. AWS MQ for IBM MQ (2022+):");
    println!("       - AWS-managed IBM MQ");
    println!("       - Pay AWS, get a managed IBM MQ broker");
    println!("       - Significant: AWS hosts an IBM product (rare collaboration)");
    println!("    9. Kafka Connect for IBM MQ:");
    println!("       - Bidirectional Kafka <-> MQ");
    println!("       - For modernization paths");
    println!("    10. JMS bindings (Jakarta Messaging):");
    println!("       - First-class JMS 2.0 + 3.0 support");
    println!("       - Spring JMS integration");
    println!("       - Java EE / Jakarta EE app server integration (WebSphere Liberty etc.)");
    println!("  The mainframe heritage:");
    println!("    - IBM MQ on z/OS is the gold standard mainframe messaging");
    println!("    - Banks: virtually every major bank has IBM MQ on mainframe");
    println!("    - 'Daily settlement runs on IBM MQ'");
    println!("    - SWIFT messaging often transported via IBM MQ underneath");
    println!("    - Insurance, government, retail mainframes — all MQ");
    println!("    - 'Where there's a mainframe, there's MQ'");
    println!("  The 'every platform' angle:");
    println!("    - Supports z/OS, AIX, Linux (x86 + Power + s390x + ARM64), Windows, iSeries (i5/OS), macOS, Solaris");
    println!("    - HP-UX still supported until recently");
    println!("    - No other broker spans this many platforms");
    println!("    - Critical for hybrid: mainframe ↔ AIX ↔ Linux ↔ cloud");
    println!("    - 'The one broker that talks to everything you own'");
    println!("  Integrations:");
    println!("    - runmqsc (MQSC CLI for QMgr admin)");
    println!("    - dmpmqcfg (dump configuration)");
    println!("    - mqsiservice (newer service CLI)");
    println!("    - MQ Explorer GUI (Eclipse-based)");
    println!("    - MQ Web Console (modern web UI)");
    println!("    - JMS API (Java EE / Jakarta EE)");
    println!("    - C/COBOL/PL/I MQI native API");
    println!("    - .NET API (XMS .NET)");
    println!("    - Go, Python, Node.js clients (newer)");
    println!("    - Spring JMS integration");
    println!("    - WebSphere Liberty / Open Liberty app server");
    println!("    - Kafka Connect for IBM MQ");
    println!("    - DataPower gateway");
    println!("    - IBM App Connect, IBM API Connect");
    println!("    - DataDog, Splunk, Dynatrace monitoring");
    println!("    - CICS + IMS (mainframe transaction monitors) tight integration");
    println!("  IBM MQ CLI usage:");
    println!("    # MQSC interactive console:");
    println!("    runmqsc QMGR1");
    println!("    > DEFINE QLOCAL(MY.QUEUE) DEFPSIST(YES) MAXMSGL(4194304)");
    println!("    > DEFINE TOPIC(MY.TOPIC) TOPICSTR('events/order')");
    println!("    > DEFINE CHANNEL(TO.QMGR2) CHLTYPE(SDR) CONNAME('host(1414)') XMITQ(QMGR2)");
    println!("    > DISPLAY QSTATUS(MY.QUEUE) ALL");
    println!("    > DISPLAY CHANNEL(*) STATUS");
    println!("    > END");
    println!("    # CLI tools:");
    println!("    crtmqm QMGR1                                             # create queue manager");
    println!("    strmqm QMGR1                                             # start queue manager");
    println!("    endmqm -i QMGR1                                          # stop (immediate)");
    println!("    dspmq                                                    # list QMgrs");
    println!("    dmpmqcfg -m QMGR1 -t all                                # dump full config");
    println!("    amqsput MY.QUEUE QMGR1                                  # sample put utility");
    println!("    amqsget MY.QUEUE QMGR1                                  # sample get utility");
    println!("  Customers (every Fortune 500 bank, basically):");
    println!("    - JPMorgan Chase, Citi, Bank of America, Wells Fargo, Goldman, Morgan Stanley");
    println!("    - Barclays, HSBC, RBS, Deutsche Bank, BNP Paribas (EU banks)");
    println!("    - SWIFT (interbank messaging — partially MQ-based)");
    println!("    - State Farm, Allstate, MetLife (insurance)");
    println!("    - Walmart, Target, Costco (retail mainframes)");
    println!("    - IRS, SSA, DoD (US government)");
    println!("    - Lufthansa, Delta (airlines)");
    println!("    - ~95% of Fortune 500 have IBM MQ somewhere");
    println!("    - 'If a transaction happens at a bank, it probably touched IBM MQ'");
    println!("  Critique: PVU-based licensing is procurement-hostile");
    println!("           legacy reputation among modern dev teams");
    println!("           MQSC syntax is dated (runmqsc CLI feels 1990s)");
    println!("           IBM Cloud + Cloud Pak adoption slower than AWS-native");
    println!("           losing greenfield workloads to Kafka + cloud-native brokers");
    println!("           expensive vs open-source (RabbitMQ, ActiveMQ, NATS)");
    println!("           mainframe-centric architecture concepts confusing for cloud-native devs");
    println!("           IBM acquisition activity churns product strategy");
    println!("           CICS + IMS dependencies for older patterns");
    println!("  Differentiator: 30+ years (MQSeries 1993 → WebSphere MQ 2002 → IBM MQ 2014) + the granddaddy of message queuing (IBM coined 'MQ' terminology) + every-platform broker (z/OS + AIX + Linux x86/Power/s390x/ARM64 + Windows + iSeries + macOS + Solaris) + Queue Manager + Queues + Topics + Channels + Clusters + persistent + transactional (XA + 2PC) + JMS 2.0 + 3.0 compliance + IBM MQ Advanced (AMS app-level encryption + RDQM replication + MFT managed file transfer) + IBM MQ Appliance (DataPower hardware) + AWS MQ for IBM MQ (Amazon hosts IBM MQ, 2022+) + Cloud Pak for Integration (OpenShift containerized) + Kafka Connect bidirectional bridge + mainframe gold standard (z/OS MQ on virtually every Fortune 500 mainframe) + JPMorgan/Citi/BofA/Wells/Goldman/Barclays/HSBC/SWIFT/IRS/SSA-proven + CICS + IMS tight integration + ~95% of Fortune 500 have IBM MQ somewhere + DataPower + IBM App Connect ecosystem + the architecture term 'MQ' itself was coined by this product — the bank-grade transactional messaging system that powers Fortune 500 financial services + insurance + government IT and underlies SWIFT interbank messaging, the most platform-spanning broker in existence");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ibmmq".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ibmmq(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ibmmq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ibmmq"), "ibmmq");
        assert_eq!(basename(r"C:\bin\ibmmq.exe"), "ibmmq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ibmmq.exe"), "ibmmq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ibmmq(&["--help".to_string()], "ibmmq"), 0);
        assert_eq!(run_ibmmq(&["-h".to_string()], "ibmmq"), 0);
        assert_eq!(run_ibmmq(&["--version".to_string()], "ibmmq"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ibmmq(&[], "ibmmq"), 0);
    }
}
