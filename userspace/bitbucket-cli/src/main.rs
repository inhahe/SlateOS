#![deny(clippy::all)]

//! bitbucket-cli — Slate OS Bitbucket (Atlassian's Git host)
//!
//! Single personality: `bitbucket`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bitbucket [OPTIONS]");
        println!("Bitbucket Cloud (Slate OS) — Atlassian's Git hosting platform");
        println!();
        println!("Options:");
        println!("  --pull-request         Create PR");
        println!("  --pipelines            Bitbucket Pipelines (CI/CD)");
        println!("  --jira-integration     Link commits/branches to Jira issues");
        println!("  --trello-integration   Trello board attachment");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bitbucket Cloud (Slate OS)"); return 0; }
    println!("Bitbucket Cloud (Slate OS)");
    println!("  Vendor: Atlassian Corporation (Sydney, Australia → NASDAQ:TEAM)");
    println!("  Founders: Mike Cannon-Brookes + Scott Farquhar (Atlassian 2002)");
    println!("           Bitbucket originally founded by Jesper Nøhr in 2008 (Mercurial host)");
    println!("           Atlassian acquired Bitbucket Sep 2010");
    println!("  History: started as Mercurial-only — added Git support 2011");
    println!("          dropped Mercurial support June 2020 (controversial — broke many repos)");
    println!("          now Git-only");
    println!("  Pricing: Free tier — unlimited private repos, up to 5 users");
    println!("          Standard $3/user/mo — 2,500 build minutes, branch permissions");
    println!("          Premium $6/user/mo — 3,500 build minutes, deployment permissions, IP allowlists");
    println!("  Editions: Bitbucket Cloud (SaaS at bitbucket.org)");
    println!("           Bitbucket Data Center (self-hosted, replaces deprecated Bitbucket Server)");
    println!("  Features:");
    println!("    - Git hosting (unlimited private repos)");
    println!("    - Pull requests with code review, inline comments, approvals");
    println!("    - Bitbucket Pipelines — YAML-defined CI/CD (Docker-based)");
    println!("    - Jira Software integration (the killer feature — first-class commit/branch/PR linking)");
    println!("    - Trello, Confluence, Jenkins, Bamboo integration");
    println!("    - Code Insights — third-party scan results in PRs");
    println!("    - Smart Mirroring (Data Center) — geo-replicated repo caches");
    println!("    - Snippets — gist-equivalent");
    println!("    - Wiki per repo, issue tracker (basic)");
    println!("  Atlassian suite: Jira + Confluence + Bitbucket + Trello = full DevOps platform");
    println!("  Critique: smaller community than GitHub/GitLab — primarily used by Atlassian-shop enterprises");
    println!("           Pipelines slower + more limited than GitHub Actions/GitLab CI");
    println!("           Mercurial drop angered die-hard hg users (Facebook, Mozilla used hg)");
    println!("  Differentiator: deepest Jira integration in the industry — first choice for Jira-heavy teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bitbucket".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bitbucket"), "bitbucket");
        assert_eq!(basename(r"C:\bin\bitbucket.exe"), "bitbucket.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bitbucket.exe"), "bitbucket");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bb(&["--help".to_string()], "bitbucket"), 0);
        assert_eq!(run_bb(&["-h".to_string()], "bitbucket"), 0);
        let _ = run_bb(&["--version".to_string()], "bitbucket");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bb(&[], "bitbucket");
    }
}
