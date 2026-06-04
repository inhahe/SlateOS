#![deny(clippy::all)]

//! sonatype-cli — OurOS Sonatype (Nexus + Lifecycle, SCA + supply chain, Fulton MD)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_son(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sonatype [OPTIONS]");
        println!("Sonatype (OurOS) — Software Composition Analysis + Supply Chain Security");
        println!();
        println!("Options:");
        println!("  --nexus                Nexus Repository (Maven Central operator)");
        println!("  --lifecycle            Nexus Lifecycle SCA scanner");
        println!("  --firewall             Nexus Firewall — block bad packages at edge");
        println!("  --scan PATH            Scan project dependencies");
        println!("  --sbom                 Generate SBOM (CycloneDX, SPDX)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sonatype Nexus 3.69 (OurOS)"); return 0; }
    println!("Sonatype 2024 (OurOS) — Supply Chain Security");
    println!("  Vendor: Sonatype, Inc. (Fulton, MD)");
    println!("  Founders: Jason van Zyl (Apache Maven creator) + others, 2008");
    println!("          Jason van Zyl: created Apache Maven build tool (2002) — foundation of Java ecosystem");
    println!("          founded Sonatype to commercialize Maven + run Maven Central repository");
    println!("          longest-running supply-chain security vendor (16+ years)");
    println!("          Sonatype operates MAVEN CENTRAL — the official Maven artifact repository");
    println!("          80+ billion Maven Central downloads/year — Sonatype runs this");
    println!("  Funding: ~$160M total");
    println!("         Series E 2019: $42M led by TPG");
    println!("         majority recapitalization by Vista Equity Partners 2019");
    println!("         private equity-owned since (Vista)");
    println!("  Strategic position: 'developers love us, security needs us':");
    println!("                    pitch: 'protect your software supply chain from open-source risks'");
    println!("                    target: any company shipping Java + .NET + JavaScript + Python applications");
    println!("                    primary competitor: Snyk (newer, developer-first), JFrog (artifact mgmt overlap)");
    println!("                    secondary: GitHub Advanced Security, Veracode SCA, Mend (was WhiteSource), Black Duck");
    println!("                    Sonatype's moat: 16+ year SCA history + Maven Central operator + 80%+ Fortune 100");
    println!("                    sales motion: enterprise direct + heavy government/regulated industries");
    println!("  Pricing (enterprise sales-led):");
    println!("    Nexus Repository OSS — FREE (Maven/npm/PyPI/NuGet artifact repo)");
    println!("    Nexus Repository Pro — $3K-30K+/yr (enterprise features)");
    println!("    Nexus Lifecycle (SCA) — $50K-500K+/yr typical");
    println!("    Nexus Firewall — add-on to Lifecycle, $30K-300K/yr");
    println!("    Full Sonatype platform: $100K-$5M+/yr Fortune 500 deals");
    println!("  Product portfolio:");
    println!("    1. Maven Central operator — 80B+ downloads/yr (the foundation of Java ecosystem)");
    println!("    2. Nexus Repository Manager — artifact repository (Maven, npm, PyPI, NuGet, Docker, Helm)");
    println!("       - Most-deployed binary repository worldwide");
    println!("       - Compete with: JFrog Artifactory");
    println!("    3. Nexus Lifecycle — SCA scanner with policy enforcement");
    println!("       - Detects known vulnerabilities in dependencies");
    println!("       - Policy-based blocking in CI/CD");
    println!("       - License compliance");
    println!("    4. Nexus Firewall — block malicious packages at proxy");
    println!("       - Prevents typosquatting + namespace confusion attacks");
    println!("       - 'malicious by design' detection (heuristic + ML)");
    println!("    5. Nexus Container — container image scanning");
    println!("    6. Advanced Development Pack (IDE plugins, GitHub integration)");
    println!("  Maven Central operator role:");
    println!("    - Free public Maven repository for the world's Java ecosystem");
    println!("    - 80B+ artifact downloads/year");
    println!("    - 700K+ unique artifacts");
    println!("    - 300K+ unique groups (publishers)");
    println!("    - Sonatype hosts + secures + curates this on its own infra");
    println!("    - Unique vendor position: every Java developer indirectly relies on Sonatype");
    println!("  Sonatype Intelligence (the data moat):");
    println!("    - 100M+ open-source components tracked");
    println!("    - 35M+ known vulnerabilities mapped to specific component versions");
    println!("    - Behavioral analysis of malicious packages (Sonatype Lift)");
    println!("    - Discovered + blocked 1M+ malicious packages 2020-2024");
    println!("    - Original research: Sonatype State of the Software Supply Chain report (annual)");
    println!("  Languages + Ecosystems:");
    println!("    - Java (Maven, Gradle): native + deepest support");
    println!("    - .NET (NuGet)");
    println!("    - JavaScript (npm, yarn)");
    println!("    - Python (pip, poetry)");
    println!("    - Ruby (gems)");
    println!("    - Go modules");
    println!("    - Docker + Helm + container images");
    println!("    - C/C++ (Conan)");
    println!("    - Rust (Cargo) — added 2023");
    println!("  Compliance + Standards:");
    println!("    - CycloneDX SBOM (Sonatype is a CycloneDX co-author)");
    println!("    - SPDX SBOM");
    println!("    - SLSA framework support");
    println!("    - FedRAMP + FIPS for government deployments");
    println!("    - SOC 2 + ISO 27001");
    println!("    - PCI-DSS + HIPAA compliance attestations");
    println!("  CI/CD integrations:");
    println!("    - Jenkins, GitHub Actions, GitLab CI, CircleCI, Azure DevOps, Bamboo, Bitbucket");
    println!("    - IDEs: IntelliJ IDEA, Eclipse, VS Code");
    println!("    - SCM: GitHub, GitLab, Bitbucket");
    println!("    - Ticketing: Jira, ServiceNow");
    println!("  Sonatype CLI usage:");
    println!("    sonatype scan ./pom.xml --application my-app");
    println!("    sonatype evaluate --waiver policy-violation-xyz");
    println!("    sonatype sbom generate --format cyclonedx --output sbom.json");
    println!("    sonatype firewall status");
    println!("    sonatype lifecycle policy list");
    println!("  Customers (2,000+ enterprise):");
    println!("    - 75% of Fortune 100");
    println!("    - JPMorgan, Bank of America, Wells Fargo, Citi, Goldman Sachs");
    println!("    - Lockheed Martin, Boeing, NASA, DoD, Federal Reserve, IRS");
    println!("    - Pfizer, GSK, Walmart, Target, Adobe (yes, Adobe uses Sonatype)");
    println!("    - heavy in: financial services, government/defense, pharma, large tech");
    println!("    - dominant in regulated industries requiring strict SCA + SBOM");
    println!("  Sonatype CEO 2024: Mitchell Johnson took over from Wayne Jackson");
    println!("  Recent:");
    println!("    - Sonatype Lift (CodeQL-style code analysis, 2022) — free for OSS projects");
    println!("    - SBOM Manager (2024) — SBOM lifecycle management");
    println!("    - 'log4shell defenders' positioning post Log4j (Dec 2021)");
    println!("    - Vista PE ownership pressures for IPO 2026+");
    println!("  Critique: developer experience trails Snyk significantly");
    println!("           policy engine complex to configure (typical 3-6 month onboarding)");
    println!("           expensive vs Snyk for SMB/mid-market");
    println!("           UI dated compared to newer SCA tools");
    println!("           GitHub Advanced Security (free for OSS, low-cost for repos) threatens from below");
    println!("           Snyk's developer-first growth pulled developer mindshare");
    println!("           Vista PE-owned = focus on profitability vs innovation");
    println!("           Maven Central operator role = mission-critical but unmonetized infrastructure");
    println!("  Differentiator: 16+ years SCA leadership + Maven Central operator + 80%+ Fortune 100 + deep policy engine + government/defense customer base + CycloneDX co-author — the enterprise SCA platform for regulated industries that need defensible supply-chain security");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sonatype".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_son(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_son};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sonatype"), "sonatype");
        assert_eq!(basename(r"C:\bin\sonatype.exe"), "sonatype.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sonatype.exe"), "sonatype");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_son(&["--help".to_string()], "sonatype"), 0);
        assert_eq!(run_son(&["-h".to_string()], "sonatype"), 0);
        let _ = run_son(&["--version".to_string()], "sonatype");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_son(&[], "sonatype");
    }
}
