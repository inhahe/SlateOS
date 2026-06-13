#![deny(clippy::all)]
//! route53-cli — Slate OS personality CLI for Amazon Route 53, AWS's managed DNS.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("Amazon Route 53 — AWS managed DNS, named for port 53.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Launch history and the AWS DNS thesis");
    println!("    records     Record types and Alias records");
    println!("    routing     Simple, Weighted, Latency, Geo, Failover, Multi-Value");
    println!("    health      Health checks driving DNS failover");
    println!("    private     Private hosted zones inside VPCs");
    println!("    pricing     Per-zone, per-query, per-health-check");
    println!("    resolver    Route 53 Resolver and hybrid DNS");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("100% SLA on responding to DNS queries. The first AWS service to offer it.");
}

fn print_version() {
    println!("route53-cli 0.1.0");
    println!("Amazon Web Services — Seattle, WA. Route 53 launched December 5, 2010.");
}

fn cmd_about() {
    println!("Amazon Route 53");
    println!();
    println!("LAUNCHED");
    println!("  December 5, 2010, as AWS's managed authoritative DNS service.");
    println!("  Built on a global network of edge locations (then ~50; now");
    println!("  100+) with anycast announcements so queries hit the nearest");
    println!("  responder. The '53' is the well-known UDP/TCP port for DNS.");
    println!();
    println!("THE THESIS");
    println!("  DNS is the first thing a user's browser does and the most");
    println!("  brittle piece of infrastructure most people deploy. Operating");
    println!("  a global, low-latency, DDoS-resilient anycast DNS network is");
    println!("  hard. AWS could amortize that fleet across every customer.");
    println!();
    println!("LANDMARK MOMENTS");
    println!("  - Dec 2010   Launch with hosted zones + ChangeBatch API.");
    println!("  - Aug 2013   Latency-based routing.");
    println!("  - Apr 2014   Health checks + DNS failover.");
    println!("  - Dec 2014   Domain registration (Route 53 Domains).");
    println!("  - Sep 2016   100% query-availability SLA.");
    println!("  - Nov 2017   Route 53 Resolver for hybrid DNS (VPC <-> on-prem).");
    println!("  - Dec 2018   Resolver Endpoints (inbound/outbound forwarders).");
    println!("  - Mar 2020   DNS Firewall (filter outbound from VPCs).");
}

fn cmd_records() {
    println!("Route 53 record types and Alias records");
    println!();
    println!("STANDARD TYPES");
    println!("  A, AAAA, CNAME, MX, NS, PTR, SOA, SPF, SRV, TXT, NAPTR,");
    println!("  CAA, DS, HTTPS, SVCB. Standard TTL semantics.");
    println!();
    println!("ALIAS RECORDS (Route 53-only magic)");
    println!("  Look like A/AAAA at the apex (example.com -> CloudFront)");
    println!("  but resolve internally to an AWS resource:");
    println!("    - CloudFront distribution");
    println!("    - Application/Network/Classic Elastic Load Balancer");
    println!("    - API Gateway endpoint");
    println!("    - S3 website-hosted bucket");
    println!("    - VPC endpoint");
    println!("    - Global Accelerator endpoint");
    println!("    - Elastic Beanstalk env, App Runner service");
    println!("    - Another Route 53 record in the same zone");
    println!("  Alias queries are free (vs. paid CNAME queries) and resolve");
    println!("  at the apex (CNAME is forbidden at apex per RFC 1034).");
    println!();
    println!("CHANGEBATCH MODEL");
    println!("  All record changes are submitted as a transactional batch");
    println!("  via ChangeResourceRecordSets. Changes are eventually");
    println!("  consistent across the anycast fleet, usually within 60 sec.");
}

fn cmd_routing() {
    println!("Route 53 routing policies");
    println!();
    println!("  Simple        One record set, one or more values, round-robin");
    println!("                shuffled at resolver.");
    println!();
    println!("  Weighted      Multiple records with the same name; each gets");
    println!("                a weight (0-255). Use for blue/green and canary.");
    println!();
    println!("  Latency       Multiple records in different AWS regions; query");
    println!("                returns the region with lowest measured latency");
    println!("                to the resolver. Background latency measurements");
    println!("                refresh continuously.");
    println!();
    println!("  Geolocation   Match on resolver country/continent/subdivision.");
    println!("                'US-CA users -> west endpoint; EU users -> Frankfurt.'");
    println!();
    println!("  Geoproximity  Resolves based on geographic distance from a");
    println!("                point you specify, with optional bias factor.");
    println!();
    println!("  Failover      Primary + Secondary; secondary returned when");
    println!("                primary's health check fails.");
    println!();
    println!("  Multivalue    Up to 8 healthy records returned, similar to");
    println!("                simple-with-healthchecks. Not a load balancer");
    println!("                substitute but useful for client-side LB.");
    println!();
    println!("  IP-based      (2023) Resolves based on resolver's source IP CIDR.");
}

