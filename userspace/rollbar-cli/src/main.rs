#![deny(clippy::all)]

//! rollbar-cli — SlateOS Rollbar (error tracking, real-time, RQL queries)
//!
//! Single personality: `rollbar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_roll(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rollbar [OPTIONS]");
        println!("Rollbar (SlateOS) — Real-time error tracking & monitoring");
        println!();
        println!("Options:");
        println!("  --items                List error items");
        println!("  --rql                  Rollbar Query Language");
        println!("  --deploys              Track deploys with version tracking");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Rollbar (SlateOS)"); return 0; }
    println!("Rollbar (SlateOS)");
    println!("  Vendor: Rollbar, Inc. (San Francisco, founded 2012 — Y Combinator W12)");
    println!("  Founders: Brian Rue + Cory Virok (ex-Lookout engineers)");
    println!("  Origin: built it for their own sites — productized after demand from other engineers");
    println!("  Pricing: Free tier — 5,000 events/mo, 30-day retention");
    println!("          Essentials $21/mo — 50K events");
    println!("          Advanced $83/mo — 200K events, deploy tracking, source maps");
    println!("          Enterprise — custom (SSO, SOC2, audit logs)");
    println!("  Features:");
    println!("    - SDKs for ~30 languages (Ruby, Python, Node, PHP, Java, .NET, Go, Rust, Elixir, Swift, ...)");
    println!("    - Real-time error grouping (fingerprint dedup → 'items')");
    println!("    - Source map ingestion for JavaScript stack traces");
    println!("    - Deploy tracking — annotate spikes with version + author");
    println!("    - People — first-occurrence + last-occurrence + affected user counts");
    println!("    - Telemetry breadcrumbs (network/console/DOM events leading up to error)");
    println!("    - Notifier rules: alert Slack/Discord/PagerDuty/email on conditions");
    println!("    - RQL — SQL-like query language for items + occurrences");
    println!("    - Bidirectional Jira/Linear/GitHub Issues integration");
    println!("    - Automation Workflows (no-code rules engine for auto-assign, auto-resolve)");
    println!("  Niche: best-in-class for backend error tracking (vs Sentry's frontend strength)");
    println!("        strong in Ruby/Rails + Python/Django shops (early adopters)");
    println!("  Customers: Twilio, Salesforce, Affirm, Twitch, Heroku — engineering-driven SaaS");
    println!("  History: scrappy YC startup era → steady growth, never IPO'd");
    println!("          remained independent + private — focused on product not hype");
    println!("  Critique: smaller mindshare than Sentry post-2020 (Sentry pulled ahead with self-hosted + perf monitoring)");
    println!("           UI feels less modern than Datadog Errors / Sentry");
    println!("  Differentiator: deploy-aware error spikes + RQL for ad-hoc queries");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rollbar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_roll(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_roll};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rollbar"), "rollbar");
        assert_eq!(basename(r"C:\bin\rollbar.exe"), "rollbar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rollbar.exe"), "rollbar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_roll(&["--help".to_string()], "rollbar"), 0);
        assert_eq!(run_roll(&["-h".to_string()], "rollbar"), 0);
        let _ = run_roll(&["--version".to_string()], "rollbar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_roll(&[], "rollbar");
    }
}
