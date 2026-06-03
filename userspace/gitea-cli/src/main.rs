#![deny(clippy::all)]

//! gitea-cli — OurOS Gitea (self-hosted lightweight Git service, Go)
//!
//! Single personality: `gitea`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gitea(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gitea [OPTIONS]");
        println!("Gitea 1.22 (OurOS) — Painless self-hosted Git service");
        println!();
        println!("Options:");
        println!("  web                    Start Gitea web server");
        println!("  --actions              Gitea Actions (GitHub Actions compatible)");
        println!("  --packages             Built-in package registry");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gitea v1.22.3 (OurOS)"); return 0; }
    println!("Gitea v1.22.3 (OurOS)");
    println!("  Project: community-driven, Go-based Git service");
    println!("  History: forked from Gogs (Go Git Service by Jiahua Chen) Nov 2016");
    println!("          fork triggered by trademark/governance disputes with upstream");
    println!("  License: MIT (FOSS)");
    println!("  Governance: Gitea Limited (UK) registered 2022 — controversial — community split");
    println!("             Forgejo forked from Gitea Dec 2022 in response (Codeberg-led)");
    println!("  Pricing: free + open source — self-host on your own server");
    println!("  Resource profile: tiny — runs on Raspberry Pi, ~50MB RAM at idle");
    println!("                    single Go binary, embedded assets, SQLite/MySQL/Postgres backend");
    println!("  Features:");
    println!("    - Git hosting (HTTPS + SSH, LFS)");
    println!("    - Issues, milestones, projects (kanban), wiki");
    println!("    - Pull requests with code review, inline comments");
    println!("    - Gitea Actions (since v1.19, Jul 2023) — GitHub Actions compatible workflows");
    println!("    - Built-in package registry: npm, NuGet, Maven, PyPI, Cargo, Composer, Conan,");
    println!("      Container (OCI), Generic, Go, Helm, Pub, RPM, Debian, Vagrant, Chef, etc.");
    println!("    - Webhooks (GitHub-compatible, Slack, Discord, MS Teams, Telegram, custom)");
    println!("    - OAuth2 server + OAuth2/OIDC/LDAP/SAML SSO client");
    println!("    - Mirroring (push + pull) — keep clone of external Git repos");
    println!("    - Repo migrations from GitHub/GitLab/Gogs/Bitbucket");
    println!("    - Code search (built-in or Bleve indexer)");
    println!("    - Two-factor auth, signed commits verification, branch protection");
    println!("  Themes: arc-green (default dark), gitea (light), custom CSS support");
    println!("  i18n: 35+ languages — strong international community");
    println!("  Critique: smaller user base than GitLab CE → fewer enterprise integrations");
    println!("           governance drama led to Forgejo fork (now used by Codeberg)");
    println!("           Actions not yet at full parity with GitHub Actions");
    println!("  Differentiator: lightweight + easy to self-host vs heavyweight GitLab");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gitea".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gitea(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gitea};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gitea"), "gitea");
        assert_eq!(basename(r"C:\bin\gitea.exe"), "gitea.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gitea.exe"), "gitea");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gitea(&["--help".to_string()], "gitea"), 0);
        assert_eq!(run_gitea(&["-h".to_string()], "gitea"), 0);
        assert_eq!(run_gitea(&["--version".to_string()], "gitea"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gitea(&[], "gitea"), 0);
    }
}
