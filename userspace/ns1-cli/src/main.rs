#![deny(clippy::all)]
//! ns1-cli — Slate OS personality CLI for NS1, the intelligent DNS + traffic steering platform.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("NS1 — managed DNS + traffic steering with the Filter Chain engine.");
    println!();
    println!("USAGE:");
    println!("    {prog} <SUBCOMMAND> [ARGS...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about       Founders, NYC HQ, and the IBM acquisition");
    println!("    pulsar      Pulsar real user measurements + RUM data plane");
    println!("    filters     Filter Chains — composable traffic-steering logic");
    println!("    api         Data + Managed DNS API");
    println!("    private     NS1 Private DNS for hybrid + edge deployments");
    println!("    ibm         The August 2023 IBM acquisition");
    println!("    customers   LinkedIn, Salesforce, Spotify, Dropbox, others");
    println!("    help, -h    Show this help");
    println!("    version, -V Show version");
    println!();
    println!("Smart DNS — built by ex-Tumblr SREs, now an IBM company.");
}

fn print_version() {
    println!("ns1-cli 0.1.0");
    println!("NS1 (now IBM NS1 Connect) — New York City. Founded 2013. Acquired by IBM Aug 2023.");
}

fn cmd_about() {
    println!("NS1 — the intelligent DNS platform");
    println!();
    println!("FOUNDED");
    println!("  2013 in New York City by Kris Beevers, Jonathan Sullivan, and");
    println!("  Boaz Avital. Kris had been VP of Engineering at Voxel (acquired");
    println!("  by Internap), running large-scale infrastructure for ad tech");
    println!("  customers; Jonathan was an early Tumblr SRE; Boaz had been an");
    println!("  engineering lead at OnSite. They saw that legacy DNS providers");
    println!("  were limited to a few geographic-routing knobs and built NS1");
    println!("  to expose a programmable, data-driven traffic-steering engine.");
    println!();
    println!("HEADQUARTERS");
    println!("  New York City. Engineering + operations across NYC, London,");
    println!("  Singapore. ~150 employees pre-acquisition.");
    println!();
    println!("FUNDING (pre-acquisition)");
    println!("  Total raised: ~$103M across Seed/A/B/C/D rounds led by Sigma");
    println!("  Prime, Hyde Park Venture, Sapphire Ventures, Cisco Investments,");
    println!("  Energy Impact Partners. Last reported valuation ~$300M (2020).");
    println!();
    println!("ACQUIRED BY IBM");
    println!("  Announced August 9, 2023; closed shortly after. Terms not");
    println!("  disclosed. NS1 became 'IBM NS1 Connect' inside the IBM Cloud");
    println!("  + AI business unit. Kris Beevers remained as GM of NS1.");
}

fn cmd_pulsar() {
    println!("Pulsar — NS1's real user measurement layer");
    println!();
    println!("WHAT IT IS");
    println!("  A JavaScript snippet you embed on your website that measures,");
    println!("  for each visitor, latency + availability from their resolver");
    println!("  to your CDN endpoints. The measurements stream back to NS1's");
    println!("  data plane, where they aggregate into a per-network, per-PoP");
    println!("  performance map.");
    println!();
    println!("HOW IT FEEDS DNS");
    println!("  Filter Chains can use 'Up' or 'Best Performance' filters that");
    println!("  consult Pulsar data when answering queries. Result: at query");
    println!("  time, NS1 picks the CDN/PoP that's actually fastest for this");
    println!("  resolver's network — empirically measured, not just");
    println!("  geographically guessed.");
    println!();
    println!("MULTI-CDN");
    println!("  Combine Pulsar with weighted/priority filters to steer traffic");
    println!("  across multiple CDNs (Akamai+Cloudfront+Fastly) so each user");
    println!("  hits the fastest CDN at the moment they query. Failover when");
    println!("  one CDN's performance degrades is automatic and per-resolver.");
}

