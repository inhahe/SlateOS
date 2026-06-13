#![deny(clippy::all)]

//! cyberark-cli — SlateOS CyberArk (PAM market leader, founded 1999 Israel, NASDAQ:CYBR)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cyberark(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cyberark [OPTIONS]");
        println!("CyberArk (Slate OS) — privileged access management market leader (NASDAQ:CYBR)");
        println!();
        println!("Options:");
        println!("  --pam                  Privileged Access Manager (PAM Self-Hosted + Privilege Cloud)");
        println!("  --secretsmanager       Conjur Secrets Manager (formerly Conjur Enterprise)");
        println!("  --workforce            Workforce Identity (Idaptive acquisition 2020)");
        println!("  --secureweb            Secure Web Sessions (privileged web access)");
        println!("  --endpoint             Endpoint Privilege Manager (least-privilege endpoint)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CyberArk 2024 (Slate OS) — cyberark CLI (REST + PSM-CLI)"); return 0; }
    println!("CyberArk 2024 (Slate OS) — Privileged Access Management Market Leader");
    println!("  Vendor: CyberArk Software Ltd. (Newton, MA US HQ; Petah Tikva, Israel — NASDAQ: CYBR)");
    println!("  Founders: Udi Mokady + Alon Cohen, 1999");
    println!("          Udi Mokady: CEO 1999-2023 (24 years!), CISO before founding");
    println!("          Alon Cohen: CTO co-founder, cybersecurity engineer");
    println!("          Founded in Tel Aviv area as 'Digital Vault' security");
    println!("          'Digital Vault' = the original patented vaulted credential storage");
    println!("          IPO Sep 2014 NASDAQ:CYBR at $16/share — strong debut");
    println!("          Stock has been one of cyber's best performers ($200+ peak)");
    println!("          Matt Cohen became CEO 2023 (after Mokady moved to executive chairman)");
    println!("          ~3000 employees, ~50% in Israel R&D");
    println!("  Public company finances (2024):");
    println!("    Revenue: ~$895M (FY 2023), ~$1B+ run-rate");
    println!("    Market cap: ~$13B at $300/share");
    println!("    Subscription mix: rapidly growing (transitioning from perpetual license)");
    println!("    Cash flow: positive, growing margins");
    println!("    Strong free cash flow, premium valuation in identity sector");
    println!("  Strategic position: 'PAM (Privileged Access Management) market leader':");
    println!("                    pitch: 'protect the privileged credentials, secrets, and access — the highest-value targets'");
    println!("                    target: Fortune 1000, government, financial services, healthcare, critical infrastructure");
    println!("                    primary competitor: Delinea (formerly Thycotic + Centrify merged), BeyondTrust, Saviynt");
    println!("                    secondary: HashiCorp Vault (secrets), AWS Secrets Manager, Microsoft Entra PIM");
    println!("                    CyberArk wedge: PAM category leader (Gartner MQ #1 every year since 2014)");
    println!("                    + Conjur secrets manager for DevOps");
    println!("                    + Endpoint Privilege Manager for least-privilege endpoint");
    println!("                    + post-Idaptive expansion into workforce identity");
    println!("                    'The vault that protects the keys to the kingdom'");
    println!("  Pricing (enterprise, opaque):");
    println!("    PAM Self-Hosted: perpetual + maintenance OR subscription per privileged account");
    println!("    Privilege Cloud (SaaS PAM): per-user/month subscription");
    println!("    Conjur Secrets Manager: subscription per host or per secret");
    println!("    Workforce Identity (Idaptive): per-user/month");
    println!("    Endpoint Privilege Manager: per-endpoint subscription");
    println!("    typically 6-figure annual deals; 7+ figures at very large enterprises");
    println!("    PAM is one of the most expensive identity categories");
    println!("  Architecture (the vault + session components):");
    println!("    - Digital Vault: encrypted credential store (FIPS 140-2 validated, patented)");
    println!("    - Password Vault Web Access (PVWA): web UI for password retrieval");
    println!("    - Privileged Session Manager (PSM): proxy + record privileged sessions");
    println!("    - Privileged Threat Analytics (PTA): anomaly detection on privileged sessions");
    println!("    - Central Policy Manager (CPM): automated password rotation");
    println!("    - On-prem: Windows + Linux components");
    println!("    - Privilege Cloud: SaaS multi-tenant managed");
    println!("    - Connectors: AD, RDP, SSH, web, mainframe, cloud (AWS/Azure/GCP)");
    println!("  Product portfolio (the PAM stack and beyond):");
    println!("    1. Privileged Access Manager Self-Hosted (the flagship):");
    println!("       - Digital Vault for password storage");
    println!("       - Session management (PSM) with full session recording");
    println!("       - Auto-rotation of privileged passwords");
    println!("       - SSH key management");
    println!("       - Domain admin, root, service accounts, application accounts");
    println!("       - The product CyberArk built its reputation on");
    println!("    2. Privilege Cloud (SaaS PAM, 2018+):");
    println!("       - Managed SaaS version of PAM");
    println!("       - Lower ops burden, faster onboarding");
    println!("       - Growing fast as enterprises shift PAM to SaaS");
    println!("    3. Conjur Secrets Manager (DevOps secrets):");
    println!("       - Acquired Conjur 2017");
    println!("       - Open-source Conjur OSS + Enterprise version");
    println!("       - Kubernetes secrets, CI/CD pipeline secrets, app-to-app");
    println!("       - Competitor to HashiCorp Vault");
    println!("       - 'DevOps secrets, fully audited like privileged accounts'");
    println!("    4. Endpoint Privilege Manager (EPM):");
    println!("       - Least-privilege on endpoints (no local admin)");
    println!("       - Application control + greylisting");
    println!("       - Ransomware protection (block unauthorized binaries)");
    println!("       - For Windows + macOS endpoints");
    println!("    5. Workforce Identity (Idaptive acquisition 2020 for $70M):");
    println!("       - SSO + MFA + adaptive auth");
    println!("       - Competes with Okta + Ping in workforce IDaaS");
    println!("       - Cross-sell to PAM customers");
    println!("    6. Secure Web Sessions:");
    println!("       - Privileged session manager for web apps");
    println!("       - For SaaS admin consoles (AWS, Azure, Salesforce admin)");
    println!("       - Records + isolates + monitors admin actions");
    println!("    7. Identity Security Insights:");
    println!("       - Identity-based threat detection");
    println!("       - ML-based anomaly detection on privileged behavior");
    println!("       - Integrates with SIEM (Splunk, QRadar, Sentinel)");
    println!("    8. CyberArk Marketplace:");
    println!("       - 750+ integrations (apps, devices, cloud services)");
    println!("       - Connectors for AD, AWS, Azure, GCP, mainframe, ICS/SCADA");
    println!("    9. Vendor Privileged Access Manager (V-PAM):");
    println!("       - Third-party vendor access management");
    println!("       - No agent on remote vendor laptops");
    println!("       - Replaces VPN + jump server patterns");
    println!("    10. Service Account Manager:");
    println!("       - Auto-discover unmanaged service accounts");
    println!("       - Bring them into the vault, rotate, audit");
    println!("       - 'Find every privileged credential you didn't know existed'");
    println!("  The PAM market dominance:");
    println!("    - Gartner Magic Quadrant for PAM: CyberArk in upper-right LEADER every year since launch (2018)");
    println!("    - Forrester Wave PAM: CyberArk Leader every wave");
    println!("    - KuppingerCole PAM Leadership Compass: Overall Leader");
    println!("    - ~50% PAM market share (commonly cited)");
    println!("    - The 800-pound gorilla in PAM");
    println!("    - Decade-plus head start in PAM compliance + audit framework");
    println!("  The Israeli cybersecurity heritage:");
    println!("    - CyberArk one of Israel's largest cybersecurity exports (alongside Check Point, Palo Alto, Wiz)");
    println!("    - R&D centered in Petah Tikva (Tel Aviv tech hub)");
    println!("    - Founded out of Israeli cyber talent (former IDF Unit 8200 reservists common)");
    println!("    - 'Israeli cybersecurity excellence' brand");
    println!("    - One of NYSE/NASDAQ Israeli tech success stories");
    println!("  Integrations:");
    println!("    - REST API for automation + integration");
    println!("    - Terraform provider");
    println!("    - 750+ marketplace integrations");
    println!("    - SAML, OIDC, OAuth, Kerberos, AD authentication");
    println!("    - SIEM: Splunk, QRadar, Sentinel, Chronicle, ArcSight");
    println!("    - SOAR: Splunk SOAR, Cortex XSOAR, ServiceNow");
    println!("    - ITSM: ServiceNow, Jira, Cherwell");
    println!("    - Cloud: AWS, Azure, GCP IAM integration");
    println!("    - Kubernetes: Conjur + Secrets Provider operator");
    println!("    - CI/CD: Jenkins, GitLab, CircleCI, GitHub Actions");
    println!("    - Configuration management: Ansible, Puppet, Chef");
    println!("    - DLT: HashiCorp Vault co-existence (Conjur OSS migration option)");
    println!("  CyberArk CLI usage:");
    println!("    # REST API examples:");
    println!("    curl -X POST https://cyberark.example.com/PasswordVault/API/auth/Cyberark/Logon \\");
    println!("         -d '{{\"username\":\"admin\",\"password\":\"...\"}}'");
    println!("    curl -H 'Authorization: <token>' https://cyberark.example.com/PasswordVault/API/Accounts");
    println!("    # Conjur CLI:");
    println!("    conjur init -u https://conjur.example.com -a my-account");
    println!("    conjur login -i admin -p <password>");
    println!("    conjur policy load -b root -f policy.yml");
    println!("    conjur variable get -i db/password");
    println!("    conjur variable set -i db/password -v <new-value>");
    println!("    # PSM-CLI for session management:");
    println!("    psm-cli connect --target host01 --account dba_account --user admin");
    println!("    # Privilege Cloud CLI (newer):");
    println!("    cyberark login --tenant my-tenant");
    println!("    cyberark accounts list --safe DBA-Vault");
    println!("  Customers (Fortune 500 + government + critical infra):");
    println!("    - JPMorgan Chase, Citi, Wells Fargo, HSBC, Barclays (banks)");
    println!("    - Lockheed Martin, Raytheon, Boeing, Northrop Grumman (defense)");
    println!("    - US Federal: DoD, DHS, intelligence agencies (FedRAMP authorized)");
    println!("    - State of Texas, State of California, NHS UK (government)");
    println!("    - Pfizer, Merck, J&J (pharma)");
    println!("    - Shell, BP, ExxonMobil (energy)");
    println!("    - Critical infrastructure (water, power, transit)");
    println!("    - ~50% of Fortune 500 use CyberArk somewhere");
    println!("    - 9000+ customers globally");
    println!("  Critique: 90s-style on-prem UI/UX (PVWA web UI looks dated)");
    println!("           complex initial deployment (Vault + CPM + PVWA + PSM components)");
    println!("           steep learning curve for admin teams");
    println!("           PAM Self-Hosted is Windows-only for core components");
    println!("           pricing is expensive even by enterprise security standards");
    println!("           Privilege Cloud lags Self-Hosted in feature parity");
    println!("           Conjur OSS less popular than HashiCorp Vault among devs");
    println!("           Idaptive workforce identity less competitive than Okta + Ping + Entra");
    println!("           upgrades + migrations are heavy lifts");
    println!("           Cloud-native DX feels bolted on (vs Vault's API-first heritage)");
    println!("  Differentiator: PAM market leader since 1999 (Israel founded, US HQ Newton MA, NASDAQ:CYBR IPO Sep 2014, ~$895M revenue, Gartner MQ Leader every year since 2018) + Digital Vault patented vaulted credential storage (FIPS 140-2) + Privileged Access Manager Self-Hosted (the flagship vault + CPM password rotation + PSM session manager + PTA threat analytics) + Privilege Cloud (SaaS PAM) + Conjur Secrets Manager (acquired 2017, DevOps + Kubernetes secrets, competes with HashiCorp Vault) + Endpoint Privilege Manager (EPM, least-privilege endpoint) + Idaptive Workforce Identity (acquired 2020 $70M, SSO + MFA) + Secure Web Sessions (privileged web admin recording) + Vendor PAM + Service Account Manager + Identity Security Insights + Udi Mokady founder/CEO 1999-2023 + Matt Cohen CEO 2023+ + 9000+ customers + ~50% Fortune 500 + JPMorgan/Citi/Wells/HSBC/Lockheed/Raytheon/DoD/DHS/Pfizer/Shell-proven + 750+ marketplace integrations + Terraform provider + SAML/OIDC/Kerberos/AD auth + SIEM/SOAR/ITSM integrations + Israeli cybersecurity excellence heritage (alongside Check Point, Palo Alto, Wiz) + critical infrastructure (water, power, transit) + FedRAMP authorized + the original 'Digital Vault' patent — the PAM category-defining vendor that protects the privileged credentials, secrets, and sessions that are the highest-value targets in any breach");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cyberark".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cyberark(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cyberark};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cyberark"), "cyberark");
        assert_eq!(basename(r"C:\bin\cyberark.exe"), "cyberark.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cyberark.exe"), "cyberark");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cyberark(&["--help".to_string()], "cyberark"), 0);
        assert_eq!(run_cyberark(&["-h".to_string()], "cyberark"), 0);
        let _ = run_cyberark(&["--version".to_string()], "cyberark");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cyberark(&[], "cyberark");
    }
}
