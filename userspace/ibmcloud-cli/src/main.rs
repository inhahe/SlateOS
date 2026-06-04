#![deny(clippy::all)]

//! ibmcloud-cli — OurOS IBM Cloud (hybrid cloud + Red Hat OpenShift + watsonx AI, Armonk NY, NYSE:IBM)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ibmcloud(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ibmcloud [OPTIONS]");
        println!("IBM Cloud (OurOS) — hybrid cloud + Red Hat OpenShift + watsonx AI (parent NYSE:IBM)");
        println!();
        println!("Options:");
        println!("  --openshift            Red Hat OpenShift on IBM Cloud (the strategic flagship)");
        println!("  --watsonx              watsonx AI platform (LLM training + governance)");
        println!("  --kubernetes           IBM Kubernetes Service (IKS)");
        println!("  --power-systems        Power Systems Virtual Server (POWER10 in cloud)");
        println!("  --mainframe            Mainframe-as-a-Service (Z architecture in cloud)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IBM Cloud 2024 (OurOS) — ibmcloud CLI 2.x"); return 0; }
    println!("IBM Cloud 2024 (OurOS) — Hybrid Cloud + Red Hat + watsonx AI");
    println!("  Vendor: IBM Corporation (Armonk, NY — NYSE:IBM since 1924, oldest IT company)");
    println!("  Founder: Charles Ranlett Flint (CTR merger 1911) — renamed IBM 1924 by Thomas J. Watson Sr.");
    println!("          Thomas J. Watson Sr.: CEO 1914-1956 — built IBM, motto 'THINK'");
    println!("          Thomas J. Watson Jr.: led System/360 mainframe revolution 1960s");
    println!("          Arvind Krishna: CEO 2020+ — engineer-CEO, hybrid cloud + AI bet");
    println!("          IBM Cloud history: SoftLayer ($2B acq 2013) → Bluemix → IBM Cloud (~2017)");
    println!("  Public market (NYSE:IBM):");
    println!("         Founded 1911 (CTR), renamed IBM 1924");
    println!("         FY2024 revenue: ~$62.8B (+3% YoY, post-Kyndryl spin)");
    println!("         Market cap: $180-220B range (recent IBM stock recovery 2024)");
    println!("         Cloud + cognitive software: ~30%+ revenue, growing");
    println!("         IBM Cloud + Red Hat: ~$25B revenue (2024 estimate, Red Hat alone ~$7B+)");
    println!("         Dividend aristocrat (28+ years of increases)");
    println!("  Strategic position: 'hybrid + multi-cloud + AI — for enterprises':");
    println!("                    pitch: 'open hybrid cloud + trusted AI — run anywhere, no lock-in'");
    println!("                    target: large enterprise + regulated industries (finance + healthcare + government)");
    println!("                    primary competitor: AWS, Azure, GCP (in hybrid; not pure public cloud anymore)");
    println!("                    secondary: VMware (now Broadcom), Oracle Cloud");
    println!("                    IBM Cloud's wedge: Red Hat OpenShift + mainframe heritage + regulated industries + watsonx");
    println!("                    'IBM is not trying to be AWS' — focus on hybrid + enterprise + AI for governance-sensitive customers");
    println!("                    'Cloud Pak' = OpenShift-based pre-packaged enterprise software");
    println!("  Pricing (transparent for cloud, custom for enterprise):");
    println!("    IBM Cloud VM: $0.04-$5/hr (various sizes)");
    println!("    Red Hat OpenShift on IBM Cloud: from ~$0.80/hr per worker node");
    println!("    Power Systems Virtual Server: from $0.50/hr (POWER10)");
    println!("    Kubernetes (IKS): $0.10/cluster/hr + node prices");
    println!("    watsonx.ai: $0.0006-$0.06 per 1K tokens (Granite models)");
    println!("    Object Storage: $0.0185-$0.022/GB-month");
    println!("    typically 10-30% premium vs AWS, justified by enterprise services/integration");
    println!("    enterprise: 'Cloud Pak' bundles (custom $100K-$10M+/yr)");
    println!("  Product portfolio:");
    println!("    1. Red Hat OpenShift on IBM Cloud (the flagship):");
    println!("       - Managed OpenShift Kubernetes platform");
    println!("       - 'Run anywhere' (IBM Cloud, AWS, Azure, GCP, on-prem)");
    println!("       - Strategic foundation for IBM hybrid cloud bet ($34B Red Hat acq 2019)");
    println!("       - 4,000+ enterprise customers");
    println!("    2. watsonx (the AI platform, 2023+):");
    println!("       - watsonx.ai: foundation model studio (LLM training + tuning)");
    println!("       - watsonx.data: lakehouse for AI data");
    println!("       - watsonx.governance: model governance + observability");
    println!("       - Granite models: IBM's open-source enterprise-grade LLMs");
    println!("       - Granite-Code: code generation models");
    println!("       - Bring-your-own model OR Granite + Llama + Mistral + Mixtral support");
    println!("    3. IBM Kubernetes Service (IKS):");
    println!("       - Managed K8s without OpenShift (lighter-weight)");
    println!("       - Multi-zone clusters in regions");
    println!("       - VPC-based networking");
    println!("    4. Power Systems Virtual Server (POWER10):");
    println!("       - IBM POWER architecture in cloud (AIX, IBM i, Linux)");
    println!("       - Critical for AIX/IBM i workload migration");
    println!("       - Strong in: insurance, banking back-office, ERP migrations");
    println!("    5. Z (Mainframe) Cloud:");
    println!("       - z/OS as cloud service (LinuxONE for Linux on Z)");
    println!("       - 'Hyper Protect' confidential computing on Z");
    println!("       - World's largest mainframe customers stay with IBM via this");
    println!("    6. Cloud Paks (Red Hat OpenShift-based packaged software):");
    println!("       - Cloud Pak for Data (data integration + governance)");
    println!("       - Cloud Pak for Integration (API management + ESB)");
    println!("       - Cloud Pak for Business Automation (BPM + RPA)");
    println!("       - Cloud Pak for Security (QRadar SIEM + SOAR)");
    println!("       - Cloud Pak for AIOps (operations automation)");
    println!("    7. Database services:");
    println!("       - Db2 on Cloud (IBM's flagship enterprise database)");
    println!("       - Cloudant (managed CouchDB NoSQL)");
    println!("       - Databases for PostgreSQL/MongoDB/Redis/Elasticsearch");
    println!("       - Informix on Cloud (legacy ERP)");
    println!("    8. Security products:");
    println!("       - IBM Security Verify (IAM)");
    println!("       - IBM Cloud Hyper Protect Crypto Services (FIPS 140-2 Level 4 HSM)");
    println!("       - Guardium (database security)");
    println!("       - QRadar SIEM (recently divested to Palo Alto $500M 2024)");
    println!("    9. Quantum Cloud (IBM Quantum):");
    println!("       - Real quantum hardware access (Heron, Eagle processors)");
    println!("       - Qiskit (open-source quantum framework)");
    println!("       - Research + commercial quantum experimentation");
    println!("       - Differentiator: actual quantum computers, not just emulators");
    println!("  Red Hat acquisition ($34B, July 2019 — defining moment):");
    println!("    - Largest software acquisition in history at the time");
    println!("    - Bet: hybrid cloud will be 'open' (Linux + K8s + OpenShift)");
    println!("    - IBM committed to keeping Red Hat operating independently");
    println!("    - 'Red Hat will remain Red Hat' — Jim Whitehurst (former Red Hat CEO)");
    println!("    - Strategic: OpenShift as the 'Linux of cloud' across all clouds");
    println!("    - Generated $7B+ revenue/yr by 2024 (vs $3.4B at acquisition)");
    println!("    - Validation: hybrid + multi-cloud thesis broadly correct");
    println!("  Kyndryl spin-off (Nov 2021):");
    println!("    - IBM spun out managed infrastructure services ($19B revenue)");
    println!("    - Renamed Kyndryl (NYSE:KD) — independent company");
    println!("    - IBM kept hybrid cloud + cognitive software");
    println!("    - Cleaner focus on cloud + AI + consulting");
    println!("  watsonx + AI strategy:");
    println!("    - Watson Health divested 2022 (after disappointing results)");
    println!("    - watsonx (2023) reset: foundation models + governance focus");
    println!("    - Granite models: enterprise-trained, openly licensed");
    println!("    - Bet: enterprises need AI with governance + indemnification IBM provides");
    println!("    - Partners: NASA, SAP, Adobe, Microsoft (cross-cloud)");
    println!("  Integrations:");
    println!("    - IBM Cloud CLI (Go-based, plugin architecture)");
    println!("    - Terraform + Ansible providers");
    println!("    - Kubernetes/OpenShift native");
    println!("    - SDKs: Java, JS/Node, Python, Go, Ruby, .NET, Swift");
    println!("    - Strong enterprise consulting (IBM Consulting — formerly IGS — 100K+ consultants)");
    println!("    - Cloud Foundry (legacy Bluemix lineage) deprecation in progress");
    println!("    - watsonx.ai notebooks + Jupyter integration");
    println!("  IBM Cloud CLI usage:");
    println!("    ibmcloud login --sso                                     # SAML/SSO login");
    println!("    ibmcloud target -r us-south -g default");
    println!("    ibmcloud is instance-create my-vm us-south-1 cx2-2x4 vpc-id subnet-id image-id");
    println!("    ibmcloud oc cluster create vpc-gen2 --name my-openshift --version 4.14_openshift");
    println!("    ibmcloud ks cluster create vpc-gen2 --name my-iks --kube-version 1.28");
    println!("    ibmcloud cos bucket-create --bucket my-bucket --class smart");
    println!("    ibmcloud cdb create my-db --service postgresql --plan standard");
    println!("    ibmcloud pi instance create my-power --image AIX-72-1 --processors 1 --memory 4");
    println!("    ibmcloud sl vs create --hostname my-vm --domain example.com --cpu 2 --memory 4096");
    println!("    ibmcloud cf push my-app                                  # legacy Cloud Foundry");
    println!("  Customers (enterprise + regulated industries):");
    println!("    - Major: Citi, BNP Paribas, HSBC, Deutsche Bank (banking)");
    println!("    - Insurance: Generali, AIG, MetLife");
    println!("    - Government: US fed (FedRAMP), UK gov, EU governments");
    println!("    - Healthcare: Anthem, Humana, NHS");
    println!("    - 95%+ of Fortune 500 use IBM something (incl. IBM Consulting)");
    println!("    - IBM Cloud customers: 4,000+ enterprises on OpenShift");
    println!("  Critique: IBM Cloud lost the public cloud race to AWS/Azure/GCP");
    println!("           shrinking compared to hyperscaler growth");
    println!("           Cloud Foundry / legacy Bluemix in slow deprecation");
    println!("           watsonx is generation-2 AI bet — Watson Health (gen-1) divested 2022");
    println!("           POWER + Mainframe heritage limits modern developer mindshare");
    println!("           IBM Consulting (formerly Global Services) creates services-vs-product tension");
    println!("           QRadar SIEM divested to Palo Alto 2024 = strategic narrowing");
    println!("           perception: 'enterprise-only, not for startups/devs'");
    println!("           pace of innovation slower than hyperscalers");
    println!("  Differentiator: Red Hat OpenShift acquisition ($34B 2019 — largest software acq in history) + Power Systems + Z mainframe cloud (no one else has POWER10/z/OS in cloud) + watsonx AI platform with Granite open-source enterprise models + IBM Quantum (real quantum hardware) + IBM Consulting 100K+ consultants + Arvind Krishna engineer-CEO + Watson legacy + 95%+ of Fortune 500 customer relationships + hybrid + 'open hybrid cloud' messaging + 100+ year heritage — the enterprise hybrid cloud platform for regulated industries, mainframe migration paths, and AI governance — IBM is not trying to be AWS, it's trying to be the trusted hybrid AI platform");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ibmcloud".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ibmcloud(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ibmcloud};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ibmcloud"), "ibmcloud");
        assert_eq!(basename(r"C:\bin\ibmcloud.exe"), "ibmcloud.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ibmcloud.exe"), "ibmcloud");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ibmcloud(&["--help".to_string()], "ibmcloud"), 0);
        assert_eq!(run_ibmcloud(&["-h".to_string()], "ibmcloud"), 0);
        let _ = run_ibmcloud(&["--version".to_string()], "ibmcloud");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ibmcloud(&[], "ibmcloud");
    }
}
