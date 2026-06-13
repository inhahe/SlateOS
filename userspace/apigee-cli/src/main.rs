#![deny(clippy::all)]
//! apigee-cli — Slate OS Google Apigee API management personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Google Apigee enterprise API management.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about          Apigee's story from Sonoa Systems to Google");
    println!("    products       API Platform, hybrid, X (modern), Edge legacy");
    println!("    architecture   Proxies, policies, flows, Message Processors");
    println!("    pricing        Apigee X / hybrid / Pay-As-You-Go pricing");
    println!("    customers      Walgreens, Walgreens, US Bank, AT&T, Pearson");
    println!("    differentiator Why enterprises pick Apigee over Kong/Mulesoft");
    println!("    critique       Honest limitations of the Apigee approach");
    println!("    help / version");
}

fn print_about() {
    println!("Apigee — Google Cloud's enterprise API management platform.");
    println!();
    println!("Founded 2004 in Palo Alto as Sonoa Systems by Raj Singh and a small");
    println!("team building a hardware appliance for XML/SOAP acceleration —");
    println!("essentially an SSL+XML offload box for the era when SOAP web");
    println!("services were peaking. Renamed Apigee in 2010 as the product pivoted");
    println!("from appliance to software API management platform, riding the");
    println!("REST/JSON wave.");
    println!();
    println!("IPO on NASDAQ:APIC April 2015, raising $87M at ~$507M valuation.");
    println!("Public for ~16 months before Google announced the $625M acquisition");
    println!("in September 2016 — at the time, Google's largest enterprise");
    println!("software purchase. The strategic logic: Google Cloud needed an");
    println!("enterprise API story to compete with AWS API Gateway and Azure");
    println!("API Management, and building one from scratch would lose years.");
    println!();
    println!("Today Apigee is Google Cloud's flagship API management product,");
    println!("part of the Application Modernization portfolio. Two SKUs: Apigee");
    println!("X (modern, fully cloud-managed on GCP) and Apigee hybrid (control");
    println!("plane in GCP, runtime in your Kubernetes — on prem or any cloud).");
    println!("The legacy 'Apigee Edge' on-prem product is end-of-life with");
    println!("migrations strongly encouraged to Apigee X.");
}

fn print_products() {
    println!("Apigee product line:");
    println!();
    println!("• Apigee X");
    println!("    Fully managed on Google Cloud. Built on GKE + Anthos under the");
    println!("    covers but presented as a SaaS. Multi-region active-active, MIG-");
    println!("    based scaling, Envoy-based runtime, deep BigQuery + Looker");
    println!("    analytics integration. Targets new Apigee customers in 2025.");
    println!();
    println!("• Apigee hybrid");
    println!("    Control plane (UI, analytics, policy editor) runs in Apigee X");
    println!("    cloud; runtime ('Message Processors' + ingress) runs in your");
    println!("    GKE/EKS/AKS/Anthos clusters. For regulated industries that");
    println!("    require traffic never to leave their VPC.");
    println!();
    println!("• Apigee Edge (legacy, EOL)");
    println!("    Pre-Google architecture. Java-based runtime, Cassandra +");
    println!("    Zookeeper data plane. Public Cloud, Private Cloud (on-prem),");
    println!("    OPDK (Operating Platform Developer Kit) variants. Public Cloud");
    println!("    Edge sunsets in stages through 2026.");
    println!();
    println!("• API hub (newer)");
    println!("    Catalog and discovery across ALL APIs in your org, not just");
    println!("    Apigee-managed ones. Auto-discovers AWS API Gateway, Mulesoft,");
    println!("    Azure APIM endpoints. Single inventory for governance.");
    println!();
    println!("• Cloud API Gateway (separate product)");
    println!("    Lightweight, serverless API gateway for Cloud Run / Cloud");
    println!("    Functions backends. Not Apigee — for teams that don't need");
    println!("    full enterprise API management.");
}