fn cmd_filters() {
    println!("Filter Chains — NS1's traffic steering primitive");
    println!();
    println!("THE MODEL");
    println!("  A record set is associated with a set of Answers. A Filter");
    println!("  Chain is an ordered list of filters that progressively narrow");
    println!("  the answer set; whatever Answers survive at the end are");
    println!("  returned to the resolver.");
    println!();
    println!("BUILT-IN FILTERS");
    println!("  - Up                          Drop answers that are not 'up' (per monitor).");
    println!("  - Geofence Country / Region   Drop answers outside resolver's locale.");
    println!("  - Geotarget Country / Region  Prefer answers tagged for resolver's locale.");
    println!("  - Network                     Match by ASN.");
    println!("  - Shuffle                     Randomize for round-robin.");
    println!("  - Weighted Shuffle            Random with per-answer weights.");
    println!("  - Sticky Region               Same resolver always gets same answer.");
    println!("  - Select First N              Cap remaining answers.");
    println!("  - Priority                    Order by 'priority' meta-field.");
    println!("  - Pulsar Best Performance     Use Pulsar latency data.");
    println!("  - Select Healthy from Group   Group-aware failover.");
    println!();
    println!("META FIELDS");
    println!("  Each Answer carries meta-fields (priority, weight, region,");
    println!("  asn, up, latitude, longitude, custom). Filters consult these");
    println!("  on every query. Meta-fields can be set per-answer or read");
    println!("  dynamically from data feeds.");
    println!();
    println!("DATA FEEDS");
    println!("  External data sources (Datadog, NS1 Monitoring, Pulsar,");
    println!("  custom webhook) update meta-fields in real time without");
    println!("  re-publishing the zone.");
}

fn cmd_api() {
    println!("NS1 APIs");
    println!();
    println!("MANAGED DNS API");
    println!("  Base:  https://api.nsone.net/v1/");
    println!("  Auth:  X-NSONE-Key: <api-key>");
    println!("  Resources: /zones, /zones/<zone>/<domain>/<type>, /monitoring/jobs,");
    println!("             /pulsar, /data/sources, /data/feeds, /redirect,");
    println!("             /networks, /alerts, /tsig, /views, /acl.");
    println!();
    println!("DDI MANAGER (formerly DDI/Constellix capabilities)");
    println!("  IP address management + DHCP + on-prem authoritative DNS,");
    println!("  centrally managed from the same console as managed DNS.");
    println!();
    println!("TERRAFORM");
    println!("  The ns1-labs/ns1 Terraform provider exposes zones, records,");
    println!("  data sources, feeds, monitors. The dominant deployment");
    println!("  pattern for NS1 customers — DNS as code, peer-reviewed in PR.");
    println!();
    println!("MONITORING");
    println!("  HTTP/HTTPS/TCP/DNS/PING monitors from ~20 locations globally.");
    println!("  Results feed the 'up' meta-field consumed by Filter Chains.");
}

fn cmd_private() {
    println!("NS1 Private DNS");
    println!();
    println!("WHAT IT IS");
    println!("  Same NS1 control plane and Filter Chain engine, but the");
    println!("  authoritative nameservers run inside your own environment");
    println!("  (Kubernetes, VM, bare metal). The control plane in NS1's");
    println!("  cloud pushes config; queries are served entirely on-prem or");
    println!("  at the edge — no external dependency on NS1 for resolution.");
    println!();
    println!("DEPLOYMENT");
    println!("  - VM:         OVA / raw image deploy in vSphere, KVM, AWS, GCP.");
    println!("  - Container:  ns1-private-dns Docker images for K8s clusters.");
    println!("  - Pop:        Edge appliances for ISPs + telecom.");
    println!();
    println!("USE CASES");
    println!("  - Telco core DNS that must survive Internet partitions.");
    println!("  - Air-gapped enterprise networks with internal namespaces.");
    println!("  - Hybrid cloud where private + public zones live side-by-side.");
    println!("  - Edge / 5G MEC deployments needing local DNS at the PoP.");
    println!();
    println!("LICENSING");
    println!("  Subscription per query volume + nameserver instance.");
    println!("  Combinable with NS1's managed DNS for hybrid hosting.");
}

