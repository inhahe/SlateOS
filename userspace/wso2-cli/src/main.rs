#![deny(clippy::all)]
//! wso2-cli — OurOS WSO2 open-source integration platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — WSO2 open-source integration and identity platform.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           WSO2's Sri Lanka origin and Apache roots");
    println!("    products        API Manager, Identity Server, Micro Integrator, Choreo, Ballerina");
    println!("    apim            WSO2 API Manager deep dive");
    println!("    ballerina       The Ballerina integration language");
    println!("    choreo          WSO2 Choreo iPaaS");
    println!("    customers       Public sector and enterprise references");
    println!("    differentiator  Apache-licensed, integration-first heritage");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("WSO2 — open-source middleware out of Colombo, Sri Lanka.");
    println!();
    println!("Founded 2005 by Dr. Sanjiva Weerawarana and Paul Fremantle in");
    println!("Colombo. Sanjiva was a key Apache contributor and architect of");
    println!("Apache Axis (the SOAP stack) at IBM Research before returning");
    println!("home to Sri Lanka to start WSO2. Paul came from IBM's Web Services");
    println!("group and had been a contributor to the WS-* specifications era.");
    println!();
    println!("WSO2's founding thesis: enterprise middleware (IBM WebSphere,");
    println!("Oracle Fusion, TIBCO) was overpriced and locked-in. Apache");
    println!("foundation projects were the technology future, and a company");
    println!("could build a profitable enterprise-support business around");
    println!("100% Apache-licensed middleware. They committed: every WSO2");
    println!("product ships under Apache 2.0, no open-core games.");
    println!();
    println!("Funding history: relatively bootstrapped early years on consulting");
    println!("and support revenue. Series D 2018 ~$30M led by Pacific Controls,");
    println!("then a $93M growth round 2022 led by EQT Growth and Info Edge,");
    println!("valuing WSO2 around $600M. Headquartered Colombo with offices");
    println!("in Mountain View, NYC, São Paulo, London, Sydney.");
    println!();
    println!("Sanjiva passed away unexpectedly in 2022. His legacy continues");
    println!("at WSO2 under CEO Sanjiva Weerawarana's chosen successor Eric");
    println!("Newcomer (formerly IBM, Iona, Credit Suisse) and the engineering");
    println!("leadership he mentored over two decades.");
}

fn print_products() {
    println!("WSO2 product portfolio:");
    println!();
    println!("• WSO2 API Manager (APIM)");
    println!("    Full API management platform — gateway, publisher portal,");
    println!("    developer portal, key manager, traffic manager, analytics.");
    println!("    Built on top of WSO2's own Carbon/Synapse Apache foundation.");
    println!();
    println!("• WSO2 Identity Server (IS)");
    println!("    Open-source IdP. SAML 2.0, OIDC, OAuth 2.0, WS-Federation,");
    println!("    multi-factor authentication, account management, federated");
    println!("    identity bridging across multiple IdPs.");
    println!();
    println!("• WSO2 Micro Integrator (MI)");
    println!("    Lightweight integration runtime — successor to WSO2 ESB");
    println!("    (Enterprise Service Bus). Apache Synapse-based, Camel-like");
    println!("    mediation, ~100 connectors. Cloud-native packaging.");
    println!();
    println!("• WSO2 Choreo");
    println!("    Cloud-native iPaaS. SaaS service for building and running");
    println!("    integrations, APIs, and services. Built on the open-source");
    println!("    stack plus a managed cloud control plane.");
    println!();
    println!("• Ballerina");
    println!("    Open-source programming language designed by WSO2 specifically");
    println!("    for integration and network-native services. Apache 2.0,");
    println!("    governed independently via the Ballerina Foundation.");
    println!();
    println!("• WSO2 Streaming Integrator");
    println!("    SQL-based stream processing (formerly WSO2 Stream Processor).");
    println!("    Builds on Siddhi engine for complex event processing.");
    println!();
    println!("• WSO2 Open Banking");
    println!("    PSD2/CDR/Open Banking Brasil compliance accelerator. Layered");
    println!("    on top of APIM + Identity Server with regulatory templates.");
}

