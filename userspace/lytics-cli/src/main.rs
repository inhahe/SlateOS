#![deny(clippy::all)]

//! lytics-cli — OurOS Lytics (composable CDP + behavioral scoring, Portland)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lytics(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lytics [OPTIONS]");
        println!("Lytics (OurOS) — composable CDP + behavioral scoring (Portland)");
        println!();
        println!("Options:");
        println!("  --conductor            Lytics Conductor — composable CDP control plane");
        println!("  --decision-engine      ML-based behavioral scoring + content affinity");
        println!("  --gcp-native           BigQuery-native architecture");
        println!("  --audiences            Real-time audience builder");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lytics 2024 (OurOS)"); return 0; }
    println!("Lytics 2024 (OurOS) — Composable CDP");
    println!("  Vendor: Lytics, Inc. (Portland, OR)");
    println!("  Founders: James McDermott (CEO) + Aaron Raddon (CTO), 2012");
    println!("          James: ex-Webtrends VP (Portland web analytics legacy)");
    println!("          Aaron: ex-Cnet + Webtrends engineering");
    println!("          one of the earliest 'CDP' companies (predates the term being widely used)");
    println!("          Webtrends spinoff vibes — heavy Pacific Northwest data-analytics lineage");
    println!("  Funding: ~$83M total");
    println!("         Series C 2019: $35M led by JMI Equity");
    println!("         Series B 2017: $20M led by Two Sigma Ventures");
    println!("         earlier: Voyager Capital, Comcast Ventures, Rembrandt Venture Partners");
    println!("         no recent major raise — bootstrapped-feeling growth post-2019");
    println!("  Strategic position: 'composable CDP — works ON your warehouse, not next to it':");
    println!("                    pitch: 'CDP that runs natively in your BigQuery — no data movement'");
    println!("                    target: media + publishing + B2C marketing teams on GCP");
    println!("                    primary competitor: Segment, mParticle, Tealium (packaged CDPs)");
    println!("                    secondary: Hightouch + Census (reverse-ETL competitors)");
    println!("                    Lytics' wedge: BigQuery-native architecture + content affinity ML");
    println!("                    pivoted to 'composable' positioning early (2021) — ahead of trend");
    println!("                    Google partnership: deep BigQuery + GA4 + GMP integration");
    println!("  Pricing:");
    println!("    Free tier — limited (Lytics for Startups)");
    println!("    Growth — $30K-150K/yr");
    println!("    Enterprise — $150K-1M+/yr");
    println!("    pricing pegged to data volume + audience sizes");
    println!("  Core platform:");
    println!("    - Lytics Conductor: composable-CDP control plane (orchestrates data in YOUR warehouse)");
    println!("    - BigQuery-native: queries run in customer's BigQuery, no data exfiltration");
    println!("    - Cloud Connect: bidirectional sync with Salesforce + Adobe + Marketo");
    println!("    - Real-time + batch event ingestion");
    println!("    - Identity resolution (deterministic + probabilistic)");
    println!("  Decision Engine (the ML differentiator):");
    println!("    - Content affinity scoring (which topics interest each user?)");
    println!("    - Lifecycle stage modeling (prospect → engaged → loyal → at-risk)");
    println!("    - Behavioral propensity (purchase, churn, conversion)");
    println!("    - Content recommendations (next-best-content)");
    println!("    - Particularly strong for publishers + media (content-affinity heritage)");
    println!("  Composable CDP (Lytics' early bet):");
    println!("    - 'Your warehouse is your CDP' — Lytics provides the workflow layer");
    println!("    - Compose: warehouse + Lytics workflow + reverse-ETL destinations");
    println!("    - Compete head-on with: Hightouch (which pivoted from rETL to composable CDP)");
    println!("    - Lytics' advantage: 10+ years of ML + content affinity built-in");
    println!("  Google Cloud partnership:");
    println!("    - GCP Marketplace deep integration");
    println!("    - GA4 native source");
    println!("    - Google Ads + DV360 (Display & Video 360) destination");
    println!("    - BigQuery-native Subscription product");
    println!("    - Listed in Google Cloud's Customer Data Platform partner ecosystem");
    println!("  Integrations (100+):");
    println!("    - Warehouses: BigQuery (deepest), Snowflake, Redshift, Databricks");
    println!("    - Analytics: GA4, Adobe Analytics, Mixpanel, Amplitude");
    println!("    - Ads: Google Ads, Facebook, LinkedIn (Conversions APIs)");
    println!("    - Marketing: Salesforce Marketing Cloud, Adobe Campaign, Marketo, Iterable");
    println!("    - Personalization: Adobe Target, Optimizely, Dynamic Yield");
    println!("    - CRM: Salesforce, Dynamics 365, HubSpot");
    println!("  Lytics CLI usage:");
    println!("    lytics login");
    println!("    lytics conductor segment list");
    println!("    lytics segment create --name 'cart-abandoners' --query 'shoppers WHERE last_cart > 0'");
    println!("    lytics decisions score --user-id u-123 --model content-affinity");
    println!("    lytics export --segment cart-abandoners --destination facebook-ads");
    println!("  Customers (~150+ paying):");
    println!("    - The Economist, The Atlantic, USA Today, NHL, ESPN");
    println!("    - HanesBrands, Whole Foods, ServiceNow (enterprise B2B)");
    println!("    - General Mills, NBCSports, Live Nation");
    println!("    - sweet spot: media + publishing + B2C with deep content");
    println!("    - heavy in: media/publishing, sports/entertainment, retail");
    println!("  Critique: smaller than Segment / mParticle / Tealium (less brand awareness)");
    println!("           BigQuery-native = limits adoption among Snowflake/Databricks shops");
    println!("           growth slowing — composable CDP narrative now claimed by Hightouch too");
    println!("           Hightouch raised much more capital + has stronger marketing");
    println!("           ML/AI features need refresh vs Anthropic/OpenAI-powered competitors");
    println!("           no recent major funding round = capital constraints");
    println!("           Google partnership double-edged — depends on Google's data partner strategy");
    println!("  Differentiator: composable-CDP pioneer + BigQuery-native architecture + content-affinity ML + media/publishing vertical strength + Pacific Northwest data-analytics heritage — the CDP choice for content-heavy B2C brands on Google Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lytics".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lytics(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lytics};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lytics"), "lytics");
        assert_eq!(basename(r"C:\bin\lytics.exe"), "lytics.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lytics.exe"), "lytics");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lytics(&["--help".to_string()], "lytics"), 0);
        assert_eq!(run_lytics(&["-h".to_string()], "lytics"), 0);
        let _ = run_lytics(&["--version".to_string()], "lytics");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lytics(&[], "lytics");
    }
}