fn print_architecture() {
    println!("Apigee architecture concepts:");
    println!();
    println!("• Organization: top-level tenant. Usually one per enterprise.");
    println!("• Environments: dev, test, prod isolation within an org.");
    println!("• Environment Groups (Apigee X): hostname routing across envs.");
    println!("• API Proxies: the unit of deployment. A proxy fronts a backend.");
    println!("• Proxy Endpoints: incoming-traffic configuration (hostname, path).");
    println!("• Target Endpoints: outgoing-traffic configuration (backend URL).");
    println!("• Flows: request/response pipeline stages where policies attach.");
    println!("• Policies: pre-built XML-configured logic units (~50+ types):");
    println!("    OAuthV2, VerifyAPIKey, Quota, SpikeArrest, JSONThreatProtection,");
    println!("    XMLThreatProtection, AssignMessage, ExtractVariables, JSON-to-");
    println!("    XML, XSLTransform, ServiceCallout, JavaScript, JavaCallout,");
    println!("    PythonScript, ResponseCache, KeyValueMapOperations, MessageLogging,");
    println!("    GenerateJWT, VerifyJWT, RaiseFault, FlowCallout, AccessControl,");
    println!("    BasicAuthentication, etc.");
    println!("• Shared Flows: reusable policy chains imported into proxies.");
    println!("• KVM (Key-Value Maps): encrypted config storage at org/env/proxy.");
    println!("• Developer Portal: external-facing API catalog with self-service");
    println!("    app registration and API key issuance.");
    println!();
    println!("The mental model: think of an Apigee proxy as a configurable HTTP");
    println!("middleware chain expressed in XML, where you compose pre-built");
    println!("policy nodes into a flow graph. Custom logic via JavaScript,");
    println!("Java callouts, or Python scripts when policies don't suffice.");
}

fn print_pricing() {
    println!("Apigee pricing (USD list, indicative — enterprise negotiates):");
    println!();
    println!("• Apigee X Standard");
    println!("    ~$20K/month entry-level annual commit");
    println!("    Includes ~180M API calls/month, basic analytics");
    println!();
    println!("• Apigee X Enterprise");
    println!("    ~$30K-50K/month for production-scale workloads");
    println!("    Higher call volumes, advanced security, monetization features");
    println!();
    println!("• Apigee X Enterprise Plus");
    println!("    Custom pricing, typically $100K+/month for large enterprises");
    println!("    Dedicated SLO, support, multi-region, custom monetization");
    println!();
    println!("• Apigee Pay-As-You-Go (2023+)");
    println!("    $20/M API calls (first 50M free/month)");
    println!("    No annual commit, GCP-billed, designed for cloud-native teams");
    println!("    that found classic Apigee pricing prohibitive");
    println!();
    println!("• Apigee hybrid runtime");
    println!("    Same control-plane fees + you pay for your own K8s compute");
    println!();
    println!("Honest take: Apigee is enterprise-priced. If you're a startup or");
    println!("you treat $20K/mo as 'not nothing,' look at Kong, Tyk, KrakenD,");
    println!("or roll your own with Envoy. Apigee earns its price for Fortune");
    println!("1000 customers who need deep monetization, robust analytics, and");
    println!("a vendor with Google's enterprise contract muscle behind it.");
}

fn print_customers() {
    println!("Apigee customer references (public):");
    println!();
    println!("  • Walgreens — pharmacy API platform fronting 9K+ stores");
    println!("  • US Bank — open banking and partner APIs");
    println!("  • AT&T — telecom B2B APIs (number lookup, location services)");
    println!("  • Pearson — education content APIs");
    println!("  • The Home Depot — store-locator, inventory, checkout APIs");
    println!("  • Bechtel — engineering systems integration");
    println!("  • Magazine Luiza (Brazil) — retail platform APIs");
    println!("  • Equinix — colocation API for hybrid cloud workflows");
    println!("  • TELUS — Canadian telecom developer portal");
    println!("  • Lloyds Banking Group — UK open banking compliance");
    println!();
    println!("Pattern: large enterprises with hundreds-to-thousands of APIs,");
    println!("strong compliance posture (PCI, HIPAA, PSD2 open banking), and");
    println!("the engineering staff to operate the platform. Not for startups.");
}

