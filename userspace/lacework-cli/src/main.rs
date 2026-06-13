#![deny(clippy::all)]

//! lacework-cli — Slate OS Lacework (cloud security, San Jose, acquired by Fortinet 2024)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lacework [OPTIONS]");
        println!("Lacework (Slate OS) — cloud security platform (acquired by Fortinet Aug 2024)");
        println!();
        println!("Options:");
        println!("  --polygraph            Polygraph Data Platform — behavioral baseline");
        println!("  --posture              CSPM + KSPM");
        println!("  --workload             Container + host runtime");
        println!("  --code                 IaC scanning + secrets + SAST");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lacework 2024 (Slate OS)"); return 0; }
    println!("Lacework 2024 (Slate OS) — Cloud Security Platform");
    println!("  Vendor: Lacework, Inc. (San Jose) — ACQUIRED by Fortinet Aug 2024 for ~$150-250M");
    println!("  Founders: Vikram Kapoor + Sanjay Kalra, 2014");
    println!("          Vikram: ex-NextGenJV + EMC + Sun Microsystems infrastructure");
    println!("          Sanjay: ex-Cisco + Aruba networking security");
    println!("          founded as 'breach detection for cloud' — early CNAPP-adjacent vendor");
    println!("  Funding history (cautionary tale of overvaluation):");
    println!("         Series D Nov 2021: $1.3B led by Sutter Hill + Altimeter + Snowflake");
    println!("         valuation at peak: $8.3B");
    println!("         Total raised pre-down-round: ~$1.85B");
    println!("         Series E Aug 2023: down round — undisclosed but at fraction of peak ~$1-2B");
    println!("         layoffs 2022 (300 employees) + 2023 (20% staff)");
    println!("         classic 2021 zirp valuation hangover");
    println!("  Fortinet acquisition Aug 2024:");
    println!("         Fortinet announced acquisition (terms undisclosed)");
    println!("         Estimated $150-250M deal value — vast loss for late-stage investors");
    println!("         Strategic fit: Fortinet wanted CNAPP capability to compete with Palo Alto Prisma + Crowdstrike");
    println!("         Fortinet stock NASDAQ:FTNT (one of the largest pure-play security companies)");
    println!("         Lacework operates as 'Fortinet Lacework Cloud Security'");
    println!("         Co-founders departed during acquisition transition");
    println!("  Strategic position (pre-acquisition):");
    println!("                    pitch: 'Polygraph Data Platform — anomaly detection via behavioral baselines'");
    println!("                    target: cloud-native engineering teams + cloud-first companies");
    println!("                    primary competitor: Wiz, Palo Alto Prisma Cloud, CrowdStrike Falcon Cloud, Orca");
    println!("                    Lacework's wedge: Polygraph behavioral ML + Snowflake-native architecture");
    println!("                    lost ground to Wiz 2022-2024 due to slower agentless transition + product gaps");
    println!("                    'we got Wizzed' became industry shorthand for Lacework's competitive struggles");
    println!("  Pricing (pre-Fortinet integration):");
    println!("    Enterprise — $50K-$2M+/yr typical");
    println!("    Pricing per workload + per cloud account");
    println!("    More expensive than Wiz at comparable scale (a contributor to lost deals)");
    println!("    Post-acquisition: bundled into Fortinet Security Fabric pricing");
    println!("  Polygraph Data Platform (the technology bet):");
    println!("    - Behavioral baseline of every workload + process + connection");
    println!("    - Anomaly detection vs known-good behavior (not signature-based)");
    println!("    - Snowflake-native data architecture (Lacework ran on Snowflake)");
    println!("    - 'show me what changed' approach to runtime security");
    println!("    - Strong tech for detection — but UX + onboarding slower than Wiz");
    println!("  CSPM + CIEM + Vulnerability Management:");
    println!("    - Configuration drift detection");
    println!("    - Identity entitlement analysis");
    println!("    - Vulnerability scanning across containers + VMs");
    println!("    - Compliance frameworks: SOC 2, PCI-DSS, HIPAA, ISO 27001, CIS");
    println!("  Container + Kubernetes:");
    println!("    - Container image scanning");
    println!("    - K8s admission control");
    println!("    - Runtime behavior monitoring (eBPF-based agent)");
    println!("    - Pod-level network visibility");
    println!("  IaC + Code:");
    println!("    - Terraform + CloudFormation + ARM template scanning");
    println!("    - Secrets detection");
    println!("    - SAST integration");
    println!("    - GitHub/GitLab/Bitbucket integrations");
    println!("  Integrations:");
    println!("    - Clouds: AWS, Azure, GCP, OCI");
    println!("    - SIEM: Splunk, Microsoft Sentinel, Sumo Logic, Datadog");
    println!("    - Ticketing: Jira, ServiceNow, PagerDuty");
    println!("    - CI/CD: Jenkins, GitHub Actions, GitLab, CircleCI");
    println!("    - SSO: Okta, Azure AD, Google");
    println!("  Lacework CLI usage:");
    println!("    lacework configure");
    println!("    lacework vulnerability list --severity critical");
    println!("    lacework iac scan ./terraform/");
    println!("    lacework compliance evaluate --framework cis-aws");
    println!("    lacework policy list --type runtime");
    println!("  Customers (pre-acquisition, ~1,500 paying):");
    println!("    - VMware, Snowflake (which also invested $$$), Snap, BlueVoyant, Drift");
    println!("    - Many cloud-native scale-ups + some Fortune 500");
    println!("    - sweet spot: cloud-native engineering orgs (less Fortune 500 dominance than Wiz)");
    println!("    - heavy in: tech, SaaS, fintech, healthcare");
    println!("    - Post-Fortinet: customers migrating + new wins flow through Fortinet sales");
    println!("  Critique (legacy + acquisition era):");
    println!("           lost to Wiz on agentless + product velocity 2022-2024");
    println!("           valuation collapse from $8.3B (2021) to <$300M (Fortinet acq) — 96%+ drop");
    println!("           management churn — co-founders departed pre/post acquisition");
    println!("           UX behind Wiz + Orca");
    println!("           Polygraph was great tech but couldn't overcome slow product execution");
    println!("           Fortinet integration may take 1-2 years for full Security Fabric fit");
    println!("           customers worry about Fortinet bundling pricing pressure");
    println!("           cautionary tale of late-stage zirp overvaluation in security");
    println!("  Differentiator (legacy + Fortinet era): Polygraph behavioral ML approach + Snowflake-native data architecture + now backed by Fortinet's Security Fabric distribution and customer base — the cloud security platform absorbed into a major network-security vendor's portfolio");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lacework".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lacework"), "lacework");
        assert_eq!(basename(r"C:\bin\lacework.exe"), "lacework.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lacework.exe"), "lacework");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lw(&["--help".to_string()], "lacework"), 0);
        assert_eq!(run_lw(&["-h".to_string()], "lacework"), 0);
        let _ = run_lw(&["--version".to_string()], "lacework");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lw(&[], "lacework");
    }
}
