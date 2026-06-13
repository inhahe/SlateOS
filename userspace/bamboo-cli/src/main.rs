#![deny(clippy::all)]

//! bamboo-cli — SlateOS Bamboo (Atlassian's on-prem CI/CD, complement to Bitbucket)
//!
//! Single personality: `bamboo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bamboo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bamboo [OPTIONS]");
        println!("Bamboo Data Center 10.2 (Slate OS) — Atlassian on-prem CI/CD");
        println!();
        println!("Options:");
        println!("  --plan                 Build plan (job + stages + tasks)");
        println!("  --deployment-project   Deployment project");
        println!("  --remote-agent         Remote agent setup");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bamboo Data Center 10.2.1 (Slate OS)"); return 0; }
    println!("Bamboo Data Center 10.2.1 (Slate OS)");
    println!("  Vendor: Atlassian Corporation (Sydney, Australia → NASDAQ:TEAM)");
    println!("  History: launched 2007 — CI/CD complement to Jira + Bitbucket");
    println!("          Atlassian discontinued Bamboo Cloud Jan 2017 (Bitbucket Pipelines replaced it)");
    println!("          Bamboo Server EOL Feb 2024 — Data Center (large enterprise) is the only edition now");
    println!("  License: commercial perpetual (Server) → subscription (Data Center)");
    println!("  Pricing: Data Center starts $1,200/yr for small remote agent counts");
    println!("          scales by remote agent count (1 → unlimited)");
    println!("  Editions: only Bamboo Data Center now (Server discontinued)");
    println!("  Features:");
    println!("    - Build plans: stages → jobs → tasks (parallelism at each level)");
    println!("    - Deployment projects: build artifact → environments (Dev/Staging/Prod) with approvals");
    println!("    - Remote agents (run on dedicated build boxes, Windows/Linux/Mac)");
    println!("    - Elastic agents on AWS EC2 (auto-scaling pool)");
    println!("    - Branch builds + automatic branch detection from Bitbucket");
    println!("    - First-class Jira integration (every build linked to issues/versions/releases)");
    println!("    - Bitbucket + GitHub + GitLab + SVN + Mercurial repo support");
    println!("    - YAML Specs (config-as-code) — bamboo-specs/*.yaml or Java DSL");
    println!("    - Test result parsing (JUnit, TestNG, NUnit, Mocha, etc.) with quarantining");
    println!("    - Approve/reject deployments, release notes from Jira issues");
    println!("  Architecture: Java/Tomcat server + remote agents (push or pull) + Postgres/MySQL/Oracle DB");
    println!("  Customers: large enterprises with Jira+Bitbucket Data Center stacks");
    println!("            Australian government, banks, regulated industries");
    println!("  Critique: aging UI, complex configuration, slow plan UI vs modern alternatives");
    println!("           licensing tied to agent counts feels archaic vs minute-based CI");
    println!("  Differentiator: enterprise on-prem CI for Atlassian-stack shops — when cloud isn't allowed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bamboo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bamboo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bamboo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bamboo"), "bamboo");
        assert_eq!(basename(r"C:\bin\bamboo.exe"), "bamboo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bamboo.exe"), "bamboo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bamboo(&["--help".to_string()], "bamboo"), 0);
        assert_eq!(run_bamboo(&["-h".to_string()], "bamboo"), 0);
        let _ = run_bamboo(&["--version".to_string()], "bamboo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bamboo(&[], "bamboo");
    }
}
