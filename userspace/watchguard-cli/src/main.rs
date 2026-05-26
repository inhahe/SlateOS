#![deny(clippy::all)]
//! watchguard-cli — personality CLI for WatchGuard Technologies, the
//! Seattle-headquartered mid-market security vendor specialising in
//! firewall + EDR + MFA + secure Wi-Fi for SMB + mid-market customers
//! delivered through a heavy channel + MSP partner motion.
//!
//! Founded 1996 in Seattle, originally as Seattle Software Labs, by
//! Christopher Slatt with the goal of bringing firewall protection to
//! the small + medium business segment that was being underserved by
//! the enterprise-priced Check Point + Cisco PIX products of the era.
//! Took private 2006 by Francisco Partners + Vector Capital. Acquired
//! Panda Security (Bilbao Spain endpoint vendor) in 2020 for the
//! endpoint protection + EDR portfolio, then DataPatrol + CyGlass for
//! the rest of the security stack. As of the mid-2020s WatchGuard is
//! owned by Vector Capital + others through multiple private-equity
//! rounds; the playbook is unapologetically channel + MSP-led.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — WatchGuard mid-market firewall + EDR + MFA + Wi-Fi personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Slatt 1996 Seattle; private-equity-owned mid-market security");
    println!("    firebox       Firebox firewall appliances + Fireware OS");
    println!("    epdr          EPDR endpoint protection + EDR — Panda Security 2020 acq");
    println!("    authpoint     AuthPoint multi-factor authentication cloud service");
    println!("    wifi          WatchGuard secure Wi-Fi access points + WIPS");
    println!("    channel       MSP-first channel motion + ConnectWise + Datto integrations");
    println!("    pricing       Subscription bundle: Basic + Standard + Total Security");
    println!("    customers     SMB + mid-market + MSPs + dispersed-branch retail + education");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("watchguard-cli 0.1.0 (mid-market-security-bundle personality build)"); }

fn run_about() {
    println!("WatchGuard Technologies, Inc.");
    println!("  Founded:    1996, Seattle, Washington as Seattle Software Labs.");
    println!("  Founder:    Christopher Slatt + early team.");
    println!("  Renamed:    WatchGuard Technologies in 1997.");
    println!("  IPO:        listed Nasdaq:WGRD 1999; taken private 2006.");
    println!("  Owners:     Vector Capital + Francisco Partners + others through PE rounds.");
    println!("  Acquisitions: Panda Security (Bilbao Spain, 2020) for endpoint + EDR, plus");
    println!("                CyGlass + DataPatrol + Manito Networks for the rest of the stack.");
    println!("  Position:   mid-market security bundle vendor sold heavily through MSP partners.");
    println!("  Anti-segment: the Palo Alto + Fortinet + Check Point Fortune-500 firewall niche.");
}

fn run_firebox() {
    println!("Firebox (the flagship firewall product line).");
    println!("  Firebox T-series: SMB + branch desktop appliances from T15 up to T85.");
    println!("  Firebox M-series: mid-market rack-mount + chassis from M270 to M5800.");
    println!("  FireboxV / FireboxCloud: virtual appliances for VMware + Hyper-V + AWS + Azure.");
    println!("  Fireware OS: WatchGuard's proprietary OS shared across the firewall line.");
    println!("  Features: NGFW + IPS + URL filtering + APT Blocker sandbox + DNSWatch + DLP +");
    println!("            ThreatSync XDR feed + Mobile VPN + SSL VPN + BOVPN site-to-site.");
    println!("  Cloud-managed via WatchGuard Cloud or on-premises via WatchGuard System Manager.");
}

fn run_epdr() {
    println!("EPDR — Endpoint Protection + Detection + Response (Panda Security heritage).");
    println!("  Acquired 2020 from Panda Security (Bilbao, Spain) — established 1990 antivirus");
    println!("  vendor with the unusual 'attestation' approach to threat classification.");
    println!("  Adaptive Defense 360: zero-trust attestation model — every binary on every");
    println!("  endpoint is classified as known-good, known-bad, or pending classification by");
    println!("  WatchGuard's cloud labs — pending binaries can be blocked by default policy.");
    println!("  EDR: process tree visualisation + threat-hunting search across the fleet.");
    println!("  ThreatSync XDR: correlate endpoint + firewall + identity telemetry across the");
    println!("  WatchGuard portfolio for a unified incident view.");
}

