#![deny(clippy::all)]
//! barracuda-cli — personality CLI for Barracuda Networks, the
//! Campbell-California-based security + storage vendor known for
//! email security gateways, web application firewall, backup +
//! recovery, and an extremely broad SMB / mid-market portfolio.
//!
//! Founded 2003 in Campbell, California by Dean Drako (CEO + a
//! co-founder of Drako Motors years later) and Michael Perone +
//! Zach Levow (early team). Started with the Barracuda Spam Firewall,
//! a cheap rack-mount email-anti-spam appliance that took share from
//! the Symantec + IronPort enterprise tier. IPO 2013 NYSE:CUDA. Taken
//! private 2017 by Thoma Bravo at \$1.6B; sold to KKR in 2022 for
//! ~\$4B (one of the larger PE-to-PE security deals of the cycle). The
//! 2023 ESG appliance 0-day (CVE-2023-2868) was one of the worst
//! exploited-in-the-wild Chinese-state espionage incidents of recent
//! enterprise-security history and forced full appliance replacement.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Barracuda Networks email + WAF + backup security personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Dean Drako 2003 Campbell; CUDA -> Thoma Bravo -> KKR \\$4B 2022");
    println!("    email         Email security: ESG + Email Protection + Impersonation Protection");
    println!("    waf           Web Application Firewall + WAF-as-a-Service");
    println!("    backup        Barracuda Backup + Cloud-to-Cloud Backup");
    println!("    cloudgen      CloudGen Firewall + secure-access SD-WAN");
    println!("    esg2023       The 2023 ESG appliance CVE-2023-2868 + nation-state campaign");
    println!("    msp           MSP + channel motion + Barracuda MSP Sandbox");
    println!("    customers     SMB + mid-market + education + healthcare + MSP-served");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("barracuda-cli 0.1.0 (smb-security-and-backup-portfolio personality build)"); }

fn run_about() {
    println!("Barracuda Networks, Inc.");
    println!("  Founded:    2003, Campbell, California.");
    println!("  Founders:   Dean Drako (CEO), Michael Perone, Zach Levow + co-founder team.");
    println!("  IPO:        November 2013 on NYSE:CUDA.");
    println!("  Taken private: 2017 by Thoma Bravo at ~\\$1.6B.");
    println!("  Sold to KKR: November 2022 at ~\\$4B (Thoma Bravo PE -> KKR PE).");
    println!("  Recent CEOs: BJ Jenkins (long-time CEO + later Palo Alto president);");
    println!("               Hatem Naguib CEO from 2021.");
    println!("  Position:   broad SMB + mid-market portfolio across email, WAF, backup, +");
    println!("              network security. Channel + MSP-heavy distribution.");
    println!("  Brand:      consumer-visible 'Barracuda' name + the cartoon barracuda logo.");
}

fn run_email() {
    println!("Email security (the founding wedge).");
    println!("  Barracuda Email Protection: cloud-native secure email gateway + API-based");
    println!("  Microsoft 365 protection + impersonation + account-takeover + DMARC defence.");
    println!("  Barracuda Email Security Gateway (ESG): the legacy on-premises appliance line.");
    println!("  PhishLine + Forensics + Sentinel: post-delivery threat hunting + remediation.");
    println!("  Phishing-simulation training: PhishLine acquisition 2018 + integrated training.");
    println!("  Common deployment: SMB + mid-market Microsoft 365 customers wanting more than");
    println!("  Microsoft Defender for Office 365 alone; sold heavily through MSPs.");
    println!("  Email security is Barracuda's strongest brand association + best growth segment.");
}

fn run_waf() {
    println!("Web Application Firewall.");
    println!("  Barracuda WAF (on-prem appliance) + WAF-as-a-Service (cloud SaaS WAF).");
    println!("  WAF rules + OWASP Top 10 coverage + bot mitigation + API security.");
    println!("  DDoS protection + virtual patching + Active Threat Intelligence updates.");
    println!("  Application Protection Platform launched 2021 unifying WAF + WAAP + bot defence.");
    println!("  Positioning: mid-market alternative to F5 + Imperva + Akamai + Cloudflare WAF.");
    println!("  Common deployment: e-commerce sites, regional banks, healthcare portals, SaaS");
    println!("  startups wanting WAF without paying premium F5 + Akamai pricing.");
}

