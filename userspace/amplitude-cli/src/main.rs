#![deny(clippy::all)]

//! amplitude-cli — OurOS Amplitude (product analytics, the category leader)
//!
//! Single personality: `amplitude`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_amplitude(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amplitude [OPTIONS]");
        println!("Amplitude (OurOS) — product analytics platform");
        println!();
        println!("Options:");
        println!("  --analytics            Amplitude Analytics (core product)");
        println!("  --experiment           Experiment (feature flags + A/B testing)");
        println!("  --cdp                  Customer Data Platform (audience sync)");
        println!("  --recommend            Recommend (personalization engine)");
        println!("  --plus                 Plus tier $61/mo (small teams)");
        println!("  --growth               Growth tier (custom, mid-market)");
        println!("  --enterprise           Enterprise (custom, large)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Amplitude 2024 (OurOS)"); return 0; }
    println!("Amplitude 2024 (OurOS)");
    println!("  Vendor: Amplitude, Inc. (San Francisco, CA — NASDAQ:AMPL)");
    println!("  Founders: Spenser Skates (CEO), Curtis Liu, Jeffrey Wang, 2012");
    println!("          all three MIT grads — original product was 'Sonalight' voice dialer");
    println!("          pivoted to analytics after struggling to instrument their own app");
    println!("  Founded: 2012 — YC Winter 2012 with Sonalight, pivoted to Amplitude 2014");
    println!("  IPO: direct listing on NASDAQ Sep 2021 at $50/share, popped to $85");
    println!("       now ~$10-15 (suffered major post-IPO compression like most data SaaS)");
    println!("       FY2024 revenue ~$290M, ~$30M+ operating loss (running for cash flow break-even)");
    println!("       ~2,500 customers, ~750 employees");
    println!("  Defining concept — 'product analytics' as a category:");
    println!("    - Different from 'web analytics' (GA) — designed for in-product, logged-in behavior");
    println!("    - Different from BI tools (Looker/Tableau) — pre-built for product/growth team questions");
    println!("    - Behavioral cohorts, funnels, retention, paths analysis as first-class objects");
    println!("    - North Star Framework (NSM) — Amplitude evangelized + open-sourced this growth method");
    println!("  Pricing:");
    println!("    Starter — free (10M events/mo, 3 destinations, limited dashboards)");
    println!("    Plus — $61/mo (50K MTUs starting, scales by volume)");
    println!("    Growth — custom (typically $50K-200K/yr, mid-market)");
    println!("    Enterprise — custom (six-figure deals common at scale)");
    println!("    Pricing controversy: 'MTU' (monthly tracked user) jumps non-linear; 'tracked events' add-on confusing");
    println!("  Core Analytics features:");
    println!("    - Event tracking (any custom event with properties)");
    println!("    - User properties + group analytics (track org/account-level behavior)");
    println!("    - Behavioral Cohorts — segment users by what they did/didn't do");
    println!("    - Funnels — multi-step conversion analysis with conversion windows");
    println!("    - Retention — N-day/N-week retention curves with cohort breakdown");
    println!("    - Paths (Pathfinder) — sankey diagram of user journeys");
    println!("    - Compass — predictive power score for habits → retention");
    println!("    - Stickiness — DAU/WAU/MAU ratios + heatmaps");
    println!("    - Custom dashboards + scheduled email reports");
    println!("  Amplitude Experiment:");
    println!("    - Feature flags (kill switches + gradual rollouts)");
    println!("    - A/B testing with statistical significance computed automatically");
    println!("    - Server-side + client-side SDK support");
    println!("    - Variant-level metric tracking — measure impact on any analytics event");
    println!("    - Mutual Exclusion Groups for non-conflicting parallel tests");
    println!("  Amplitude CDP (Customer Data Platform):");
    println!("    - Single Customer 360 view");
    println!("    - Audience Sync — push cohorts to Facebook Ads, Google Ads, Salesforce, Iterable, Braze");
    println!("    - Data Quality Engine — schema validation + alerting");
    println!("    - Reverse ETL from your warehouse (Snowflake/BigQuery/Redshift)");
    println!("  Amplitude Recommend:");
    println!("    - AI-driven personalization recommendations (next-best-content, next-best-product)");
    println!("    - Realtime API serving (lower latency than batch BI personalization)");
    println!("    - based on cohort behavior + collaborative filtering");
    println!("  Data ingestion:");
    println!("    - Client SDKs: JS, iOS, Android, React Native, Flutter, Unity, Unreal");
    println!("    - Server SDKs: Node, Python, Ruby, Java, Go, PHP, .NET");
    println!("    - Source integrations: Segment, mParticle, RudderStack, Rivery, Stitch, Fivetran");
    println!("    - HTTP API for custom pipelines");
    println!("  Integrations: 100+ destinations");
    println!("              Snowflake, BigQuery, Redshift (data warehouse sync — both directions)");
    println!("              Segment, mParticle as upstream");
    println!("              Hubspot, Salesforce, Marketo, Mailchimp, Braze, Iterable as downstream");
    println!("              Slack alerts on metric anomalies");
    println!("  Customers: 2,500+ paying customers");
    println!("            Atlassian, Disney+, Ford, Block (Square+Cash App), Instacart, NBCUniversal");
    println!("            Capital One, Hubspot, Hyatt, PayPal, Roblox, Walmart");
    println!("            sweet spot: high-volume B2C and PLG B2B SaaS (50M+ users)");
    println!("  Critique: pricing famously painful at scale — many high-growth customers churn to PostHog/in-house");
    println!("           dashboards can get slow on large volume");
    println!("           SQL access on Growth+ — expensive at lower tiers");
    println!("           competes with PostHog (lower price, open core) on the lower end + Heap/Mixpanel mid-market");
    println!("  Differentiator: most polished product analytics UI + 'North Star Framework' thought leadership + enterprise governance");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "amplitude".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_amplitude(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_amplitude};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/amplitude"), "amplitude");
        assert_eq!(basename(r"C:\bin\amplitude.exe"), "amplitude.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("amplitude.exe"), "amplitude");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_amplitude(&["--help".to_string()], "amplitude"), 0);
        assert_eq!(run_amplitude(&["-h".to_string()], "amplitude"), 0);
        assert_eq!(run_amplitude(&["--version".to_string()], "amplitude"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_amplitude(&[], "amplitude"), 0);
    }
}
