#![deny(clippy::all)]
//! checkpoint-cli — personality CLI for Check Point Software Technologies,
//! the Israeli enterprise-security pioneer that invented stateful packet
//! inspection in the early 1990s.
//!
//! Founded 1993 in Ramat Gan, Israel by Gil Shwed (CEO + Chairman),
//! Shlomo Kramer (left in 2003), and Marius Nacht. Check Point shipped
//! Firewall-1 in 1994 with the world's first stateful-inspection firewall
//! — the patent on which (US 5606668) effectively defined the
//! enterprise-firewall category for the next decade. IPO 1996 on
//! NASDAQ:CHKP. Gil Shwed remained CEO for 30+ years, finally
//! transitioning to executive chairman in 2024 with Nadav Zafrir
//! (ex-Team8 + ex-Unit 8200) taking the CEO role. Strong in EU + Israel
//! + APAC enterprises; the historical-incumbent brand in network security.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Check Point Software Technologies enterprise-security personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Gil Shwed + Shlomo Kramer + Marius Nacht 1993 Ramat Gan; CHKP");
    println!("    firewall1     Firewall-1 1994 — invented stateful inspection (US 5606668)");
    println!("    infinity      Infinity architecture: Quantum + CloudGuard + Harmony");
    println!("    quantum       Quantum NGFW + Maestro + Smart-1 management");
    println!("    cloudguard    CloudGuard CNAPP + posture + workload + network security");
    println!("    harmony       Harmony endpoint + email + browse + SASE");
    println!("    history       Shwed 30 years as CEO; Nadav Zafrir takeover Dec 2024");
    println!("    customers     Long-tenure EU + Israeli + APAC enterprise + government");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("checkpoint-cli 0.1.0 (stateful-inspection-pioneer personality build)"); }

fn run_about() {
    println!("Check Point Software Technologies Ltd.");
    println!("  Founded:    1993, Ramat Gan, Israel.");
    println!("  Founders:   Gil Shwed (CEO + Chairman 1993-2024), Shlomo Kramer (left 2003,");
    println!("              later founded Imperva + Cato Networks), Marius Nacht (active");
    println!("              chairman + investor in Israeli cyber for decades).");
    println!("  IPO:        June 1996 on NASDAQ:CHKP — among the earliest Israeli tech IPOs.");
    println!("  Headquarters: dual Tel Aviv (R&D) + San Carlos California (commercial).");
    println!("  Market cap: ~\\$20-25B+ depending on cycle.");
    println!("  CEO transition: Dec 2024 — Gil Shwed steps up to Executive Chairman;");
    println!("                  Nadav Zafrir (Team8 + Unit 8200 alum) takes CEO role.");
    println!("  Position:   the historical-incumbent EU + Israeli enterprise security brand.");
}

fn run_firewall1() {
    println!("Firewall-1 (the foundational product).");
    println!("  Released 1994; effectively the first commercial stateful-inspection firewall.");
    println!("  Stateful inspection: track connection state in a kernel table + allow return");
    println!("  traffic of established sessions without per-packet rule evaluation.");
    println!("  Patent US 5606668 'System for securing inbound and outbound data packet flow");
    println!("  in a computer network' filed Dec 1993 + granted Feb 1997 to Shwed et al.");
    println!("  Effectively defined the enterprise-firewall category for the following decade.");
    println!("  Nir Zuk (future Palo Alto Networks CTO) worked on Firewall-1 in this era +");
    println!("  later left to found NetScreen + then Palo Alto in a different direction.");
    println!("  Firewall-1 evolved into VPN-1, then into the modern Quantum security gateways.");
}

fn run_infinity() {
    println!("Check Point Infinity architecture.");
    println!("  Single-platform brand spanning the three product lines:");
    println!("    Quantum:    network security (NGFW + Maestro hyperscale + IoT + SD-WAN).");
    println!("    CloudGuard: cloud security (CNAPP + workload + posture + network).");
    println!("    Harmony:    user + access security (endpoint + email + browser + mobile).");
    println!("  Infinity Total Protection: bundled all-you-can-eat enterprise license tier.");
    println!("  Infinity ThreatCloud: shared global threat-intel data plane feeding all three.");
    println!("  Positioning vs Palo Alto's Prisma + Cortex + NGFW story: very similar shape,");
    println!("  arrived at via a different historical path (organic > acquisition-heavy).");
}

