#![deny(clippy::all)]

//! forgejo-cli — OurOS Forgejo (Codeberg-backed copyleft fork of Gitea)
//!
//! Single personality: `forgejo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_forgejo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: forgejo [OPTIONS]");
        println!("Forgejo 9.0 (OurOS) — Self-hosted lightweight software forge");
        println!();
        println!("Options:");
        println!("  web                    Start Forgejo server");
        println!("  --actions              Forgejo Actions (CI/CD)");
        println!("  --federation           ForgeFed (federated git forges, ActivityPub)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Forgejo v9.0.3 (OurOS)"); return 0; }
    println!("Forgejo v9.0.3 (OurOS)");
    println!("  Project: 100% community-governed Git service (no corporate owner)");
    println!("  Pronunciation: 'for-jay-yo' (Esperanto: 'forge')");
    println!("  History: forked from Gitea Dec 2022 by Codeberg e.V. (Berlin non-profit)");
    println!("          fork triggered by Gitea Ltd. (UK private company) takeover of upstream");
    println!("          community wanted: copyleft license, community governance, no corporate gatekeeping");
    println!("  License: GPL v3+ (copyleft — vs Gitea's MIT)");
    println!("          forces hosted forks to share modifications");
    println!("  Governance: Codeberg e.V. (Berlin non-profit) holds the trademark");
    println!("             development by community contributors, no shareholder pressure");
    println!("  Pricing: free + open source — self-host on your own server, or use codeberg.org (free)");
    println!("  Tech: Go single binary, ~50MB RAM, SQLite/MySQL/Postgres backend");
    println!("       still close enough to Gitea that switching is one-command (`gitea` binary symlink works)");
    println!("  Features (inherited from Gitea base, then diverging):");
    println!("    - Git hosting, issues, PRs, wiki, projects, releases");
    println!("    - Forgejo Actions — Gitea Actions compatible (GitHub Actions workflow runner)");
    println!("    - Package registry: npm, NuGet, Maven, PyPI, Cargo, Container (OCI), Helm, ...");
    println!("    - Webhooks, OAuth2/OIDC/LDAP/SAML auth, 2FA");
    println!("    - Mirroring, GitHub/GitLab migrations");
    println!("    - ForgeFed (federation): experimental ActivityPub-based federation between forges");
    println!("      → goal: cross-instance issues/PRs/comments");
    println!("  Codeberg.org: largest public Forgejo instance — alternative to GitHub for FOSS projects");
    println!("               used by KDE, FreeBSD ports, many libre-software projects");
    println!("  Differentiator: copyleft + non-profit governance — anti-enclosure forge");
    println!("                  the 'GitLab/GitHub of FOSS purists'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "forgejo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_forgejo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_forgejo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/forgejo"), "forgejo");
        assert_eq!(basename(r"C:\bin\forgejo.exe"), "forgejo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("forgejo.exe"), "forgejo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_forgejo(&["--help".to_string()], "forgejo"), 0);
        assert_eq!(run_forgejo(&["-h".to_string()], "forgejo"), 0);
        assert_eq!(run_forgejo(&["--version".to_string()], "forgejo"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_forgejo(&[], "forgejo"), 0);
    }
}
