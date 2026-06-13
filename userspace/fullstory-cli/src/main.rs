#![deny(clippy::all)]

//! fullstory-cli — SlateOS FullStory (session replay + digital experience intelligence)
//!
//! Single personality: `fullstory`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fullstory(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fullstory [OPTIONS]");
        println!("FullStory (SlateOS) — session replay + digital experience intelligence");
        println!();
        println!("Options:");
        println!("  --free                 Free tier (1K sessions/mo)");
        println!("  --business             Business — custom mid-market");
        println!("  --enterprise           Enterprise — custom large");
        println!("  --replay               Session replay (the core)");
        println!("  --funnels              Funnels analytics");
        println!("  --heatmaps             Heatmaps + click maps");
        println!("  --frustration          Frustration signals (rage clicks, dead clicks)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FullStory 2024 (SlateOS)"); return 0; }
    println!("FullStory 2024 (SlateOS)");
    println!("  Vendor: FullStory, Inc. (Atlanta, GA — private)");
    println!("  Founders: Scott Voigt (CEO), Bruce Johnson, Joel Webber, Hugh Higgins, 2014");
    println!("          all four ex-Google Wave/Gmail/AppEngine — deep Java/JS systems pedigree");
    println!("          Atlanta tech scene's biggest SaaS exit candidate (alongside Calendly, Mailchimp)");
    println!("  Founded: 2014 in Atlanta");
    println!("  Funding: ~$214M total raised — Series E Oct 2022 $25M at $1.8B valuation (down from $1.8B peak)");
    println!("          Permira, Stripes, Kleiner Perkins, Glynn Capital");
    println!("          ~$150M+ ARR (rumored)");
    println!("          IPO candidate but delayed by 2023-2024 public-market malaise");
    println!("  Defining category — Digital Experience Intelligence (DXI):");
    println!("    - Session replay was the original — watch real user sessions like a video");
    println!("    - Now expanded to event analytics, heatmaps, frustration scoring, conversions");
    println!("    - Positions against Hotjar (cheaper, smaller) + Glassbox (enterprise) + Contentsquare (acquired Heap)");
    println!("  Pricing:");
    println!("    Free — 1K sessions/mo (full replay, basic funnels)");
    println!("    Business — custom (typically $80K-150K/yr for mid-market)");
    println!("    Enterprise — custom (six-figure deals common at large B2C)");
    println!("    pricing tiered by session volume + features + data retention");
    println!("  Session Replay (the killer feature):");
    println!("    - 100% of sessions recorded (configurable sampling for cost control)");
    println!("    - DOM-based recording (not video) — small bandwidth, replay scales");
    println!("    - Searchable + filterable by event/property");
    println!("    - Console + network logs alongside the replay (debug like a developer)");
    println!("    - Co-browse: developer mode shows your app's JS state during the recorded session");
    println!("    - Privacy: auto-mask PII + secure mode for HIPAA/PCI");
    println!("    - iOS + Android replay (mobile recording with redaction)");
    println!("  Frustration Signals:");
    println!("    - Rage clicks (clicking the same element 5+ times rapidly)");
    println!("    - Dead clicks (clicking something that doesn't respond)");
    println!("    - Error clicks (clicks that triggered a JS error)");
    println!("    - U-turns (back-button after just landing)");
    println!("    - Form abandonment");
    println!("    - 'Frustration Score' aggregates these into a single 0-100 metric per page/feature");
    println!("  Analytics features:");
    println!("    - Funnels (multi-step with drop-off analysis)");
    println!("    - Conversions (track any goal across sessions)");
    println!("    - Segments + Saved Searches");
    println!("    - Page Performance dashboards (Core Web Vitals)");
    println!("    - Heatmaps (clicks, attention, scroll)");
    println!("    - Page Insights — auto-surface friction on a specific URL");
    println!("  Mobile FullStory (FullStory for iOS/Android):");
    println!("    - Native app session replay");
    println!("    - Crash + performance monitoring");
    println!("    - Gesture replay + screen path analysis");
    println!("  Integrations: 50+ destinations");
    println!("              Segment, mParticle, Tealium (upstream)");
    println!("              Salesforce, HubSpot, Marketo, Iterable");
    println!("              Zendesk, Intercom, Help Scout (drop session replay into support tickets)");
    println!("              Sentry, New Relic, Datadog (link error to replay)");
    println!("              Slack alerts on frustration signals + anomalies");
    println!("  API + integrations: REST + webhooks + S3 + warehouse data export");
    println!("                    Data Direct — stream session data to your warehouse");
    println!("                    DXI Insights API for headless integration");
    println!("  Customers: 3,200+ paying companies");
    println!("            Peloton, Reddit (parts), Hyatt, Brooks Running, JetBlue, BCBS, HP, Vodafone");
    println!("            Lululemon, Indeed, M1 Finance, Stack Overflow, Sysco");
    println!("            sweet spot: high-volume B2C and PLG B2B (think 1M+ MAU)");
    println!("  Critique: pricing famously creeps as sessions grow — frequent re-negotiations");
    println!("           storage of full sessions = compliance/privacy review overhead");
    println!("           competing against Heap+Contentsquare merger (now bundled cheaper)");
    println!("           AI/insights features mature but less proactive than newer rivals");
    println!("           Atlanta location = harder to recruit Bay Area talent (but huge cost advantage)");
    println!("  Differentiator: gold-standard session replay + frustration signals + co-browse — the 'reproduce the bug from 2pm' tool");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fullstory".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fullstory(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fullstory};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fullstory"), "fullstory");
        assert_eq!(basename(r"C:\bin\fullstory.exe"), "fullstory.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fullstory.exe"), "fullstory");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fullstory(&["--help".to_string()], "fullstory"), 0);
        assert_eq!(run_fullstory(&["-h".to_string()], "fullstory"), 0);
        let _ = run_fullstory(&["--version".to_string()], "fullstory");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fullstory(&[], "fullstory");
    }
}
