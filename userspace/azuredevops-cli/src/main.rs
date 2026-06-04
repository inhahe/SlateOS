#![deny(clippy::all)]

//! azuredevops-cli — OurOS Azure DevOps (Microsoft's enterprise dev suite, formerly VSTS/TFS)
//!
//! Single personality: `azuredevops`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ado(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: azuredevops [OPTIONS]");
        println!("Azure DevOps Services (OurOS) — Microsoft's enterprise DevOps platform");
        println!();
        println!("Options:");
        println!("  --boards               Azure Boards (work tracking / kanban / sprints)");
        println!("  --repos                Azure Repos (Git / TFVC)");
        println!("  --pipelines            Azure Pipelines (CI/CD)");
        println!("  --test-plans           Azure Test Plans (manual + automated)");
        println!("  --artifacts            Azure Artifacts (npm/NuGet/Maven feeds)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Azure DevOps Services (OurOS)"); return 0; }
    println!("Azure DevOps Services (OurOS)");
    println!("  Vendor: Microsoft Corporation (Redmond, WA — NASDAQ:MSFT)");
    println!("  History: started as Visual Studio Team System 2005");
    println!("          → Team Foundation Server (TFS) 2008–2017 (on-prem)");
    println!("          → Visual Studio Team Services (VSTS) 2013–2018 (cloud)");
    println!("          → renamed Azure DevOps Sep 2018 — split into 5 standalone services");
    println!("  Pricing: First 5 users FREE — unlimited private Git repos");
    println!("          Basic $6/user/mo — Boards + Repos + Pipelines + Artifacts");
    println!("          Basic + Test Plans $52/user/mo (test management premium)");
    println!("          Pipeline minutes: 1,800 free/mo MS-hosted, unlimited self-hosted");
    println!("  Five services:");
    println!("    1. Azure Boards — work items, kanban, sprints, queries (Jira competitor)");
    println!("    2. Azure Repos — Git + TFVC (centralized version control still supported for legacy)");
    println!("    3. Azure Pipelines — YAML or Classic — Windows/Linux/Mac agents, 'release pipelines'");
    println!("    4. Azure Test Plans — exploratory testing, manual test cases, parameter-driven");
    println!("    5. Azure Artifacts — feed-based package management (npm, NuGet, Maven, Python, Universal)");
    println!("  Editions: Azure DevOps Services (SaaS at dev.azure.com)");
    println!("           Azure DevOps Server (on-prem, formerly TFS — 2022 release)");
    println!("  Features:");
    println!("    - Work item types: Epic, Feature, User Story/PBI, Task, Bug — fully customizable process templates");
    println!("    - Pipeline integration with every Azure service (App Service, AKS, Functions, etc.)");
    println!("    - Variable groups + Azure Key Vault integration for secrets");
    println!("    - Deployment groups, approvals, gates, environments");
    println!("    - GitHub Advanced Security for Azure DevOps (CodeQL, secret scanning)");
    println!("    - Wiki per project, dashboards, analytics views");
    println!("  Strategy: Microsoft positions GitHub as cloud-first, Azure DevOps as enterprise/on-prem-friendly");
    println!("           After GitHub acquisition (2018, $7.5B), some Microsoft teams migrating to GitHub Enterprise");
    println!("  Critique: aging UI compared to GitHub, classic pipelines feel dated");
    println!("           still THE choice for large Microsoft-stack enterprises");
    println!("  Differentiator: tightest integration with Visual Studio + .NET tooling, mature work tracking");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "azuredevops".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ado(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ado};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/azuredevops"), "azuredevops");
        assert_eq!(basename(r"C:\bin\azuredevops.exe"), "azuredevops.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("azuredevops.exe"), "azuredevops");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ado(&["--help".to_string()], "azuredevops"), 0);
        assert_eq!(run_ado(&["-h".to_string()], "azuredevops"), 0);
        let _ = run_ado(&["--version".to_string()], "azuredevops");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ado(&[], "azuredevops");
    }
}
