#![deny(clippy::all)]

//! posthog-cli — OurOS PostHog (open-source product analytics — the Amplitude killer)
//!
//! Single personality: `posthog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_posthog(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: posthog [OPTIONS]");
        println!("PostHog (OurOS) — open-source all-in-one product OS");
        println!();
        println!("Options:");
        println!("  --cloud                PostHog Cloud (US + EU regions)");
        println!("  --self-host            Self-hosted (Docker Compose / Helm)");
        println!("  --product-analytics    Event analytics (funnels, retention, paths)");
        println!("  --session-recording    Session replay (web + mobile)");
        println!("  --feature-flags        Feature flags + A/B testing");
        println!("  --experiments          Experimentation engine");
        println!("  --surveys              In-app surveys");
        println!("  --llm-observability    LLM observability (track AI app traces)");
        println!("  --warehouse            Data warehouse (DuckDB-on-S3)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PostHog 2024 (OurOS)"); return 0; }
    println!("PostHog 2024 (OurOS)");
    println!("  Vendor: PostHog Inc. (London, UK + SF, CA — distributed, public handbook)");
    println!("  Founders: James Hawkins (CEO), Tim Glaser (CTO), 2020 (!!)");
    println!("          both ex-British SaaS — built PostHog in Y Combinator W20 batch");
    println!("          stunningly fast climb: $0 to $50M+ ARR in ~4 years");
    println!("  Founded: 2020 — YC Winter 2020 batch, open-sourced immediately");
    println!("          first product analytics tool to be MIT-licensed at the core (PostHog Cloud is hosted variant)");
    println!("  Funding: ~$78M raised");
    println!("          Series A 2021 led by GV (Google Ventures)");
    println!("          Series B Oct 2021 $15M at unknown valuation");
    println!("          Series C Apr 2024 $70M at ~$2B valuation led by Stripe (!!)");
    println!("          investors: Y Combinator, GV, Stripe, Hanwha, Mango, SignalFire");
    println!("  Defining strategy — 'do everything modern product teams need, in one repo':");
    println!("    - Product analytics (Amplitude/Mixpanel/Heap territory)");
    println!("    - Session recording (FullStory/Hotjar territory)");
    println!("    - Feature flags + A/B testing (LaunchDarkly/Split territory)");
    println!("    - Surveys (Sprig/Hotjar Ask territory)");
    println!("    - LLM observability (Helicone/Langfuse territory)");
    println!("    - Data warehouse (DuckDB-on-S3 — bring your own data)");
    println!("    - All open-source + self-hostable + cheap on cloud");
    println!("  Pricing (usage-based, transparent):");
    println!("    Free — 1M events/mo, 5K recordings/mo, 1M flag requests/mo (incredibly generous)");
    println!("    Pay-as-you-go: $0.00005/event after free tier (~5K per $1)");
    println!("    Session recordings: $0.005 per recording after free tier");
    println!("    Feature flags: $0.0001 per request after free tier");
    println!("    Surveys: $0.20 per response after first 250 free");
    println!("    No per-seat pricing — unlimited team members FREE");
    println!("    enterprise add-ons: SSO, audit logs, dedicated support, advanced perms");
    println!("  Product analytics features:");
    println!("    - Events + persons (auto-capture OR explicit, your choice)");
    println!("    - Insights (trends, funnels, retention, paths, stickiness, lifecycle)");
    println!("    - SQL Insights — write Postgres-style SQL against your events");
    println!("    - Cohorts (static + dynamic)");
    println!("    - Dashboards");
    println!("    - Notebooks (mixed text + chart docs, like Hex/Mode)");
    println!("    - Group analytics (organization/account/company-level cohorts)");
    println!("  Session Recording:");
    println!("    - Web + iOS + Android session replay");
    println!("    - Console + network logs + Performance metrics inline");
    println!("    - Link recordings to specific events/persons/flags");
    println!("    - Privacy: PII masking + sampling");
    println!("    - Mobile recordings with screen redaction");
    println!("  Feature Flags + Experiments:");
    println!("    - Boolean + multivariate flags");
    println!("    - Targeting by cohort/property/percentage rollout");
    println!("    - A/B test framework with statistical significance");
    println!("    - Local evaluation SDKs (no network round-trip on flag check)");
    println!("    - SDKs in 12+ languages");
    println!("  LLM Observability (2024+):");
    println!("    - Trace LLM calls (prompts, responses, tokens, latency, cost)");
    println!("    - Tie LLM events to product analytics + user journeys");
    println!("    - Eval workflows for prompt + model comparison");
    println!("    - Direct competition with Langfuse + Helicone + Arize Phoenix");
    println!("  Data Warehouse:");
    println!("    - DuckDB-on-S3 architecture (huge cost advantage vs Snowflake)");
    println!("    - Connect external sources: Stripe, Hubspot, Postgres, Salesforce");
    println!("    - Query everything with SQL");
    println!("    - Source-agnostic events + revenue + customer data unified");
    println!("  AI features:");
    println!("    - Max — AI assistant for natural language analytics queries");
    println!("    - AI-suggested cohorts + insights");
    println!("    - Generative dashboard creation");
    println!("  Integrations: 80+ apps + Zapier");
    println!("              Slack, Microsoft Teams, GitHub, GitLab, Sentry, Datadog");
    println!("              Segment (both upstream + downstream)");
    println!("              Snowflake, BigQuery, Redshift exports");
    println!("              Stripe (revenue events native)");
    println!("              full REST API + GraphQL + webhooks");
    println!("  Open-source momentum:");
    println!("    - 22,000+ GitHub stars (one of the most-starred MIT-licensed SaaS products)");
    println!("    - 2,000+ open-source contributors");
    println!("    - public handbook + transparent salaries + open metrics dashboards");
    println!("    - 'Hog Tied' podcast, 'PostHogers' culture branding");
    println!("  Customers: 50,000+ accounts (free + paid)");
    println!("            Y Combinator (uses PostHog internally), Hasura, Vercel, Replit, Resend");
    println!("            Brex, Airbyte, ClickHouse, Drift, Mistral AI, Linear");
    println!("            heavy startup + scale-up adoption — 'PostHog is the default for new YC companies'");
    println!("            sweet spot: 5-500 person teams that want one tool not seven");
    println!("  Critique: dashboards can feel sprawling — 7 products in one UI");
    println!("           query performance lags Amplitude on very large datasets");
    println!("           enterprise governance still maturing vs Amplitude/Heap");
    println!("           LLM observability features early — Langfuse still deeper for prompt eval");
    println!("           self-hosted requires real ops work (Helm chart non-trivial)");
    println!("  Differentiator: only product where analytics + replay + flags + LLM obs in ONE open-source repo at one price");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "posthog".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_posthog(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_posthog};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/posthog"), "posthog");
        assert_eq!(basename(r"C:\bin\posthog.exe"), "posthog.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("posthog.exe"), "posthog");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_posthog(&["--help".to_string()], "posthog"), 0);
        assert_eq!(run_posthog(&["-h".to_string()], "posthog"), 0);
        let _ = run_posthog(&["--version".to_string()], "posthog");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_posthog(&[], "posthog");
    }
}
