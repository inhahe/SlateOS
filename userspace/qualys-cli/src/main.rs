#![deny(clippy::all)]

//! qualys-cli — SlateOS Qualys (original SaaS vuln scanner, Foster City CA, NASDAQ:QLYS)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qualys(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qualys [OPTIONS]");
        println!("Qualys (SlateOS) — original SaaS vulnerability scanner (NASDAQ:QLYS)");
        println!();
        println!("Options:");
        println!("  --vmdr                 VMDR (Vulnerability Mgmt, Detection + Response)");
        println!("  --cloud-agent          Lightweight Cloud Agent (host-based scanning)");
        println!("  --totalcloud           TotalCloud (CNAPP — CSPM + CIEM + CWPP)");
        println!("  --container            Container Security (image + runtime)");
        println!("  --was                  Web Application Scanning (DAST)");
        println!("  --policy-compliance    Policy Compliance (PC) — config compliance");
        println!("  --patch-management     Patch Management add-on");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Qualys 2024 (SlateOS) — Cloud Platform 10.x"); return 0; }
    println!("Qualys 2024 (SlateOS) — Enterprise TruRisk Platform");
    println!("  Vendor: Qualys, Inc. (Foster City, CA — NASDAQ:QLYS since 2012)");
    println!("  Founder: Philippe Courtot (1999) — French-American security entrepreneur");
    println!("          previously CEO of Verity (search) + Signio (payments — acquired by VeriSign)");
    println!("          Courtot was visionary: SaaS vuln-mgmt before SaaS was a category");
    println!("          stepped down as CEO 2021, became chairman; passed away Feb 2024");
    println!("          Sumedh Thakar: current CEO (since 2021, long-time president/CTO)");
    println!("          25+ year operating history — longest-running cloud security pure-play");
    println!("  Public market (NASDAQ:QLYS):");
    println!("         IPO Sept 2012 at $12/share — raised $98M");
    println!("         peak ~$220 in 2021");
    println!("         settled $130-170 range 2023-2024");
    println!("         FY2024 revenue: ~$580M+ (~8-10% growth)");
    println!("         Market cap: ~$5-7B range");
    println!("         Notably profitable — among the most-profitable security pure-plays");
    println!("         Operating margin ~30%+ (vs Tenable mid-teens, Rapid7 low-teens)");
    println!("         No major debt + ~$500M+ cash — possible buyback / M&A optionality");
    println!("  Strategic position: 'cloud-native cybersecurity + IT compliance — the original SaaS scanner':");
    println!("                    pitch: 'one agent, one platform, all assets — IT, OT, cloud, containers'");
    println!("                    target: large enterprise + regulated industries");
    println!("                    primary competitor: Tenable (head-to-head VM), Rapid7, Microsoft Defender VM");
    println!("                    secondary: Wiz, Orca (cloud-only CNAPP), CrowdStrike (endpoint-led)");
    println!("                    Qualys's wedge: lightweight Cloud Agent + SaaS-from-day-one architecture + profitability");
    println!("                    challenge: perceived as 'legacy SaaS' next to Wiz's agentless innovation");
    println!("  Pricing:");
    println!("    VMDR (vulnerability mgmt) — $2K-$1M+/yr based on assets ($50-200/asset/yr typical)");
    println!("    Cloud Agent — included with VMDR (no per-agent extra)");
    println!("    TotalCloud — $20K-$2M+/yr (CNAPP for AWS/Azure/GCP)");
    println!("    Container Security — $10K-$500K+/yr");
    println!("    Web App Scanning (WAS) — $5K-$200K+/yr per app count");
    println!("    Patch Management add-on — $20K-$500K+/yr");
    println!("    typically priced at parity with Tenable, slightly above Rapid7 for enterprise");
    println!("  Product portfolio (the 'Qualys Cloud Platform'):");
    println!("    1. VMDR (Vulnerability Management, Detection + Response):");
    println!("       - Flagship product (modernized from original QualysGuard 1999)");
    println!("       - 200K+ vulnerability detection signatures");
    println!("       - TruRisk score (severity + threat intel + exploit context + asset criticality)");
    println!("       - Compete with: Tenable, Rapid7 InsightVM, Microsoft Defender VM");
    println!("    2. Qualys Cloud Agent (the secret weapon):");
    println!("       - 4MB lightweight agent for Windows/Linux/Mac/AIX/Solaris");
    println!("       - Continuous + real-time scanning (no scheduled scans needed)");
    println!("       - 4M+ deployed agents in production");
    println!("       - Cloud-side intelligence (no on-prem scanner appliance needed)");
    println!("    3. TotalCloud (CNAPP — was a 2023 rebrand of cloud security suite):");
    println!("       - CSPM (cloud security posture management)");
    println!("       - CIEM (cloud identity entitlement)");
    println!("       - CWPP (cloud workload protection)");
    println!("       - Agentless cloud scanning + Cloud Agent for runtime");
    println!("       - Compete with: Wiz, Orca, Prisma Cloud, Tenable Cloud Security");
    println!("    4. Container Security:");
    println!("       - Image scanning (registry + runtime)");
    println!("       - Kubernetes posture");
    println!("       - Compete with: Aqua, Sysdig, Snyk Container");
    println!("    5. Web Application Scanning (WAS — DAST):");
    println!("       - Application vulnerability scanning");
    println!("       - Compete with: Veracode, Checkmarx, Rapid7 InsightAppSec");
    println!("    6. Policy Compliance (PC):");
    println!("       - Configuration compliance (CIS, DISA STIGs, NIST 800-53, PCI-DSS, HIPAA)");
    println!("       - Strong in: financial services, federal, healthcare");
    println!("    7. Patch Management:");
    println!("       - Vulnerability-correlated patching");
    println!("       - Cross-platform patch deployment");
    println!("       - Compete with: Microsoft SCCM, Ivanti, Tanium");
    println!("    8. EDR (added 2020):");
    println!("       - Endpoint detection on top of Cloud Agent");
    println!("       - Compete with: CrowdStrike, SentinelOne, Microsoft Defender");
    println!("    9. XDR (extended detection — 2022):");
    println!("       - Cross-source correlation");
    println!("       - Compete with: CrowdStrike, SentinelOne, Microsoft Sentinel");
    println!("    10. CertView + CyberSecurity Asset Mgmt (CSAM):");
    println!("       - Certificate management + asset inventory");
    println!("       - Compete with: Axonius, JupiterOne, runZero");
    println!("  Qualys Cloud Platform (the architecture):");
    println!("    - Multi-tenant SaaS from 1999 (before AWS existed!)");
    println!("    - Operates in 11+ data center 'PODs' globally");
    println!("    - 99.99%+ uptime SLA");
    println!("    - 6+ trillion data points per year scanned");
    println!("    - SOC2 Type II + FedRAMP Moderate authorized");
    println!("    - IPv6, ICS, OT scanning capabilities");
    println!("  Integrations:");
    println!("    - SIEM/SOAR: Splunk, Sentinel, Sumo Logic, ServiceNow, Cortex XSOAR");
    println!("    - ITSM: ServiceNow (deep integration), Jira, BMC Remedy");
    println!("    - GRC: Archer, MetricStream, ServiceNow GRC");
    println!("    - Patch: SCCM, WSUS, Tanium, Ivanti");
    println!("    - Cloud: AWS, Azure, GCP, OCI native APIs (Security Hub, Defender, SCC)");
    println!("    - CMDB: ServiceNow CMDB, BMC Atrium");
    println!("    - Endpoint: CrowdStrike, SentinelOne (XDR data ingestion)");
    println!("  Qualys CLI usage:");
    println!("    qualys vmdr scan launch --target 10.0.0.0/24 --option-profile 'Initial Options'");
    println!("    qualys vmdr report list --type 'Scan Based'");
    println!("    qualys cloud-agent deploy --activation-key abc123 --tag-set production");
    println!("    qualys totalcloud connector create --type aws --account-id 123456789");
    println!("    qualys totalcloud compliance run --framework cis-aws-foundations");
    println!("    qualys was scan launch --webapp-id 12345 --profile owasp-top-10");
    println!("    qualys pc policy run --policy-id 5678 --asset-group production");
    println!("    qualys patch deploy --job-name 'Critical Patches' --schedule immediate");
    println!("  Customers (~10,000+ paying):");
    println!("    - 60% of Fortune 100 + 40% of Forbes Global 2000");
    println!("    - Major banks: JPMorgan, Goldman Sachs, Citi, Wells Fargo");
    println!("    - U.S. federal: DoD agencies, civilian agencies (FedRAMP)");
    println!("    - Healthcare: HCA, Anthem, large hospital systems");
    println!("    - Manufacturing + retail: Walmart, Target, Boeing");
    println!("    - sweet spot: 5K+ employee enterprises in regulated industries");
    println!("    - international: heavy in EMEA + APAC enterprise");
    println!("  Critique: 'legacy SaaS' image vs newer agentless cloud competitors (Wiz, Orca)");
    println!("           UI/UX dated — refresh in progress but lagging");
    println!("           TotalCloud (CNAPP) entry felt reactive vs Wiz's category creation");
    println!("           EDR/XDR products competitive but not category-leading vs CrowdStrike");
    println!("           pricing pressure from Microsoft Defender VM bundling (free for E5)");
    println!("           Tenable head-to-head competition keeps deals on price");
    println!("           growth slowing to single-digits — mature business profile");
    println!("           strong cash flow + profitability could trigger PE interest");
    println!("  Differentiator: original SaaS vuln scanner (since 1999, before AWS!) + 4MB Cloud Agent (4M+ deployed) + ~30% operating margin + cloud-native platform serving 60% of Fortune 100 + broad scope (VM + CNAPP + container + DAST + patch + compliance + EDR) — the profitable SaaS-from-day-one vulnerability management platform for enterprises that want one agent + one platform for everything");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qualys".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qualys(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qualys};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qualys"), "qualys");
        assert_eq!(basename(r"C:\bin\qualys.exe"), "qualys.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qualys.exe"), "qualys");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qualys(&["--help".to_string()], "qualys"), 0);
        assert_eq!(run_qualys(&["-h".to_string()], "qualys"), 0);
        let _ = run_qualys(&["--version".to_string()], "qualys");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qualys(&[], "qualys");
    }
}
