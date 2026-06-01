#![deny(clippy::all)]
//! carbonblack-cli — personality CLI for Carbon Black, the EDR
//! pioneer that grew from Bit9 application-whitelisting roots, IPO'd
//! as CBLK in 2018, and was acquired by VMware in 2019 for \$2.1B —
//! becoming VMware Carbon Black + later Broadcom Carbon Black after
//! the 2023 Broadcom + VMware deal.
//!
//! Origins go back to Bit9, founded in 2002 in Waltham Massachusetts
//! by Todd Brennan, John Hanratty, and Allen Hillery, originally
//! commercialising MIT-derived application-whitelisting research. The
//! separate Carbon Black product began life as a Kyrus Tech tool, was
//! spun off, and merged with Bit9 in 2014 — the combined company
//! rebranded to Carbon Black in 2016. CEO Patrick Morley led the
//! company through the 2018 NASDAQ:CBLK IPO. VMware acquired Carbon
//! Black October 2019 for \$2.1B + integrated as VMware Carbon Black
//! across vSphere + Workspace ONE. Broadcom completed the VMware
//! acquisition November 2023; Carbon Black now sits inside Broadcom
//! Software Group alongside Symantec + AppDynamics + Tanzu.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Carbon Black EDR + endpoint security personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Bit9 2002 Waltham; merged Carbon Black 2014; rebranded 2016");
    println!("    history       Bit9 -> Carbon Black merger -> CBLK IPO -> VMware -> Broadcom");
    println!("    cbcloud       Carbon Black Cloud platform + Endpoint Standard / Advanced / Enterprise");
    println!("    edr           Carbon Black EDR (legacy on-prem) + threat-hunting heritage");
    println!("    appcontrol    App Control (Bit9 lineage) + application whitelisting");
    println!("    vmware        VMware Carbon Black integration with vSphere + Workspace ONE");
    println!("    broadcom      2023 Broadcom + VMware deal — Carbon Black inside Broadcom Software");
    println!("    customers     Fortune 500 + government + long-running enterprise EDR install base");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("carbonblack-cli 0.1.0 (edr-pioneer-vmware-broadcom personality build)"); }

fn run_about() {
    println!("Carbon Black (now Broadcom Carbon Black).");
    println!("  Origin (Bit9): 2002 in Waltham, Massachusetts. Founders Todd Brennan,");
    println!("                 John Hanratty, Allen Hillery commercialised MIT-derived");
    println!("                 application-whitelisting research as Bit9 Parity.");
    println!("  Carbon Black product line: spun off from Kyrus Tech security consultancy.");
    println!("  2014:    Bit9 + Carbon Black merge under Patrick Morley as CEO.");
    println!("  2016:    combined company rebrands to Carbon Black, Inc.");
    println!("  May 2018: IPO on NASDAQ:CBLK at \\$19/share (\\$1.3B initial market cap).");
    println!("  Oct 2019: VMware acquires for \\$2.1B; becomes VMware Carbon Black.");
    println!("  Nov 2023: Broadcom completes VMware acquisition; Carbon Black inside Broadcom.");
    println!("  Position: EDR pioneer with long-running enterprise install base + roadmap");
    println!("            consolidation inside the Broadcom enterprise-strategic motion.");
}

fn run_history() {
    println!("Compressed corporate history.");
    println!("  2002:  Bit9 founded in Waltham MA — application whitelisting (Parity).");
    println!("  ~2007: Carbon Black product line begins at Kyrus Tech, focused on EDR");
    println!("         + endpoint recording before EDR was a coined category name.");
    println!("  2014:  Bit9 + Carbon Black merge; named 'Bit9 + Carbon Black' transitionally.");
    println!("  2016:  rebrand to Carbon Black, Inc; Patrick Morley CEO; Waltham HQ.");
    println!("  May 2018: NASDAQ:CBLK IPO at \\$19 + spiked over \\$25 on debut.");
    println!("  Aug 2019: VMware announces acquisition for \\$2.1B all-cash.");
    println!("  Oct 2019: deal closes; Carbon Black becomes VMware Carbon Black.");
    println!("  2020-2023: integration across vSphere + Workspace ONE + Tanzu portfolio.");
    println!("  Nov 2023: Broadcom completes VMware acquisition; Carbon Black moves into");
    println!("            Broadcom Software Group alongside Symantec + AppDynamics + CA.");
}

fn run_cbcloud() {
    println!("Carbon Black Cloud (the platform).");
    println!("  Cloud-native multi-tenant SaaS endpoint platform launched ~2019.");
    println!("  Carbon Black Cloud Endpoint Standard: next-gen antivirus + behavioural prevention.");
    println!("  Carbon Black Cloud Endpoint Advanced: + EDR + threat-hunting + remediation.");
    println!("  Carbon Black Cloud Endpoint Enterprise: + Threat Hunter Service + managed detection.");
    println!("  Carbon Black Cloud Workload: dedicated workload protection for vSphere + clouds.");
    println!("  Carbon Black Cloud Container: container + Kubernetes runtime protection.");
    println!("  Telemetry: continuous endpoint event recording — the original 'DVR for endpoint'");
    println!("  pitch — enabling threat-hunting + retrospective investigations across the fleet.");
}

