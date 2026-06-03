#![deny(clippy::all)]

//! celigo-cli — OurOS Celigo (NetSuite-centric iPaaS, San Mateo CA, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_celigo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: celigo [OPTIONS]");
        println!("Celigo (OurOS) — integrator.io iPaaS (NetSuite + e-commerce focus, private)");
        println!();
        println!("Options:");
        println!("  --integrator           integrator.io (the iPaaS platform)");
        println!("  --integration-apps     Pre-built integration apps (NetSuite + Shopify + etc.)");
        println!("  --flows                Flow Builder (workflow automation)");
        println!("  --templates            Template marketplace (community + Celigo-built)");
        println!("  --celigo-ai            Celigo AI (LLM-assisted integration)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Celigo integrator.io 2024 (OurOS)"); return 0; }
    println!("Celigo 2024 (OurOS) — integrator.io Integration Platform");
    println!("  Vendor: Celigo, Inc. (San Mateo, CA — private)");
    println!("  Founders: Jan Arendtsz + Rico Andrade + Scott Henderson, 2006");
    println!("          founded with a focus on NetSuite-centric integrations");
    println!("          'Celigo' name = Latin-ish portmanteau (no formal etymology)");
    println!("          Jan Arendtsz: long-time CEO + technical founder");
    println!("          ~700 employees, profitable per CEO statements");
    println!("  Private funding:");
    println!("         Series C Sept 2021: $77M (TCV led at unspecified valuation)");
    println!("         total raised: ~$110M (modest by iPaaS standards — capital efficient)");
    println!("         TCV, NewSpring, OurCrowd backers");
    println!("         estimated $80-100M ARR (private)");
    println!("         IPO not actively discussed");
    println!("  Strategic position: 'NetSuite-centric iPaaS for the mid-market — pre-built integration apps':");
    println!("                    pitch: 'AppCloud of pre-built integrations + flexible iPaaS for custom needs'");
    println!("                    target: NetSuite customers (e-commerce, distribution, manufacturing)");
    println!("                    primary competitor: Boomi, Workato, MuleSoft (in NetSuite shops)");
    println!("                    secondary: Jitterbit, Zapier (lower-end), FarApp, In8 (smaller NetSuite specialists)");
    println!("                    Celigo's wedge: deepest NetSuite integration depth + pre-built apps + Shopify ecosystem");
    println!("                    '#1 iPaaS on NetSuite SuiteApp marketplace' is a real moat");
    println!("  Pricing:");
    println!("    Free tier: limited flows, no production");
    println!("    Starter: $10K-$30K/yr");
    println!("    Standard: $30K-$80K/yr (most common SMB-to-mid-market)");
    println!("    Premium: $80K-$300K/yr (enterprise NetSuite shops)");
    println!("    Pre-built Integration Apps: $5K-$50K/yr each (Celigo-managed, easy install)");
    println!("    typically 3-5x cheaper than MuleSoft for NetSuite-centric customers");
    println!("  Product portfolio:");
    println!("    1. integrator.io (the iPaaS platform):");
    println!("       - Visual flow builder (mappings + transforms + branching)");
    println!("       - 300+ connectors (NetSuite, Salesforce, Shopify, Amazon, eBay, ServiceNow, etc.)");
    println!("       - Real-time + batch flows");
    println!("       - Built-in error handling + retry semantics");
    println!("    2. Integration Apps (the differentiator):");
    println!("       - Pre-built, Celigo-maintained integrations for common scenarios");
    println!("       - NetSuite ↔ Shopify, NetSuite ↔ Amazon, NetSuite ↔ Salesforce, etc.");
    println!("       - Drop-in install with config (vs build-from-scratch)");
    println!("       - ~30+ pre-built Integration Apps");
    println!("       - Major share of Celigo revenue");
    println!("    3. Flow Builder (custom workflows):");
    println!("       - For scenarios not covered by Integration Apps");
    println!("       - Visual designer + JavaScript scripting steps");
    println!("    4. Template Marketplace:");
    println!("       - Community-shared + Celigo-built flow templates");
    println!("       - Starter point for common patterns");
    println!("    5. Celigo AI (2023-2024):");
    println!("       - AI Copilot for flow building (natural-language to mapping)");
    println!("       - AI-suggested error remediation");
    println!("    6. API Management (lightweight):");
    println!("       - Expose flows as REST APIs");
    println!("       - Auth + throttling");
    println!("    7. Connector Builder:");
    println!("       - Build custom connectors via REST/OpenAPI");
    println!("  NetSuite + Shopify ecosystem strength:");
    println!("    - #1 iPaaS in NetSuite SuiteApp marketplace by install count");
    println!("    - Deep, native NetSuite expertise (~18 years)");
    println!("    - NetSuite SuiteCloud Developer Network premier partner");
    println!("    - Shopify Plus Technology Partner (e-commerce flows)");
    println!("    - Amazon Selling Partner Network");
    println!("    - eBay Network");
    println!("    - common deployment: NetSuite (ERP) ↔ Shopify + Amazon + eBay (channels) — Celigo specializes here");
    println!("  Integrations (300+ connectors):");
    println!("    - ERP: NetSuite (the anchor), SAP, Oracle, Microsoft Dynamics, Sage Intacct");
    println!("    - E-commerce: Shopify, BigCommerce, Magento, WooCommerce, Salesforce Commerce Cloud");
    println!("    - Marketplaces: Amazon, eBay, Walmart, Etsy, Wayfair");
    println!("    - 3PL/WMS: ShipStation, ShipHero, Stord, Deliverr (now Shopify)");
    println!("    - CRM: Salesforce, HubSpot, Zoho, Microsoft Dynamics");
    println!("    - Marketing: Marketo, HubSpot, Mailchimp, Klaviyo");
    println!("    - Financial: Stripe, PayPal, Adyen, Avalara (tax)");
    println!("    - Database: PostgreSQL, MySQL, SQL Server, Snowflake");
    println!("    - Cloud: AWS, Azure, GCP, FTP/SFTP");
    println!("  Celigo CLI usage:");
    println!("    celigo login --account my-workspace");
    println!("    celigo flow list --status enabled");
    println!("    celigo flow deploy --flow-id ABC123 --env production");
    println!("    celigo integration-app install --app netsuite-shopify --account-id 12345");
    println!("    celigo template browse --category netsuite");
    println!("    celigo connector test --name netsuite-prod");
    println!("    celigo ai suggest --flow-id ABC123 --error-type 'mapping'");
    println!("  Customers (~6,000+):");
    println!("    - Sweet spot: NetSuite customers $5M-$500M revenue");
    println!("    - E-commerce: Bombas, Ergobaby, Honest Company, Allbirds (some)");
    println!("    - Distribution + manufacturing + retail mid-market");
    println!("    - International: heavy in Australia + UK + Asia (NetSuite footprint)");
    println!("    - 95%+ customer retention");
    println!("  Critique: NetSuite-centric concentration = exposure if NetSuite share declines");
    println!("           outside the NetSuite ecosystem, Celigo is a follower not a leader");
    println!("           UI feels dated next to Workato/Tray cloud-native UX");
    println!("           AI features early stage (smaller R&D budget vs MuleSoft/Workato)");
    println!("           competition with Boomi increasing in NetSuite installed base");
    println!("           connector count (300) below Workato (1,000) and Zapier (7,000) — long tail thinner");
    println!("           enterprise governance lighter than MuleSoft for very-large shops");
    println!("           growth depends on NetSuite + Shopify ecosystem health");
    println!("  Differentiator: deepest NetSuite integration platform (#1 on SuiteApp marketplace) + 30+ pre-built Integration Apps (NetSuite ↔ Shopify/Amazon/eBay/Salesforce) + e-commerce + multi-channel selling sweet spot + capital-efficient profitable growth ($110M raised — modest by iPaaS standards) + 6K+ customers in NetSuite mid-market — the iPaaS that NetSuite + Shopify customers actually use for their day-one integration needs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "celigo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_celigo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_celigo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/celigo"), "celigo");
        assert_eq!(basename(r"C:\bin\celigo.exe"), "celigo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("celigo.exe"), "celigo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_celigo(&["--help".to_string()], "celigo"), 0);
        assert_eq!(run_celigo(&["-h".to_string()], "celigo"), 0);
        assert_eq!(run_celigo(&["--version".to_string()], "celigo"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_celigo(&[], "celigo"), 0);
    }
}
