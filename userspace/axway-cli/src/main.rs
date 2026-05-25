#![deny(clippy::all)]
//! axway-cli — OurOS Axway Amplify Platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Axway Amplify enterprise integration + API platform.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Axway's MFT roots and API mgmt evolution");
    println!("    products        Amplify, API Manager, MFT, Embedded Analytics");
    println!("    amplify         Amplify Platform unified portfolio");
    println!("    mft             Managed File Transfer heritage");
    println!("    pricing         Enterprise subscription");
    println!("    customers       Financial services and government");
    println!("    differentiator  MFT + API mgmt + B2B integration combined");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Axway — French-American enterprise integration veteran.");
    println!();
    println!("Spun out of Sopra Group in 2001 with headquarters in Phoenix,");
    println!("Arizona and Annecy/Puteaux, France. Axway's roots go further back");
    println!("to the early Tumbleweed Communications (founded 1993, US) and");
    println!("Cyclone Commerce — both pioneers in Managed File Transfer (MFT)");
    println!("and B2B integration. The current Axway is the result of several");
    println!("mergers and acquisitions across the 2000s consolidating the MFT");
    println!("and integration middleware space:");
    println!();
    println!("  • 2008: Axway acquires Tumbleweed Communications (~$143M)");
    println!("  • 2014: Acquires Systar (BAM/business activity monitoring)");
    println!("  • 2016: Acquires Appcelerator (mobile app dev) for ~$37.5M");
    println!("  • 2017: Acquires Syncplicity (file sync/share)");
    println!("  • 2024+: Privatization / take-private discussions");
    println!();
    println!("Axway is listed on Euronext Paris (AXW.PA). Sopra Steria remains");
    println!("a major shareholder. Revenue ~€300M annually with strong recurring");
    println!("subscription base from financial-services and government customers.");
    println!();
    println!("The Amplify Platform launched 2017 to unify Axway's portfolio");
    println!("(API mgmt, MFT, B2B integration, analytics) under a single cloud-");
    println!("based catalog and governance plane.");
}

fn print_products() {
    println!("Axway product portfolio:");
    println!();
    println!("• Amplify Platform");
    println!("    Unified governance, catalog, and analytics across all Axway");
    println!("    products. SaaS control plane that meshes with on-prem or");
    println!("    cloud-deployed runtimes.");
    println!();
    println!("• Amplify API Management");
    println!("    Full API management — gateway, developer portal, lifecycle,");
    println!("    monetization. Based on the API Gateway product from the");
    println!("    Vordel acquisition (2012). Java-based runtime, mature.");
    println!();
    println!("• Amplify B2B Integration");
    println!("    EDI, AS2, AS4, OFTP2, X12, EDIFACT trading partner integration.");
    println!("    Decades of B2B protocol support. Critical infrastructure");
    println!("    for retailers, manufacturers, logistics, healthcare.");
    println!();
    println!("• Amplify Managed File Transfer (MFT)");
    println!("    Secure file transfer at enterprise scale. The product");
    println!("    originally called SecureTransport (Tumbleweed acquisition).");
    println!("    SFTP, FTPS, HTTPS, AS2, OFTP. Audit, encryption, workflow");
    println!("    automation. Heavily used in financial services and supply");
    println!("    chain.");
    println!();
    println!("• Amplify Open Banking");
    println!("    PSD2 / Open Banking compliance accelerator. Built on the");
    println!("    API mgmt layer with regulatory templates and the EBA RTS");
    println!("    technical standards baked in.");
    println!();
    println!("• Amplify Embedded Analytics (Axway BAM)");
    println!("    Business Activity Monitoring across the integration");
    println!("    landscape — track SLAs, file delivery, API usage, partner");
    println!("    activity in real time.");
    println!();
    println!("• Amplify Application Integration (App-to-App)");
    println!("    iPaaS-style integration runtime for connecting SaaS apps");
    println!("    and on-prem systems. Smaller footprint than Mulesoft.");
}

