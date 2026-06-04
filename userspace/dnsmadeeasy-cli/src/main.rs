#![deny(clippy::all)]
//! dnsmadeeasy-cli — OurOS personality CLI for DNS Made Easy, the long-running enterprise DNS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("DNS Made Easy — enterprise managed DNS since 2002.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Steven Job, Reston VA origins, Tiggee LLC ownership");
    println!("    network     IP Anycast+ globally-distributed PoPs");
    println!("    features    Records, queries, monitoring, GTM");
    println!("    api         REST API and Terraform provider");
    println!("    uptime      The 100% uptime SLA boast (since 2010)");
    println!("    pricing     Tier-based volume pricing");
    println!("    digi        Digicert ownership era (2022 acquisition)");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Authoritative DNS for the customers who need it to be there.");
}

fn print_version() {
    println!("dnsmadeeasy-cli 0.1.0");
    println!("Tiggee LLC (dba DNS Made Easy) — Reston, Virginia. Founded 2002. Acquired by DigiCert 2022.");
}

fn cmd_about() {
    println!("DNS Made Easy");
    println!();
    println!("FOUNDED");
    println!("  2002 by Steven Job in Reston, Virginia. The original business");
    println!("  is operated by Tiggee LLC; DNS Made Easy is the consumer/");
    println!("  customer-facing brand. Steven built the company as a");
    println!("  bootstrapped, owner-led operation focused exclusively on");
    println!("  authoritative DNS for businesses — no registrar drama,");
    println!("  no hosting upsells.");
    println!();
    println!("PHILOSOPHY");
    println!("  Operate a worldwide IP Anycast network optimized for query");
    println!("  speed + uptime. Bill enterprises a flat annual rate for the");
    println!("  capacity they need. Maintain a small team that knows the");
    println!("  customer base personally.");
    println!();
    println!("HEADQUARTERS");
    println!("  Reston, Virginia (NoVA tech corridor). Operations + engineering");
    println!("  in the US + EU; small support and SRE org.");
    println!();
    println!("LANDMARK INCIDENT (May 2010)");
    println!("  Sustained 50Gbps DDoS attack rendered the DNS Made Easy");
    println!("  network temporarily unavailable to customers. The company's");
    println!("  response — rapid scaling of anycast capacity, public post-");
    println!("  mortem, hardware investment — became a case study in DNS-tier");
    println!("  DDoS mitigation. Since then DNSME has loudly publicized");
    println!("  the post-2010 capacity-expansion arc and 100% uptime claim.");
}

fn cmd_network() {
    println!("DNS Made Easy network architecture");
    println!();
    println!("IP ANYCAST+");
    println!("  ~25 global PoPs across North America, Europe, Asia, South");
    println!("  America, Australia, Africa. All PoPs announce the same");
    println!("  anycast IP prefixes; BGP delivers queries to the topologically");
    println!("  nearest available responder.");
    println!();
    println!("DUAL NAMESERVERS PER ZONE");
    println!("  Every zone gets 4 nameserver records by default (ns0-ns3),");
    println!("  each backed by independent anycast prefixes. A complete");
    println!("  prefix-level outage (rare but possible) leaves three other");
    println!("  prefixes responding.");
    println!();
    println!("RESILIENCE PROFILE");
    println!("  After May 2010, DNSME made substantial investment in DDoS");
    println!("  mitigation infrastructure, peering relationships, and");
    println!("  scrubbing. The company has been public about scaling to");
    println!("  multi-hundred-Gbps query volumes.");
    println!();
    println!("DNSSEC");
    println!("  Supported on all eligible TLDs. Customer can opt in per-zone;");
    println!("  DS-record submission to the registry is the customer's");
    println!("  responsibility (DNSME does not control the registrar leg).");
}

