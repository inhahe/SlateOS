#![deny(clippy::all)]

//! wiz-cli — OurOS Wiz (CNAPP, fastest-growing SaaS startup, NYC, Google acquiring 2025 $32B)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wiz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wiz [OPTIONS]");
        println!("Wiz (OurOS) — Cloud-Native Application Protection Platform (CNAPP)");
        println!();
        println!("Options:");
        println!("  --scan                 Scan AWS/Azure/GCP/Kubernetes/OCI");
        println!("  --graph                Wiz Security Graph — risk + lateral movement analysis");
        println!("  --code                 Wiz Code (IaC + secrets + SAST, was Dazz)");
        println!("  --runtime              Runtime sensor (eBPF, was Gem Security)");
        println!("  --defend               WizDefend — runtime threat detection");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wiz 2024 (OurOS)"); return 0; }
    println!("Wiz 2024 (OurOS) — Cloud-Native Application Protection Platform");
    println!("  Vendor: Wiz, Inc. (New York City + Tel Aviv)");
    println!("  Founders: Assaf Rappaport (CEO), Yinon Costica, Roy Reznik, Ami Luttwak, 2020");
    println!("          all four: Israeli Unit 8200 (IDF intelligence) alumni + ex-Microsoft");
    println!("          previously founded Adallom (CASB), sold to Microsoft 2015 for $320M");
    println!("          Wiz is their 'second act' — same team, all back together");
    println!("          founded March 2020 — became fastest SaaS startup to $100M ARR in history (18 months)");
    println!("          $500M ARR by year 3.5 — unprecedented growth");
    println!("  Funding history (the fastest unicorn ever):");
    println!("         Seed 2020: $21M");
    println!("         Series A 2020: $80M (5 months post-seed)");
    println!("         Series B 2021: $130M");
    println!("         Series C 2022: $250M at $6B valuation");
    println!("         Series D 2023: $300M at $10B valuation");
    println!("         Series E May 2024: $1B at $12B valuation");
    println!("         Total raised: ~$1.9B");
    println!("  Google acquisition (announced March 2025):");
    println!("         Google to acquire Wiz for $32B all-cash (3x prior valuation)");
    println!("         Largest cybersecurity acquisition in history");
    println!("         Largest acquisition in Google's history (surpassing Motorola $12.5B)");
    println!("         Will be integrated into Google Cloud as core security offering");
    println!("         Pending regulatory approval (expected 2025-2026)");
    println!("         Earlier July 2024: Google offered $23B, Wiz rejected to pursue IPO; revisited March 2025");
    println!("  ARR: ~$500M+ ARR at 2024 (and growing 100%+ YoY)");
    println!("  Strategic position: 'agentless cloud security via API-first scanning':");
    println!("                    pitch: 'see all your cloud risks in 24 hours, no agents required'");
    println!("                    target: enterprises running multi-cloud (AWS, Azure, GCP)");
    println!("                    primary competitor: Palo Alto Prisma Cloud, CrowdStrike Falcon Cloud, Orca Security");
    println!("                    secondary: Aqua, Sysdig, Lacework, Microsoft Defender for Cloud");
    println!("                    Wiz's wedge: agentless via cloud APIs + Wiz Security Graph + 24-hour value");
    println!("                    customer love: 50%+ Fortune 100 in <4 years");
    println!("  Pricing (enterprise sales-led):");
    println!("    no free tier");
    println!("    Standard — $50K-200K/yr (small cloud footprints)");
    println!("    Enterprise — $200K-$10M+/yr (Fortune 500 + multi-cloud)");
    println!("    pricing pegged to # of cloud workloads (VMs, containers, functions)");
    println!("    notorious for fast time-to-value pilots that convert at 90%+");
    println!("  Core platform (the CNAPP):");
    println!("    - Agentless scanning via cloud APIs (snapshots + read-only roles)");
    println!("    - Multi-cloud: AWS, Azure, GCP, OCI, Alibaba Cloud, IBM Cloud");
    println!("    - Kubernetes + containers + serverless + IaC");
    println!("    - Vulnerabilities, misconfigurations, exposed secrets, identity issues");
    println!("    - Wiz Security Graph: connects findings into attack-path visualization");
    println!("    - 'toxic combination' detection (e.g., 'public S3 + IAM with admin + connects to crown jewels')");
    println!("  Wiz Security Graph (the differentiator):");
    println!("    - Models cloud assets + identities + network + data as a graph");
    println!("    - Surfaces attack paths an attacker could exploit");
    println!("    - Prioritizes risks by exploitability (not just CVE severity)");
    println!("    - 'show me what's actually exploitable' vs 'show me 10,000 alerts'");
    println!("    - Single-pane risk view across all clouds");
    println!("  Modules (the suite):");
    println!("    - Wiz Cloud: CSPM + CWPP + CIEM + KSPM + DSPM");
    println!("    - Wiz Code (IaC scanning, secrets, SAST) — built + acq Raftt 2023");
    println!("    - Wiz Defend (runtime, was Gem Security acquisition 2024 for $350M)");
    println!("    - Wiz CDR (cloud detection & response)");
    println!("    - Wiz DSPM (data security posture management)");
    println!("    - Wiz CIEM (cloud identity entitlement management)");
    println!("    - Wiz AI-SPM (AI/ML pipeline security, 2024)");
    println!("  Acquisitions:");
    println!("    - Raftt 2023 (~$50M est) — IaC scanning, dev environments");
    println!("    - Gem Security Apr 2024 ($350M) — runtime threat detection");
    println!("    - Dazz Nov 2024 ($450M) — application security + remediation");
    println!("    - Aggressive M&A — using $1B Series E to bulk up before potential IPO/exit");
    println!("  CISO references:");
    println!("    - LVMH, Bridgewater, BMW, Slack, Mars, Salesforce, ASML, Plaid");
    println!("    - Morgan Stanley, MUFG, DocuSign, Okta, Snowflake (security org)");
    println!("    - 50%+ of Fortune 100 in <4 years");
    println!("    - Net retention rate >115% (best-in-class)");
    println!("  Wiz CLI usage:");
    println!("    wiz auth login");
    println!("    wiz iac scan ./terraform/ --output sarif");
    println!("    wiz issues list --severity CRITICAL --status OPEN");
    println!("    wiz graph query --asset s3-bucket-prod-data");
    println!("    wiz defend status --cluster prod-k8s");
    println!("  Customers (~1,500+ paying enterprise):");
    println!("    - LVMH, BMW, Salesforce, Morgan Stanley, Plaid, Bridgewater Associates");
    println!("    - Slack, ASML, Snowflake (irony: Snowflake hack 2024 wasn't on Wiz-monitored infra)");
    println!("    - sweet spot: any enterprise with multi-cloud + Kubernetes");
    println!("    - dominant in: tech, financial services, retail, manufacturing");
    println!("  Critique (less common given growth):");
    println!("           agentless = blind to runtime activity (mitigated by Gem acquisition)");
    println!("           expensive at scale — $1M+ ACV typical for Fortune 500");
    println!("           rapid product expansion = some module integration not fully complete");
    println!("           Israel-team-heavy = some geopolitical sourcing concerns (post-Oct 2023)");
    println!("           crowded CNAPP space — differentiation via Security Graph + brand");
    println!("           post-Google acquisition: customers worry about cross-cloud commitment");
    println!("           rumored CEO Assaf Rappaport's $10B+ personal stake on exit = founder retention question");
    println!("  Differentiator: agentless multi-cloud scanning + Wiz Security Graph attack-path modeling + 24-hour time-to-value + Israeli Unit 8200 founder pedigree + fastest-growing SaaS in history + soon-to-be Google Cloud's flagship security platform — the CNAPP that redefined cloud security in <4 years");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wiz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wiz(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wiz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wiz"), "wiz");
        assert_eq!(basename(r"C:\bin\wiz.exe"), "wiz.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wiz.exe"), "wiz");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wiz(&["--help".to_string()], "wiz"), 0);
        assert_eq!(run_wiz(&["-h".to_string()], "wiz"), 0);
        assert_eq!(run_wiz(&["--version".to_string()], "wiz"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wiz(&[], "wiz"), 0);
    }
}