fn print_amplify() {
    println!("Amplify Platform — the unifying layer.");
    println!();
    println!("Conceptually, Amplify Platform is what Axway calls 'the");
    println!("hybrid integration platform' — a SaaS control plane that");
    println!("federates many runtimes (API Manager, MFT, B2B) and provides:");
    println!();
    println!("  • Unified Catalog: discover and govern all assets across the");
    println!("    landscape (APIs, file flows, trading-partner agreements,");
    println!("    integrations). Catalog is the platform's source of truth.");
    println!();
    println!("  • Marketplace: external-facing self-service catalog where");
    println!("    consumers (developers, partners, even other internal teams)");
    println!("    discover and subscribe to assets.");
    println!();
    println!("  • Governance: assign owners, lifecycle stages (planned, in-");
    println!("    use, deprecated), approval workflows, audit trails.");
    println!();
    println!("  • Engagement: developer-portal-style documentation, sandbox");
    println!("    keys, support ticketing per asset.");
    println!();
    println!("  • Integration Builder: low-code integration designer for");
    println!("    composing SaaS-to-SaaS flows.");
    println!();
    println!("  • API Builder: code-first or visual builder for hosting REST");
    println!("    APIs on Amplify's serverless runtime.");
    println!();
    println!("  • API Mocking & Testing: Stoplight-style mocking integrated.");
    println!();
    println!("Amplify connects to existing API gateways from competitors too:");
    println!("AWS API Gateway, Azure APIM, Apigee, Kong. The 'agentic'");
    println!("discovery model imports their catalogs into Amplify governance.");
}

fn print_mft() {
    println!("Axway's MFT (Managed File Transfer) heritage.");
    println!();
    println!("MFT is the unsexy but critical backbone of B2B commerce. When");
    println!("a bank settles overnight payment files with another bank, when");
    println!("a retailer sends EDI POs to thousands of suppliers, when a");
    println!("healthcare clearinghouse moves claims between providers and");
    println!("payers — it's MFT moving the files. Axway has been in this");
    println!("market for 30+ years across multiple product names.");
    println!();
    println!("Capabilities:");
    println!("  • Protocols: SFTP, FTPS, HTTPS, AS1/AS2/AS3/AS4, OFTP2,");
    println!("    PeSIT (French banking), Connect:Direct (formerly Sterling)");
    println!("  • Trading-partner agreements with per-partner policies");
    println!("  • End-to-end encryption (PGP, S/MIME)");
    println!("  • Hub-and-spoke topology for thousands of trading partners");
    println!("  • Workflow automation: transforms, virus scans, delivery");
    println!("    routing, fallback paths");
    println!("  • Detailed audit trails for compliance (SOX, PCI, HIPAA)");
    println!("  • High availability and disaster recovery configurations");
    println!("  • SLA enforcement with notification");
    println!();
    println!("Competitors in this space: IBM Sterling B2B Integrator, OpenText");
    println!("Trading Grid, GoAnywhere MFT, Globalscape EFT, IBM Connect:Direct");
    println!("(legacy). Axway is among the top 2-3 in IDC/Gartner MFT");
    println!("evaluations. Customers tend to be sticky for decades because");
    println!("re-platforming MFT is operationally risky.");
}

fn print_pricing() {
    println!("Axway pricing.");
    println!();
    println!("Axway is enterprise-only. No published list prices, no SaaS");
    println!("self-serve sign-up. Sales-led only.");
    println!();
    println!("Indicative tiers (from industry analyst comparisons):");
    println!();
    println!("  • Amplify API Management subscription: $50K-$500K+/year");
    println!("    depending on transaction volume, environments, and");
    println!("    enterprise features (multi-tenant, monetization, SSO).");
    println!();
    println!("  • Amplify MFT: $75K-$1M+/year depending on partner count,");
    println!("    file volume, and HA topology. Some large banks spend $5M+");
    println!("    per year on MFT alone.");
    println!();
    println!("  • Amplify B2B Integration: similar scale to MFT, often");
    println!("    bundled together.");
    println!();
    println!("  • Amplify Platform (the control plane): included with major");
    println!("    runtime subscriptions or available standalone for");
    println!("    governing multi-vendor landscapes.");
    println!();
    println!("Typical TCO for an enterprise running multiple Amplify modules:");
    println!("$500K-$5M+/year. Not a startup product. Customers usually have");
    println!("multi-year framework agreements with Axway, sometimes 5-10");
    println!("year deals tied to capital projects.");
}