fn print_differentiator() {
    println!("Why enterprises pick Apigee:");
    println!();
    println!("vs. AWS API Gateway:");
    println!("  • Apigee is cloud-agnostic (hybrid runs anywhere); AWS APIGW is");
    println!("    locked to AWS. Multi-cloud enterprises pick Apigee");
    println!("  • Apigee's analytics + monetization is dramatically richer");
    println!("  • AWS APIGW is cheaper per call but lacks developer portal,");
    println!("    monetization, advanced policy chaining");
    println!();
    println!("vs. Kong:");
    println!("  • Kong is open-source, faster, dev-friendly, cloud-native first");
    println!("  • Apigee has deeper enterprise features (monetization,");
    println!("    developer portal, fine-grained analytics) and Google's");
    println!("    enterprise support");
    println!("  • Kong wins for greenfield microservices; Apigee wins for");
    println!("    'we have 800 SOAP services and a partner API channel'");
    println!();
    println!("vs. Mulesoft Anypoint:");
    println!("  • Mulesoft is integration-first (iPaaS + APIs); Apigee is");
    println!("    API-management first");
    println!("  • Both expensive enterprise tier; Salesforce ownership of");
    println!("    Mulesoft pushes integration into the CRM ecosystem");
    println!();
    println!("vs. Azure API Management:");
    println!("  • Azure APIM is cheaper and tightly integrated with Azure");
    println!("  • Apigee runtime is more battle-tested, analytics more mature");
    println!();
    println!("Apigee's enterprise differentiators:");
    println!("  • API monetization built-in (rate plans, billing, settlements)");
    println!("  • Developer portal with self-service onboarding");
    println!("  • Deep BigQuery analytics — 100+ pre-built reports");
    println!("  • Google's enterprise sales and support muscle");
    println!("  • Hybrid runtime for sovereignty / data-residency");
    println!("  • Mature SOAP + REST + GraphQL + gRPC support");
}

fn print_critique() {
    println!("Honest critique of Apigee:");
    println!();
    println!("• XML-everywhere config. Policies are XML; proxies are XML bundles.");
    println!("  In 2025 this feels archaic compared to YAML/HCL/CRD-first");
    println!("  competitors. Apigee has added some YAML and a CLI, but the");
    println!("  core is still XML.");
    println!();
    println!("• Steep learning curve. The flow/policy/shared-flow/KVM mental");
    println!("  model takes weeks to internalize. Teams hire dedicated 'Apigee");
    println!("  engineers' or consultants. Total cost of ownership is high.");
    println!();
    println!("• Expensive at any volume. Pay-As-You-Go helped, but if you're");
    println!("  doing >1B calls/month the bills get attention-grabbing.");
    println!();
    println!("• Hybrid is operationally complex. You're running a K8s-based");
    println!("  Envoy fleet that you have to monitor, patch, and scale.");
    println!("  'Hybrid' isn't 'easy mode.'");
    println!();
    println!("• Edge legacy migration is painful. Customers on Apigee Edge");
    println!("  Public Cloud face a migration to Apigee X that involves");
    println!("  rewriting custom policies, retesting flows, re-issuing");
    println!("  developer keys.");
    println!();
    println!("• Google Cloud lock-in. Apigee X runs only on GCP. Hybrid lets");
    println!("  you run runtime anywhere but the control plane is GCP-only.");
    println!();
    println!("• Roadmap velocity slower than cloud-native open-source");
    println!("  alternatives (Kong, Envoy/Istio). New protocol support and");
    println!("  developer-experience improvements arrive on enterprise time.");
}

fn run_apigee(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
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
        .unwrap_or_else(|| "apigee".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_apigee(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/apigee"), "apigee"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("apigee.exe"), "apigee"); }
    #[test] fn t_help() { assert_eq!(run_apigee(&[], "apigee"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_apigee(&["xx".to_string()], "apigee"), 2); }
}
