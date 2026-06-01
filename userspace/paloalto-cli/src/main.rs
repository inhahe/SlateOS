#![deny(clippy::all)]
//! paloalto-cli — personality CLI for Palo Alto Networks, the
//! next-generation firewall + cloud security + endpoint platform.
//!
//! Founded 2005 in Santa Clara by Nir Zuk (CTO, ex-Check Point + ex-
//! NetScreen) along with Yuming Mao + Rajiv Batra. Zuk had previously
//! co-invented stateful inspection while at Check Point in the 1990s,
//! and Palo Alto Networks was his bet that next-generation firewalls
//! needed to operate at the application layer rather than just at
//! ports + protocols. IPO 2012 NYSE:PANW; under CEO Nikesh Arora (joined
//! 2018 from SoftBank) the company has aggressively rolled up the
//! security category — Prisma Cloud, Cortex XDR + XSIAM endpoint
//! security, Unit 42 incident response — into a single platform-style
//! consolidation strategy. Market cap routinely exceeds $100B, one of
//! the largest pure-play cybersecurity companies in the world.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Palo Alto Networks next-gen firewall + security personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Nir Zuk + Yuming Mao + Rajiv Batra 2005 Santa Clara; PANW");
    println!("    ngfw          Next-generation firewall: App-ID + User-ID + Content-ID");
    println!("    prisma        Prisma Cloud + Prisma Access + Prisma SASE platform");
    println!("    cortex        Cortex XDR + XSIAM + XSOAR endpoint + SOC platform");
    println!("    unit42        Unit 42 threat-intel + incident-response consulting arm");
    println!("    rollup        Aggressive M&A roll-up of point-product security companies");
    println!("    financials    NYSE:PANW; >\\$100B market cap; Nikesh Arora as CEO");
    println!("    customers     Fortune 500 + governments + every regulated industry");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("paloalto-cli 0.1.0 (next-gen-firewall-platform personality build)"); }

fn run_about() {
    println!("Palo Alto Networks, Inc.");
    println!("  Founded:    2005, Santa Clara, California.");
    println!("  Founders:   Nir Zuk (CTO; ex-Check Point + ex-NetScreen, co-invented");
    println!("              stateful inspection at Check Point in the 1990s),");
    println!("              Yuming Mao (Chief Architect), Rajiv Batra (Engineering).");
    println!("  IPO:        July 2012 on NYSE:PANW.");
    println!("  CEO:        Nikesh Arora since June 2018 (ex-SoftBank president + ex-Google).");
    println!("  Market cap: >\\$100B routinely — one of the world's largest pure-play");
    println!("              cybersecurity companies by capitalisation.");
    println!("  Position:   platform consolidator across firewall + cloud + endpoint +");
    println!("              SOC + SASE + threat intelligence.");
}

fn run_ngfw() {
    println!("Next-Generation Firewall (the founding product line).");
    println!("  PA-Series hardware appliances: from PA-220 SMB to PA-7500 data-centre chassis.");
    println!("  VM-Series virtual firewalls for VMware, AWS, Azure, GCP, OCI, Hyper-V, KVM.");
    println!("  CN-Series containerised firewall for Kubernetes east-west traffic.");
    println!("  App-ID:     application-layer identification of 4,000+ apps regardless of port.");
    println!("  User-ID:    integrate AD + LDAP + SAML to map traffic to users not just IPs.");
    println!("  Content-ID: inline IPS + URL filtering + file-type + spyware + WildFire sandbox.");
    println!("  Panorama:   centralised management of thousands of firewalls + policy push.");
    println!("  Defining the 'next-generation firewall' category circa 2009-2011.");
}

fn run_prisma() {
    println!("Prisma — cloud security + SASE platform.");
    println!("  Prisma Cloud:    Cloud-Native Application Protection Platform (CNAPP) covering");
    println!("                   CSPM + CWP + CIEM + DSPM + Code Security across AWS / Azure /");
    println!("                   GCP / OCI. Originated from RedLock + Bridgecrew acquisitions.");
    println!("  Prisma Access:   global Secure Access Service Edge (SASE) cloud — agents on");
    println!("                   user endpoints route through PA-owned cloud nodes for filtering.");
    println!("  Prisma SD-WAN:   CloudGenix-acquired SD-WAN; integrated with SASE for full");
    println!("                   branch-office connectivity story.");
    println!("  Bundles together as the Strata Cloud Manager + Strata platform offering.");
}

