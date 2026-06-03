#![deny(clippy::all)]

//! workato-cli — OurOS Workato (modern iPaaS + automation, Mountain View CA, private unicorn)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_workato(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: workato [OPTIONS]");
        println!("Workato (OurOS) — modern enterprise iPaaS + automation (private unicorn)");
        println!();
        println!("Options:");
        println!("  --recipes              Recipes (workflow automations)");
        println!("  --workbot             Workbot (Slack/Teams chatbot platform)");
        println!("  --api-platform        Workato API Platform");
        println!("  --enterprise-key-mgmt Customer-managed encryption keys");
        println!("  --workato-ai          Workato AI (LLM-powered automations)");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Workato 2024 (OurOS)"); return 0; }
    println!("Workato 2024 (OurOS) — Enterprise Automation Platform");
    println!("  Vendor: Workato, Inc. (Mountain View, CA — private unicorn)");
    println!("  Founders: Vijay Tella + Gautham Viswanathan + Harish Shetty, 2013");
    println!("          Vijay Tella: long-time CEO + ex-TIBCO + ex-Oracle Fusion Middleware");
    println!("          founded with thesis: 'iPaaS for the cloud era — built for business users not just IT'");
    println!("          headquartered in Mountain View; large engineering presence in Singapore");
    println!("  Private funding:");
    println!("         Series E Nov 2021: $200M at $5.7B valuation (Battery, Insight, Altimeter)");
    println!("         total raised: ~$430M");
    println!("         Battery, Insight, Altimeter, Salesforce Ventures, Workday Ventures backers");
    println!("         expected IPO 2024-2025 (delayed by market conditions)");
    println!("         revenue: ~$200M+ ARR (private, estimated)");
    println!("  Strategic position: 'enterprise automation platform — citizen + IT — recipes for everything':");
    println!("                    pitch: 'one platform for integration + workflow + chatbots + APIs + AI'");
    println!("                    target: mid-market to large enterprise (challenger to MuleSoft/Boomi)");
    println!("                    primary competitor: MuleSoft, Boomi, Microsoft Power Automate, Zapier (lower-end)");
    println!("                    secondary: Tray.io, Celigo, Jitterbit, n8n (open-source)");
    println!("                    Workato's wedge: business-user-friendly + IT-grade governance + native chatbots + AI");
    println!("                    'Embedded automation' for SaaS vendors — many partners embed Workato in their products");
    println!("  Pricing:");
    println!("    Workspace plan: $10K-$50K/yr starter");
    println!("    Enterprise plan: $100K-$2M+/yr (most customers)");
    println!("    Recipe-based pricing (number of active recipes + connector tasks)");
    println!("    Embedded OEM: revenue-share or per-seat pricing for ISVs");
    println!("    typically priced 30-50% below MuleSoft for similar functional scope");
    println!("  Product portfolio:");
    println!("    1. Recipes (the core unit):");
    println!("       - Workflow automations (triggers + actions)");
    println!("       - Visual recipe builder (low-code) with code-mode for advanced");
    println!("       - 1,000+ pre-built connectors");
    println!("       - Recipe IQ: ML-suggested next steps + error recovery");
    println!("    2. Workbot (chatbot platform):");
    println!("       - Conversational interface to enterprise apps via Slack + Teams");
    println!("       - 'Workbot for HR' / 'Workbot for IT' / etc.");
    println!("       - One of the early enterprise chatbot platforms (since 2017)");
    println!("    3. Workato API Platform (API mgmt):");
    println!("       - API design + gateway + lifecycle");
    println!("       - Compete with: MuleSoft API Mgr, Kong, Apigee");
    println!("    4. Workato Embedded (OEM platform):");
    println!("       - SaaS vendors embed Workato in their products as integration layer");
    println!("       - Customers: Atlassian, Box, Procore, HubSpot embed Workato connectors");
    println!("    5. Enterprise Key Management:");
    println!("       - Customer-managed encryption keys");
    println!("       - SOC2 + ISO27001 + HIPAA + GDPR compliance");
    println!("    6. Workato AI (2023 — generative AI integration):");
    println!("       - LLM-powered recipe generation + summarization");
    println!("       - Natural-language recipe building");
    println!("       - AI agents that execute automations conversationally");
    println!("    7. Workato Insights (governance + observability):");
    println!("       - Recipe inventory, ownership, performance, error rates");
    println!("       - Critical for enterprise rollout at scale");
    println!("  Recipes architecture:");
    println!("    - Recipes consist of triggers (when X happens) + actions (do Y)");
    println!("    - Run on-demand, scheduled, or event-driven (webhooks)");
    println!("    - Recipe lifecycle: dev → staging → production with promotion");
    println!("    - Version-controlled + audit-logged");
    println!("    - Conditional logic + loops + transformations + error handlers");
    println!("    - Idempotency + retry semantics built-in");
    println!("  Integrations (1,000+ connectors):");
    println!("    - CRM: Salesforce, HubSpot, Microsoft Dynamics, Zoho");
    println!("    - HCM: Workday, BambooHR, ADP, SAP SuccessFactors");
    println!("    - ITSM: ServiceNow, Jira, Zendesk, Freshservice");
    println!("    - ERP: NetSuite, Sage Intacct, SAP, Oracle ERP");
    println!("    - Marketing: Marketo, HubSpot, Mailchimp, Iterable, Braze");
    println!("    - Collaboration: Slack, Teams, Asana, Monday, Notion");
    println!("    - Storage: Box, Dropbox, Google Drive, OneDrive, SharePoint");
    println!("    - Cloud: AWS, Azure, GCP, Snowflake, Databricks");
    println!("    - Database: Oracle, SQL Server, PostgreSQL, MongoDB");
    println!("  Workato CLI usage:");
    println!("    workato recipe list --workspace marketing-ops");
    println!("    workato recipe run --recipe-id 12345 --input '{{\"order_id\": 999}}'");
    println!("    workato recipe export --recipe-id 12345 --output recipe.json");
    println!("    workato connector test --connection salesforce-prod");
    println!("    workato workbot deploy --bot 'HR Helper' --channel slack");
    println!("    workato api-platform deploy --collection orders-v1");
    println!("  Customers (~21,000+ — biggest claim in iPaaS):");
    println!("    - HP, Box, Slack, Atlassian, Cloudflare, Broadcom (large customer)");
    println!("    - HCA Healthcare, Procter & Gamble, Office Depot, Mahindra");
    println!("    - heavy in: tech companies (sweet spot — they adopt fast)");
    println!("    - growing in: financial services, healthcare, retail");
    println!("    - 80%+ of customer renewals (very sticky)");
    println!("    - average customer uses 5,000+ recipes at scale");
    println!("  Critique: per-recipe pricing can balloon at scale (audit needed)");
    println!("           enterprise governance lighter than MuleSoft for hardcore IT departments");
    println!("           on-prem deployment less mature than MuleSoft (cloud-native heritage)");
    println!("           AI features still maturing");
    println!("           expected IPO has slipped — market timing concerns");
    println!("           Microsoft Power Automate's E5 bundling threatens low-end");
    println!("           focus on tech-company sweet spot = challenging diversification to other verticals");
    println!("  Differentiator: 'iPaaS for everyone — IT + business + developers' + Workbot (early enterprise chatbot platform) + Recipes (1,000+ connectors with smart error recovery + Recipe IQ) + Workato Embedded (the platform many SaaS vendors embed for their integrations) + AI-powered recipe building + ~$5.7B last-round valuation with 21K+ customers — the next-generation iPaaS that's growing share against MuleSoft and pushing Microsoft Power Automate out of the enterprise");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "workato".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_workato(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_workato};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/workato"), "workato");
        assert_eq!(basename(r"C:\bin\workato.exe"), "workato.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("workato.exe"), "workato");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_workato(&["--help".to_string()], "workato"), 0);
        assert_eq!(run_workato(&["-h".to_string()], "workato"), 0);
        assert_eq!(run_workato(&["--version".to_string()], "workato"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_workato(&[], "workato"), 0);
    }
}