fn run_quantum() {
    println!("Quantum (the network-security product line).");
    println!("  Quantum security gateways: appliances from SMB up to data-centre + telco scale.");
    println!("  Quantum Maestro: hyperscale orchestrator running up to 52 gateways in one logical");
    println!("                   firewall — competitive with Palo Alto + Fortinet chassis.");
    println!("  Quantum Smart-1: centralised management appliance / virtual machine.");
    println!("  Quantum IoT Protect: dedicated security for IoT + OT device traffic.");
    println!("  Quantum SD-WAN: SD-WAN bundled on the same gateway hardware.");
    println!("  Quantum Spark: SMB + branch-office appliance line.");
    println!("  Performance: competitive throughput vs Palo Alto NGFW + Fortinet FortiGate at");
    println!("  comparable price tiers; long history of Common Criteria + FIPS certifications.");
}

fn run_cloudguard() {
    println!("CloudGuard (cloud security).");
    println!("  CloudGuard CNAPP: Cloud-Native Application Protection Platform covering");
    println!("                    posture (CSPM) + workload (CWPP) + IAM (CIEM) + DSPM.");
    println!("  CloudGuard Network Security: virtual NGFW for AWS / Azure / GCP / OCI.");
    println!("  CloudGuard Application Security (Spectral acq 2023): code-to-cloud DevSecOps.");
    println!("  CloudGuard Web Application + API Protection (WAAP) for web + API traffic.");
    println!("  Integrates into ThreatCloud shared threat-intel + Infinity management plane.");
    println!("  Positioned vs Palo Alto Prisma Cloud + Wiz + Lacework on CNAPP feature surface.");
}

fn run_harmony() {
    println!("Harmony (user + access security).");
    println!("  Harmony Endpoint: EDR + EPP + threat hunting + forensics on Windows + macOS +");
    println!("                    Linux. Comparable to CrowdStrike + SentinelOne tier.");
    println!("  Harmony Email + Collaboration: API-based email security for Microsoft 365 +");
    println!("                                  Google Workspace; the Avanan acquisition (2021).");
    println!("  Harmony Browse + SASE: secure web gateway + zero-trust web access.");
    println!("  Harmony Mobile: mobile device threat defence for iOS + Android (Lacoon heritage).");
    println!("  Harmony Connect: cloud-delivered ZTNA + SWG bundle for remote-worker access.");
    println!("  Bundles available under the Infinity Total Protection umbrella license.");
}

fn run_history() {
    println!("Notable history.");
    println!("  1993:  Shwed + Kramer + Nacht found Check Point in Ramat Gan.");
    println!("  1994:  Firewall-1 ships — first commercial stateful-inspection firewall.");
    println!("  1996:  NASDAQ:CHKP IPO.");
    println!("  1997:  US 5606668 stateful-inspection patent granted.");
    println!("  2003:  Shlomo Kramer leaves; later founds Imperva (1999, in parallel) + Cato.");
    println!("  ~2010: Nir Zuk + Palo Alto Networks NGFW emerges as the application-layer rival.");
    println!("  2019:  Avanan acquired (email security).");
    println!("  2021-2023: Avanan + Spectral + Atmosec + Perimeter 81 acquisitions integrated.");
    println!("  2024:  Avanan rebranded Harmony Email; Perimeter 81 acquired \\$490M (SASE).");
    println!("  Dec 2024: Gil Shwed steps up to Executive Chairman after 30 years as CEO;");
    println!("            Nadav Zafrir (Team8 + Unit 8200) becomes CEO.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: large EU + Israeli + APAC + Latin America enterprises +");
    println!("  governments + service providers + telecoms + heavy industry.");
    println!("  Particular strength: long-tenured incumbent at large banks, telcos, utilities,");
    println!("  manufacturers, defence ministries, where the Check Point brand has decades");
    println!("  of deployment + the operational muscle memory is sticky.");
    println!("  Geographic: very heavy Israel + EU + Middle East + APAC + LATAM; modest US.");
    println!("  Channel: long-tenured global channel + Diamond + Platinum partner ecosystem.");
    println!("  Anti-segment: net-new US cloud-first enterprises (default to Palo + CrowdStrike).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "checkpoint-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "firewall1" => run_firewall1(),
        "infinity" => run_infinity(),
        "quantum" => run_quantum(),
        "cloudguard" => run_cloudguard(),
        "harmony" => run_harmony(),
        "history" => run_history(),
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
        run_firewall1();
        run_infinity();
        run_quantum();
        run_cloudguard();
        run_harmony();
        run_history();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("checkpoint-cli");
        print_version();
    }
}