fn run_authpoint() {
    println!("AuthPoint (cloud multi-factor authentication).");
    println!("  Push-notification MFA via the AuthPoint mobile app (iOS + Android).");
    println!("  OTP + hardware-token MFA for cases where push is impractical.");
    println!("  RADIUS + SAML integrations with VPN gateways, web apps, network devices.");
    println!("  Mobile-device DNA: device fingerprint binds tokens to a specific device so a");
    println!("  stolen seed cannot be reused on a different phone.");
    println!("  Risk-based authentication + geofencing + time-based policies.");
    println!("  Pricing tuned to the MSP per-seat economics — common bundled with EPDR + Firebox.");
}

fn run_wifi() {
    println!("Secure Wi-Fi.");
    println!("  WatchGuard Access Points: indoor + outdoor + Wi-Fi 6 / 6E hardware lineup.");
    println!("  WatchGuard Wi-Fi Cloud: cloud-managed wireless controller + analytics.");
    println!("  Wireless Intrusion Prevention System (WIPS): rogue + evil-twin AP detection.");
    println!("  Captive portals + guest access management for retail + hospitality.");
    println!("  Marketing analytics: footfall + dwell-time reporting from anonymised RSSI data.");
    println!("  Common deployment: retail chains, hotels, schools, distributed branch offices.");
}

fn run_channel() {
    println!("MSP + channel motion (the strategic core).");
    println!("  ~95%+ of revenue flows through resellers + MSP partners.");
    println!("  WatchGuard Cloud: multi-tenant management plane purpose-built for MSPs to");
    println!("                    administer hundreds of customer tenants from one console.");
    println!("  Integrations: deep ConnectWise PSA + Datto RMM + Kaseya hooks for ticketing,");
    println!("                billing, monitoring + remote remediation workflows.");
    println!("  WatchGuardONE: tiered partner programme (Silver / Gold / Platinum) with");
    println!("                 deal-registration, MDF, training, certifications.");
    println!("  Common partner: regional IT services + MSPs serving 50-500 SMB customers.");
    println!("  Strategy: be the 'easy single bill + single pane' bundle that an MSP can carry.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Subscription bundles applied to Firebox appliances:");
    println!("    Basic Security Suite:    base NGFW + IPS + URL filtering + spamBlocker.");
    println!("    Standard Security Suite: adds APT Blocker + DNSWatch + Threat Detection.");
    println!("    Total Security Suite:    adds EPDR endpoint + AuthPoint MFA + Wi-Fi included.");
    println!("  Per-seat / per-endpoint pricing for EPDR + AuthPoint outside of bundles.");
    println!("  MSP monthly subscription billing model — explicit alternative to perpetual");
    println!("  licenses + multi-year upfront contracts of larger vendors.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: SMB + mid-market customers 20-2,000 employees who buy security");
    println!("  through their MSP, not from a Fortune-500 vendor directly.");
    println!("  Industries: regional banking + credit unions, healthcare clinics, retail +");
    println!("  hospitality chains, school districts, local government, manufacturing,");
    println!("  professional services, dispersed-branch operations.");
    println!("  Geographic: heavy US + EU + LATAM (Panda heritage helps); growing APAC.");
    println!("  Channel: 95%+ MSP + reseller + distributor sourced revenue.");
    println!("  Common pitch: 'one vendor for firewall + endpoint + MFA + Wi-Fi billed monthly");
    println!("  by your MSP — no Palo Alto pricing, no Fortune-500 procurement complexity'.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "watchguard-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "firebox" => run_firebox(),
        "epdr" => run_epdr(),
        "authpoint" => run_authpoint(),
        "wifi" => run_wifi(),
        "channel" => run_channel(),
        "pricing" => run_pricing(),
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
        run_firebox();
        run_epdr();
        run_authpoint();
        run_wifi();
        run_channel();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("watchguard-cli");
        print_version();
    }
}
