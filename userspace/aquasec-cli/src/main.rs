#![deny(clippy::all)]

//! aquasec-cli — OurOS Aqua Security (container/Kubernetes security, Israel + Burlington MA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aqua(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aquasec [OPTIONS]");
        println!("Aqua Security (OurOS) — container + cloud-native security platform");
        println!();
        println!("Options:");
        println!("  trivy SCAN_TARGET      Trivy (Aqua's open-source scanner)");
        println!("  --enforce              Aqua Enforcer (runtime, eBPF + drift prevention)");
        println!("  --tracee               Tracee — eBPF-based runtime security (OSS)");
        println!("  --cspm                 Cloud Security Posture Management");
        println!("  --supply-chain         Software supply chain security");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Aqua Security 2024 + Trivy 0.55 (OurOS)"); return 0; }
    println!("Aqua Security 2024 (OurOS) — Cloud-Native Security");
    println!("  Vendor: Aqua Security Software Ltd. (Ramat Gan, Israel + Burlington, MA)");
    println!("  Founders: Dror Davidoff (CEO) + Amir Jerbi (CTO) + Rami Sass + Idan Plotnik, 2015");
    println!("          Dror: ex-CA Technologies + Mercury Interactive");
    println!("          Amir: ex-CA + serial security entrepreneur");
    println!("          founded as 'Scalock' to secure Docker containers (rebranded to Aqua 2016)");
    println!("          one of the original three 'container security' vendors (with Twistlock + StackRox)");
    println!("          Twistlock acquired by Palo Alto (2019, $410M), StackRox by Red Hat (2021, $250M)");
    println!("          Aqua remained independent — last big container-security pure-play");
    println!("  Funding: ~$325M total");
    println!("         Series E Mar 2021: $135M at $1B+ valuation (became unicorn)");
    println!("         Series D 2020: $30M");
    println!("         Series C 2019: $62M");
    println!("         earlier: Lightspeed, Microsoft Ventures, Insight, IBM Ventures, TLV Partners");
    println!("         private + profitable as of 2024");
    println!("  Strategic position: 'cloud-native security from build to runtime — open-source DNA':");
    println!("                    pitch: 'protect containers, K8s, serverless, and cloud — across the lifecycle'");
    println!("                    target: cloud-native engineering teams + Kubernetes operators");
    println!("                    primary competitor: Wiz, Palo Alto Prisma Cloud, CrowdStrike Falcon Cloud, Sysdig");
    println!("                    Aqua's wedge: deepest container/K8s heritage + OSS (Trivy + Tracee) leadership");
    println!("                    moat: free OSS adoption funnel — Trivy 22K+ stars, default in 80%+ K8s CI scans");
    println!("                    challenge: CSPM/agentless features behind Wiz");
    println!("  Pricing:");
    println!("    Trivy OSS — FREE, Apache 2.0 (the world's most-used container vulnerability scanner)");
    println!("    Aqua Cloud Native Security Platform — $50K-$2M+/yr enterprise");
    println!("    Aqua Enterprise — $100K-$5M+/yr for full lifecycle + compliance");
    println!("    Aqua Tracee — FREE OSS (runtime detection via eBPF)");
    println!("    pricing pegged to workloads + nodes + cloud accounts");
    println!("  Trivy (the OSS hit — 22K+ GitHub stars):");
    println!("    - Single-binary container/IaC/dependency scanner");
    println!("    - Vulnerability DB updated daily (sourced from NVD, GHSA, OS vendors)");
    println!("    - Scans: container images, K8s manifests, Terraform, Dockerfile, CycloneDX SBOMs");
    println!("    - Default scanner in: GitHub Container Registry, Harbor, AWS ECR, GCP Artifact Registry");
    println!("    - Massive funnel for commercial Aqua sales");
    println!("    - 60M+ downloads/year");
    println!("  Tracee (the OSS eBPF tool — 4K+ stars):");
    println!("    - Runtime security via eBPF event tracking");
    println!("    - Detects: container escapes, privilege escalation, suspicious syscalls");
    println!("    - Rego policy language for custom rules");
    println!("    - Foundation for commercial Aqua Runtime Protection");
    println!("  Commercial platform (the Cloud-Native Security suite):");
    println!("    1. Aqua Repository Scanning — image scanning in registries (Trivy-powered + commercial features)");
    println!("    2. Aqua CSPM — multi-cloud posture management (AWS/Azure/GCP)");
    println!("    3. Aqua KSPM — Kubernetes security posture");
    println!("    4. Aqua Enforcer — runtime protection agent");
    println!("       - eBPF-based syscall monitoring");
    println!("       - Drift prevention (block deviations from baseline)");
    println!("       - Behavioral profiling");
    println!("    5. Aqua Supply Chain Security — SBOM, signing, attestation, repos");
    println!("    6. Aqua CWP — cloud workload protection (VMs + serverless)");
    println!("    7. Aqua DTA (Dynamic Threat Analysis) — sandbox-based image analysis");
    println!("  Aqua Nautilus research team:");
    println!("    - Original security research on cloud-native attacks");
    println!("    - Discovered HiddenWasp Linux malware, EleKtra-Leak (AWS keys in GitHub), and many K8s attacks");
    println!("    - Strong InfoSec community presence via blog + conference talks");
    println!("  Integrations:");
    println!("    - Registries: ECR, GCR, ACR, Harbor, Docker Hub, JFrog, Quay, GitLab Registry");
    println!("    - CI/CD: Jenkins, GitHub Actions, GitLab CI, CircleCI, Tekton, Argo");
    println!("    - K8s: EKS, GKE, AKS, OpenShift, Rancher, vanilla K8s");
    println!("    - SIEM: Splunk, Sentinel, Sumo Logic, Datadog, ELK");
    println!("    - Ticketing: Jira, ServiceNow, PagerDuty");
    println!("    - Cloud Provider: AWS Security Hub, Microsoft Defender, Google Security Command Center");
    println!("  Aqua CLI usage:");
    println!("    trivy image alpine:latest");
    println!("    trivy fs --security-checks vuln,config ./");
    println!("    trivy k8s --report summary cluster");
    println!("    aqua-cli scan image registry/myapp:v1 --policy strict");
    println!("    aqua-cli enforcer status --cluster prod-k8s");
    println!("    aqua-cli cspm assess --cloud aws --account 123");
    println!("  Customers (~600+ paying enterprise):");
    println!("    - PayPal, ING, JP Morgan, Comcast, NetApp");
    println!("    - Vodafone, Daimler, AmTrust, Saint-Gobain, T-Systems");
    println!("    - U.S. Air Force, NASA, multiple European government customers");
    println!("    - sweet spot: large enterprises with substantial Kubernetes deployments");
    println!("    - heavy in: financial services, telco, government, manufacturing");
    println!("  Critique: ceded CNAPP momentum to Wiz post-2021");
    println!("           CSPM features less polished than agentless competitors (Wiz, Orca)");
    println!("           DevSecOps positioning sometimes too dev-friendly for security-team-led buying");
    println!("           Trivy success cannibalizes some commercial scanning revenue");
    println!("           must compete on full lifecycle vs Wiz's CNAPP simplicity message");
    println!("           Israeli engineering concentration = post-Oct 2023 sourcing scrutiny in some sectors");
    println!("           IPO talked about for years — still private, growth pace below peers");
    println!("  Differentiator: deepest container + Kubernetes security heritage + Trivy OSS distribution (22K+ stars) + Tracee eBPF leadership + Aqua Nautilus research team + full lifecycle build-to-runtime coverage — the cloud-native security platform with the strongest open-source community foundation");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "aquasec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aqua(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aqua};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/aquasec"), "aquasec");
        assert_eq!(basename(r"C:\bin\aquasec.exe"), "aquasec.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("aquasec.exe"), "aquasec");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_aqua(&["--help".to_string()], "aquasec"), 0);
        assert_eq!(run_aqua(&["-h".to_string()], "aquasec"), 0);
        let _ = run_aqua(&["--version".to_string()], "aquasec");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_aqua(&[], "aquasec");
    }
}
