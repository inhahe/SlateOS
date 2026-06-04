#![deny(clippy::all)]

//! census-cli — OurOS Census (the OG reverse-ETL, Hightouch's main rival)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_census(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: census [OPTIONS]");
        println!("Census (OurOS) — operational analytics + reverse ETL (warehouse → SaaS sync)");
        println!();
        println!("Options:");
        println!("  --free                 Free — up to 10 destinations, 30 syncs");
        println!("  --starter              Starter — $300/mo (basic sync + Census Embedded)");
        println!("  --growth               Growth — custom (typically $20K-$80K/yr)");
        println!("  --enterprise           Enterprise — custom (typically $80K+/yr)");
        println!("  --embedded             Census Embedded (white-label reverse-ETL for SaaS vendors)");
        println!("  --datasets             Datasets (warehouse-native semantic layer)");
        println!("  --activation-api       Activation API (real-time data activation)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Census 2024 (OurOS)"); return 0; }
    println!("Census 2024 (OurOS)");
    println!("  Vendor: Census, Inc. (San Francisco, CA — private)");
    println!("  Founders: Boris Jabes (CEO), Sean Lynch, Bill Marrs, 2018");
    println!("          Jabes ex-product at Couchbase + Mixpanel, repeat founder (sold a startup to Salesforce)");
    println!("          built Census initially as a 'CRM sync' product, expanded into full reverse ETL category");
    println!("          quieter + more 'enterprise-respectable' positioning than Hightouch");
    println!("  Founded: 2018 in San Francisco (around the same time as Hightouch — natural rivalry from day one)");
    println!("          raised ~$80M total (Sequoia, A16Z, Insight)");
    println!("          ~$30M+ ARR estimated (private)");
    println!("          ~150-200 employees");
    println!("          coined-the-term 'Operational Analytics' (vs Hightouch's 'Reverse ETL') — same category");
    println!("  Strategic position: 'data activation for the modern data stack':");
    println!("                    primary competitor: Hightouch (very close rival), Polytomic, Workato");
    println!("                    Hightouch slightly bigger by most measures but Census wins big enterprises");
    println!("                    enterprise-respectable tone (vs Hightouch's louder marketing)");
    println!("                    differentiator: Census Embedded (white-label for SaaS to build in-product reverse-ETL)");
    println!("                    'modern data stack' partner-of-choice: Snowflake, dbt, Fivetran sales motion overlap");
    println!("  Pricing (similar shape to Hightouch — opaque at enterprise):");
    println!("    Free — 10 destinations, 30 syncs/mo, 1M MTUs");
    println!("    Starter — $300/mo (1 dataset, more destinations, basic features)");
    println!("    Growth — custom (typical $20K-$80K/yr, MTU + sync-count scaled)");
    println!("    Enterprise — custom ($80K+, dedicated CSM, SLA, premium support)");
    println!("    pricing axis: MTUs + syncs + destinations + Datasets + Embedded usage");
    println!("  Core architecture (reverse ETL, similar to Hightouch):");
    println!("    - Connect warehouse (Snowflake, BigQuery, Redshift, Databricks, Postgres) as source");
    println!("    - Build model via SQL — define what to sync");
    println!("    - Configure sync: destination + field mapping + mode (full, incremental, etc.)");
    println!("    - 200+ destinations (Salesforce, HubSpot, Marketo, Klaviyo, Iterable, Braze, etc.)");
    println!("    - Schedule or trigger via dbt completion or webhook");
    println!("  Census Embedded (the differentiator):");
    println!("    - White-label reverse-ETL infrastructure for SaaS vendors");
    println!("    - SaaS product can offer 'sync your data to X' to its customers without building infra");
    println!("    - Customers of: Sigma Computing, Mode, Hex, Klaviyo (uses Census Embedded for Klaviyo CDP layer)");
    println!("    - This is Census's growth wedge vs Hightouch — building B2B2B reverse ETL");
    println!("    - Reportedly Klaviyo CDP's underlying tech is Census Embedded");
    println!("  Datasets (semantic layer, 2023+):");
    println!("    - Define reusable 'Datasets' (Customer, Account, Product) once in Census");
    println!("    - Compose audiences + traits on top of Datasets");
    println!("    - Sync any audience to any destination from same Dataset");
    println!("    - Census's answer to 'composable CDP' positioning — vs Hightouch Customer Studio");
    println!("  Audience Hub (composable CDP layer):");
    println!("    - Marketer-friendly audience builder");
    println!("    - Drag-and-drop segment definition");
    println!("    - Activation across email + ads + CRM + push from one UI");
    println!("    - Competes with: Hightouch Customer Studio, Segment Personas");
    println!("  Activation API:");
    println!("    - Real-time API to query Census audiences + traits");
    println!("    - Use cases: real-time site personalization, app feature flagging by segment");
    println!("    - Similar to Hightouch Personalization API");
    println!("  Operations + audit:");
    println!("    - Detailed sync logs + Snowflake/BigQuery view of every API call");
    println!("    - Sync-level retries, error notifications, Slack/email alerts");
    println!("    - SOC 2 Type II, HIPAA, GDPR compliance");
    println!("    - Field-level security + masking");
    println!("    - 'observability-first' positioning vs Hightouch");
    println!("  dbt-native:");
    println!("    - dbt Cloud integration: trigger syncs after dbt models complete");
    println!("    - Census auto-detects dbt models as candidate datasets");
    println!("    - Census models can reference dbt models directly");
    println!("    - Strong partnership marketing with dbt Labs");
    println!("  Integrations: 200+ destinations:");
    println!("              CRM (Salesforce, HubSpot, Pipedrive, Microsoft Dynamics, Outreach, Salesloft)");
    println!("              Marketing (Marketo, Klaviyo, Braze, Iterable, Customer.io, ActiveCampaign)");
    println!("              Ads (Meta, Google, LinkedIn, TikTok, Microsoft, Reddit, Snap)");
    println!("              Support (Zendesk, Intercom, Front, Freshdesk)");
    println!("              Analytics (Mixpanel, Amplitude, Heap, Hotjar)");
    println!("              REST + GraphQL Webhooks + custom destination SDK");
    println!("  Customers: ~400+ paying customers");
    println!("            Notion, Figma, Lyft (some teams), Mux, Loom, Plaid, Carta, Ramp, Brex (some shared with Hightouch)");
    println!("            Trustpilot, Sonos, ZeroFox, Canva, Whoop, Carvana");
    println!("            sweet spot: B2B SaaS + fintech + media companies with mature data teams");
    println!("            embedded Customers: Klaviyo, Sigma, Hex, Mode build reverse-ETL into their products via Census");
    println!("  Critique: very similar product to Hightouch — differentiation is positioning, not feature gap");
    println!("           smaller than Hightouch in deals + employee count, but wins enterprise + embedded plays");
    println!("           UI feels more 'data team' less 'marketer' than Hightouch Customer Studio");
    println!("           pricing opaque past Starter — many customers report surprises at renewal");
    println!("           sync frequency: minutes (not real-time) for warehouse-driven workflows");
    println!("           growth slower than Hightouch in 2023-2024");
    println!("           AI features lag Hightouch's AI Decisioning push");
    println!("  Differentiator: Census Embedded (white-label reverse-ETL for SaaS vendors) + enterprise-respectable positioning + Datasets semantic layer — for B2B SaaS vendors building data sync as a feature + enterprise data teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "census".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_census(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_census};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/census"), "census");
        assert_eq!(basename(r"C:\bin\census.exe"), "census.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("census.exe"), "census");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_census(&["--help".to_string()], "census"), 0);
        assert_eq!(run_census(&["-h".to_string()], "census"), 0);
        let _ = run_census(&["--version".to_string()], "census");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_census(&[], "census");
    }
}
