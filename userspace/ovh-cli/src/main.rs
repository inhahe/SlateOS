#![deny(clippy::all)]

//! ovh-cli — OurOS OVHcloud (French sovereign cloud, bare metal, Roubaix, ENXTPA:OVH)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ovh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ovh [OPTIONS]");
        println!("OVHcloud (OurOS) — European sovereign cloud (bare metal, VPS, public cloud, ENXTPA:OVH)");
        println!();
        println!("Options:");
        println!("  --bare-metal           Bare Metal Servers (the iconic OVH offering)");
        println!("  --public-cloud         Public Cloud (OpenStack-based IaaS)");
        println!("  --vps                  VPS (low-cost virtual private servers)");
        println!("  --hosted-private-cloud Hosted Private Cloud (VMware-based)");
        println!("  --gpu                  GPU servers (AI/ML — NVIDIA H100/A100/L40S)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OVHcloud 2024 (OurOS) — ovhai CLI 1.x + manager API"); return 0; }
    println!("OVHcloud 2024 (OurOS) — Sovereign European Cloud + Bare Metal");
    println!("  Vendor: OVH Groupe SA (Roubaix, France — Euronext Paris: ENXTPA:OVH since 2021)");
    println!("  Founders: Octave Klaba (CEO/Chairman/major shareholder), 1999");
    println!("          Polish-French entrepreneur, founded OVH at age 21");
    println!("          'OVH' = 'On Vous Héberge' (we host you) — humble origin");
    println!("          family-controlled (Klaba family ~60% of shares post-IPO)");
    println!("          Octave: now Chairman (CEO transitions: Michel Paulin 2018-2023, Benjamin Revcolevschi 2023+)");
    println!("          Octave Klaba: still public face — tweets data center operations updates");
    println!("  Public market (ENXTPA:OVH):");
    println!("         IPO Oct 2021 on Euronext Paris at €18.50/share — raised €350M");
    println!("         peak ~€25 late 2021");
    println!("         settled €7-12 range 2023-2024 (deep drawdown)");
    println!("         FY2024 revenue: ~€990M (+11% YoY, fiscal year ends Aug)");
    println!("         Market cap: €1.5-2.5B range");
    println!("         Largest European cloud provider by revenue");
    println!("         March 2021: Strasbourg data center SBG2 fire destroyed ~12,000 servers");
    println!("  Strategic position: 'European sovereign cloud — predictable pricing + GDPR-native':");
    println!("                    pitch: 'Europe's largest cloud — data sovereignty, predictable pricing, no AWS lock-in'");
    println!("                    target: European enterprises + governments + price-sensitive workloads globally");
    println!("                    primary competitor: AWS, Azure, GCP (for European customers)");
    println!("                    secondary: Hetzner (Germany), Scaleway (France), IONOS (Germany)");
    println!("                    OVHcloud's wedge: own fiber + own data centers + own server design = lower costs");
    println!("                    GAIA-X founding member: European cloud sovereignty initiative");
    println!("                    challenge: AWS/Azure trojan-horsing 'sovereign' offerings in EU");
    println!("  Pricing (notably aggressive — owns the stack):");
    println!("    VPS: €3.50-€100/mo (low-cost virtual servers)");
    println!("    Bare metal: €40-€2,000+/mo (1-2 Xeon servers, full dedicated)");
    println!("    Public Cloud Instances: €0.003-€2/hr (OpenStack-based)");
    println!("    GPU servers: €0.79/hr (T4) up to €3.50+/hr (H100)");
    println!("    Hosted Private Cloud: from €600/mo (VMware-based)");
    println!("    typically 30-60% cheaper than AWS for equivalent bare metal");
    println!("    notably: bandwidth often included (no AWS-style egress fees)");
    println!("  Product portfolio:");
    println!("    1. Bare Metal Servers (the iconic offering):");
    println!("       - Dedicated physical servers (no virtualization)");
    println!("       - Rise (entry), Advance (workhorse), Scale (heavy), High Grade (premium), Game (gaming)");
    println!("       - Custom server designs (in-house engineering)");
    println!("       - 30+ data centers in 13+ countries");
    println!("       - Anti-DDoS included on all servers (Octave's pet project)");
    println!("    2. Public Cloud (OpenStack-based IaaS):");
    println!("       - Compute instances + S3-compatible object storage + block storage");
    println!("       - K8s service (OVHcloud Managed Kubernetes)");
    println!("       - Open source = no vendor lock-in (claim)");
    println!("    3. VPS (low-cost virtual servers):");
    println!("       - From €3.50/mo");
    println!("       - Ubuntu/Debian/Windows images");
    println!("       - 1-click apps (LAMP, WordPress, etc.)");
    println!("    4. Hosted Private Cloud (VMware):");
    println!("       - vSphere/vSAN/NSX as managed service");
    println!("       - Enterprise migration target");
    println!("       - Strategic concern: VMware/Broadcom pricing changes 2024");
    println!("    5. GPU Servers (AI/ML push):");
    println!("       - NVIDIA H100, A100, V100, L40S, T4, A10");
    println!("       - On-demand + reserved + bare metal options");
    println!("       - 'OVHcloud AI Endpoints' (managed model inference, 2024 launch)");
    println!("       - Partnership with Mistral AI (French — strategic alignment)");
    println!("    6. Hybrid Cloud + Connect:");
    println!("       - Direct Connect-style private links to AWS/Azure/GCP");
    println!("       - OVHcloud Connect: dedicated fiber to OVH data centers");
    println!("    7. Web hosting:");
    println!("       - Shared hosting (the historic 1999 product)");
    println!("       - Email + DNS + domains (registrar)");
    println!("       - 1.6M+ domains under management");
    println!("    8. Storage products:");
    println!("       - Object storage (S3-compatible)");
    println!("       - Cold archive (Glacier-equivalent)");
    println!("       - NAS (managed storage)");
    println!("    9. Anti-DDoS (notable since the beginning):");
    println!("       - Free unlimited DDoS protection on all servers");
    println!("       - 17+ Tbps mitigation capacity (2024)");
    println!("       - Octave personally famous for tweeting attack stats");
    println!("  The vertical integration story:");
    println!("    - OVH owns the entire stack: data centers, fiber, servers (own design)");
    println!("    - Manufactures its own servers in Croix, France");
    println!("    - 100K+ servers per year manufactured");
    println!("    - Backup power, cooling (water cooling famous), networking — all in-house");
    println!("    - 18+ Tbps own backbone");
    println!("    - This vertical integration = unmatched cost structure");
    println!("    - Trade-off: less elastic scaling, lower margins per server");
    println!("  The Strasbourg fire (March 10 2021):");
    println!("    - SBG2 data center caught fire — UPS-related cause");
    println!("    - 12,000 servers destroyed, ~3.6M websites offline");
    println!("    - Customers: Rust gaming server, Centre Pompidou, French government services");
    println!("    - Some customers' data unrecoverable (no off-site backups)");
    println!("    - Led to lawsuits + insurance crisis + reputation damage");
    println!("    - Sparked industry-wide review of cloud DR practices");
    println!("    - Octave's transparent handling (live tweet updates) earned grudging respect");
    println!("  Sovereign cloud + GAIA-X:");
    println!("    - GAIA-X founding member (European cloud federation initiative)");
    println!("    - GDPR-native (French + EU servers exclusively for EU customers)");
    println!("    - Schrems II compliance (CJEU ruling against US data transfers)");
    println!("    - 'No US-headquartered cloud' option for sensitive EU workloads");
    println!("    - Growing French + German + Italian government adoption");
    println!("  Integrations:");
    println!("    - OVHcloud API (REST, public)");
    println!("    - Terraform + Pulumi providers");
    println!("    - Ansible modules + Kubernetes integrations");
    println!("    - OpenStack APIs for Public Cloud");
    println!("    - S3-compatible object storage API");
    println!("    - Manager (web UI) + ovh CLI + ovh-api Python/Node/Go SDK");
    println!("    - Mistral AI partnership for sovereign LLM hosting");
    println!("  OVHcloud CLI usage:");
    println!("    ovh-eu --init                                            # configure auth");
    println!("    ovh-eu cloud project list                                # list public cloud projects");
    println!("    ovh-eu cloud project SERVICE_NAME instance create --name=my-vm --flavorId=b2-7 --imageId=ubuntu-22-04 --region=GRA9");
    println!("    ovh-eu dedicated server list                             # list bare metal servers");
    println!("    ovh-eu dedicated server SERVICE_NAME boot list           # list boot options");
    println!("    ovh-eu cloud project SERVICE_NAME kube create --name=my-cluster --region=GRA9");
    println!("    ovh-eu domain DOMAIN dnsRecord create --fieldType=A --subDomain=www --target=1.2.3.4");
    println!("    ovh-eu cloud project SERVICE_NAME storage create --containerName=my-bucket --region=GRA");
    println!("    ovh-eu license windows create --serverName=my-server --version=2022");
    println!("  Customers (European + global price-sensitive):");
    println!("    - 1.6M+ customers worldwide");
    println!("    - 50%+ revenue from France + Europe");
    println!("    - Major: French government (DINUM), Centre Pompidou, Rust (gaming), Mistral AI");
    println!("    - International: Tata, NHL.com (CDN edge), various game studios");
    println!("    - Strong in: SMB hosting, gaming servers, GDPR-conscious EU enterprises");
    println!("    - Weak in: large US enterprise, advanced managed services");
    println!("  Critique: SBG2 fire 2021 damaged brand + sparked DR questions");
    println!("           limited managed services vs AWS (fewer high-level products)");
    println!("           growth modest (~11%) vs hyperscalers (20-30%)");
    println!("           VMware/Broadcom price changes 2024 threaten Hosted Private Cloud margins");
    println!("           Hetzner more aggressive on raw price for bare metal");
    println!("           dependence on Klaba family control raises governance questions");
    println!("           AWS/Azure 'sovereign' offerings in EU eroding sovereignty pitch");
    println!("           IPO performance disappointing (-50% from IPO price)");
    println!("           lacks marquee AI offerings beyond Mistral partnership");
    println!("  Differentiator: largest European cloud provider (€990M revenue) + own data centers + own server manufacturing (100K+/yr) + own fiber backbone (18+ Tbps) + 30+ DCs in 13+ countries + bare metal heritage + Octave Klaba founder (since 1999, age 21) + GAIA-X sovereign cloud founder + GDPR-native + free unlimited DDoS protection on all servers + 30-60% cheaper than AWS bare metal + Mistral AI partnership + recovering from SBG2 fire 2021 — the European sovereign cloud option for GDPR-conscious EU enterprises and price-sensitive bare metal workloads worldwide");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ovh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ovh(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ovh};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ovh"), "ovh");
        assert_eq!(basename(r"C:\bin\ovh.exe"), "ovh.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ovh.exe"), "ovh");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ovh(&["--help".to_string()], "ovh"), 0);
        assert_eq!(run_ovh(&["-h".to_string()], "ovh"), 0);
        assert_eq!(run_ovh(&["--version".to_string()], "ovh"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ovh(&[], "ovh"), 0);
    }
}