fn cmd_features() {
    println!("DNS Made Easy features");
    println!();
    println!("RECORD TYPES");
    println!("  A, AAAA, CNAME, MX, NS, PTR, SOA, SPF, TXT, SRV, CAA,");
    println!("  HTTPS, SVCB, NAPTR — full modern set.");
    println!();
    println!("TRAFFIC POLICIES (GTM)");
    println!("  Global Traffic Manager: rules-based routing on query attributes.");
    println!("  Options:");
    println!("    - Geo-based: country/region/state");
    println!("    - Resource-pool: pool of A records, fail-over on monitor");
    println!("    - Load-balancer: weighted, round-robin, ratio modes");
    println!("    - DNS Failover (System): automatic fail-over to backup IP");
    println!("    - Vanity DNS: white-label your own NS hostnames");
    println!();
    println!("SYSTEM MONITORING");
    println!("  HTTP/HTTPS/TCP/UDP/ICMP monitors with configurable intervals,");
    println!("  multi-region check, threshold for state-change. Monitors feed");
    println!("  Failover decisions; alerts via email + webhook + SMS.");
    println!();
    println!("SECONDARY DNS");
    println!("  Operate DNS Made Easy as a secondary (slave) to a customer's");
    println!("  on-prem BIND or NSD primary. AXFR/IXFR transfers; TSIG-signed");
    println!("  zone transfers supported.");
    println!();
    println!("QUERY LOG STREAMING");
    println!("  Real-time query log delivery to S3, GCS, or Azure Blob via");
    println!("  the customer's chosen credentials. Useful for fraud detection,");
    println!("  capacity planning, geo-traffic analysis.");
}

fn cmd_api() {
    println!("DNS Made Easy API");
    println!();
    println!("BASE URL");
    println!("  https://api.dnsmadeeasy.com/V2.0/");
    println!("  Sandbox: https://api.sandbox.dnsmadeeasy.com/V2.0/");
    println!();
    println!("AUTH");
    println!("  HMAC-style headers:");
    println!("    x-dnsme-apiKey: <key>");
    println!("    x-dnsme-requestDate: <UTC timestamp>");
    println!("    x-dnsme-hmac: HMAC-SHA1(secret, requestDate)");
    println!("  The HMAC ties each request to a specific timestamp, mitigating");
    println!("  replay attacks. ~10-min skew tolerance.");
    println!();
    println!("RESOURCES");
    println!("  /dns/managed                       list/create zones");
    println!("  /dns/managed/<id>/records          CRUD records in a zone");
    println!("  /dns/managed/<id>/records/<rid>    update/delete a record");
    println!("  /dns/secondary                     secondary zones");
    println!("  /dns/template                      record templates");
    println!("  /monitor/                          system monitoring");
    println!("  /reports/queries                   query statistics");
    println!();
    println!("TERRAFORM");
    println!("  community-maintained Terraform provider available; widely");
    println!("  used by ops teams in finance + healthcare regulated workloads.");
}

fn cmd_uptime() {
    println!("The 100% uptime SLA claim");
    println!();
    println!("THE CLAIM");
    println!("  Since 2010 (post-DDoS-incident hardening), DNS Made Easy has");
    println!("  marketed a '100% uptime SLA' for authoritative query response.");
    println!("  Customers receive credits for any measured outage of the");
    println!("  service's ability to respond to queries.");
    println!();
    println!("WHAT THE SLA COVERS");
    println!("  Inability to respond to DNS queries from a meaningful portion");
    println!("  of the Internet. Slow queries (high latency) and degraded");
    println!("  resolution against a specific resolver/network are excluded.");
    println!();
    println!("HOW IT'S MEASURED");
    println!("  DNS Made Easy operates its own external probing infrastructure");
    println!("  + cross-checks customer reports + third-party monitors.");
    println!("  Disputes are reviewed manually. Credits are pro-rated against");
    println!("  annual subscriptions; historically rare to be invoked.");
    println!();
    println!("REPUTATION");
    println!("  The company's marketing leans heavily on this 14+-year track");
    println!("  record. Engineering investment is overwhelmingly biased");
    println!("  toward reliability over feature breadth.");
}

