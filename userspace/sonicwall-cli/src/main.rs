#![deny(clippy::all)]
//! sonicwall-cli — personality CLI for SonicWall, the mid-market
//! firewall + UTM vendor with a 30+ year history of ownership
//! changes and a strong SMB / mid-market channel motion.
//!
//! Founded 1991 in Sunnyvale, California by Sreekanth Ravi + Sudhakar
//! Ravi (brothers) under the name Sonic Systems. Renamed SonicWall in
//! the late 1990s; IPO 1999 on NASDAQ:SNWL. Acquired by Thoma Bravo in
//! 2010 + then by Dell in 2012 (folded into Dell Software). Split from
//! Dell in 2016 + sold to Francisco Partners + Elliott Management who
//! re-launched SonicWall as a standalone PE-owned company. Bill
//! Conner ran it 2016-2023; SonicWall has weathered multiple high-
//! profile firmware vulnerability disclosures (SonicOS) over the past
//! decade. Today: mid-market firewall + SOAR + SonicCore + Wireless +
//! Cloud Edge / SASE bundles sold heavily through MSP / channel routes.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — SonicWall mid-market firewall + SMB security personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Sreekanth + Sudhakar Ravi 1991 Sunnyvale; PE-owned");
    println!("    history       1991 founding -> NASDAQ -> Dell -> Francisco Partners + Elliott");
    println!("    tz            TZ series + SOHO + NSa + NSsp firewall appliances");
    println!("    sonicos       SonicOS 7.x with rebuilt management UX + REST API");
    println!("    cse           Cloud Secure Edge (SASE) + ZTNA + cloud-managed firewall");
    println!("    cves          Long history of firmware vulnerabilities + customer impact");
    println!("    channel       Channel + MSP-first motion + SonicWall University training");
    println!("    customers     SMB + mid-market + distributed-branch retail + education");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("sonicwall-cli 0.1.0 (mid-market-utm-pe-owned personality build)"); }

fn run_about() {
    println!("SonicWall, Inc.");
    println!("  Founded:    1991 in Sunnyvale, California as Sonic Systems.");
    println!("  Founders:   Sreekanth Ravi + Sudhakar Ravi (brothers).");
    println!("  Renamed:    SonicWall in the late 1990s as the firewall product line took over.");
    println!("  IPO:        1999 on NASDAQ:SNWL.");
    println!("  Owners (timeline):  1999-2010 public; 2010 Thoma Bravo \\$717M go-private;");
    println!("                      2012 Dell acquires + folds into Dell Software;");
    println!("                      2016 Francisco Partners + Elliott Management buy out from");
    println!("                      Dell + re-launch SonicWall as a PE-owned standalone vendor.");
    println!("  CEOs (recent): Bill Conner 2016-2023; Bob VanKirk CEO from 2023.");
    println!("  Position:   mid-market firewall + UTM bundle vendor sold through MSP + channel.");
}

fn run_history() {
    println!("Ownership + corporate history.");
    println!("  1991:  founded as Sonic Systems by the Ravi brothers in Sunnyvale.");
    println!("  1999:  IPO on NASDAQ:SNWL as SonicWall.");
    println!("  2010:  Thoma Bravo takes private at ~\\$717M.");
    println!("  2012:  Dell acquires + folds into Dell Software alongside Quest + KACE.");
    println!("  2016:  Dell sells Dell Software portfolio (including SonicWall) to Francisco");
    println!("         Partners + Elliott Management for ~\\$2B. SonicWall relaunches standalone.");
    println!("  2016-2023: Bill Conner as CEO; portfolio expansion into endpoint + email + ZTNA.");
    println!("  2021:  SonicWall NSM + GMS + SMA / SRA series targeted by serious 0-days.");
    println!("  2023:  Bob VanKirk takes over as CEO.");
    println!("  Multiple PE-led 'one more turn' attempts at scaling up the customer base.");
}

fn run_tz() {
    println!("Firewall hardware lineup.");
    println!("  SOHO:     small-office / home-office firewall appliance (entry-tier).");
    println!("  TZ-series: SMB + branch desktop appliances from TZ270 up to TZ670.");
    println!("  NSa-series: rack-mount mid-market firewalls (NSa 2700 -> NSa 6700).");
    println!("  NSsp-series: data-centre + service-provider chassis-class (NSsp 11700 -> 15700).");
    println!("  NSv-series: virtual firewalls for VMware, Hyper-V, AWS, Azure, KVM.");
    println!("  Real-Time Deep Memory Inspection (RTDMI): SonicWall's sandboxing engine for");
    println!("  encrypted-traffic threat detection — a long-running marketing differentiator.");
    println!("  Common deployments: retail chains, school districts, regional banks, healthcare.");
}

