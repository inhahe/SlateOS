#![deny(clippy::all)]

//! tray-cli — OurOS Tray.io (general automation, San Francisco/London, private)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tray(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tray [OPTIONS]");
        println!("Tray.io (OurOS) — general automation platform (Merlin AI, private)");
        println!();
        println!("Options:");
        println!("  --workflows            Workflows (the core automations)");
        println!("  --merlin               Tray Merlin (AI-augmented automation, 2024)");
        println!("  --embedded             Tray Embedded (OEM for SaaS vendors)");
        println!("  --connector-builder    Universal Connector Builder");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tray.io 2024 (OurOS) — Merlin GPT 1.0"); return 0; }
    println!("Tray.io 2024 (OurOS) — General Automation Platform");
    println!("  Vendor: Tray.io, Inc. (San Francisco + London, private)");
    println!("  Founders: Rich Waldron + Alistair Russell + Dom Lewis, 2012");
    println!("          founded in London — early UK-founded iPaaS unicorn");
    println!("          'general automation platform' positioning (vs 'integration platform' framing)");
    println!("          Rich Waldron: long-time CEO");
    println!("          dual HQ (San Francisco + London); strong European presence");
    println!("  Private funding:");
    println!("         Series E June 2021: $172M at $1.3B valuation (Spark Capital led)");
    println!("         total raised: ~$285M");
    println!("         Spark Capital, GGV, Salesforce Ventures, ServiceNow Ventures backers");
    println!("         valuation likely lower in 2023-2024 down rounds (private)");
    println!("         estimated $100M+ ARR (private)");
    println!("  Strategic position: 'general automation — AI-driven, developer-friendly, embedded':");
    println!("                    pitch: 'connect-everything platform built for the AI era + composability'");
    println!("                    target: tech-forward enterprise + SaaS vendors (Embedded)");
    println!("                    primary competitor: Workato, MuleSoft, Boomi, Zapier (upper-end)");
    println!("                    secondary: Celigo, Jitterbit, n8n (open-source), Microsoft Power Automate");
    println!("                    Tray's wedge: developer-friendly + Merlin AI (LLM-powered) + Embedded OEM business");
    println!("                    pivot to 'GenAI-native' positioning 2023-2024 with Merlin");
    println!("  Pricing:");
    println!("    Starter: $25K-$50K/yr");
    println!("    Professional: $50K-$200K/yr (most common)");
    println!("    Enterprise: $200K-$2M+/yr");
    println!("    Embedded (OEM): revenue-share or per-seat pricing");
    println!("    typically priced 20-30% below MuleSoft for similar functional scope");
    println!("  Product portfolio:");
    println!("    1. Tray Workflows (the core):");
    println!("       - Visual workflow builder (drag-and-drop)");
    println!("       - 700+ pre-built connectors");
    println!("       - JavaScript steps for advanced logic");
    println!("       - Branching, looping, error handling");
    println!("       - JSONata + custom code for data transformation");
    println!("    2. Tray Merlin (2024 — the AI-era pivot):");
    println!("       - LLM-powered automation design from natural language");
    println!("       - 'Merlin Agents' = autonomous LLM agents using Tray tools");
    println!("       - Conversational interface to enterprise data (RAG-style)");
    println!("       - Big strategic bet on the GenAI automation thesis");
    println!("    3. Tray Embedded (the OEM business — material revenue):");
    println!("       - SaaS vendors embed Tray as integration layer for their customers");
    println!("       - White-label + co-branded options");
    println!("       - Customers: Snowflake, Atlassian, Lattice, Sprinklr embed Tray");
    println!("       - Competes head-to-head with Workato Embedded");
    println!("    4. Universal Connector Builder:");
    println!("       - Build custom connectors via OpenAPI/REST/SOAP definitions");
    println!("       - Share custom connectors org-wide");
    println!("    5. Tray Observability + Logs:");
    println!("       - Workflow run history, error logs, performance metrics");
    println!("       - Lighter than enterprise iPaaS observability");
    println!("    6. API Gateway (lighter API mgmt than MuleSoft/Kong):");
    println!("       - Expose workflows as REST APIs");
    println!("       - Auth + throttling + versioning");
    println!("  Tray Merlin strategy (the big bet):");
    println!("    - Pivoted positioning in 2023-2024 to 'GenAI-native iPaaS'");
    println!("    - Merlin = LLM agent platform that uses Tray workflows as 'tools'");
    println!("    - Natural-language workflow generation + AI-augmented business processes");
    println!("    - Aligned with the 'AI agents that do work' narrative");
    println!("    - High-variance bet: success = category leadership; failure = lost focus on classic iPaaS");
    println!("  Integrations (700+ connectors):");
    println!("    - SaaS: Salesforce, HubSpot, NetSuite, Workday, ServiceNow, Slack, Microsoft 365");
    println!("    - Marketing: Marketo, Eloqua, Mailchimp, Pardot, Iterable, Braze");
    println!("    - CRM: Salesforce (deep), HubSpot, Pipedrive, Close");
    println!("    - Database: PostgreSQL, MySQL, Snowflake, BigQuery, Redshift, MongoDB");
    println!("    - Cloud: AWS (S3, Lambda, SQS, SNS), Azure, GCP");
    println!("    - Messaging: Slack, Teams, Discord, SendGrid, Twilio");
    println!("    - Data: Snowflake, Databricks, Segment, Census, Hightouch");
    println!("    - DevOps: GitHub, GitLab, Jira, Linear, PagerDuty");
    println!("    - AI: OpenAI, Anthropic, Pinecone, Weaviate (recent AI-stack integrations)");
    println!("  Tray CLI usage:");
    println!("    tray login --account my-workspace");
    println!("    tray workflow list --tag production");
    println!("    tray workflow deploy --workflow-id ABC123 --env prod");
    println!("    tray workflow run --workflow-id ABC123 --input @input.json");
    println!("    tray connector test --name salesforce");
    println!("    tray merlin chat --workflow 'lead-routing'");
    println!("    tray embedded customer create --name acme-corp --plan standard");
    println!("  Customers (~1,500+ direct + millions via Embedded):");
    println!("    - Direct: Coca-Cola, Sky, Bose, Lyft, IBM, Cognizant, AT&T");
    println!("    - Embedded partners: Snowflake, Atlassian, Lattice, Sprinklr, Outreach");
    println!("    - Strong in: marketing ops, RevOps, customer ops use cases");
    println!("    - International: heavy in UK + Europe (London heritage)");
    println!("    - Tech-company sweet spot (similar to Workato)");
    println!("  Critique: 2023-2024 layoffs + restructuring around Merlin pivot");
    println!("           down-round risk if Merlin doesn't land");
    println!("           classic iPaaS feature parity not as strong as Workato");
    println!("           connector count (700) below Workato (1,000) and Zapier (7,000)");
    println!("           Embedded business material but margin profile uncertain");
    println!("           valuation pressure private market: $1.3B unicorn likely needs to grow into");
    println!("           AI/Merlin a follower-not-leader move — OpenAI + Anthropic own the LLM layer");
    println!("           competition with Workato Embedded particularly intense");
    println!("  Differentiator: 'general automation platform' positioning + Tray Embedded (large OEM business — SaaS vendors embed Tray for their integrations) + Tray Merlin (early GenAI-native iPaaS bet, 2024) + JavaScript steps for developer-friendly automation + 700+ connectors + UK + US dual HQ — the developer-friendly iPaaS that's betting on the AI-agent future and has carved out a material OEM business white-labeling integrations for SaaS vendors");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tray".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tray(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
