#![deny(clippy::all)]
//! fortinet-cli — personality CLI for Fortinet, the FortiGate
//! firewall + Security Fabric enterprise security platform.
//!
//! Founded 2000 in Sunnyvale, California by Ken Xie (CEO) and his
//! brother Michael Xie (CTO + President). Ken Xie had previously
//! founded NetScreen Technologies (sold to Juniper 2004 for \$4B);
//! Fortinet was his second act. Defining technical bet: custom ASIC-
//! accelerated firewall appliances (the FortiASIC chips) delivering
//! competitive throughput vs commodity x86 firewalls at a much lower
//! cost-per-Gbps. IPO 2009 NASDAQ:FTNT. Market cap routinely
//! >\$60B-80B. Aggressively mid-market + service-provider focused;
//! the Fortinet Security Fabric integrates FortiGate (firewall) with
//! FortiAnalyzer, FortiManager, FortiClient, FortiSwitch, FortiAP,
//! FortiSIEM into a single-vendor stack — a sharply differentiated
//! position from Palo Alto's premium NGFW + roll-up strategy.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Fortinet FortiGate firewall + Security Fabric personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Ken + Michael Xie 2000 Sunnyvale; FTNT; \\$60B+ mkt cap");
    println!("    fortigate     FortiGate NGFW + FortiASIC custom-silicon acceleration");
    println!("    fabric        Security Fabric: single-vendor integrated stack");
    println!("    fortios       FortiOS — proprietary OS shared across the Fortinet product line");
    println!("    sase          FortiSASE + FortiSD-WAN + FortiClient ZTNA");
    println!("    fortiguard    FortiGuard Labs threat-intelligence research arm");
    println!("    positioning   vs Palo Alto + Check Point — mid-market + service-provider");
    println!("    customers     Service providers + mid-market + emerging markets + MSSPs");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("fortinet-cli 0.1.0 (fortigate-security-fabric personality build)"); }

fn run_about() {
    println!("Fortinet, Inc.");
    println!("  Founded:    2000, Sunnyvale, California.");
    println!("  Founders:   Ken Xie (CEO) + Michael Xie (CTO + President), brothers.");
    println!("              Ken Xie previously founded NetScreen Technologies (sold to");
    println!("              Juniper Networks 2004 for ~\\$4B).");
    println!("  IPO:        November 2009 on NASDAQ:FTNT.");
    println!("  Market cap: routinely \\$60-80B+ — top-3 pure-play cybersecurity company.");
    println!("  Revenue:    ~\\$5.3B annual run-rate as of recent fiscal years.");
    println!("  Headcount:  ~14,000 globally.");
    println!("  Position:   custom-silicon firewall pricing-leader; Security Fabric");
    println!("              single-vendor integrated stack; service-provider-friendly.");
}

fn run_fortigate() {
    println!("FortiGate (the flagship NGFW product line).");
    println!("  Form factors: from FG-30E SMB desktop appliance up to FG-7000F chassis-class.");
    println!("  Virtual: FortiGate-VM for AWS, Azure, GCP, OCI, VMware, KVM, Hyper-V, Xen.");
    println!("  FortiASIC: custom Network Processor (NP) + Security Processor (SP) + Content");
    println!("             Processor (CP) chips accelerate stateful inspection, IPS, crypto,");
    println!("             and content inspection — competitive throughput at lower price.");
    println!("  Features: NGFW + IPS + SSL inspection + sandboxing + URL filtering +");
    println!("            anti-malware + DLP + SD-WAN + IPsec/SSL VPN + ZTNA — bundled.");
    println!("  Pricing: substantially undercuts Palo Alto + Check Point at equivalent throughput.");
}

fn run_fabric() {
    println!("Security Fabric (the single-vendor integration story).");
    println!("  FortiGate:     NGFW + the integration hub.");
    println!("  FortiAnalyzer: centralised log + report + analytics across the fabric.");
    println!("  FortiManager:  policy management + multi-firewall provisioning + change mgmt.");
    println!("  FortiClient:   endpoint protection + VPN + ZTNA agent.");
    println!("  FortiSwitch:   managed switches that report into the fabric.");
    println!("  FortiAP:       managed Wi-Fi access points.");
    println!("  FortiSIEM:     security information + event management.");
    println!("  FortiSOAR:     security orchestration + automation + response.");
    println!("  FortiEDR + FortiXDR: endpoint + extended detection + response (enSilo acq 2019).");
    println!("  All bound by a shared API + telemetry plane — sold as a one-vendor alternative.");
}

