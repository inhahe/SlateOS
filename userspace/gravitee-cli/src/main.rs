#![deny(clippy::all)]
//! gravitee-cli — SlateOS Gravitee.io open-source API platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Gravitee.io open-source API management platform.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Gravitee's French open-source story");
    println!("    products        API Management, Access Management, Cockpit, AM");
    println!("    architecture    Gateway, Management API, Console, AM, Alert Engine");
    println!("    eventnative     Why Gravitee leads on async APIs and event-native");
    println!("    pricing         Open-source + Enterprise tiers");
    println!("    customers       BPCE, Michelin, Total, MAIF, La Redoute");
    println!("    differentiator  Open-source, event-native, EU-headquartered");
    println!("    critique        Honest limitations");
    println!("    help / version");
}

fn print_about() {
    println!("Gravitee.io — open-source API management out of France.");
    println!();
    println!("Founded 2015 in Lille, France by David Brassely, Nicolas Géraud,");
    println!("Titouan Compiègne, and Azarias Rabearimanana. The four engineers");
    println!("came from API consulting work and decided that the existing API");
    println!("management market (CA, IBM, Apigee, Mulesoft) was too expensive");
    println!("and proprietary for the open-source-native era. They launched");
    println!("Gravitee.io as Apache 2.0 from day one.");
    println!();
    println!("Initial traction came from European enterprises looking for an");
    println!("Apigee/Mulesoft alternative without the price tag or vendor lock-in.");
    println!("Headquartered in Lille and London with offices in NYC and");
    println!("Bordeaux. The company adopted the open-core model: open-source");
    println!("APIM is free and fully featured for most needs; enterprise tier");
    println!("adds advanced security, monetization, multi-org governance.");
    println!();
    println!("Funding: $60M Series C announced 2022 led by Albion VC, with");
    println!("Bertelsmann Investments and Five Elms Capital. Earlier Series A");
    println!("led by Albion ~$11M 2020, Series B ~$25M 2021. Cumulative ~$96M.");
    println!("Strong revenue growth, European customers especially banks,");
    println!("insurers, public sector clients (which favor open source).");
}

fn print_products() {
    println!("Gravitee product line:");
    println!();
    println!("• API Management (APIM) — flagship");
    println!("    Open-source, full-featured. Synchronous + asynchronous APIs,");
    println!("    HTTP/HTTPS/gRPC/Kafka/MQTT/WebSocket/Server-Sent Events.");
    println!("    Plug-in policies, multi-tenant, deploys on K8s / VM / Docker.");
    println!();
    println!("• Access Management (AM)");
    println!("    Identity provider broker. OAuth 2.0, OIDC, SAML, social login,");
    println!("    MFA, account management. Separable from APIM, usable as a");
    println!("    standalone IDP. Open-source, similar Keycloak's positioning.");
    println!();
    println!("• Cockpit");
    println!("    Multi-environment, multi-organization governance plane. Cloud");
    println!("    SaaS for managing fleets of Gravitee APIM and AM installs");
    println!("    across dev/test/prod/regional clusters.");
    println!();
    println!("• Alert Engine");
    println!("    Real-time monitoring and alerting on API traffic patterns,");
    println!("    SLA breaches, anomaly detection. Integrates with PagerDuty,");
    println!("    Slack, webhooks.");
    println!();
    println!("• API Designer");
    println!("    Visual API spec authoring (OpenAPI 3 / AsyncAPI 2). Round-trips");
    println!("    to YAML/JSON. Integrated into the APIM console.");
}

