#![deny(clippy::all)]

//! bugsnag-cli — OurOS BugSnag / SmartBear Insight Hub (app stability monitoring, mobile focus)
//!
//! Single personality: `bugsnag`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bugsnag [OPTIONS]");
        println!("BugSnag / SmartBear Insight Hub (OurOS) — App stability monitoring");
        println!();
        println!("Options:");
        println!("  --stability-score      App stability score per release");
        println!("  --releases             Release health dashboard");
        println!("  --pipeline             Stability-gated CI/CD pipeline");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BugSnag / SmartBear Insight Hub (OurOS)"); return 0; }
    println!("BugSnag / SmartBear Insight Hub (OurOS)");
    println!("  Vendor: BugSnag Inc. (San Francisco, founded 2013)");
    println!("          acquired by SmartBear May 2021 (~$100M est.)");
    println!("          rebranded 'SmartBear Insight Hub' 2023 (BugSnag still used informally)");
    println!("  Founders: James Smith + Simon Maynard (Manchester, UK — moved to SF)");
    println!("           previously founded 'Heroku-style PaaS for iOS' (Heya, 2010)");
    println!("  History: started focused on mobile crash reporting — JS error tracking added later");
    println!("          name from a portmanteau of 'bug' + 'snag' (catching a snag)");
    println!("  Pricing: Free tier — 7,500 events/mo");
    println!("          Lite $29/mo, Pro $99/mo, Enterprise custom (SAML/SOC2)");
    println!("  Killer concept — Stability Score:");
    println!("    % of user sessions that completed without an unhandled error");
    println!("    set a 'target stability score' per release");
    println!("    automatically alert if a release drops below threshold");
    println!("    BugSnag pioneered this metric, now industry-standard for mobile");
    println!("  Features:");
    println!("    - 50+ SDKs (mobile + backend + frontend)");
    println!("    - First-class iOS/Android/React Native/Flutter/Unity support (mobile crash heritage)");
    println!("    - Breadcrumbs (timeline of events leading up to crash)");
    println!("    - Severity classification (error / warning / info)");
    println!("    - Release tracking with adoption stage tracking (Canary → 100% rollout)");
    println!("    - Bookmarks (save common filter views)");
    println!("    - Pipeline (stability gate before promoting build to next env)");
    println!("    - Source map upload, ProGuard mapping upload, dSYM upload");
    println!("    - Slack/Jira/Linear/GitHub/PagerDuty/Trello integrations");
    println!("  Niche: app stability for mobile-heavy product teams");
    println!("        used by Airbnb, Lyft, Yelp, Mailchimp, Square, Etsy");
    println!("  SmartBear context: testing tools conglomerate (Swagger/OpenAPI, TestComplete, ReadyAPI, Cucumber)");
    println!("                     Insight Hub fits beside their other dev tools");
    println!("  Differentiator: stability-score-driven release process — mobile crash heritage");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bugsnag".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bugsnag"), "bugsnag");
        assert_eq!(basename(r"C:\bin\bugsnag.exe"), "bugsnag.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bugsnag.exe"), "bugsnag");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bs(&["--help".to_string()], "bugsnag"), 0);
        assert_eq!(run_bs(&["-h".to_string()], "bugsnag"), 0);
        assert_eq!(run_bs(&["--version".to_string()], "bugsnag"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bs(&[], "bugsnag"), 0);
    }
}