fn cmd_ibm() {
    println!("The IBM acquisition");
    println!();
    println!("ANNOUNCED");
    println!("  August 9, 2023. Terms undisclosed. Closed in late 2023.");
    println!();
    println!("RATIONALE (per IBM's announcement)");
    println!("  NS1 fits IBM's networking + hybrid-cloud automation stack");
    println!("  alongside the 2022 acquisition of Turbonomic and the existing");
    println!("  Red Hat networking portfolio. Goal: a complete enterprise");
    println!("  network automation suite (DDI + DNS + traffic management).");
    println!();
    println!("BRANDING");
    println!("  Product rebranded as 'IBM NS1 Connect'. ns1.com URLs still");
    println!("  work as of 2024; new customer-facing portals live under");
    println!("  ibm.com/products/ns1-connect.");
    println!();
    println!("WHAT'S PRESERVED");
    println!("  - The API surface remains stable (existing Terraform configs work).");
    println!("  - The NS1 brand survives within the IBM portfolio.");
    println!("  - The engineering team stayed in NYC.");
    println!();
    println!("WHAT'S NEW UNDER IBM");
    println!("  Deeper integration with IBM Cloud Internet Services (CIS),");
    println!("  joint NS1 + IBM Hybrid Cloud Mesh offerings, and IBM-grade");
    println!("  procurement / compliance certifications (FedRAMP, IL5, IRAP).");
}

fn cmd_customers() {
    println!("Selected NS1 customers (now IBM NS1 Connect)");
    println!();
    println!("  LinkedIn       — multi-CDN steering for linkedin.com");
    println!("  Salesforce     — managed authoritative DNS, monitoring");
    println!("  Dropbox        — global DNS + Pulsar-driven CDN selection");
    println!("  Spotify        — selected zones and edge steering");
    println!("  Pinterest      — multi-region DNS failover");
    println!("  Square (Block) — payments DNS, regulated workload");
    println!("  Imgur          — media-heavy multi-CDN setup");
    println!("  TripAdvisor    — Pulsar-driven region routing");
    println!("  The Trade Desk — ad-tech low-latency DNS");
    println!("  Telecom Italia — private DNS for ISP infrastructure");
    println!();
    println!("Sweet spot: scale-out web companies running multi-CDN or");
    println!("multi-region active-active deployments; telecoms needing");
    println!("a programmable DNS plane; enterprises moving from BIND/Infoblox.");
}

fn run_ns1(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "help" | "-h" | "--help" => { print_help(prog); 0 }
        "version" | "-V" | "--version" => { print_version(); 0 }
        "about" => { cmd_about(); 0 }
        "pulsar" => { cmd_pulsar(); 0 }
        "filters" => { cmd_filters(); 0 }
        "api" => { cmd_api(); 0 }
        "private" => { cmd_private(); 0 }
        "ibm" => { cmd_ibm(); 0 }
        "customers" => { cmd_customers(); 0 }
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
        .unwrap_or_else(|| "ns1".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_ns1(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_dirs() {
        assert_eq!(basename("/usr/bin/ns1"), "ns1");
        assert_eq!(basename("C:\\Tools\\ns1.exe"), "ns1.exe");
        assert_eq!(basename("ns1"), "ns1");
    }

    #[test]
    fn strip_ext_drops_extension() {
        assert_eq!(strip_ext("ns1.exe"), "ns1");
        assert_eq!(strip_ext("ns1"), "ns1");
    }

    #[test]
    fn help_returns_zero() {
        assert_eq!(run_ns1(&["help".to_string()], "ns1"), 0);
        let _ = run_ns1(&[], "ns1");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_ns1(&["nope".to_string()], "ns1"), 2);
    }
}