fn print_architecture() {
    println!("Gravitee APIM architecture (open-source distribution):");
    println!();
    println!("• Gateway — the runtime");
    println!("    Java/Vert.x reactive, non-blocking. Handles incoming traffic,");
    println!("    routes to backends, applies policies. Stateless, horizontally");
    println!("    scaled. Reads config from MongoDB or Postgres + Elasticsearch.");
    println!();
    println!("• Management API");
    println!("    REST + GraphQL API for managing APIs, plans, applications,");
    println!("    subscriptions, users. The Console UI calls this API.");
    println!();
    println!("• Management Console");
    println!("    React-based admin UI for API publishers and administrators.");
    println!();
    println!("• Developer Portal");
    println!("    Public-facing self-service portal where consumers browse the");
    println!("    API catalog, sign up for plans, manage API keys, view docs.");
    println!();
    println!("• Repository (MongoDB or JDBC)");
    println!("    Stores API definitions, plans, apps, subscriptions, users.");
    println!();
    println!("• Analytics (Elasticsearch / OpenSearch)");
    println!("    Stores access logs, metrics, audit events. Powers analytics");
    println!("    dashboards and search.");
    println!();
    println!("Concepts:");
    println!("  • APIs: published proxy/endpoint definitions");
    println!("  • Plans: subscription tiers with rate limits and auth methods");
    println!("  • Applications: consumer apps that subscribe to plans");
    println!("  • Subscriptions: app + plan + API binding with credentials");
    println!("  • Policies: chained logic (rate-limit, transform, auth, log)");
    println!("  • Dictionaries: dynamic value lookups (e.g., A/B routing)");
    println!("  • Flows: ordered policy chains per API or per plan");
}

fn print_eventnative() {
    println!("Gravitee's event-native API management — the differentiator.");
    println!();
    println!("Most API gateways treat asynchronous protocols (Kafka, MQTT, AMQP,");
    println!("WebSocket, SSE) as second-class. Either unsupported, or proxied");
    println!("through awkward bridges that lose semantics. Gravitee built APIM");
    println!("4.x with event-native as a first-class architecture from the start.");
    println!();
    println!("Capabilities:");
    println!("  • Kafka topics as managed APIs — subscribe via HTTP, WebSocket,");
    println!("    or SSE; publish via HTTP POST. Gravitee bridges sync↔async.");
    println!("  • MQTT brokers exposed to OAuth-protected web/mobile clients");
    println!("    without giving every client direct broker credentials.");
    println!("  • WebSocket gateway with message-level policies (auth on");
    println!("    connect, rate-limit messages, transform payloads).");
    println!("  • SSE for server-push of events to browsers.");
    println!("  • Schema validation (AsyncAPI / JSON Schema / Protobuf).");
    println!("  • Backpressure handling: pause consumers, drop messages,");
    println!("    buffer with watermarks.");
    println!();
    println!("Why this matters: in 2025 every modern app has real-time needs");
    println!("(live dashboards, chat, IoT, market data, collaboration). Most");
    println!("API gateways force you to manage the async stack separately.");
    println!("Gravitee unifies sync REST + async streaming under one policy and");
    println!("identity layer. Apigee, Kong Enterprise, Tyk all have partial");
    println!("support — Gravitee made it the core architecture.");
}

fn print_pricing() {
    println!("Gravitee pricing tiers:");
    println!();
    println!("• Open Source (Apache 2.0)");
    println!("    Free forever. Full APIM gateway, full AM, full Management");
    println!("    Console + Developer Portal, all sync + async protocol support,");
    println!("    self-hosted on your infrastructure. Community support via");
    println!("    forum, Slack, GitHub issues.");
    println!();
    println!("• Enterprise Edition (commercial)");
    println!("    Subscription pricing, contact sales. Adds:");
    println!("    - Advanced security: bot detection, IP intelligence, payload");
    println!("      threat protection, advanced rate limiting");
    println!("    - Monetization: rate plans, billing, invoicing");
    println!("    - Multi-organization governance via Cockpit");
    println!("    - Federated identity at enterprise scale");
    println!("    - Production support with SLA, 24/7 incidents");
    println!("    - Long-term support (LTS) releases with security patches");
    println!();
    println!("• Gravitee Cloud (SaaS, newer)");
    println!("    Fully managed Gravitee in the Gravitee cloud. Removes ops");
    println!("    burden. Per-API and per-call usage pricing.");
    println!();
    println!("Pricing typically 30-60% lower than Apigee X / Mulesoft for");
    println!("equivalent feature coverage at enterprise tier. Open-source");
    println!("path means you can prototype free and only convert to paid when");
    println!("you need the enterprise features.");
}

