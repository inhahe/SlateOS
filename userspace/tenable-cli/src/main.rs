#![deny(clippy::all)]

//! tenable-cli — SlateOS Tenable (Nessus creator, exposure management, Columbia MD, NASDAQ:TENB)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tenable(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tenable [OPTIONS]");
        println!("Tenable (SlateOS) — exposure management (Nessus creator, NASDAQ:TENB)");
        println!();
        println!("Options:");
        println!("  nessus                 Nessus Pro vulnerability scanner (the OG)");
        println!("  --io                   Tenable.io / Tenable One cloud platform");
        println!("  --sc                   Tenable Security Center (on-prem flagship)");
        println!("  --ot                   Tenable OT Security (operational technology)");
        println!("  --identity             Tenable Identity Exposure (was Alsid)");
        println!("  --cloud-security       Tenable Cloud Security (CNAPP, was Ermetic)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tenable 2024 (SlateOS), Nessus 10.8"); return 0; }
    println!("Tenable 2024 (SlateOS) — Exposure Management");
    println!("  Vendor: Tenable Holdings, Inc. (Columbia, MD — NASDAQ:TENB since 2018)");
    println!("  Founders: Ron Gula + Jack Huffard + Renaud Deraison, 2002");
    println!("          Renaud Deraison: original creator of Nessus (1998) — at age 17 from France");
    println!("          Nessus = the foundational open-source vulnerability scanner of the 1990s-2000s");
    println!("          Nessus closed-source 2005 (controversial — angered OSS community)");
    println!("          Tenable founded to commercialize Nessus + enterprise vulnerability management");
    println!("          longest-running pure-play vulnerability management vendor");
    println!("  Public market (NASDAQ:TENB):");
    println!("         IPO July 2018 at $23/share, raised $250M");
    println!("         peak ~$70 in 2021, settled $40-50 in 2023-2024");
    println!("         FY2024 revenue: ~$830M+ (+11% YoY)");
    println!("         Market cap: ~$5-7B range");
    println!("         Operating margin improving — focus on profitability post-2022");
    println!("         Long-time CEO Amit Yoran stepped down 2023, replaced by Steve Vintz + Mark Thurmond");
    println!("  Strategic position: 'exposure management — see and reduce cyber risk across your attack surface':");
    println!("                    pitch: 'know all your assets, prioritize what matters, fix what hurts'");
    println!("                    target: every enterprise (broadest vuln-mgmt customer base)");
    println!("                    primary competitor: Qualys (head-to-head), Rapid7, Microsoft Defender Vulnerability Mgmt");
    println!("                    secondary: Wiz, Orca (cloud-only), CrowdStrike (endpoint-led)");
    println!("                    Tenable's wedge: Nessus install base + on-prem dominance + OT + identity");
    println!("                    'Exposure Management' rebrand 2023 to compete with CNAPP/CTEM trends");
    println!("  Pricing:");
    println!("    Nessus Essentials — FREE for up to 16 IPs (home/personal use)");
    println!("    Nessus Pro — $4K/yr (single scanner, unlimited IPs)");
    println!("    Nessus Expert — $5.7K/yr (with web app scanning + external attack surface)");
    println!("    Tenable.io (cloud) — $3K-$5K/100 assets/yr starting");
    println!("    Tenable Security Center (on-prem) — $5K-$1M+/yr based on scale");
    println!("    Tenable One (exposure mgmt platform bundle) — premium tier, $50K-$5M+/yr");
    println!("  Product portfolio:");
    println!("    1. Nessus (the OG vulnerability scanner):");
    println!("       - 47K+ vulnerability checks across OS, apps, network devices");
    println!("       - Most-deployed vulnerability scanner in history (40K+ enterprises)");
    println!("       - Used in PCI-DSS + HIPAA + FedRAMP scans");
    println!("    2. Tenable.io (cloud-based vuln management):");
    println!("       - SaaS scanner + management console");
    println!("       - Container scanning, web app scanning, external attack surface");
    println!("    3. Tenable Security Center (the on-prem flagship):");
    println!("       - Self-hosted vulnerability management");
    println!("       - Government + regulated industries dominant");
    println!("       - FedRAMP authorized");
    println!("    4. Tenable OT Security (operational tech / ICS):");
    println!("       - Acquired Indegy 2019 ($78M)");
    println!("       - Industrial control system + SCADA visibility");
    println!("       - Strong in: utilities, manufacturing, oil & gas");
    println!("    5. Tenable Identity Exposure (was Alsid):");
    println!("       - Acquired Alsid 2021 (~$100M)");
    println!("       - Active Directory + Entra ID security posture");
    println!("    6. Tenable Cloud Security (CNAPP — was Ermetic):");
    println!("       - Acquired Ermetic Oct 2023 for $240M");
    println!("       - CSPM + CIEM + workload security across AWS/Azure/GCP");
    println!("       - Tenable's late-but-strategic entry into CNAPP");
    println!("    7. Tenable Attack Surface Management (was Bit Discovery):");
    println!("       - Acquired 2022 ($44M)");
    println!("       - External attack surface discovery");
    println!("    8. Tenable One (the platform bundle 2023):");
    println!("       - Unifies all the above with shared asset inventory + exposure score");
    println!("       - 'CTEM' (Continuous Threat Exposure Management) positioning");
    println!("  Nessus heritage (the OG vulnerability scanner):");
    println!("    - Released 1998 by Renaud Deraison");
    println!("    - Open-source GPL until 2005");
    println!("    - 47,000+ vulnerability plugins maintained today");
    println!("    - Default scanner for most enterprise penetration testing teams");
    println!("    - 30M+ downloads over project lifetime");
    println!("    - Tenable releases new vulnerability detections within hours of CVE publication");
    println!("  Tenable Research:");
    println!("    - Tenable Zero Day Initiative (vulnerability research)");
    println!("    - Released many high-profile CVE discoveries");
    println!("    - 'Beyond CVE' research on supply chain + cloud + identity attacks");
    println!("    - Highly-cited industry threat reports");
    println!("  Integrations:");
    println!("    - SIEM: Splunk, Sentinel, Sumo Logic, Elastic, IBM QRadar, Datadog");
    println!("    - SOAR: ServiceNow, Tines, Palo Alto Cortex XSOAR, Splunk Phantom");
    println!("    - Cloud: AWS, Azure, GCP native APIs");
    println!("    - Ticketing: Jira, ServiceNow, Remedy, BMC Helix");
    println!("    - CMDB integration: ServiceNow, BMC Atrium");
    println!("    - GRC: Archer, RSA, MetricStream");
    println!("    - Patch management: WSUS, SCCM, Tanium, Red Hat Satellite");
    println!("  Tenable CLI usage:");
    println!("    nessus scan create --target 10.0.0.0/24 --policy basic-network");
    println!("    nessus scan launch --scan-id 12345");
    println!("    tenable.io assets list --tag production");
    println!("    tenable.io workbench plugins critical");
    println!("    tenable.one exposure-score --asset webapp-prod");
    println!("    tenable.cs compliance evaluate --framework cis-aws");
    println!("  Customers (~44,000+ paying):");
    println!("    - 65% of Fortune 500");
    println!("    - U.S. Department of Defense, NSA, IRS, federal agencies");
    println!("    - All major U.S. banks + most globals");
    println!("    - Pfizer, Walmart, Verizon, AT&T, IBM (internal use)");
    println!("    - International governments + utilities");
    println!("    - Widest install base of any vuln-mgmt vendor");
    println!("  Critique: legacy vulnerability-scanning positioning vs modern CNAPP/CTEM");
    println!("           agentless cloud security entry was late (Ermetic acq Oct 2023 vs Wiz 2020 founding)");
    println!("           on-prem dominance = exposure to cloud-migration headwinds");
    println!("           UX dated vs newer competitors (Wiz)");
    println!("           stock price under pressure: cloud transition + competitive intensity");
    println!("           Qualys head-to-head competition keeps margins compressed");
    println!("           Microsoft Defender Vulnerability Management free for E5 customers = price pressure");
    println!("           Tenable One bundling helps but unbundled pricing still revenue base");
    println!("  Differentiator: Nessus creator (47K+ detection plugins) + 65% Fortune 500 footprint + dominant on-prem + OT/ICS leadership + Identity Exposure (Alsid) + recent CNAPP entry (Ermetic) — the exposure-management platform for organizations that need broad scope across IT, cloud, identity, and OT");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tenable".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tenable(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tenable};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tenable"), "tenable");
        assert_eq!(basename(r"C:\bin\tenable.exe"), "tenable.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tenable.exe"), "tenable");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tenable(&["--help".to_string()], "tenable"), 0);
        assert_eq!(run_tenable(&["-h".to_string()], "tenable"), 0);
        let _ = run_tenable(&["--version".to_string()], "tenable");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tenable(&[], "tenable");
    }
}
