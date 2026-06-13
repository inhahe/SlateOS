#![deny(clippy::all)]

//! teamcity-cli — SlateOS JetBrains TeamCity CI/CD
//!
//! Single personality: `teamcity`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: teamcity [OPTIONS]");
        println!("JetBrains TeamCity 2024.07 (Slate OS) — CI/CD build management server");
        println!();
        println!("Options:");
        println!("  --server URL           TeamCity server URL");
        println!("  --agent                TeamCity Build Agent");
        println!("  --rest                 REST API client mode");
        println!("  --kotlin-dsl           Kotlin DSL for build configuration");
        println!("  --cloud                TeamCity Cloud (SaaS)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("JetBrains TeamCity 2024.07.3 (build 147512) (Slate OS)"); return 0; }
    println!("JetBrains TeamCity 2024.07.3 (Slate OS)");
    println!("  Editions: Professional (free, 3 agents), Enterprise (unlimited), Cloud (SaaS)");
    println!("  Build runners: Maven, Gradle, MSBuild, .NET, Ant, npm, Python, Docker, ...");
    println!("  Configuration: Web UI + Kotlin DSL (versioned in repo)");
    println!("  Integrations: 150+ build tools, VCS (Git/HG/SVN/Perforce/TFS)");
    println!("  Features: build chains, snapshot dependencies, build queue, agents");
    println!("  Testing: test reports, flaky test detection, code coverage aggregation");
    println!("  Cloud agents: AWS, Azure, GCP, Kubernetes, Docker auto-provisioning");
    println!("  License: Free (Pro 3 agents/100 configs); Enterprise per-agent");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "teamcity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/teamcity"), "teamcity");
        assert_eq!(basename(r"C:\bin\teamcity.exe"), "teamcity.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("teamcity.exe"), "teamcity");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tc(&["--help".to_string()], "teamcity"), 0);
        assert_eq!(run_tc(&["-h".to_string()], "teamcity"), 0);
        let _ = run_tc(&["--version".to_string()], "teamcity");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tc(&[], "teamcity");
    }
}
