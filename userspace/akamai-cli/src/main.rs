#![deny(clippy::all)]

//! akamai-cli — SlateOS Akamai (original CDN + edge security + cloud compute, Cambridge MA, NASDAQ:AKAM)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_akamai(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: akamai [OPTIONS]");
        println!("Akamai (SlateOS) — original CDN, security, edge compute, Linode cloud (NASDAQ:AKAM)");
        println!();
        println!("Options:");
        println!("  --cdn                  Akamai CDN (original 1998, 4000+ POPs)");
        println!("  --security             Security (Kona, Bot Manager, App & API Protector)");
        println!("  --connected-cloud      Connected Cloud (Linode-based IaaS, $900M acq 2022)");
        println!("  --edgeworkers          EdgeWorkers (JavaScript at edge, V8-isolate-based)");
        println!("  --guardicore           Guardicore (microsegmentation, $600M acq 2021)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Akamai 2024 (SlateOS) — akamai CLI 2.x"); return 0; }
    println!("Akamai 2024 (SlateOS) — Cloud Computing, Security, and Content Delivery");
    println!("  Vendor: Akamai Technologies, Inc. (Cambridge, MA — NASDAQ:AKAM since 1999)");
    println!("  Founders: Tom Leighton (MIT prof) + Daniel Lewin (MIT student, killed on AA Flight 11 9/11/2001) + Jonathan Seelig, 1998");
    println!("          MIT spin-off — Leighton's research on consistent hashing");
    println!("          Daniel Lewin: brilliant co-founder, age 31 — Israeli special forces, killed by hijackers stopping them on 9/11");
    println!("          Tom Leighton: still CEO 2024 (one of longest-running founder-CEOs in tech)");
    println!("          'Akamai' = Hawaiian for 'clever/smart'");
    println!("          The original CDN — coined many CDN concepts that became industry standard");
    println!("  Public market (NASDAQ:AKAM):");
    println!("         IPO Oct 1999 at $26/share — peaked at $345 during dot-com bubble");
    println!("         post-bubble crash to ~$1 in 2002 — nearly went bankrupt");
    println!("         steadily recovered + diversified");
    println!("         FY2024 revenue: ~$3.99B (+5% YoY)");
    println!("         Market cap: $14-18B range");
    println!("         Stable, profitable, mature CDN incumbent");
    println!("         Security + Cloud = growth engines (CDN is mature/declining)");
    println!("  Strategic position: 'world's most distributed compute + security + delivery platform':");
    println!("                    pitch: 'we secure and deliver more of the internet than anyone else'");
    println!("                    target: Fortune 500 enterprises + government + media + financial");
    println!("                    primary competitor: Cloudflare (modern), AWS CloudFront, Fastly");
    println!("                    secondary: Imperva, Zscaler (security), Linode-tier IaaS");
    println!("                    Akamai's wedge: 4000+ POPs (largest in the world by site count) + enterprise relationships");
    println!("                    diversifying CDN-mono to compute + security + cloud — 'Connected Cloud' branding");
    println!("                    Q4 2024: 67% revenue from security + cloud (vs 33% CDN) — successful pivot");
    println!("  Pricing (enterprise-only, custom):");
    println!("    CDN: custom enterprise contracts (typically $50K-$10M+/yr)");
    println!("    Security: Kona + Bot Manager + AAP custom pricing");
    println!("    Linode Connected Cloud: published SKU pricing ($5/mo Nanode → $1000+/mo dedicated)");
    println!("    EdgeWorkers: $0.50 per 1M invocations + compute");
    println!("    Guardicore: per-host microsegmentation licensing");
    println!("    no free tier outside Linode + dev accounts");
    println!("  Product portfolio:");
    println!("    1. CDN (the original — 1998 onwards):");
    println!("       - 4,000+ POPs in 130+ countries (largest CDN footprint by sites)");
    println!("       - Tiered architecture: parent/child caching hierarchies");
    println!("       - Ion (web), Adaptive Media Delivery (video), Download Delivery");
    println!("       - Property Manager: visual config + version control");
    println!("       - mPulse: real user monitoring (RUM)");
    println!("    2. Security (~50% revenue):");
    println!("       - Kona Site Defender: WAF + DDoS mitigation");
    println!("       - Bot Manager: bot detection + mitigation");
    println!("       - App & API Protector: unified WAF/API security");
    println!("       - Account Protector: credential stuffing prevention");
    println!("       - Page Integrity Manager: client-side security");
    println!("       - Prolexic: dedicated DDoS scrubbing (legacy enterprise)");
    println!("    3. Connected Cloud (Linode, $900M acq Mar 2022):");
    println!("       - IaaS: Linodes (VMs), object storage, K8s, managed databases");
    println!("       - Akamai's 4000+ POPs becoming compute locations");
    println!("       - Strategy: 'distributed cloud' — compute closer to users than AWS");
    println!("       - 11+ core regions + expanding to 25+ via Akamai POPs");
    println!("    4. EdgeWorkers (V8-isolate JS at edge):");
    println!("       - JavaScript serverless at Akamai POPs");
    println!("       - V8 isolates (like Cloudflare Workers)");
    println!("       - Slower to traction than Cloudflare/Fastly");
    println!("    5. Guardicore (microsegmentation, $600M acq Oct 2021):");
    println!("       - East-west traffic visibility + segmentation");
    println!("       - Zero Trust network segmentation");
    println!("       - Strong in financial services + healthcare");
    println!("    6. API Security (Noname Security $450M acq June 2024):");
    println!("       - API discovery + posture management");
    println!("       - Compete with: Salt Security, Wallarm, Traceable");
    println!("    7. Image & Video Manager:");
    println!("       - Real-time image/video optimization");
    println!("       - Adaptive streaming (HLS/DASH)");
    println!("    8. Cloud Wrapper:");
    println!("       - Reduce origin egress (AWS/Azure/GCP)");
    println!("       - Sit between origin + Akamai's CDN");
    println!("    9. Enterprise Application Access (ZTNA):");
    println!("       - Zero Trust Network Access");
    println!("       - Compete with: Zscaler ZPA, Cloudflare Access");
    println!("  Akamai network (the moat):");
    println!("    - 4,000+ POPs (largest by raw site count)");
    println!("    - 350,000+ edge servers globally");
    println!("    - Tiered hierarchical architecture (parent/child caches)");
    println!("    - Anycast routing + DNS-based load balancing");
    println!("    - Origins range: Tier 1 ISPs to last-mile networks");
    println!("    - Strategy: pushing compute INTO ISP networks (not just IXPs)");
    println!("  Linode acquisition (Mar 2022 $900M):");
    println!("    - Linode founded 2003 by Christopher Aker — early VPS provider, dev-favorite");
    println!("    - Akamai's bet to become 'distributed cloud' provider");
    println!("    - Linode Kubernetes Engine (LKE), Linode object storage");
    println!("    - Cheaper IaaS than AWS for many SMB/dev workloads");
    println!("    - 'Akamai Connected Cloud' branding 2023+");
    println!("    - Compete with: DigitalOcean, Vultr, Hetzner, OVH");
    println!("    - Strategic challenge: convince enterprise to use Akamai as primary cloud, not just CDN");
    println!("  Integrations:");
    println!("    - Akamai CLI (Go-based, plugin architecture)");
    println!("    - Terraform + Pulumi providers");
    println!("    - SIEM integrations: Splunk, QRadar, ArcSight, Sumo Logic");
    println!("    - Open standards: HTTP/3, QUIC, RFC8030 push, IPv6");
    println!("    - SDKs: Java, JS, Python, Go, .NET, PHP");
    println!("    - Linode + Akamai unified console (rolling out)");
    println!("  Akamai CLI usage:");
    println!("    akamai install property                                 # install plugin");
    println!("    akamai property-manager list-properties");
    println!("    akamai property-manager activate-property --propertyId=PRP_123 --network=staging");
    println!("    akamai purge invalidate --staging /assets/css/main.css");
    println!("    akamai edgeworkers list-ids");
    println!("    akamai edgeworkers register MY_WORKER");
    println!("    akamai linode-cli linodes create --type g6-nanode-1 --region us-east --image linode/ubuntu22.04");
    println!("    akamai linode-cli lke cluster-create --label my-cluster --region us-east --k8s_version 1.28");
    println!("    akamai botman list-categories");
    println!("    akamai jsonnet apply --config network-config.jsonnet     # IaC");
    println!("  Customers (Fortune 500 + government):");
    println!("    - 30%+ of Fortune 500");
    println!("    - Major: Apple (CDN), Microsoft (CDN), Adobe, Salesforce, IBM");
    println!("    - Government: DoD, US fed agencies, 200+ governments worldwide");
    println!("    - Media: ESPN, Disney+, MLB, NBC, Sony, Yahoo");
    println!("    - Financial: JPMorgan, Goldman, Citi, MasterCard, Visa");
    println!("    - 7,000+ enterprise customers");
    println!("    - 90% retention rate (enterprise customers very sticky)");
    println!("  Critique: Cloudflare's free tier compressing high-end CDN pricing pressure");
    println!("           EdgeWorkers slower to traction than Cloudflare Workers");
    println!("           Linode integration ongoing — separate consoles still");
    println!("           legacy enterprise UX (Property Manager + Luna control center are dated)");
    println!("           expensive: 5-10x cost of Cloudflare for equivalent");
    println!("           dev/SMB segment has largely abandoned Akamai for Cloudflare/Vercel");
    println!("           growth modest 5% — mature/declining CDN partially offset by security growth");
    println!("           Tom Leighton (CEO since 1998) succession question looms");
    println!("  Differentiator: 4,000+ POPs (largest CDN footprint by sites) + 350K+ edge servers + Fortune 500/government enterprise base (30%+ Fortune 500, DoD, financial) + Linode Connected Cloud ($900M acq distributed cloud play) + Guardicore microsegmentation ($600M) + Noname API security ($450M 2024) + Tom Leighton co-founder/CEO since 1998 (MIT consistent-hashing research) + Daniel Lewin legacy (killed 9/11) + $3.99B revenue with profitable margins — the original CDN that quietly delivers a huge fraction of the world's internet traffic and is pivoting from CDN-mono to security + distributed cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "akamai".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_akamai(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_akamai};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/akamai"), "akamai");
        assert_eq!(basename(r"C:\bin\akamai.exe"), "akamai.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("akamai.exe"), "akamai");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_akamai(&["--help".to_string()], "akamai"), 0);
        assert_eq!(run_akamai(&["-h".to_string()], "akamai"), 0);
        let _ = run_akamai(&["--version".to_string()], "akamai");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_akamai(&[], "akamai");
    }
}