fn print_customers() {
    println!("Gravitee customer references (public):");
    println!();
    println!("  • BPCE — French banking group (Banques Populaires / Caisses");
    println!("    d'Épargne) using Gravitee for open banking APIs (PSD2)");
    println!("  • Michelin — tire-and-mobility company, IoT and B2B APIs");
    println!("  • Total Energies — energy company, partner integration APIs");
    println!("  • MAIF — French mutual insurer");
    println!("  • La Redoute — French e-commerce");
    println!("  • SNCF — French railway, real-time train data APIs");
    println!("  • Société Générale — investment bank open banking");
    println!("  • Orange Business Services — telecom partner APIs");
    println!("  • DXC Technology — IT services, customer integrations");
    println!("  • UN Global Compact — public-sector data APIs");
    println!();
    println!("Pattern: European enterprises favoring EU-headquartered vendors");
    println!("for sovereignty and data-residency reasons, plus large companies");
    println!("everywhere that want open-source escape hatches from Apigee/");
    println!("Mulesoft pricing.");
}

fn print_differentiator() {
    println!("Why teams pick Gravitee:");
    println!();
    println!("• True open source (Apache 2.0). Not 'community edition that's");
    println!("  crippled' — the OSS APIM is production-ready and used by");
    println!("  Fortune 500 enterprises in production at the OSS tier.");
    println!();
    println!("• Event-native architecture. First-class async protocols (Kafka,");
    println!("  MQTT, WebSocket, SSE) integrated into the same gateway as REST.");
    println!("  Other vendors retrofitted async; Gravitee built it in.");
    println!();
    println!("• European headquarters. Important for EU data residency,");
    println!("  GDPR, and public-sector procurement that disfavors US vendors.");
    println!();
    println!("• Integrated identity (Gravitee AM). Unlike pure API gateways,");
    println!("  the IdP is part of the platform — OAuth, MFA, social login,");
    println!("  WebAuthn handled in-product without a separate Keycloak install.");
    println!();
    println!("• Reasonable pricing. Enterprise tier substantially under Apigee/");
    println!("  Mulesoft. Open-source ceiling is high enough that many teams");
    println!("  never need to upgrade.");
    println!();
    println!("• Cockpit multi-environment governance scales to large orgs.");
    println!();
    println!("vs. Kong: Gravitee has richer monetization, developer portal,");
    println!("  and event-native depth. Kong has more polished dev-experience");
    println!("  and Konnect cloud control plane.");
    println!();
    println!("vs. Apigee: Gravitee is dramatically cheaper, open-source, no");
    println!("  GCP lock-in. Apigee has more enterprise checkboxes and bigger");
    println!("  sales/support footprint.");
    println!();
    println!("vs. Tyk: Both have strong OSS positioning. Gravitee has stronger");
    println!("  event-native + integrated AM. Tyk has stronger multi-cloud,");
    println!("  GraphQL universal data graph, and is more JS-friendly.");
}

fn print_critique() {
    println!("Honest critique:");
    println!();
    println!("• Console UX has historically been less polished than Kong");
    println!("  Konnect or Apigee. The React rewrite (Management Console v4)");
    println!("  improved this but corners still feel functional-first.");
    println!();
    println!("• Documentation can be uneven. Core paths are well-documented;");
    println!("  edge cases and migrations between major versions sometimes");
    println!("  require reading the forum.");
    println!();
    println!("• Java + Vert.x runtime is performant but the JVM overhead is");
    println!("  noticeable vs. Go-based alternatives (Kong, KrakenD, Tyk).");
    println!("  Cold-start memory ~300-500MB per gateway pod.");
    println!();
    println!("• Major version upgrades (v2 → v3 → v4) have been disruptive.");
    println!("  Policies and API definitions evolved across versions; teams");
    println!("  on older deployments need migration windows.");
    println!();
    println!("• Smaller plugin ecosystem than Kong. Gravitee policies are");
    println!("  pluggable but the community marketplace is thinner.");
    println!();
    println!("• Brand awareness still lower than Kong / Apigee in North");
    println!("  America. European reference customers dominate.");
}

fn run_gravitee(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
        "eventnative" | "events" => { print_eventnative(); 0 }
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
        .unwrap_or_else(|| "gravitee".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_gravitee(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/gravitee"), "gravitee"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("gravitee.exe"), "gravitee"); }
    #[test] fn t_help() { assert_eq!(run_gravitee(&[], "gravitee"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_gravitee(&["xx".to_string()], "gravitee"), 2); }
}