fn print_apim() {
    println!("WSO2 API Manager — flagship API management product.");
    println!();
    println!("Architecture:");
    println!("  • Gateway: traffic-handling runtime. Apache Synapse-based,");
    println!("    sequence mediation, message transformation.");
    println!("  • Publisher Portal: where API providers design, deploy, and");
    println!("    govern APIs. Lifecycle: CREATED → PUBLISHED → DEPRECATED.");
    println!("  • Developer Portal (formerly Store): consumer-facing catalog.");
    println!("    Subscribe to APIs, manage applications and keys.");
    println!("  • Key Manager: token issuance, validation, introspection.");
    println!("    Can delegate to WSO2 Identity Server or external IdPs");
    println!("    (Okta, Auth0, ForgeRock, Keycloak).");
    println!("  • Traffic Manager: distributed rate-limit policy enforcement.");
    println!("  • Analytics: ELK-based or pluggable. SQL-queryable streams.");
    println!();
    println!("Capabilities:");
    println!("  • REST, SOAP, GraphQL, WebSocket, Server-Sent Events APIs");
    println!("  • OAuth2 / OIDC / Basic / API key / mTLS authentication");
    println!("  • Rate limiting, spike arrest, quota");
    println!("  • Message mediation (XSLT, JSON-XML transforms, header manip)");
    println!("  • Service Discovery (Consul, Etcd) backend integration");
    println!("  • OpenAPI 3.x and AsyncAPI authoring + import");
    println!("  • Multi-tenancy at the gateway level");
    println!("  • Deployment: VM, Docker, Kubernetes (Helm charts published)");
    println!();
    println!("Notable in API Manager 4.x: control plane / data plane split,");
    println!("allowing the gateway data plane to deploy independently from");
    println!("the publisher/portal/key-manager control plane.");
}

fn print_ballerina() {
    println!("Ballerina — WSO2's integration-native programming language.");
    println!();
    println!("First announced 2017, hit 1.0 in September 2019. Now governed by");
    println!("the Ballerina Foundation as an independent open-source project,");
    println!("with WSO2 as primary contributor. Apache 2.0.");
    println!();
    println!("Language thesis: integration code (REST clients, message");
    println!("transformations, service compositions, retry loops, error");
    println!("handling) is awkward in general-purpose languages. Ballerina");
    println!("makes networking, distribution, and integration first-class:");
    println!();
    println!("  • Network types: service {{ resource function get hello() }}");
    println!("  • Visual representation: every Ballerina program is also a");
    println!("    sequence diagram. The IDE renders code↔diagram in real time.");
    println!("  • Strong static typing with structural typing");
    println!("  • Built-in JSON, XML as first-class data types");
    println!("  • Async/await semantics via 'workers' and 'strands'");
    println!("  • Compiles to JVM bytecode; experimental native via LLVM");
    println!("  • Choreography-first error handling: check + checkpanic");
    println!("  • Cloud-native deployment annotations: @kubernetes:Deployment");
    println!();
    println!("Niche but loyal user base. Not displacing Go/TS/Java broadly, but");
    println!("for teams doing API-orchestration heavy work it's a quietly");
    println!("excellent fit. Powers internals of WSO2 Choreo.");
}

fn print_choreo() {
    println!("WSO2 Choreo — cloud-native iPaaS.");
    println!();
    println!("Launched 2021 as WSO2's bet on a fully managed SaaS integration");
    println!("platform competing with MuleSoft Anypoint, Boomi, Workato, and");
    println!("Azure Logic Apps. Built on top of the open-source stack:");
    println!("  • Choreo runs Ballerina + APIM + Identity Server in managed K8s");
    println!("  • Multi-cloud target — runs on Azure, AWS, GCP, or your own K8s");
    println!("  • Per-organization tenancy with environments and projects");
    println!();
    println!("Capabilities:");
    println!("  • Build services and integrations in Ballerina, Java, Python,");
    println!("    Node.js, .NET, or visual no-code");
    println!("  • Auto-generated CI/CD pipelines, observability, logs");
    println!("  • API-led architecture with managed API gateway");
    println!("  • AI gateway features (LLM routing, prompt management, token");
    println!("    metering) added 2024");
    println!("  • Identity integration via Asgardeo (WSO2's IDaaS — Identity");
    println!("    Server as SaaS)");
    println!();
    println!("Pricing: free tier with limits, then per-active-component +");
    println!("per-API-call usage pricing. Tier-2 monthly minimums.");
    println!();
    println!("Adoption: still earlier than WSO2's on-prem products. Strongest");
    println!("in markets where WSO2 has existing enterprise relationships");
    println!("(South Asia, Middle East, Brazil, parts of Europe).");
}