fn print_customers() {
    println!("Axway customer references (public):");
    println!();
    println!("  • BNP Paribas — banking integration, payment files");
    println!("  • Société Générale — investment banking MFT and APIs");
    println!("  • HSBC — global MFT for payment networks");
    println!("  • Credit Agricole — banking integration");
    println!("  • US Department of Defense (multiple agencies) — secure file");
    println!("    transfer between federated systems");
    println!("  • US Air Force, US Navy — supply-chain integration");
    println!("  • UK Department of Work and Pensions");
    println!("  • Renault, PSA (Stellantis) — automotive supply chain EDI");
    println!("  • Carrefour — retail trading partner integration");
    println!("  • SNCF — railway logistics MFT");
    println!("  • Lloyd's of London — insurance market integration");
    println!("  • Multiple national clearinghouses and payment networks");
    println!();
    println!("Pattern: large enterprises with regulated workloads, established");
    println!("trading-partner networks, multi-decade compliance obligations.");
    println!("Heavy European presence, growing US federal footprint.");
}

fn print_differentiator() {
    println!("Why enterprises pick Axway:");
    println!();
    println!("• MFT depth. Axway's protocol breadth and B2B integration");
    println!("  maturity is unmatched outside IBM Sterling. Decades of");
    println!("  regulatory and compliance battle-testing.");
    println!();
    println!("• Unified platform across MFT + API + integration. Single");
    println!("  vendor, single support contract, single governance plane");
    println!("  for the entire integration estate. Reduces operational");
    println!("  complexity for large enterprises.");
    println!();
    println!("• Strong financial-services and government presence. Existing");
    println!("  procurement frameworks at major banks and federal agencies");
    println!("  ease addition of new Axway modules.");
    println!();
    println!("• European headquarters (partial). Important for EU data");
    println!("  residency and procurement in some sectors.");
    println!();
    println!("• Hybrid deployment maturity. Axway has long supported");
    println!("  customer-premise, hosted, and hybrid topologies. Less");
    println!("  cloud-first than cloud-natives but better cloud-or-prem");
    println!("  than pure-cloud vendors.");
    println!();
    println!("vs. IBM Sterling: closest direct competitor. Axway has more");
    println!("  modern API mgmt; Sterling has deeper mainframe + EDI VAN");
    println!("  integration. Many customers run both for political/historical");
    println!("  reasons.");
    println!();
    println!("vs. Apigee/Mulesoft: those are API-mgmt focused. Axway covers");
    println!("  MFT + B2B + APIs in one platform — strong fit if you need");
    println!("  more than just APIs.");
    println!();
    println!("vs. open-source (Kong, Gravitee): Axway is more expensive and");
    println!("  less modern but provides MFT and B2B that OSS vendors don't.");
}

fn print_critique() {
    println!("Honest critique of Axway:");
    println!();
    println!("• Older codebases. The API Gateway product (Vordel heritage)");
    println!("  is Java + AppServer-era technology. UI feels dated. Modern");
    println!("  cloud-native shops experience friction with the operational");
    println!("  model.");
    println!();
    println!("• Cloud-native maturity lags newer vendors. Kubernetes operators");
    println!("  exist but the products weren't born cloud-native; some");
    println!("  components still feel VM-shaped.");
    println!();
    println!("• Pricing opaque and enterprise-only. No way to evaluate by");
    println!("  small teams or startups. Long sales cycles.");
    println!();
    println!("• Brand awareness in the modern developer ecosystem is");
    println!("  minimal. The product is well-known to enterprise architects");
    println!("  and integration specialists; not so much to Hacker News.");
    println!();
    println!("• Documentation is enterprise-style. Comprehensive but dense");
    println!("  and assumes you're going through Axway services / partners.");
    println!("  Self-service learning is harder than with developer-first");
    println!("  vendors.");
    println!();
    println!("• Product proliferation. Many overlapping or evolving SKUs.");
    println!("  Names change (Amplify, AMPLIFY, etc.). Hard to know what to");
    println!("  buy if you're new to the portfolio.");
    println!();
    println!("• Take-private discussions and shareholder activity have");
    println!("  created uncertainty about long-term product strategy.");
    println!();
    println!("• Not the best for greenfield. For new cloud-native API");
    println!("  programs starting in 2025, Kong / Apigee / Zuplo / Kong");
    println!("  Konnect / Gravitee are likely better starting points unless");
    println!("  you specifically need Axway's MFT or B2B integration.");
}

fn run_axway(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "amplify" => { print_amplify(); 0 }
        "mft" => { print_mft(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for usage.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "axway".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_axway(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/axway"), "axway"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("axway.exe"), "axway"); }
    #[test] fn t_help() { assert_eq!(run_axway(&[], "axway"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_axway(&["xx".to_string()], "axway"), 2); }
}
