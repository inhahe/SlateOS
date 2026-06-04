#![deny(clippy::all)]

//! betterstack-cli — OurOS Better Stack (uptime + logs + incidents, Prague, dev-friendly)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_betterstack(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: betterstack [OPTIONS]");
        println!("Better Stack (OurOS) — beautifully designed observability + incident mgmt for devs");
        println!();
        println!("Options:");
        println!("  --uptime               Uptime monitoring (was Better Uptime)");
        println!("  --logs                 Logs (was Logtail, ClickHouse-based)");
        println!("  --on-call              On-call schedules + escalations");
        println!("  --incident-management  Incidents + post-mortems");
        println!("  --status-pages         Status pages");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Better Stack 2024 (OurOS)"); return 0; }
    println!("Better Stack 2024 (OurOS) — Observability + Incident Management");
    println!("  Vendor: Better Stack (Tomáš Hromada + team — Prague, Czech Republic, private)");
    println!("  Founder: Tomáš Hromada, 2017 (started as 'Better Uptime')");
    println!("          Originally: Better Uptime — uptime monitoring + incident mgmt + status pages");
    println!("          Expanded into Logtail (log management on ClickHouse) — 2021");
    println!("          Rebranded to 'Better Stack' Jul 2022 to unify the products");
    println!("          Czech-founded, fully remote, EU-focused but global");
    println!("          'Dev-first' product philosophy — beautiful UX as core competitive lever");
    println!("          Bootstrapped early, raised modest VC later");
    println!("  Private funding:");
    println!("         Seed Jun 2021: $2.5M (Reflex Capital)");
    println!("         Series A Apr 2023: $18M (Norwest Venture Partners)");
    println!("         total raised: ~$22M (modest by observability standards — capital efficient)");
    println!("         estimated $15-30M ARR (private)");
    println!("  Strategic position: 'observability tools that devs actually love — uptime + logs + on-call + incidents':");
    println!("                    pitch: 'beautifully designed observability — pricing for everyone'");
    println!("                    target: SMB to mid-market dev teams (sweet spot indie + scale-up + tech)");
    println!("                    primary competitor: Pingdom + Pingdom (uptime), Datadog (full-stack), PagerDuty (on-call)");
    println!("                    secondary: New Relic, Sentry, Bugsnag, StatusPage.io (Atlassian)");
    println!("                    Better Stack's wedge: gorgeous UI + low price + all-in-one DevOps observability bundle");
    println!("                    'The tool we wish we had at our last job' indie-startup aesthetic");
    println!("  Pricing (notably cheap + transparent):");
    println!("    Free tier: 10 monitors, 1 log GB/day");
    println!("    Freelancer: $24/mo (50 monitors, 30 GB/month logs)");
    println!("    Small Team: $74/mo (200 monitors, 100 GB/month logs)");
    println!("    Business: $204/mo (1K monitors, 500 GB/month logs)");
    println!("    Enterprise: custom (5K+ monitors, SSO, audit logs)");
    println!("    typically 50-80% cheaper than Datadog + PagerDuty + Pingdom combo");
    println!("  Product portfolio:");
    println!("    1. Uptime Monitoring (was Better Uptime):");
    println!("       - HTTP/HTTPS, TCP, ping, DNS, SSL cert, keyword, SMTP");
    println!("       - 30+ global monitoring regions");
    println!("       - 30-second checks on paid tiers");
    println!("       - Screenshot + log capture on failure");
    println!("       - 99.99% uptime SLA itself");
    println!("    2. Logs (was Logtail, ClickHouse-based):");
    println!("       - SQL-compatible log search (ClickHouse engine)");
    println!("       - Live tail + sub-second query latency");
    println!("       - 'Pricing for everyone' — much cheaper than Datadog/Splunk logs");
    println!("       - Multi-source ingest: HTTP, syslog, Vector, FluentBit, Filebeat");
    println!("    3. On-Call + Escalations:");
    println!("       - Schedule rotations + override calendar");
    println!("       - Multi-channel escalation (phone, SMS, push, Slack, email)");
    println!("       - Compete with: PagerDuty, Opsgenie, Splunk On-Call");
    println!("    4. Incident Management:");
    println!("       - Auto-incident creation on monitor failure");
    println!("       - Incident timeline + chronology");
    println!("       - Stakeholder updates + comms");
    println!("       - Post-mortem authoring");
    println!("       - Compete with: Incident.io, FireHydrant, Rootly");
    println!("    5. Status Pages:");
    println!("       - Hosted public + private status pages");
    println!("       - Custom domain + branding");
    println!("       - Incident updates auto-publish");
    println!("       - Compete with: StatusPage.io (Atlassian), Statuspal");
    println!("    6. Heartbeats:");
    println!("       - Cron job + scheduled task monitoring");
    println!("       - Expect heartbeat by X, alert if missed");
    println!("    7. SSL + Domain Monitoring:");
    println!("       - SSL expiry alerts, domain expiry alerts");
    println!("       - Misconfiguration detection");
    println!("    8. SMS/Voice Alerts:");
    println!("       - Real phone calls + SMS for critical alerts");
    println!("       - Global SMS routing (Twilio-backed)");
    println!("  Dev-first UX philosophy:");
    println!("    - Visually polished UI (rare in observability — most tools look enterprise-cluttered)");
    println!("    - Fast loading, snappy interactions");
    println!("    - Dark mode + thoughtful typography");
    println!("    - Real-time updates (no manual refresh)");
    println!("    - Beautiful screenshots in marketing");
    println!("    - Indie-startup aesthetic that converts via social media");
    println!("  Integrations:");
    println!("    - Slack, Microsoft Teams, Discord, Telegram for notifications");
    println!("    - PagerDuty, Opsgenie (as secondary alert routes)");
    println!("    - GitHub, GitLab, Bitbucket for deploy events");
    println!("    - Heroku, Vercel, Netlify, Render for hosted apps");
    println!("    - AWS, Azure, GCP cloud monitoring");
    println!("    - Webhooks (universal)");
    println!("    - Vector, FluentBit, Filebeat, Logstash, syslog for log ingest");
    println!("    - OpenTelemetry (basic support)");
    println!("    - SSO: Google Workspace, Microsoft, SAML, GitHub for sign-in");
    println!("  Better Stack CLI usage:");
    println!("    betterstack login --api-token $BETTERSTACK_TOKEN");
    println!("    betterstack monitor create --url https://example.com --type http --interval 30s");
    println!("    betterstack monitor list --status down");
    println!("    betterstack incident list --status resolved --from 7d");
    println!("    betterstack on-call schedule create --name 'Primary' --rotation weekly");
    println!("    betterstack status-page create --name 'Acme Status' --domain status.acme.com");
    println!("    betterstack logs query \"level='error'\" --from -1h");
    println!("    betterstack heartbeat create --name 'Nightly Backup' --period 24h");
    println!("  Customers (10,000+):");
    println!("    - Indie devs, startups, mid-market tech");
    println!("    - International: heavy in Europe + UK + US tech scene");
    println!("    - Notable: Linear, RemNote, Bun, Hashnode, smaller SaaS startups");
    println!("    - Sweet spot: solo founders + small teams + scale-ups");
    println!("    - Word-of-mouth + Twitter/HN-driven growth");
    println!("  Critique: enterprise governance lighter than PagerDuty/Datadog");
    println!("           features less deep than dedicated leaders in each category");
    println!("           ClickHouse-based logs less mature than Splunk/Datadog for complex correlations");
    println!("           AI/ML features minimal vs leading observability vendors");
    println!("           customer base modest vs Datadog 28K+ or PagerDuty 22K+");
    println!("           Twilio-backed SMS pricing pass-through can surprise high-volume teams");
    println!("           limited APM + tracing — competes at edges with full-stack vendors");
    println!("           European HQ may slow enterprise NAm wins");
    println!("  Differentiator: gorgeous, dev-loved UI + uptime + logs + on-call + incidents + status pages in one bundle (rare combination) + 50-80% cheaper than Datadog + PagerDuty + Pingdom combined + ClickHouse-based log search (sub-second queries) + Czech-founded indie aesthetic + bootstrapped capital efficiency ($22M total raised) + 10K+ customers — the dev-first observability bundle that small teams choose because it's beautiful, affordable, and bundles the 5 things every team needs without enterprise complexity");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "betterstack".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_betterstack(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_betterstack};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/betterstack"), "betterstack");
        assert_eq!(basename(r"C:\bin\betterstack.exe"), "betterstack.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("betterstack.exe"), "betterstack");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_betterstack(&["--help".to_string()], "betterstack"), 0);
        assert_eq!(run_betterstack(&["-h".to_string()], "betterstack"), 0);
        let _ = run_betterstack(&["--version".to_string()], "betterstack");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_betterstack(&[], "betterstack");
    }
}