fn run_backup() {
    println!("Backup + recovery.");
    println!("  Barracuda Backup: on-prem + hybrid backup appliances with cloud replication.");
    println!("  Cloud-to-Cloud Backup: Microsoft 365 + Google Workspace + Entra ID + Teams.");
    println!("  Ransomware-resilient design: immutable retention + air-gapped cloud copies.");
    println!("  Recovery testing + bare-metal restore + VM-level granular restore.");
    println!("  Common with the same SMB + mid-market customers buying Barracuda email + WAF —");
    println!("  the all-in-one Barracuda relationship reduces vendor sprawl for small IT teams.");
}

fn run_cloudgen() {
    println!("CloudGen Firewall + secure-access SD-WAN.");
    println!("  CloudGen Firewall: NGFW appliances + virtual + cloud (AWS / Azure / GCP) firewalls.");
    println!("  Secure SD-WAN: built into the CloudGen Firewall — purpose-built for");
    println!("                 distributed-branch + retail customers.");
    println!("  Strong Microsoft Azure integration heritage — Barracuda was an early Azure");
    println!("  Marketplace cloud-firewall vendor + the only one with a Microsoft endorsement");
    println!("  for ExpressRoute pairing in the mid-2010s.");
    println!("  Compared with Palo Alto + Fortinet + Check Point: smaller share + lower price.");
    println!("  Compared with WatchGuard + SonicWall: similar mid-market positioning + niche.");
}

fn run_esg2023() {
    println!("CVE-2023-2868 + the ESG appliance incident.");
    println!("  May-June 2023: Barracuda discloses a zero-day in the Email Security Gateway");
    println!("  (ESG) appliance — CVE-2023-2868, a remote-command-injection in the .tar");
    println!("  attachment-scanning module.");
    println!("  Exploited in the wild since October 2022 by UNC4841 — a China-nexus espionage");
    println!("  actor per Mandiant + CISA + FBI attribution.");
    println!("  Unprecedented response: Barracuda recommended physical replacement of every");
    println!("  affected ESG appliance worldwide, not just a patch — the implant chain had");
    println!("  embedded firmware-level persistence resistant to factory reset.");
    println!("  Free hardware replacement provided to all affected customers globally.");
    println!("  Outcome: a landmark case study in supply-chain + appliance-firmware compromise +");
    println!("  in vendor accountability response to nation-state activity.");
}

fn run_msp() {
    println!("MSP + channel motion.");
    println!("  Barracuda MSP business: dedicated MSP track with multi-tenant billing + console.");
    println!("  Barracuda MSP Sandbox / portal: per-customer security service provisioning.");
    println!("  Strong integration into ConnectWise + Datto + Kaseya RMM + PSA tooling.");
    println!("  Education + SMB MSP channel: extremely large MSP base + per-seat licensing.");
    println!("  Geographic strength: heavy US + EU MSP communities; growing APAC MSP penetration.");
    println!("  Strategy: be the multi-product 'security shelf' an MSP can sell into a single");
    println!("  customer — email + WAF + backup + firewall billed per seat, monthly.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: SMB + mid-market customers 20-5,000 employees + their MSPs.");
    println!("  Industries: K-12 + higher ed (very strong), regional healthcare + clinics,");
    println!("  state + local government, regional banking + credit unions, manufacturing,");
    println!("  regional retail + hospitality, professional services.");
    println!("  Geographic: heavy US + EU; growing APAC + LATAM + Middle East presence.");
    println!("  Total customers: ~200,000+ businesses globally + millions of mailboxes protected.");
    println!("  Sales motion: ~75-90% channel + MSP-led depending on the product line.");
    println!("  Common pitch: 'one Barracuda relationship for email + WAF + backup + firewall");
    println!("  through your existing MSP rather than four separate vendor contracts'.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "barracuda-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "email" => run_email(),
        "waf" => run_waf(),
        "backup" => run_backup(),
        "cloudgen" => run_cloudgen(),
        "esg2023" => run_esg2023(),
        "msp" => run_msp(),
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
        run_email();
        run_waf();
        run_backup();
        run_cloudgen();
        run_esg2023();
        run_msp();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("barracuda-cli");
        print_version();
    }
}
