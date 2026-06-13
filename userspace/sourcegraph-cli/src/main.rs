#![deny(clippy::all)]

//! sourcegraph-cli — SlateOS Sourcegraph (universal code search + AI coding platform)
//!
//! Single personality: `sourcegraph`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sourcegraph [OPTIONS]");
        println!("Sourcegraph 5.7 (SlateOS) — Universal code search across all your repos");
        println!();
        println!("Options:");
        println!("  search QUERY           Code search (regex, structural, literal)");
        println!("  --cody                 Cody (AI coding assistant)");
        println!("  --batch-changes        Large-scale code changes across many repos");
        println!("  --code-insights        Code change tracking dashboards");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sourcegraph 5.7 (SlateOS)"); return 0; }
    println!("Sourcegraph 5.7 (SlateOS)");
    println!("  Vendor: Sourcegraph, Inc. (San Francisco, founded 2013)");
    println!("  Founders: Quinn Slack + Beyang Liu (Stanford CS)");
    println!("  Funding: a16z + Sequoia + Redpoint — $125M Series D (Mar 2021), $2.6B valuation");
    println!("  License: dual — Sourcegraph Free (Apache 2.0 core), Enterprise (commercial)");
    println!("          fully open-source version: 'OSS Sourcegraph'");
    println!("  Pricing: Free tier — 10 repos, single user");
    println!("          Pro $59/user/mo (Cody Pro included)");
    println!("          Enterprise — custom (large orgs, on-prem deploys)");
    println!("  Core features:");
    println!("    - Universal code search across ALL your code (github + gitlab + bitbucket + self-hosted)");
    println!("    - Regex, literal, AND structural search (pattern matching on AST shape, not text)");
    println!("    - Code intelligence — go-to-definition + find-references across repos (using LSIF/SCIP indexes)");
    println!("    - Batch Changes — author large-scale changes across many repos with a YAML spec");
    println!("    - Code Insights — dashboards tracking 'how many repos still use deprecated_function?'");
    println!("    - Code monitors — alerts on code matching a query (security scanning, dep drift)");
    println!("  Cody (AI assistant since 2023):");
    println!("    - Chat with your codebase using GPT-4 / Claude / Gemini");
    println!("    - Repo-wide context (RAG over your entire indexed codebase)");
    println!("    - Inline code completion, autocomplete, refactoring");
    println!("    - IDE extensions: VS Code, JetBrains, Neovim, Emacs");
    println!("    - Free Cody tier: 200 messages + 500 autocompletes/mo");
    println!("  Customers: Uber, Lyft, Yelp, GE, Indeed, Plaid — large polyrepo orgs");
    println!("  History: started as a developer's side project after Quinn struggled to navigate huge codebases at Palantir");
    println!("  Critique: enterprise license model expensive at scale");
    println!("           code intelligence depends on language-specific indexers (good for Go/JS/TS/Python/Java)");
    println!("  Differentiator: built for monorepos AND polyrepos — search ALL your code in one place");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sourcegraph".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sg(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sg};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sourcegraph"), "sourcegraph");
        assert_eq!(basename(r"C:\bin\sourcegraph.exe"), "sourcegraph.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sourcegraph.exe"), "sourcegraph");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sg(&["--help".to_string()], "sourcegraph"), 0);
        assert_eq!(run_sg(&["-h".to_string()], "sourcegraph"), 0);
        let _ = run_sg(&["--version".to_string()], "sourcegraph");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sg(&[], "sourcegraph");
    }
}