fn cmd_pricing() {
    println!("DNS Made Easy pricing (as of 2024)");
    println!();
    println!("FLAT-RATE ANNUAL TIERS");
    println!("  Business      $30/yr     10 domains, 5M queries/mo, basic features");
    println!("  Business+     $125/yr    25 domains, 10M queries/mo, GTM, failover");
    println!("  Business++    $295/yr    50 domains, 25M queries/mo, advanced GTM");
    println!("  Corporate     $570/yr    250 domains, 75M queries/mo");
    println!("  Corporate+    $1,295/yr  500 domains, 250M queries/mo");
    println!("  Corporate++   $2,395/yr  1,000 domains, 750M queries/mo");
    println!("  Pinnacle      $5,500/yr+ custom volume + dedicated infrastructure");
    println!();
    println!("OVERAGE");
    println!("  Per-million additional queries charged after the included");
    println!("  monthly cap. Most customers stay well within tier; capacity");
    println!("  planning is part of the sales conversation.");
    println!();
    println!("VS. ROUTE 53");
    println!("  At small-business scale ($30-300/yr), DNS Made Easy is often");
    println!("  cheaper than Route 53 per-query. At enterprise scale (billions");
    println!("  of queries/mo), Route 53's per-million pricing wins. DNSME's");
    println!("  market sweet spot is mid-market: a marketing site doing 50-500M");
    println!("  queries/mo with strict uptime requirements.");
}

fn cmd_digi() {
    println!("The DigiCert acquisition");
    println!();
    println!("ANNOUNCED");
    println!("  May 2022. DigiCert acquired Tiggee LLC (parent of DNS Made");
    println!("  Easy + Constellix + ConstellixDNS). Terms undisclosed.");
    println!();
    println!("RATIONALE");
    println!("  DigiCert is the leading enterprise SSL/TLS certificate");
    println!("  authority. Acquiring DNS infrastructure rounded out DigiCert's");
    println!("  'trust services' portfolio — issuance + validation + DNS-based");
    println!("  challenges + DDoS-resistant resolution. From the DigiCert");
    println!("  perspective, owning the DNS layer enables tighter integration");
    println!("  for DNS CAA enforcement, DNSSEC orchestration, and DNS-01");
    println!("  ACME challenges for managed certificate workflows.");
    println!();
    println!("WHAT'S PRESERVED");
    println!("  The DNS Made Easy brand, support contacts, customer portal,");
    println!("  and pricing continue as before. Constellix (the sister brand");
    println!("  with more advanced GTM features and IPAM) is also retained.");
    println!();
    println!("WHAT'S CHANGING");
    println!("  Tighter integration with DigiCert's CertCentral. Account");
    println!("  consolidation for customers using both products. Some shared");
    println!("  engineering investment in DDoS infrastructure and observability.");
    println!();
    println!("INDEPENDENCE STATEMENT");
    println!("  DigiCert has publicly committed to operating DNS Made Easy as");
    println!("  a standalone product line, preserving the customer experience");
    println!("  and the long-running 100% uptime SLA boast.");
}

fn run_dme(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "network" => { cmd_network(); 0 }
        "features" => { cmd_features(); 0 }
        "api" => { cmd_api(); 0 }
        "uptime" => { cmd_uptime(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "digi" => { cmd_digi(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'. Try '{prog} help'.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dnsmadeeasy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_dme(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/dnsmadeeasy"), "dnsmadeeasy");
        assert_eq!(basename("C:\\Tools\\dnsmadeeasy.exe"), "dnsmadeeasy.exe");
        assert_eq!(basename("dnsmadeeasy"), "dnsmadeeasy");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("dnsmadeeasy.exe"), "dnsmadeeasy");
        assert_eq!(strip_ext("dnsmadeeasy"), "dnsmadeeasy");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_dme(&["help".to_string()], "dnsmadeeasy"), 0);
        let _ = run_dme(&[], "dnsmadeeasy");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_dme(&["nope".to_string()], "dnsmadeeasy"), 2);
    }
}
