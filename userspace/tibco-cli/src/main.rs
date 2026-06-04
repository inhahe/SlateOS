#![deny(clippy::all)]

//! tibco-cli — OurOS TIBCO Software (the integration giant, Rendezvous + EMS heritage, Palo Alto CA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tibco(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tibco [OPTIONS]");
        println!("TIBCO Software (OurOS) — the original ESB/integration giant + EMS messaging");
        println!();
        println!("Options:");
        println!("  --ems                  TIBCO Enterprise Message Service (JMS-compliant broker)");
        println!("  --rendezvous           TIBCO Rendezvous (RV — original pub/sub middleware, 1985+)");
        println!("  --businessworks        TIBCO BusinessWorks (ESB + integration platform)");
        println!("  --spotfire             TIBCO Spotfire (analytics + BI)");
        println!("  --cloud                TIBCO Cloud platform");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("TIBCO 2024 (OurOS) — tibco CLI (multi-product)"); return 0; }
    println!("TIBCO Software 2024 (OurOS) — Enterprise Integration + Messaging + Analytics (40 years)");
    println!("  Vendor: TIBCO Software, a Cloud Software Group company (Palo Alto, CA + Fort Lauderdale FL)");
    println!("  History (one of Silicon Valley's oldest integration companies):");
    println!("    - Founded 1985 as Teknekron Software Systems by Vivek Ranadive");
    println!("    - 'The Information Bus' = Teknekron + 'tib' invented");
    println!("    - Originally built for Reuters real-time financial data delivery");
    println!("    - Reuters became major customer + investor; spun out 1992 as TIBCO");
    println!("    - IPO July 1999 NASDAQ:TIBX");
    println!("    - Acquired private by Vista Equity Partners Dec 2014 for $4.3B");
    println!("    - Merged with Citrix systems Sep 2022 under 'Cloud Software Group' umbrella (Vista + Evergreen Coast Capital)");
    println!("    - 'Vivek Ranadive: also owns Sacramento Kings NBA team, prominent tech entrepreneur'");
    println!("  Strategic position: 'integration + messaging + analytics — the 40-year incumbent':");
    println!("                    pitch: 'enterprise integration platform — connect anything to anything'");
    println!("                    target: Fortune 500 enterprises, financial services, telecom, energy");
    println!("                    primary competitor: IBM (MQ + App Connect), MuleSoft (Salesforce), Software AG, Solace");
    println!("                    secondary: Kafka + Confluent, Boomi, SnapLogic");
    println!("                    TIBCO wedge: long history + ESB heritage + broad product portfolio");
    println!("                    + the original 'Information Bus' design");
    println!("                    'Decades-old EMS still runs major exchanges + banks'");
    println!("  Pricing (enterprise, opaque, expensive):");
    println!("    Per-CPU + per-broker + per-connection licensing — complex");
    println!("    EMS Standard Edition starting ~$50K+/year per broker (estimate)");
    println!("    BusinessWorks per-CPU enterprise licensing");
    println!("    Spotfire per-named-user or per-deployer");
    println!("    TIBCO Cloud subscription-based pricing");
    println!("    typically 6-7 figure annual deals for large deployments");
    println!("    Vista PE ownership = revenue + margin optimization, not growth-driven");
    println!("    'Pricing reflects boomer-enterprise ESB market — slow to modernize'");
    println!("  Architecture (decades of products glued together):");
    println!("    - EMS: C-based broker (JMS 2.0 compliant)");
    println!("    - Rendezvous: C-based pub/sub (UDP multicast + reliable)");
    println!("    - BusinessWorks: Java-based ESB (graphical process designer)");
    println!("    - Spotfire: Java + JavaScript-based analytics");
    println!("    - Cloud platform: containerized on AWS + Azure");
    println!("    - Most products: long-running JVM processes, persistent state");
    println!("  Product portfolio (the kitchen sink):");
    println!("    1. TIBCO EMS (Enterprise Message Service):");
    println!("       - JMS 1.1 + 2.0 compliant broker");
    println!("       - Persistent + non-persistent messaging");
    println!("       - Topics + Queues + Bridges");
    println!("       - Failover (HA pair with shared storage)");
    println!("       - Used by: banks, exchanges, telcos for JMS-heavy Java apps");
    println!("       - 'The boring JMS broker that just works'");
    println!("    2. TIBCO Rendezvous (RV) — the original (1992+):");
    println!("       - UDP multicast pub/sub for low-latency");
    println!("       - Subject-based hierarchical routing");
    println!("       - Reliable Multicast (RVRD daemon)");
    println!("       - Used by: trading floors (sub-millisecond market data)");
    println!("       - Decades-old, still in production at major banks");
    println!("       - Predates JMS, AMQP, Kafka, everything modern");
    println!("       - 'The original Information Bus'");
    println!("    3. TIBCO BusinessWorks (BW) — the ESB:");
    println!("       - Graphical integration designer");
    println!("       - 100+ connectors (SAP, Oracle, mainframe, REST, SOAP, file, FTP, JMS)");
    println!("       - BusinessWorks Container Edition (BWCE) for Kubernetes");
    println!("       - The bread-and-butter ESB product");
    println!("       - Competitor to IBM App Connect + MuleSoft Anypoint");
    println!("    4. TIBCO Spotfire — analytics + BI:");
    println!("       - Interactive data viz + dashboards");
    println!("       - In-memory analytics engine");
    println!("       - Competitor to Tableau, Power BI, Qlik");
    println!("       - Acquired Spotfire 2007 ($195M)");
    println!("       - Heavy in pharma, energy, manufacturing");
    println!("    5. TIBCO Cloud Integration (TCI):");
    println!("       - Cloud iPaaS (Integration Platform as a Service)");
    println!("       - Hosted BusinessWorks");
    println!("       - 100+ pre-built connectors");
    println!("       - Competitor to MuleSoft Cloud, Boomi, Workato");
    println!("    6. TIBCO Streaming (formerly LiveView / StreamBase):");
    println!("       - Complex Event Processing (CEP)");
    println!("       - Real-time analytics on streams");
    println!("       - Acquired StreamBase 2013");
    println!("       - Used by: trading firms, fraud detection");
    println!("    7. TIBCO Data Virtualization (DV):");
    println!("       - Virtual data layer over multiple sources");
    println!("       - Acquired Composite Software 2013");
    println!("       - Competitor to Denodo, Starburst");
    println!("    8. TIBCO MFT (Managed File Transfer):");
    println!("       - Secure file transfer with audit");
    println!("       - Replaces FTP for compliance-bound transfers");
    println!("    9. TIBCO Cloud Mashery:");
    println!("       - API management + gateway");
    println!("       - Acquired Mashery 2013 ($180M)");
    println!("       - Competitor to Apigee, Kong, Tyk");
    println!("    10. TIBCO Mainframe Service Tracker:");
    println!("       - Mainframe integration");
    println!("       - z/OS connectors");
    println!("       - For the 'we still run COBOL' enterprises");
    println!("  The Vivek Ranadive story:");
    println!("    - Founder + longtime CEO");
    println!("    - Mumbai-born, MIT + Harvard MBA");
    println!("    - Coined 'The Information Bus' phrase");
    println!("    - Wrote 'The Power of Now' (1999) + 'The Two-Second Advantage' (2011)");
    println!("    - Owner of Sacramento Kings NBA team (2013+)");
    println!("    - One of Silicon Valley's most prominent Indian-American CEOs");
    println!("    - Stepped down as TIBCO CEO after Vista acquisition (2014)");
    println!("  The Reuters heritage:");
    println!("    - TIBCO's first major customer + investor");
    println!("    - Reuters built financial data delivery on top of Information Bus");
    println!("    - 'Every Reuters terminal in the 90s used TIBCO under the hood'");
    println!("    - Established TIBCO as the FS integration standard");
    println!("    - Heritage still visible: TIBCO heaviest in financial services");
    println!("  Integrations:");
    println!("    - TIBCO CLI tools per product (no single unified CLI)");
    println!("    - JMS API for EMS clients");
    println!("    - Rendezvous C/Java/.NET APIs");
    println!("    - REST + SOAP services from BusinessWorks");
    println!("    - 100+ BusinessWorks connectors (SAP, Oracle, Salesforce, Workday, etc.)");
    println!("    - Kafka connectors (TIBCO Cloud Messaging)");
    println!("    - Spotfire iframe embed for analytics");
    println!("    - TIBCO Mashery for API management");
    println!("    - LDAP + Active Directory for auth");
    println!("    - DataDog, Splunk monitoring integrations");
    println!("  TIBCO CLI usage:");
    println!("    # EMS admin CLI (tibemsadmin):");
    println!("    tibemsadmin -server tcp://localhost:7222 -user admin -password admin");
    println!("    > show queue my-queue");
    println!("    > create queue my-queue");
    println!("    > show connections");
    println!("    > show server");
    println!("    # Rendezvous tools:");
    println!("    rvsend SERVICE TOPIC 'message'");
    println!("    rvlisten SERVICE TOPIC                                  # subscribe");
    println!("    # BusinessWorks Container Edition (BWCE):");
    println!("    # build BW app, deploy as Docker image");
    println!("    bwdesign                                                  # graphical designer");
    println!("    # TIBCO Cloud CLI (newer):");
    println!("    tibco login");
    println!("    tibco list-services");
    println!("    # Spotfire JavaScript API for embedded use");
    println!("  Customers (Fortune 500 enterprise):");
    println!("    - JPMorgan Chase, Citi, Goldman Sachs (banks)");
    println!("    - Royal Bank of Scotland, Deutsche Bank (Europe)");
    println!("    - AT&T, Verizon, Vodafone (telcos)");
    println!("    - Shell, BP, ExxonMobil (energy)");
    println!("    - Pfizer, Merck, Novartis (pharma — Spotfire)");
    println!("    - FedEx, UPS, DHL (logistics)");
    println!("    - NASDAQ, LSE (exchanges)");
    println!("    - ~10,000 enterprise customers globally");
    println!("    - Heavily concentrated in: financial services + telecom + energy");
    println!("  Critique: legacy products with limited cloud-native rewrite");
    println!("           Vista PE ownership = pricing optimization > product innovation");
    println!("           ESB market shrinking — cloud iPaaS + event streaming taking share");
    println!("           opaque pricing + complex licensing = procurement nightmares");
    println!("           Rendezvous + EMS = unfashionable next to Kafka/Confluent");
    println!("           Spotfire losing market share to Tableau + Power BI");
    println!("           merger with Citrix under Cloud Software Group = uncertain direction");
    println!("           customer perception of stagnation among modern dev teams");
    println!("           BWCE container modernization came late");
    println!("           support quality varied since Vista acquisition");
    println!("  Differentiator: 40-year integration giant (founded 1985 as Teknekron, spun out as TIBCO 1992, IPO 1999, Vista acquisition 2014 $4.3B, merged with Citrix Sep 2022 under Cloud Software Group) + EMS (JMS 1.1+2.0 compliant broker, persistent + non-persistent, used by banks/exchanges/telcos for decades) + Rendezvous (1992+ original 'Information Bus' UDP multicast pub/sub, subject-based hierarchical routing, sub-millisecond trading floor messaging, predates everything modern) + BusinessWorks ESB (graphical designer + 100+ connectors for SAP/Oracle/mainframe) + Spotfire (in-memory analytics + BI, acquired 2007, heavy in pharma/energy) + TIBCO Cloud Integration iPaaS + Streaming/LiveView CEP (acquired StreamBase 2013) + Data Virtualization (acquired Composite 2013) + Mashery API management (acquired 2013 $180M) + MFT secure file transfer + JPMorgan/Citi/Goldman/RBS/Deutsche/AT&T/Vodafone/Shell/BP/Pfizer/NASDAQ-proven + Vivek Ranadive founder (MIT + Harvard, Sacramento Kings owner) + Reuters heritage (built TIBCO's first wave of financial data delivery) + ~10,000 enterprise customers — the 40-year integration + messaging incumbent that quietly runs the backbone of Fortune 500 financial services + telecom + energy IT, the original information bus that predated Kafka by 25 years");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tibco".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tibco(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tibco};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tibco"), "tibco");
        assert_eq!(basename(r"C:\bin\tibco.exe"), "tibco.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tibco.exe"), "tibco");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tibco(&["--help".to_string()], "tibco"), 0);
        assert_eq!(run_tibco(&["-h".to_string()], "tibco"), 0);
        let _ = run_tibco(&["--version".to_string()], "tibco");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tibco(&[], "tibco");
    }
}