fn cmd_health() {
    println!("Route 53 health checks");
    println!();
    println!("CHECK TYPES");
    println!("  - Endpoint health   HTTP/HTTPS/TCP probe of an IP or hostname,");
    println!("                      configurable port + path + matchers.");
    println!("  - CloudWatch alarm  Health = OK iff alarm state is OK.");
    println!("  - Calculated        Boolean AND/OR of child health checks.");
    println!();
    println!("PROBING");
    println!("  From ~15 health-checker locations on 5 continents. Each region");
    println!("  probes independently; majority vote determines health. Default");
    println!("  interval 30 sec (or 10 sec 'fast' for extra fee).");
    println!();
    println!("STRING MATCHING");
    println!("  HTTP/HTTPS checks can require the first 5,120 bytes of the");
    println!("  response body to contain a specific string. Useful for");
    println!("  application-level health (\"OK\\n\" or \"healthy\":true).");
    println!();
    println!("SNS NOTIFICATIONS");
    println!("  Health-check state changes can publish to SNS, page on-call");
    println!("  via OpsGenie/PagerDuty, or trigger Lambda for automated repair.");
}

fn cmd_private() {
    println!("Private hosted zones");
    println!();
    println!("WHAT THEY ARE");
    println!("  A hosted zone (domain space) visible only inside VPCs you");
    println!("  associate it with. Same record types and routing policies as");
    println!("  public zones — but the answers are served only to resolvers");
    println!("  inside the listed VPCs.");
    println!();
    println!("USE CASES");
    println!("  - Internal service discovery (svc-name.internal.example)");
    println!("  - Splitting public + internal views of the same domain");
    println!("    ('split-horizon DNS'): example.com publicly = your website,");
    println!("    example.com privately = internal admin endpoint.");
    println!("  - Cross-account name resolution via shared hosted zones.");
    println!();
    println!("CROSS-VPC + CROSS-REGION");
    println!("  Associating a private zone with multiple VPCs across regions");
    println!("  and accounts works through ChangeResourceRecordSets + the");
    println!("  AssociateVPCWithHostedZone API. Combined with Resolver Rules");
    println!("  for on-prem DNS forwarding, you get a hybrid namespace.");
}

fn cmd_pricing() {
    println!("Route 53 pricing (as of 2024)");
    println!();
    println!("HOSTED ZONES");
    println!("  $0.50 per hosted zone per month, first 25 zones.");
    println!("  $0.10 per hosted zone per month, zones 26-1000.");
    println!("  $0.05 per hosted zone per month, additional zones.");
    println!();
    println!("QUERIES");
    println!("  $0.40 per million standard queries (first 1B/month).");
    println!("  $0.20 per million standard queries (above 1B/month).");
    println!("  $0.60 per million latency-based queries (first 1B).");
    println!("  $0.70 per million geo / geoproximity / IP-based queries.");
    println!("  Alias queries to AWS resources: FREE.");
    println!();
    println!("HEALTH CHECKS");
    println!("  $0.50/mo per check (AWS endpoint).");
    println!("  $0.75/mo per check (non-AWS endpoint).");
    println!("  $1.00/mo extra for HTTPS / string-match / fast-interval.");
    println!();
    println!("DOMAIN REGISTRATION");
    println!("  ICANN pass-through pricing, no markup over wholesale. Most");
    println!("  .com renewals: $13/year. WHOIS privacy included.");
    println!();
    println!("RESOLVER");
    println!("  $0.125/hour per Resolver Endpoint ENI.");
    println!("  $0.40 per million queries processed by Resolver.");
}

fn cmd_resolver() {
    println!("Route 53 Resolver");
    println!();
    println!("VPC RESOLVER");
    println!("  Every VPC has an implicit DNS resolver at VPC-base+2 that");
    println!("  resolves AmazonProvidedDNS + your private hosted zones");
    println!("  + the public Internet.");
    println!();
    println!("RESOLVER ENDPOINTS");
    println!("  Inbound:  ENIs in your VPC that on-prem DNS forwarders can");
    println!("            send queries to over Direct Connect or VPN, so");
    println!("            on-prem can resolve your private AWS namespaces.");
    println!("  Outbound: ENIs your VPC uses to forward queries to on-prem");
    println!("            (or any external) resolvers based on Resolver Rules.");
    println!();
    println!("RESOLVER RULES");
    println!("  'For queries ending in corp.example, forward to 10.1.0.5');");
    println!("  shareable across accounts via Resource Access Manager.");
    println!();
    println!("DNS FIREWALL");
    println!("  Filter outbound DNS queries from VPCs against allow/block");
    println!("  lists. Use to detect malware C&C beacons, block typosquatting,");
    println!("  enforce 'no resolution outside our allowed domains' postures.");
    println!("  Managed lists from AWS for known-bad domains.");
    println!();
    println!("RESOLVER QUERY LOGGING");
    println!("  Stream every VPC DNS query to CloudWatch Logs, S3, or");
    println!("  Kinesis. Indispensable for incident response forensics.");
}

fn run_route53(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "records" => { cmd_records(); 0 }
        "routing" => { cmd_routing(); 0 }
        "health" => { cmd_health(); 0 }
        "private" => { cmd_private(); 0 }
        "pricing" => { cmd_pricing(); 0 }
        "resolver" => { cmd_resolver(); 0 }
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
        .unwrap_or_else(|| "route53".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_route53(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/route53"), "route53");
        assert_eq!(basename("C:\\Tools\\route53.exe"), "route53.exe");
        assert_eq!(basename("route53"), "route53");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("route53.exe"), "route53");
        assert_eq!(strip_ext("route53"), "route53");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_route53(&["help".to_string()], "route53"), 0);
        let _ = run_route53(&[], "route53");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_route53(&["nope".to_string()], "route53"), 2);
    }
}