fn run_fortios() {
    println!("FortiOS (the unifying operating system).");
    println!("  Proprietary OS running across FortiGate, FortiAnalyzer, FortiManager,");
    println!("  FortiSwitch, FortiAP + most of the fabric.");
    println!("  Shared CLI + REST API + configuration semantics across the product line —");
    println!("  unusual operational consistency vs competitors with multi-OS portfolios.");
    println!("  Major releases historically every 12-18 months; in-place upgradable.");
    println!("  Custom hardware drivers for the FortiASIC chip family — the OS knows how to");
    println!("  offload IPS, crypto, content inspection to the specialised processors.");
    println!("  This OS-level consistency is a real differentiator for large multi-site customers.");
}

fn run_sase() {
    println!("SASE + SD-WAN + ZTNA (the cloud + remote-work story).");
    println!("  FortiSASE:    Fortinet's cloud-delivered Secure Access Service Edge offering.");
    println!("  FortiSD-WAN:  industry-leading SD-WAN typically delivered on FortiGate appliances.");
    println!("                Fortinet routinely tops Gartner's SD-WAN Magic Quadrant.");
    println!("  FortiClient:  endpoint agent providing VPN + ZTNA tunnel into the fabric.");
    println!("  FortiZTNA:    Zero-Trust Network Access bundled into FortiGate + FortiClient.");
    println!("  Particular strength: branch + retail + manufacturing customers who want SD-WAN");
    println!("  on the same box as the firewall, with one license + one management plane.");
}

fn run_fortiguard() {
    println!("FortiGuard Labs (the threat-intelligence research arm).");
    println!("  ~500+ threat researchers globally — one of the largest pure-cybersecurity");
    println!("  research orgs in the industry.");
    println!("  Subscription-based threat intel + IPS rule + URL category + anti-malware sig");
    println!("  updates fed into the fabric continuously.");
    println!("  CTA (Cyber Threat Alliance) co-founder along with Palo Alto + Symantec +");
    println!("  McAfee + Cisco for cross-vendor threat data sharing.");
    println!("  Regular landmark APT + ransomware + IoT botnet publications — Fortinet research");
    println!("  brand is recognised alongside Mandiant + Talos + Unit 42.");
}

fn run_positioning() {
    println!("Competitive positioning.");
    println!("  Palo Alto Networks: premium NGFW + heavy M&A roll-up; higher list price.");
    println!("  Check Point:        original stateful inspection inventor; strong in EU + IL.");
    println!("  Cisco Secure:       legacy networking + security bundle; premium price.");
    println!("  Fortinet:           custom-silicon undercut + Security Fabric single-vendor.");
    println!("  Sweet spot vs Palo: anywhere price-per-Gbps matters more than absolute peak");
    println!("  feature breadth — telecoms, MSPs, mid-market, manufacturing, emerging markets.");
    println!("  Loses to Palo on: Fortune-100 NGFW deals where buyer wants the premium brand.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: mid-market + service-provider + retail + manufacturing customers,");
    println!("  plus emerging-market enterprises where firewall pricing-per-Gbps dominates.");
    println!("  Service providers: extremely heavy presence among regional + national telecoms,");
    println!("  ISPs, MSSPs — Fortinet's volume + price model is purpose-built for that segment.");
    println!("  Federal + government: US + EU + Middle East + APAC government footprints.");
    println!("  Geographic: very heavy APAC + Middle East + LATAM + EU; growing US enterprise.");
    println!("  Channel: ~95%+ channel-led revenue — among the most channel-friendly vendors.");
    println!("  Anti-segment: Fortune-100 NGFW deals where the buyer wants the Palo Alto brand.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fortinet-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "fortigate" => run_fortigate(),
        "fabric" => run_fabric(),
        "fortios" => run_fortios(),
        "sase" => run_sase(),
        "fortiguard" => run_fortiguard(),
        "positioning" => run_positioning(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_fortigate();
        run_fabric();
        run_fortios();
        run_sase();
        run_fortiguard();
        run_positioning();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("fortinet-cli");
        print_version();
    }
}
