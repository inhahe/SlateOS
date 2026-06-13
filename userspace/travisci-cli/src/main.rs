#![deny(clippy::all)]

//! travisci-cli — SlateOS Travis CI (the OG hosted CI for open source)
//!
//! Single personality: `travisci`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_travis(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: travisci [OPTIONS]");
        println!("Travis CI (Slate OS) — Hosted continuous integration");
        println!();
        println!("Options:");
        println!("  login                  Log in to Travis CI");
        println!("  status                 Status of latest build");
        println!("  logs                   Latest build log");
        println!("  restart                Restart last build");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Travis CI (Slate OS)"); return 0; }
    println!("Travis CI (Slate OS)");
    println!("  Vendor: Travis CI GmbH (Berlin, Germany)");
    println!("          acquired by Idera Inc. Jan 2019 (controversial — layoffs followed)");
    println!("  Founders: Sven Fuchs, Josh Kalderimis, Mathias Meyer + Konstantin Haase (Berlin 2011)");
    println!("  History: launched 2011 — the first hosted CI for GitHub OSS projects");
    println!("          ~2011-2018: THE de facto CI for Ruby/Python/Node OSS");
    println!("          .travis.yml was the canonical CI config (predates GitHub Actions by ~7 years)");
    println!("          Idera acquisition 2019 → mass layoffs of senior engineers");
    println!("          May 2020: ended free unlimited OSS minutes (replaced with 10K credit allotment)");
    println!("          → exodus to GitHub Actions / CircleCI accelerated");
    println!("  Pricing: Free tier — 10,000 credits one-time for OSS");
    println!("          Bootstrap $69/mo — 25,000 credits");
    println!("          Startup $129/mo — 60,000 credits");
    println!("          Small Business $249/mo, Premium $489/mo — concurrent jobs scaling");
    println!("  Domains: travis-ci.com (private repos) — travis-ci.org (OSS, sunset May 2021)");
    println!("  Features:");
    println!("    - .travis.yml in repo root — declarative YAML config");
    println!("    - Matrix builds (multi-language, multi-version)");
    println!("    - Build environments: Linux (Ubuntu Bionic/Focal/Jammy), macOS (Xcode), Windows");
    println!("    - Build stages — fan-in/fan-out workflows");
    println!("    - Encrypted secrets (RSA-encrypted in .travis.yml)");
    println!("    - First-class support for ~30 languages (Ruby, Python, Node, Go, Rust, Java, C++, Swift, ...)");
    println!("    - GitHub status integration, build matrix badges (the iconic green/red badge)");
    println!("    - Slack/email/IRC notifications");
    println!("  Cultural impact: helped popularize the GitHub Flow PR-test culture");
    println!("                   .travis.yml was synonymous with 'modern CI' for half a decade");
    println!("  Critique: post-acquisition decline — pricing changes alienated OSS community");
    println!("           UI/performance lagged GitHub Actions + CircleCI");
    println!("           security incident May 2021 (env vars leaked in logs)");
    println!("  Status: still operational but much-diminished mindshare");
    println!("  Differentiator: original brand, simple YAML, still strong in mature OSS repos");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "travisci".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_travis(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_travis};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/travisci"), "travisci");
        assert_eq!(basename(r"C:\bin\travisci.exe"), "travisci.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("travisci.exe"), "travisci");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_travis(&["--help".to_string()], "travisci"), 0);
        assert_eq!(run_travis(&["-h".to_string()], "travisci"), 0);
        let _ = run_travis(&["--version".to_string()], "travisci");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_travis(&[], "travisci");
    }
}
