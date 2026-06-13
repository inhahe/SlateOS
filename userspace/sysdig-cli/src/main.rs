#![deny(clippy::all)]

//! sysdig-cli — SlateOS Sysdig (Falco creator, runtime security + monitoring, Davis CA)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sysdig(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sysdig [OPTIONS]");
        println!("Sysdig (Slate OS) — runtime cloud security (Falco creator)");
        println!();
        println!("Options:");
        println!("  csysdig                Interactive top-like view (OSS Sysdig)");
        println!("  --falco                Falco runtime threat detection (CNCF graduated)");
        println!("  --secure               Sysdig Secure (commercial)");
        println!("  --monitor              Sysdig Monitor (Prometheus-compatible)");
        println!("  --runtime-insights     Runtime vulnerability prioritization");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sysdig + Falco 0.39 (Slate OS)"); return 0; }
    println!("Sysdig 2024 (Slate OS) — Runtime Cloud Security");
    println!("  Vendor: Sysdig, Inc. (Davis, CA + Belgrade, Serbia + Tel Aviv)");
    println!("  Founders: Loris Degioanni (CEO/CTO), 2013");
    println!("          Loris: co-creator of Wireshark (original packet capture) — Italian engineer in Davis CA");
    println!("          previously co-founded CACE Technologies (sold to Riverbed 2010)");
    println!("          founded Sysdig to bring 'Wireshark-like deep system visibility' to containers");
    println!("          created Falco (2016) — CNCF runtime security project, graduated 2024");
    println!("          unique: Sysdig is fundamentally a deep systems-engineering company");
    println!("  Funding: ~$745M total");
    println!("         Series G Dec 2021: $350M at $2.5B+ valuation");
    println!("         Series F Apr 2021: $188M (Permira, Third Point, Premji)");
    println!("         earlier: Goldman Sachs, Bain Capital Ventures, Insight Partners, Accel");
    println!("         filed S-1 IPO 2022 — withdrew due to market conditions");
    println!("         private since, IPO expected 2025-2026");
    println!("  ARR: $200M+ (well-positioned for IPO)");
    println!("  Strategic position: 'runtime-first cloud security — see what's actually happening':");
    println!("                    pitch: 'detection + visibility from kernel-level eBPF events'");
    println!("                    target: cloud-native engineering teams running production K8s");
    println!("                    primary competitor: Wiz, Palo Alto Prisma Cloud, CrowdStrike Falcon Cloud, Aqua");
    println!("                    Sysdig's wedge: deep kernel/eBPF expertise + Falco creator + best runtime visibility");
    println!("                    moat: Falco OSS = de-facto runtime detection standard");
    println!("                    challenge: agentless CNAPP (Wiz) pulls some prevention budget away");
    println!("  Pricing:");
    println!("    Falco OSS — FREE, Apache 2.0 (CNCF graduated project)");
    println!("    Sysdig OSS — FREE (Apache 2.0 — csysdig, sysdig command)");
    println!("    Sysdig Secure (commercial) — $50K-$2M+/yr typical");
    println!("    Sysdig Monitor (commercial APM) — $30K-$1M+/yr");
    println!("    Full Cloud Native Platform — $100K-$5M+/yr Fortune 500 deals");
    println!("    pricing pegged to workloads + cloud accounts + ingestion volume");
    println!("  Falco (the CNCF runtime detection engine — 7K+ stars):");
    println!("    - eBPF + kernel module-based event capture");
    println!("    - Rule language: detect privileged container starts, sensitive file reads, etc.");
    println!("    - 'security camera for K8s'");
    println!("    - CNCF Graduated 2024 — one of the most active CNCF security projects");
    println!("    - 50M+ Falco deployments worldwide");
    println!("    - Powering Sysdig Secure + many third-party products");
    println!("  Sysdig OSS (the original tool):");
    println!("    - 'strace + tcpdump + lsof + htop unified' for containers");
    println!("    - csysdig: interactive top-like view of all containers");
    println!("    - chisel scripting for custom analysis");
    println!("    - Records kernel events for post-incident replay");
    println!("    - 7K+ GitHub stars");
    println!("  Sysdig Secure (the commercial product):");
    println!("    - Built on Falco + Sysdig core");
    println!("    - Cloud Detection & Response (CDR)");
    println!("    - Container + K8s + Cloud (multi-layer)");
    println!("    - Risk Spotlight: runtime-informed vulnerability prioritization");
    println!("      → 'only 10% of CVEs are actually loaded into running memory — fix those first'");
    println!("      → unique data point: runtime tells you which vulnerabilities matter");
    println!("    - CSPM + KSPM + CIEM + CNAPP modules");
    println!("    - 555-day forensic record (replay any incident)");
    println!("  Sysdig Monitor (Prometheus + commercial APM):");
    println!("    - Prometheus-compatible metrics ingestion");
    println!("    - PromQL queries at hosted scale (multi-petabyte)");
    println!("    - Compete with: Datadog, New Relic, Grafana Cloud");
    println!("    - Adoption modest vs security side");
    println!("  Sysdig Threat Research Team:");
    println!("    - Original research on cloud-native threats");
    println!("    - Discovered Pumakit Linux rootkit, SCARLETEEL AWS attacks, AmberSquid cloud cryptomining");
    println!("    - Sysdig Threat Report (annual) — closely-watched industry benchmark");
    println!("  Integrations:");
    println!("    - K8s distros: EKS, GKE, AKS, OpenShift, Rancher, vanilla K8s");
    println!("    - Clouds: AWS, Azure, GCP, OCI, IBM Cloud");
    println!("    - SIEM: Splunk, Sentinel, Sumo Logic, Elastic, Datadog");
    println!("    - CI/CD: Jenkins, GitHub, GitLab, CircleCI");
    println!("    - Ticketing: Jira, ServiceNow, PagerDuty, Slack");
    println!("    - Container Registries: ECR, GCR, ACR, Harbor, Docker Hub, JFrog");
    println!("    - Cloud Provider: AWS Security Hub, Microsoft Defender, GCP Security Command Center");
    println!("  Sysdig CLI usage:");
    println!("    sysdig -c topfiles_bytes container=my-app  # OSS — top file I/O per container");
    println!("    csysdig -k <kubeconfig>                    # interactive K8s view");
    println!("    falco -r rules.yaml                        # Falco runtime detection");
    println!("    sysdig-cli login");
    println!("    sysdig-cli scanning evaluate ubuntu:22.04");
    println!("    sysdig-cli risk-spotlight list --severity critical");
    println!("    sysdig-cli compliance evaluate --framework cis-eks");
    println!("  Customers (~800+ paying enterprise):");
    println!("    - Goldman Sachs, BBVA, MercadoLibre, IBM Cloud (internal use)");
    println!("    - SAP, Booking.com, Worldpay, Comcast, T-Mobile");
    println!("    - U.S. Air Force, NSA, NIH, multiple federal customers");
    println!("    - sweet spot: cloud-native engineering teams with K8s in production");
    println!("    - heavy in: financial services, government, large-tech");
    println!("  Critique: complex to operate at scale (deep but high-skill product)");
    println!("           agentless trend (Wiz) pulls posture-only budget away");
    println!("           Falco OSS success cannibalizes some commercial demand");
    println!("           Monitor side (APM) competes with deeper-pocketed Datadog");
    println!("           multi-product story (Secure + Monitor) sometimes splits sales focus");
    println!("           IPO delayed since 2022 — late-stage stage for tech IPO market");
    println!("           still requires kernel-level agent for full value (vs agentless competitors)");
    println!("  Differentiator: Falco creator + Wireshark-pedigree founder + deepest eBPF/kernel runtime visibility + Risk Spotlight runtime-informed prioritization + 555-day forensic replay + CNCF community leadership — the runtime security platform built by the people who literally instrumented Linux for cloud-native");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sysdig".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sysdig(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, run_sysdig, strip_ext};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sysdig"), "sysdig");
        assert_eq!(basename(r"C:\bin\sysdig.exe"), "sysdig.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sysdig.exe"), "sysdig");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sysdig(&["--help".to_string()], "sysdig"), 0);
        assert_eq!(run_sysdig(&["-h".to_string()], "sysdig"), 0);
        let _ = run_sysdig(&["--version".to_string()], "sysdig");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sysdig(&[], "sysdig");
    }
}