fn run_cortex() {
    println!("Cortex — endpoint + SOC platform.");
    println!("  Cortex XDR:    extended detection + response across endpoint + network + cloud +");
    println!("                 identity telemetry; positioned against CrowdStrike + SentinelOne.");
    println!("  Cortex XSIAM:  AI-driven SIEM/SOAR-replacement product launched 2022;");
    println!("                 ingest all telemetry, autonomous detection + response.");
    println!("  Cortex XSOAR:  security orchestration + automation + response — the");
    println!("                 Demisto acquisition (2019, \\$560M).");
    println!("  Cortex Xpanse: external attack surface management — Expanse acquisition (2020).");
    println!("  The Cortex bet: collapse the SIEM + SOAR + EDR + XDR + ASM tower into one stack.");
}

fn run_unit42() {
    println!("Unit 42 — threat intelligence + incident response.");
    println!("  Threat-intel research team publishing landmark APT + ransomware reports.");
    println!("  Incident response retainer practice: customers can call Unit 42 during a");
    println!("  ransomware event, similar to Mandiant / Kroll positioning.");
    println!("  Crystal Eye, Crypsis (acquired 2020), and other DFIR teams folded in.");
    println!("  Unit 42 work feeds detection rules + threat intel back into Cortex + WildFire.");
    println!("  Brand recognition for Unit 42 publications is comparable to Mandiant / FireEye.");
}

fn run_rollup() {
    println!("Acquisition roll-up strategy.");
    println!("  Highlights (selected): Cyvera 2014 (endpoint), CirroSecure 2015 (cloud),");
    println!("  Demisto 2019 \\$560M (SOAR), Twistlock 2019 \\$410M (container security),");
    println!("  PureSec 2019 (serverless), Aporeto 2019, CloudGenix 2020 \\$420M (SD-WAN),");
    println!("  Crypsis 2020 \\$265M (IR), Bridgecrew 2021 \\$156M (shift-left cloud security),");
    println!("  Expanse 2020 \\$800M (ASM), Talon 2023 (enterprise browser), Dig 2023 (DSPM),");
    println!("  IBM QRadar SaaS assets 2024.");
    println!("  Strategy: own the entire security platform top to bottom + force consolidation");
    println!("  on customers tired of dozens of point-product vendors.");
}

fn run_financials() {
    println!("Financials + leadership.");
    println!("  Listing:       NYSE:PANW since July 2012 IPO.");
    println!("  Market cap:    routinely >\\$100B (sometimes >\\$130B at peaks).");
    println!("  Revenue:       ~\\$8B annual run-rate as of recent fiscal years + growing.");
    println!("  CEO:           Nikesh Arora since 2018 (\\$128M+ compensation package, one of");
    println!("                 the largest in software); architect of the Cortex + Prisma");
    println!("                 platform pivot + most of the acquisitions above.");
    println!("  CTO:           Nir Zuk (co-founder, still active).");
    println!("  Headcount:     ~15,000 globally + growing.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: Fortune 500 + Global 2000 enterprises + governments + service");
    println!("  providers + every regulated industry: finance + healthcare + critical");
    println!("  infrastructure + telecoms + manufacturing + retail.");
    println!("  US federal government: very large footprint across DoD + civilian agencies.");
    println!("  Service providers + MSSPs: large channel + partner ecosystem.");
    println!("  Geographic: heavy US + EU + Middle East + APAC; emerging-market presence.");
    println!("  Sales motion: enterprise direct + channel + Unit 42 IR engagements as a");
    println!("  high-credibility 'land' that converts into Cortex + Prisma + NGFW 'expand'.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "paloalto-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "ngfw" => run_ngfw(),
        "prisma" => run_prisma(),
        "cortex" => run_cortex(),
        "unit42" => run_unit42(),
        "rollup" => run_rollup(),
        "financials" => run_financials(),
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
        run_ngfw();
        run_prisma();
        run_cortex();
        run_unit42();
        run_rollup();
        run_financials();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("paloalto-cli");
        print_version();
    }
}