fn run_edr() {
    println!("Carbon Black EDR (the legacy on-prem product).");
    println!("  Originally 'Cb Response' / 'Carbon Black Enterprise Response' — the on-prem");
    println!("  installable EDR appliance + server that ran inside customer datacentres for");
    println!("  customers with strict data-residency or air-gapped requirements.");
    println!("  Process tree + binary lineage + network connection capture for every endpoint.");
    println!("  Threat-hunting query language: Cb Query syntax for retrospective searches.");
    println!("  Real-time response: remote shell + file pull + memory dump across the fleet.");
    println!("  Heavy historical use in financial services + government + classified-network");
    println!("  environments where cloud-only Carbon Black Cloud was not an option.");
}

fn run_appcontrol() {
    println!("App Control (the Bit9 lineage).");
    println!("  Originally Bit9 Parity / Bit9 Security Platform: application allowlisting,");
    println!("  device control + integrity monitoring on Windows + Linux + macOS endpoints.");
    println!("  Default-deny posture: only explicitly approved binaries execute on the endpoint.");
    println!("  File-integrity monitoring + change-control for regulated environments (PCI, NERC-CIP).");
    println!("  Trust scoring: software-publisher-, hash-, and category-based trust policies.");
    println!("  Use cases: fixed-function endpoints (ATMs, POS terminals, industrial workstations,");
    println!("  medical devices, SCADA servers) where allowlisting is the appropriate posture.");
    println!("  Continues as Carbon Black App Control inside the modern portfolio.");
}

fn run_vmware() {
    println!("VMware Carbon Black era (2019-2023).");
    println!("  Acquired Oct 2019 for \\$2.1B; integrated as VMware Security Business Unit.");
    println!("  Integration with vSphere: lightweight sensor + workload protection without");
    println!("  inside-the-guest agents for VMware Cloud Foundation customers.");
    println!("  Integration with Workspace ONE: unified endpoint management + endpoint security.");
    println!("  Integration with NSX: network + endpoint correlation for east-west traffic threat");
    println!("  detection across VMware-defined data centres.");
    println!("  Tanzu integration: container + Kubernetes runtime protection on Tanzu Application");
    println!("  Service + Tanzu Kubernetes Grid.");
    println!("  Strategic pitch: VMware-stack-native security with no agent in the guest OS.");
}

fn run_broadcom() {
    println!("Broadcom + VMware acquisition (closed November 2023).");
    println!("  Broadcom completes \\$69B acquisition of VMware in Nov 2023 — the largest");
    println!("  technology acquisition in history at signing.");
    println!("  Post-close: Carbon Black moves into Broadcom Software Group alongside Symantec,");
    println!("  AppDynamics, CA Technologies — Hock Tan's classic strategic-account roll-up portfolio.");
    println!("  Common Broadcom playbook: focus on top ~600 strategic accounts; rationalise");
    println!("  long-tail SKUs + channel; raise prices on strategic customers; thin out adjacent");
    println!("  product investment that does not support the strategic core.");
    println!("  Customer reaction: large enterprise customers see consolidation; long-tail SMB +");
    println!("  partner customers face uncertainty + price increases.");
    println!("  Outlook: Carbon Black continues for the strategic accounts; net-new growth tough.");
}

fn run_customers() {
    println!("Customer profile.");
    println!("  Sweet spot (current): Fortune 500 + Global 2000 enterprises + governments running");
    println!("  long-tenured Carbon Black EDR + App Control + Endpoint Cloud deployments,");
    println!("  many anchored in VMware-stack environments.");
    println!("  Industries: financial services, government + defence, healthcare, manufacturing,");
    println!("  utilities, retail point-of-sale fleets, large education systems.");
    println!("  Notable historical customers: many of the largest US banks + insurance + Fortune");
    println!("  100 industrials adopted Cb Response + App Control through the 2010s.");
    println!("  Geographic: heavy US + EU + APAC enterprise + government; modest LATAM.");
    println!("  Anti-segment (today): net-new cloud-first SMB + mid-market (default to CrowdStrike,");
    println!("  SentinelOne, Microsoft Defender for Endpoint, Sophos).");
    println!("  Channel: partner-channel rationalised under Broadcom; direct-strategic-account-led.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "carbonblack-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "cbcloud" => run_cbcloud(),
        "edr" => run_edr(),
        "appcontrol" => run_appcontrol(),
        "vmware" => run_vmware(),
        "broadcom" => run_broadcom(),
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
        run_history();
        run_cbcloud();
        run_edr();
        run_appcontrol();
        run_vmware();
        run_broadcom();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("carbonblack-cli");
        print_version();
    }
}
