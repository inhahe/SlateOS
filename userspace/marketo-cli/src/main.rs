#![deny(clippy::all)]

//! marketo-cli — OurOS Marketo Engage (Adobe-owned enterprise B2B marketing automation)
//!
//! Single personality: `marketo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_marketo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: marketo [OPTIONS]");
        println!("Marketo Engage (OurOS) — Adobe Experience Cloud B2B marketing automation");
        println!();
        println!("Options:");
        println!("  --growth               Growth tier (custom, mid-market)");
        println!("  --select               Select tier (common B2B)");
        println!("  --prime                Prime tier");
        println!("  --ultimate             Ultimate tier (largest enterprises)");
        println!("  --abm                  Account-Based Marketing (Bizible attribution + bundle)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Marketo 2024 (OurOS)"); return 0; }
    println!("Marketo Engage 2024 (OurOS)");
    println!("  Vendor: Adobe (Marketo Engage division — part of Adobe Experience Cloud)");
    println!("        San Jose, CA + San Mateo (legacy Marketo HQ)");
    println!("  Originally Marketo, Inc. — founded 2006 in San Mateo, CA");
    println!("  Founders: Phil Fernandez (CEO), Jon Miller (CMO/Product), David Morandi (CTO), 2006");
    println!("          all three ex-Epiphany (CRM, sold to SSA Global 2005)");
    println!("          Miller: later co-founded Engagio (ABM, also acquired by Demandbase)");
    println!("  Founded: 2006 → IPO May 2013 NYSE:MKTO ($13/share, popped to $30+)");
    println!("          went private Aug 2016 — Vista Equity for $1.79B");
    println!("          acquired by Adobe Oct 2018 for $4.75B");
    println!("          now integrated into Adobe Experience Cloud as 'Marketo Engage'");
    println!("  Strategic position: enterprise B2B marketing automation leader (sales-led B2B, not B2C)");
    println!("                    primary competitor: HubSpot (going upmarket) + Eloqua (Oracle) + Pardot (Salesforce)");
    println!("                    Gartner Magic Quadrant Leader in B2B Marketing Automation 10+ years running");
    println!("  Pricing (notoriously opaque, list prices for reference):");
    println!("    Growth — from ~$895/mo (10K contacts base, climbs by database size)");
    println!("    Select — from ~$1,795/mo");
    println!("    Prime — from ~$3,175/mo");
    println!("    Ultimate — custom (typically $100K-500K+/yr for large enterprises)");
    println!("    annual contracts; multi-year discounts standard");
    println!("    Marketo Measure (ex-Bizible attribution) add-on $$");
    println!("  Defining features (the B2B marketing playbook codified):");
    println!("    - Lead Scoring (behavioral + demographic) — bedrock B2B nurturing concept");
    println!("    - Lead Lifecycle stages with automated transitions");
    println!("    - Smart Lists + Smart Campaigns (trigger-based + scheduled)");
    println!("    - Engagement Programs (drip nurture with content + cadence)");
    println!("    - Marketing Calendar (cross-team campaign visibility)");
    println!("    - Email design + dynamic content + token personalization");
    println!("    - Landing pages + forms with progressive profiling");
    println!("    - Lead routing + assignment + SLA-based reminders");
    println!("    - Salesforce sync (deepest of any MAP — bidirectional, custom object support)");
    println!("    - Munchkin tracking script (the equivalent of Google Tag Manager for B2B behavior)");
    println!("  Marketo Measure (Bizible attribution):");
    println!("    - Multi-touch attribution across all channels (display, paid social, content, events)");
    println!("    - Pipeline + revenue impact analysis");
    println!("    - Custom attribution models (first-touch, last-touch, W-shaped, U-shaped, custom-weighted)");
    println!("    - Pulls cost data from Google Ads + Facebook + LinkedIn ad platforms");
    println!("  ABM (Account-Based Marketing):");
    println!("    - Target Account Lists (sync from CRM or Engagio)");
    println!("    - Account-level scoring + engagement metrics");
    println!("    - Personalization at account level (not just contact level)");
    println!("  AI features (Adobe Sensei + recent gen AI):");
    println!("    - Predictive Audiences (likelihood-to-convert scoring)");
    println!("    - Send Time Optimization (best send time per recipient)");
    println!("    - Content suggestions + subject line testing");
    println!("    - Adobe Firefly generative AI for email creative (rolling out 2024)");
    println!("  Integrations: 500+ marketplace apps + Adobe ecosystem");
    println!("              Salesforce (deepest), Microsoft Dynamics, SAP, Oracle CRM");
    println!("              Adobe Experience Manager + Real-Time CDP + Workfront (native bundles)");
    println!("              Slack, Drift, 6sense, Demandbase, ZoomInfo, LinkedIn Sales Navigator");
    println!("              Snowflake/BigQuery exports");
    println!("              REST API + SOAP API (legacy) + webhooks");
    println!("  Customers: ~6,000+ paying enterprise customers");
    println!("            Microsoft, Adobe (uses itself), GE, Panasonic, Charles Schwab, Symantec");
    println!("            CenturyLink, HSBC, RBC, Pearson, Lufthansa Group");
    println!("            sweet spot: B2B enterprises with 1K-50K employees, complex multi-product/region marketing");
    println!("            heavy in tech, financial services, manufacturing, professional services");
    println!("  Critique: famously difficult to administer — Marketo admins are a specialist role + cert track");
    println!("           UI rebuilt incrementally — still patches of dated UX inside modern shell");
    println!("           AI features lag HubSpot's velocity since Adobe acquisition");
    println!("           pricing total cost (with Measure, ABM) reaches $250K+/yr fast for serious teams");
    println!("           ecosystem lock-in to Adobe Experience Cloud — exits painful");
    println!("  Differentiator: deepest B2B-specific marketing automation feature set + most-cert'd admin community + Adobe Experience Cloud integration");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "marketo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_marketo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_marketo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/marketo"), "marketo");
        assert_eq!(basename(r"C:\bin\marketo.exe"), "marketo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("marketo.exe"), "marketo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_marketo(&["--help".to_string()], "marketo"), 0);
        assert_eq!(run_marketo(&["-h".to_string()], "marketo"), 0);
        assert_eq!(run_marketo(&["--version".to_string()], "marketo"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_marketo(&[], "marketo"), 0);
    }
}