fn print_customers() {
    println!("WSO2 customer references:");
    println!();
    println!("  • Government of Sri Lanka — national digital identity platform");
    println!("  • National Health Service (UK) — multiple trusts");
    println!("  • eBay — internal API management at scale");
    println!("  • UC Berkeley — campus identity and API platform");
    println!("  • Trimble — construction and agriculture tech APIs");
    println!("  • State of California (multiple departments)");
    println!("  • National Australia Bank — open banking compliance");
    println!("  • Banco Bradesco (Brazil) — open banking + integration");
    println!("  • DHL Supply Chain — logistics integration");
    println!("  • Etihad Airways — airline platform integration");
    println!("  • Verizon — telco partner APIs");
    println!();
    println!("Pattern: heavy public-sector adoption (Apache licensing avoids");
    println!("procurement friction), telcos and banks doing open-X compliance,");
    println!("and large enterprises in regions where WSO2's local presence");
    println!("and Apache transparency carry weight.");
}

fn print_differentiator() {
    println!("Why teams pick WSO2:");
    println!();
    println!("• 100% Apache 2.0 — no open-core trickery. Every WSO2 product");
    println!("  ships fully featured under Apache. The company makes money on");
    println!("  support subscriptions, not on hidden enterprise features.");
    println!();
    println!("• Integration heritage. WSO2 grew from ESB roots, so message");
    println!("  mediation, protocol transformation, and orchestration are");
    println!("  deeply mature — areas where pure API gateways are thin.");
    println!();
    println!("• Identity Server is a serious Keycloak alternative. Some teams");
    println!("  pick WSO2 just for IS, paired with their existing API stack.");
    println!();
    println!("• Public-sector friendly. Apache licensing avoids many");
    println!("  procurement and FOSS-policy issues that block proprietary");
    println!("  vendors in government contracts.");
    println!();
    println!("• Choreo gives a managed option for teams that don't want to");
    println!("  run the stack themselves.");
    println!();
    println!("• Strong APAC and EMEA presence. Local-language support and");
    println!("  partnerships in markets where US vendors have thinner reach.");
    println!();
    println!("vs. Kong: WSO2 has integration mediation (ESB roots) and IdP");
    println!("  built-in; Kong has more polished dev-experience and a more");
    println!("  active plugin marketplace.");
    println!();
    println!("vs. Apigee/Mulesoft: WSO2 is open-source and dramatically");
    println!("  cheaper; enterprise sales and ecosystem are smaller.");
    println!();
    println!("vs. Gravitee: closest peer (both EU-spirited Apache OSS APIM).");
    println!("  WSO2 has stronger integration story; Gravitee has stronger");
    println!("  event-native modern UI.");
}

fn print_critique() {
    println!("Honest critique of WSO2:");
    println!();
    println!("• UX inconsistency across products. Publisher, Developer Portal,");
    println!("  Identity Server, Choreo all have different UI generations.");
    println!("  Some screens still feel circa-2015. Active modernization but");
    println!("  not finished.");
    println!();
    println!("• Documentation quality varies. Some product areas have");
    println!("  exemplary docs; others have stale wiki content from older");
    println!("  versions. The product proliferation makes consistency hard.");
    println!();
    println!("• Heavy JVM footprint. All products are Java-based with");
    println!("  significant memory per node. Containerized but not lightweight.");
    println!();
    println!("• Major version upgrades historically painful. APIM 2.x → 3.x");
    println!("  → 4.x carried breaking changes. Carbon/Synapse internals are");
    println!("  not always preserved cleanly across versions.");
    println!();
    println!("• Ballerina adoption beyond WSO2's ecosystem is limited.");
    println!("  Strong language but small community vs. mainstream choices.");
    println!();
    println!("• Brand awareness in North America lags Apigee/Kong/Mulesoft");
    println!("  despite comparable technical maturity. North American sales");
    println!("  engine is smaller.");
    println!();
    println!("• Choreo is newer and roadmap-y. Some features promised are");
    println!("  works-in-progress. Adoption growing but smaller than the");
    println!("  installed base of on-prem WSO2.");
}

fn run_wso2(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "apim" => { print_apim(); 0 }
        "ballerina" => { print_ballerina(); 0 }
        "choreo" => { print_choreo(); 0 }
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
        .unwrap_or_else(|| "wso2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_wso2(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/wso2"), "wso2"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("wso2.exe"), "wso2"); }
    #[test] fn t_help() { assert_eq!(run_wso2(&[], "wso2"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_wso2(&["xx".to_string()], "wso2"), 2); }
}