fn run_sonicos() {
    println!("SonicOS (the firewall operating system).");
    println!("  SonicOS 7.x: major rewrite of the management UX, REST API, IPv6 support,");
    println!("              high-availability + clustering, multi-instance virtual firewalls.");
    println!("  Capture Security Center: cloud-based central management for the fleet.");
    println!("  GMS (Global Management System): on-premises management + reporting alternative.");
    println!("  NSM (Network Security Manager): the newer cloud-native management plane.");
    println!("  Multiple management planes are a long-standing operational quirk that customers");
    println!("  + the channel have flagged for years — consolidation is ongoing.");
}

fn run_cse() {
    println!("Cloud Secure Edge (the SASE story).");
    println!("  SonicWall Cloud Secure Edge (CSE): cloud-managed SASE bundle launched 2024,");
    println!("  built on the Banyan Security ZTNA acquisition (2023) + the Solutions Granted");
    println!("  MSSP acquisition (2024) for the managed-service half of the bundle.");
    println!("  Includes ZTNA, secure web gateway, CASB + DNS filtering through the cloud node.");
    println!("  Cloud-managed firewalls: NSv virtual firewalls administered alongside CSE.");
    println!("  Positioning: the SMB / mid-market alternative to Zscaler / Netskope / Cato.");
    println!("  Strategy: avoid being left out of the SASE generation that displaces UTM appliances.");
}

fn run_cves() {
    println!("Vulnerability history (compressed).");
    println!("  SonicWall has been the subject of multiple high-profile firmware + appliance");
    println!("  vulnerability disclosures over the past decade — disproportionate vs peers.");
    println!("  Notable incidents (selected): the 2021 SonicWall SMA 100 series 0-day exploited");
    println!("  in the wild; multiple SonicOS authentication-bypass + remote-code-execution");
    println!("  advisories; firewall management interface chains; SRA appliance EoL'd in 2021");
    println!("  due to unpatchable issues.");
    println!("  Customer + channel impact: SonicWall publishes monthly Security News + Threat");
    println!("  Report briefings; PSIRT process has been progressively professionalised.");
    println!("  Lessons inform the SonicOS 7.x rewrite + reorganised vulnerability disclosure.");
}

fn run_channel() {
    println!("Channel + MSP motion (the core sales engine).");
    println!("  SecureFirst Partner Programme: tiered Silver / Gold / Platinum partners.");
    println!("  SonicWall University: structured training + certification for partner staff.");
    println!("  MSSP Programme: dedicated managed-security-service-provider track + tooling.");
    println!("  ~100% channel-led revenue — direct sales is essentially nil.");
    println!("  Common partner: regional MSP or value-added reseller serving 50-500 SMBs each.");
    println!("  Integrations into ConnectWise, Datto, Kaseya, NinjaOne PSA + RMM platforms.");
    println!("  Strategy: be the easy-bundle competitor to WatchGuard + Fortinet inside the MSP.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: SMB + mid-market customers 20-2,000 employees buying security");
    println!("  through their regional MSP or distributor.");
    println!("  Industries: regional retail + restaurant chains, K-12 + community colleges,");
    println!("  small + mid-size healthcare systems, regional banks + credit unions, municipal");
    println!("  + state governments, manufacturing + distributed-branch operations.");
    println!("  Geographic: heavy US + EU + LATAM; growing APAC + Middle East presence.");
    println!("  Common deployment: retail chain with 50-500 stores standardised on TZ + Wireless +");
    println!("  Capture Security Center, with the MSP managing the fleet on the customer's behalf.");
    println!("  Anti-segment: Fortune-500 + cloud-first net-new enterprises (go to Palo + Z).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sonicwall-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "tz" => run_tz(),
        "sonicos" => run_sonicos(),
        "cse" => run_cse(),
        "cves" => run_cves(),
        "channel" => run_channel(),
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
        run_tz();
        run_sonicos();
        run_cse();
        run_cves();
        run_channel();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("sonicwall-cli");
        print_version();
    }
}
