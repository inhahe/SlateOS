#![deny(clippy::all)]

//! rapid7-cli — Slate OS Rapid7 (Metasploit + InsightVM + Insight platform, Boston, NASDAQ:RPD)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_r7(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rapid7 [OPTIONS]");
        println!("Rapid7 (Slate OS) — Insight Platform (security operations, NASDAQ:RPD)");
        println!();
        println!("Options:");
        println!("  msfconsole             Metasploit Framework (OSS)");
        println!("  --insight-vm           InsightVM vulnerability management");
        println!("  --insight-idr          InsightIDR cloud SIEM + XDR");
        println!("  --insight-cloud        InsightCloudSec (was DivvyCloud) CSPM");
        println!("  --insight-connect      InsightConnect SOAR");
        println!("  --insight-appsec       InsightAppSec DAST");
        println!("  --threat-command       Threat Command (digital risk, was IntSights)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rapid7 + Metasploit 6.4 (Slate OS)"); return 0; }
    println!("Rapid7 2024 (Slate OS) — Insight Platform");
    println!("  Vendor: Rapid7, Inc. (Boston, MA — NASDAQ:RPD since 2015)");
    println!("  Founders: Alan Matthews + Tas Giakouminakis + Chad Loder, 2000");
    println!("          founded in Manhattan, moved to Boston");
    println!("          early product: NeXpose (vulnerability scanner) — competed with Nessus");
    println!("          long history: 24+ years operating + most diverse pure-play security portfolio");
    println!("          Corey Thomas: long-time CEO (2012-2024)");
    println!("          Tas Giakouminakis: still CTO + founder still involved");
    println!("  Public market (NASDAQ:RPD):");
    println!("         IPO July 2015 at $16/share, raised ~$103M");
    println!("         peak ~$135 late 2021");
    println!("         Decline to $30-40 range 2023-2024 — multi-year struggles vs CrowdStrike/Wiz competition");
    println!("         FY2024 revenue: ~$830M+ (similar size to Tenable, growth ~10%)");
    println!("         Market cap: $2-3B range");
    println!("         Activist investor Jana Partners 2023 — pushed for sale/M&A");
    println!("         Strategic review and sale process underway 2024 — possible PE acquisition");
    println!("         Operating margins improving but topline pressure");
    println!("  Strategic position: 'security operations + visibility — broad portfolio for mid-market':");
    println!("                    pitch: 'unified security operations across attack surface + cloud + SIEM'");
    println!("                    target: mid-market + lower-enterprise (sweet spot vs Tenable's broader, Qualys's bigger)");
    println!("                    primary competitor: Tenable, Qualys, CrowdStrike (XDR), Microsoft Defender, Wiz (cloud)");
    println!("                    secondary: ServiceNow Security Ops (overlap with InsightConnect SOAR)");
    println!("                    Rapid7's wedge: Metasploit ownership + broad portfolio + MDR services attached");
    println!("                    challenge: not a leader in any single category — 'jack of all trades' problem");
    println!("  Pricing:");
    println!("    Metasploit Framework — FREE, BSD (community OSS)");
    println!("    Metasploit Pro — $15K+/yr (commercial pen-test platform)");
    println!("    InsightVM (vuln mgmt) — $2K-$1M+/yr based on assets");
    println!("    InsightIDR (SIEM/XDR) — $5K-$500K+/yr based on data ingestion");
    println!("    Full Insight platform — $50K-$5M+/yr enterprise deals");
    println!("    Rapid7 MDR (managed detection) — $100K-$2M+/yr typical");
    println!("    typically priced below Tenable + Qualys for mid-market wins");
    println!("  Metasploit (the iconic asset):");
    println!("    - Acquired Metasploit 2009 ($500K)");
    println!("    - Created by HD Moore (joined Rapid7 as CSO 2009-2016)");
    println!("    - The world's most-used penetration testing framework");
    println!("    - 2,000+ exploits, 1,000+ auxiliary modules, 800+ payloads");
    println!("    - 4M+ downloads/year");
    println!("    - msfconsole / msfvenom / meterpreter — household names for security pros");
    println!("    - Massive marketing funnel for commercial products");
    println!("    - Open-source community vital to Rapid7's identity");
    println!("  Insight Platform (the commercial portfolio):");
    println!("    1. InsightVM (vulnerability management):");
    println!("       - Successor to NeXpose");
    println!("       - Active Risk score (CVSS + threat intel + exploit availability)");
    println!("       - Compete with: Tenable, Qualys");
    println!("    2. InsightIDR (cloud SIEM + XDR):");
    println!("       - Cloud-native SIEM with UEBA + endpoint detection");
    println!("       - Compete with: Splunk, Microsoft Sentinel, CrowdStrike, Datadog Security");
    println!("    3. InsightCloudSec (CSPM + CIEM — was DivvyCloud, acquired 2020 $145M):");
    println!("       - Multi-cloud posture management");
    println!("       - Identity entitlement");
    println!("       - Compete with: Wiz, Palo Alto Prisma");
    println!("    4. InsightAppSec (DAST):");
    println!("       - Dynamic application security testing");
    println!("       - Compete with: Veracode, Checkmarx, Snyk DAST");
    println!("    5. InsightConnect (SOAR):");
    println!("       - Security automation + orchestration");
    println!("       - Compete with: Palo Alto Cortex XSOAR, Splunk Phantom, Tines");
    println!("    6. Threat Command (was IntSights, acquired 2021 $335M):");
    println!("       - Digital risk monitoring + dark web intelligence");
    println!("       - Compete with: Recorded Future, ZeroFox, Flashpoint");
    println!("    7. Rapid7 MDR (Managed Detection and Response):");
    println!("       - 24/7 SOC service");
    println!("       - Compete with: CrowdStrike Falcon Complete, Arctic Wolf, Expel");
    println!("    8. Velociraptor (free OSS endpoint forensics):");
    println!("       - Acquired Velocidex 2021");
    println!("       - DFIR (digital forensics + incident response) toolkit");
    println!("  Acquisitions history:");
    println!("    - Metasploit 2009 ($500K) — best ROI acquisition in security history");
    println!("    - DivvyCloud 2020 ($145M) — CSPM");
    println!("    - IntSights 2021 ($335M) — digital risk");
    println!("    - Velocidex 2021 — endpoint DFIR");
    println!("    - Other smaller: Komand (SOAR), tCell (RASP), Logentries (log mgmt)");
    println!("  Open-source community:");
    println!("    - Metasploit Framework (BSD)");
    println!("    - Velociraptor (Apache 2.0)");
    println!("    - Recog (fingerprinting database)");
    println!("    - Active in security research + DEF CON / Black Hat presence");
    println!("    - Rapid7 Labs annual 'Industry Cyber-Exposure Report' (closely-watched)");
    println!("  Integrations (200+):");
    println!("    - SIEM/SOAR: Splunk, Sentinel, Sumo Logic, Datadog, Cortex XSOAR");
    println!("    - Cloud: AWS, Azure, GCP, OCI native APIs");
    println!("    - Endpoint: CrowdStrike, SentinelOne, Defender for Endpoint (some) — XDR partners");
    println!("    - Ticketing: ServiceNow, Jira, PagerDuty, BMC Remedy");
    println!("    - CMDB: ServiceNow, BMC Atrium, Lansweeper");
    println!("    - Patch mgmt: WSUS, SCCM, Tanium");
    println!("    - GRC: Archer, MetricStream, ServiceNow GRC");
    println!("  Rapid7 CLI usage:");
    println!("    msfconsole                                  # OSS Metasploit");
    println!("    msfvenom -p windows/meterpreter/reverse_tcp # payload generator");
    println!("    rapid7 insightvm scan --site production");
    println!("    rapid7 insightidr investigation list --status open");
    println!("    rapid7 insightcloudsec compliance --pack cis-aws");
    println!("    rapid7 metasploit pro report --workspace pentest");
    println!("  Customers (~11,000+ paying):");
    println!("    - Major banks, retailers, healthcare, manufacturing");
    println!("    - U.S. federal: Air Force, DoD agencies, civilian agencies");
    println!("    - Pfizer, Disney, Marriott, IKEA, Levi's");
    println!("    - sweet spot: mid-market + lower-enterprise (1K-10K employees)");
    println!("    - heavy in: financial services, healthcare, retail, government");
    println!("  Critique: broad-but-not-deep across multiple categories");
    println!("           losing share to leaders in each segment (Wiz cloud, CrowdStrike XDR, Tenable VM)");
    println!("           InsightCloudSec (DivvyCloud) behind Wiz in CNAPP innovation");
    println!("           InsightIDR (SIEM) struggling against Microsoft Sentinel + Datadog");
    println!("           stock pressure + activist Jana = strategic review with possible sale");
    println!("           Metasploit is the brand strength but not the revenue driver");
    println!("           multi-product portfolio = complex sales motion + slower wins than focused vendors");
    println!("           pricing pressure from Microsoft Defender E5 bundling");
    println!("  Differentiator: Metasploit ownership (security icon) + Velociraptor + broadest pure-play security portfolio (VM + SIEM + CSPM + SOAR + DAST + MDR + threat intel) + mid-market sweet spot + MDR-as-add-on — the security operations platform that everyone uses Metasploit even if they don't pay for the rest");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rapid7".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_r7(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_r7};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rapid7"), "rapid7");
        assert_eq!(basename(r"C:\bin\rapid7.exe"), "rapid7.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rapid7.exe"), "rapid7");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_r7(&["--help".to_string()], "rapid7"), 0);
        assert_eq!(run_r7(&["-h".to_string()], "rapid7"), 0);
        let _ = run_r7(&["--version".to_string()], "rapid7");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_r7(&[], "rapid7");
    }
}
