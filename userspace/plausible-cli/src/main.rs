#![deny(clippy::all)]

//! plausible-cli — Slate OS Plausible (lightweight, privacy-first, GA alternative)
//!
//! Single personality: `plausible`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_plausible(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: plausible [OPTIONS]");
        println!("Plausible (Slate OS) — lightweight, open-source, privacy-friendly web analytics");
        println!();
        println!("Options:");
        println!("  --growth               Growth from $9/mo (10K pageviews/mo)");
        println!("  --business             Business from $19/mo (10K pageviews/mo, +funnels/goals)");
        println!("  --enterprise           Enterprise (custom, large volume)");
        println!("  --self-host            Self-host (AGPL — free if you run it yourself)");
        println!("  --ga-import            Google Analytics import (preserve historical data)");
        println!("  --no-cookies           No-cookie tracking (GDPR + CCPA + PECR compliant out of box)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Plausible 2024 (Slate OS)"); return 0; }
    println!("Plausible 2024 (Slate OS)");
    println!("  Vendor: Plausible Insights OÜ (Tallinn, Estonia — bootstrapped, profitable)");
    println!("  Founders: Uku Taht (Estonia) + Marko Saric (Denmark), 2018");
    println!("          Uku: developer who built the OG product in spare time");
    println!("          Marko: marketing/SEO blogger who joined as co-founder + marketer 2019");
    println!("          partnership doubled as 'tech + marketing' textbook (their blog is a content masterclass)");
    println!("  Founded: 2018 — fully bootstrapped, no VC, no debt");
    println!("          ~$2.5M ARR (publicly disclosed — they share metrics on their blog)");
    println!("          ~12 employees fully remote");
    println!("          consistently profitable, all reinvested in product + payroll");
    println!("  Open source: AGPL v3 — self-host for free, or pay for Plausible Cloud");
    println!("              ~21,000+ GitHub stars, 100+ contributors");
    println!("  Defining brand: 'an ethical alternative to Google Analytics':");
    println!("    - No cookies, no fingerprinting, no PII collection by default");
    println!("    - GDPR + PECR + CCPA compliant out of the box (no cookie banner needed)");
    println!("    - All data hosted in Germany (EU customers) or AWS US (US customers)");
    println!("    - Tiny snippet: <1KB (vs GA's ~50KB+)");
    println!("    - No 'sample' data manipulation — see every visit, not 'estimated 2.3M'");
    println!("    - Single-page dashboard fits on one screen — anti-bloat thesis");
    println!("  Pricing (transparent, page-view-based):");
    println!("    Growth 10K pageviews — $9/mo");
    println!("    Growth 100K pageviews — $19/mo");
    println!("    Growth 1M pageviews — $69/mo");
    println!("    Growth 10M pageviews — $199/mo");
    println!("    Business tier adds: custom events, funnels, goals, ecommerce revenue tracking ($+10/mo over Growth)");
    println!("    Self-host: FREE (with AGPL obligations)");
    println!("    Annual billing -33% (vs monthly)");
    println!("  Dashboard features:");
    println!("    - Unique visitors, pageviews, bounce rate, visit duration, page depth");
    println!("    - Top sources (referrers + UTM tags + UTM mediums)");
    println!("    - Top pages, top entry pages, top exit pages");
    println!("    - Top countries, regions, cities (without precise IP geo)");
    println!("    - Devices, browsers, operating systems");
    println!("    - Realtime visitor count + live event stream");
    println!("    - Custom date ranges + period comparisons");
    println!("    - Goals (URL-based + custom event)");
    println!("    - Funnels (Business tier)");
    println!("    - Ecommerce revenue tracking (Business tier)");
    println!("    - Subdirectories + subdomains in single site");
    println!("    - Multi-site dashboards (agencies)");
    println!("  Privacy approach:");
    println!("    - No persistent identifier per user (no cookies, no localStorage usage)");
    println!("    - Daily hash for visitor uniqueness — rotated daily so can't track across days");
    println!("    - No IP storage (IPs hashed + discarded immediately)");
    println!("    - No personally identifiable properties — schema deliberately limited");
    println!("    - No data warehouse exports of raw user data — only aggregates");
    println!("  Migration tools:");
    println!("    - One-click Google Analytics import (preserve historical data alongside Plausible going forward)");
    println!("    - Helper scripts for self-host migration from cloud or vice versa");
    println!("  Integrations: 30+ tools");
    println!("              Slack alerts (weekly digests + traffic spike alerts)");
    println!("              Webhooks for goal completions");
    println!("              WordPress, Webflow, Ghost, Squarespace plugins");
    println!("              REST API for custom dashboards");
    println!("              Looker Studio + Notion connectors");
    println!("  Customers: 13,000+ paying sites");
    println!("            DuckDuckGo, GitLab (some pages), Tailwind CSS, OnlineOrNot, Smashing Magazine");
    println!("            Indie hackers + privacy-conscious publications heavy");
    println!("            sweet spot: blogs, marketing sites, small SaaS landing pages");
    println!("  Critique: NOT a product analytics tool — funnels limited, no cohort retention curves");
    println!("           less useful for in-app behavior tracking (try PostHog/Mixpanel for that)");
    println!("           historical data export limited to CSV (no warehouse sync for raw events)");
    println!("           no session replay or heatmaps (out of scope by design)");
    println!("  Differentiator: simplest + most ethical web analytics — entire dashboard on one screen, no cookies, ~1KB script");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "plausible".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_plausible(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_plausible};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/plausible"), "plausible");
        assert_eq!(basename(r"C:\bin\plausible.exe"), "plausible.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("plausible.exe"), "plausible");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_plausible(&["--help".to_string()], "plausible"), 0);
        assert_eq!(run_plausible(&["-h".to_string()], "plausible"), 0);
        let _ = run_plausible(&["--version".to_string()], "plausible");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_plausible(&[], "plausible");
    }
}
