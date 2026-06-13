#![deny(clippy::all)]
//! threescale-cli — SlateOS Red Hat 3scale API Management personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Red Hat 3scale API Management Platform.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           3scale's Barcelona origin and Red Hat acquisition");
    println!("    products        APIcast, Admin Portal, Developer Portal, Backend");
    println!("    architecture    NGINX/OpenResty-based, OpenShift-native");
    println!("    apicast         The APIcast gateway component");
    println!("    pricing         Subscription via Red Hat Integration");
    println!("    customers       Notable users");
    println!("    differentiator  Why pick 3scale via Red Hat over alternatives");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("3scale — API management acquired by Red Hat in 2016.");
    println!();
    println!("Founded 2007 in Barcelona by Steven Willmott (CEO), Martin Tantow,");
    println!("and others. Steven came from a research background in distributed");
    println!("systems and AI at Universidad Politécnica de Cataluña. 3scale was");
    println!("one of the earliest pure-play API management companies, alongside");
    println!("Mashery (later acquired by Intel then TIBCO) and Apigee.");
    println!();
    println!("Funding: small early rounds (~$8M total across A and B), then a");
    println!("$5M extension in 2014. Notable backers: Nauta Capital, Caixa");
    println!("Capital, Bertelsmann Investments. The company was profitable and");
    println!("growing in the EU before US expansion.");
    println!();
    println!("Acquired by Red Hat June 2016 for an undisclosed (rumored ~$50-");
    println!("100M) amount. Red Hat positioned 3scale as the API management");
    println!("layer of its broader OpenShift / Middleware portfolio. After IBM's");
    println!("acquisition of Red Hat in 2019, 3scale became part of IBM's");
    println!("Hybrid Cloud Software group.");
    println!();
    println!("Today 3scale is sold as part of Red Hat Integration / Red Hat");
    println!("Application Foundations subscriptions. The product is in active");
    println!("development but its strategic prominence has been somewhat");
    println!("eclipsed by Red Hat's broader bets on Kubernetes/OpenShift");
    println!("(operators, GitOps, Service Mesh).");
}

fn print_products() {
    println!("3scale product components:");
    println!();
    println!("• APIcast");
    println!("    Lua/OpenResty-based gateway (NGINX module). Deploys as");
    println!("    container, sidecar, or embedded in another NGINX. Validates");
    println!("    API keys/OAuth tokens, enforces rate limits, reports usage");
    println!("    back to the 3scale backend.");
    println!();
    println!("• Admin Portal");
    println!("    Ruby on Rails-based admin UI. Manages APIs, application");
    println!("    plans, applications, developer accounts, analytics. Multi-");
    println!("    tenant: one Admin Portal can host many separate 'tenants'");
    println!("    (API providers) each with their own developer portal.");
    println!();
    println!("• Developer Portal");
    println!("    Theme-able, white-label self-service portal where API");
    println!("    consumers sign up, manage applications, view docs, get keys.");
    println!("    Liquid template engine for branding customization.");
    println!();
    println!("• Backend (Service Management API)");
    println!("    Ruby + Erlang/Elixir service that tracks usage and enforces");
    println!("    rate limits across distributed APIcast gateways. The");
    println!("    'authoritative' rate-limit counter store.");
    println!();
    println!("• System (formerly Porta)");
    println!("    Ruby on Rails app that backs the Admin Portal API and the");
    println!("    Developer Portal. Multi-tenant data model.");
    println!();
    println!("• 3scale Operator (OpenShift)");
    println!("    Kubernetes operator for deploying 3scale on OpenShift.");
    println!("    Manages APIcast instances, Admin Portal, Backend, dependencies");
    println!("    (Redis, MySQL, Memcached) via CRDs.");
}

fn print_architecture() {
    println!("3scale architecture.");
    println!();
    println!("Three-tier model:");
    println!();
    println!("• Gateway tier (APIcast)");
    println!("    NGINX + OpenResty + Lua. Stateless. Caches authorization");
    println!("    decisions from the Backend tier to avoid hitting it on every");
    println!("    request. Reports usage asynchronously in batches.");
    println!();
    println!("• Backend tier");
    println!("    Erlang/Elixir application + Redis. Authoritative store for");
    println!("    rate-limit counters, application keys, plan limits. APIcast");
    println!("    calls Backend's /authrep endpoint to validate + report.");
    println!();
    println!("• System tier");
    println!("    Ruby on Rails + MySQL + Memcached. Hosts Admin Portal,");
    println!("    Developer Portal, and the management APIs. Pushes config");
    println!("    changes down to APIcast via reload or hot-reconfigure.");
    println!();
    println!("Deployment topologies:");
    println!();
    println!("  • SaaS (3scale.net) — hosted by Red Hat. The Backend and");
    println!("    System tiers are managed; APIcast gateways run in the SaaS");
    println!("    too, or you can self-host APIcast hybrid against the SaaS");
    println!("    Backend.");
    println!();
    println!("  • On-premise — entire stack deployed in your OpenShift via");
    println!("    the 3scale Operator. Common for regulated industries.");
    println!();
    println!("  • Hybrid — Backend/System in 3scale SaaS, APIcast self-hosted");
    println!("    close to your APIs (for latency or sovereignty).");
}

fn print_apicast() {
    println!("APIcast — the 3scale gateway.");
    println!();
    println!("APIcast is built on OpenResty (NGINX + LuaJIT), the same");
    println!("foundation as Kong's data plane. It's deployed as a container");
    println!("image and configured via environment variables + a JSON");
    println!("configuration fetched from the 3scale Backend.");
    println!();
    println!("Capabilities:");
    println!();
    println!("  • API key authentication (header, query string)");
    println!("  • OAuth 2.0 (RFC 6749) — client credentials, authorization");
    println!("    code, implicit, password, plus 3scale's 'app ID + app key'");
    println!("    legacy scheme");
    println!("  • OIDC integration with Red Hat Single Sign-On (Keycloak)");
    println!("  • JWT validation against JWKS endpoints");
    println!("  • Rate limiting (per-app, per-plan, per-method)");
    println!("  • URL rewriting, header injection, query parameter manipulation");
    println!("  • Response caching (NGINX-native)");
    println!("  • Upstream load balancing across multiple backends");
    println!("  • mTLS to upstream services");
    println!("  • TLS termination with SNI");
    println!("  • Custom Lua policies (write your own logic)");
    println!("  • Built-in policies: CORS, IP whitelist/blacklist, header");
    println!("    forwarding, request/response logging, anonymous access");
    println!();
    println!("APIcast is fast (NGINX-class) but Lua scripting and the");
    println!("authrep round-trip to Backend add latency on cache misses.");
    println!("Steady-state with cache hits: ~1-3ms gateway overhead.");
}

fn print_pricing() {
    println!("3scale pricing.");
    println!();
    println!("3scale is now sold through Red Hat as part of bundled products:");
    println!();
    println!("• Red Hat Integration (3scale + Camel + Fuse + AMQ + Service");
    println!("  Registry + Debezium). Subscription based on the OpenShift");
    println!("  Container Platform sizing or core-pair counts. Indicative");
    println!("  $20K-$200K/year depending on cluster size and support tier.");
    println!();
    println!("• Red Hat Application Foundations — newer SKU positioning that");
    println!("  bundles many Integration components into per-core pricing.");
    println!();
    println!("• 3scale Hosted (SaaS) — usage-based via Red Hat for customers");
    println!("  who want the SaaS experience without OpenShift. Less promoted");
    println!("  than the OpenShift Operator path.");
    println!();
    println!("• 3scale open-source upstream — APIcast Lua code is Apache 2.0;");
    println!("  Porta (Admin Portal / System) is also Apache 2.0 upstream.");
    println!("  Building from upstream is possible but unsupported, and Red");
    println!("  Hat's downstream patches/tooling are part of the product value.");
    println!();
    println!("Honest take: 3scale pricing is enterprise-grade. Hard to");
    println!("justify for greenfield teams unless they're already on Red Hat");
    println!("OpenShift with existing enterprise agreements. Without that");
    println!("context, Kong / Tyk / Gravitee / KrakenD are cheaper and more");
    println!("modern.");
}

fn print_customers() {
    println!("3scale customer references (public):");
    println!();
    println!("  • Movistar (Telefónica) — telecom partner APIs");
    println!("  • Banco Santander — banking APIs (open banking compliance)");
    println!("  • Caixa Bank — Spanish bank, regulatory APIs");
    println!("  • EU public-sector institutions (multiple)");
    println!("  • OECD — research and statistics APIs");
    println!("  • McKesson — healthcare data APIs");
    println!("  • Posti Group (Finland Post) — logistics APIs");
    println!("  • Royal Mail — UK postal APIs");
    println!("  • Various US government agencies via Red Hat federal channels");
    println!();
    println!("Pattern: existing Red Hat enterprise customers (especially");
    println!("OpenShift adopters), European public sector, telcos with Red");
    println!("Hat OpenStack/OpenShift footprints. Less common in greenfield");
    println!("startups.");
}

fn print_differentiator() {
    println!("Why pick 3scale via Red Hat:");
    println!();
    println!("• You already run OpenShift. The 3scale Operator deploys");
    println!("  cleanly into existing clusters with the same support contract");
    println!("  as the rest of your Red Hat stack.");
    println!();
    println!("• Bundled with Red Hat Integration. If you also need Apache");
    println!("  Camel (Fuse), ActiveMQ Artemis (AMQ), Debezium CDC, the");
    println!("  combined subscription is competitive.");
    println!();
    println!("• Mature multi-tenancy. One 3scale install hosts many tenants,");
    println!("  each with separate developer portal, APIs, accounts. Strong");
    println!("  fit for B2B platforms that resell APIs.");
    println!();
    println!("• Open-source upstream (APIcast Lua + Porta). No total lock-in.");
    println!();
    println!("• Red Hat enterprise support — 24/7, multi-region, multi-language.");
    println!("  Procurement-friendly through Red Hat's existing channel.");
    println!();
    println!("• Strong analytics out of the box. Per-app per-method per-day");
    println!("  usage reporting comes built-in.");
    println!();
    println!("vs. Kong: 3scale has stronger multi-tenancy and Red Hat support;");
    println!("  Kong has more polished modern dev experience and richer plugins.");
    println!();
    println!("vs. Apigee: comparable enterprise feature breadth; 3scale wins");
    println!("  on open-source upstream + OpenShift integration; Apigee wins");
    println!("  on monetization depth and Google Cloud integration.");
    println!();
    println!("vs. Mulesoft: 3scale is gateway/management-focused; Mulesoft is");
    println!("  iPaaS-heavy. Different product surface areas — sometimes used");
    println!("  together.");
}

fn print_critique() {
    println!("Honest critique of 3scale:");
    println!();
    println!("• Strategic deprioritization risk. Inside Red Hat / IBM, 3scale");
    println!("  is one product among many. Roadmap velocity is enterprise-");
    println!("  paced. Some users worry about long-term investment levels");
    println!("  compared to dedicated API-mgmt vendors.");
    println!();
    println!("• The architecture (Ruby + Erlang + Lua + Redis + MySQL +");
    println!("  Memcached) is operationally heavy. Lots of moving parts.");
    println!("  The Operator helps but the underlying complexity remains.");
    println!();
    println!("• APIcast configuration is split between the Admin Portal");
    println!("  (online editing) and APIcast policy chains (declarative).");
    println!("  GitOps purity is harder to achieve than with declarative-only");
    println!("  alternatives like Emissary or KrakenD.");
    println!();
    println!("• Developer Portal customization (Liquid + CSS) is dated.");
    println!("  Branded portals look serviceable but not state-of-the-art.");
    println!();
    println!("• Modern features (GraphQL gateways, async API support,");
    println!("  AI gateway features) arrive later than at cloud-native");
    println!("  competitors.");
    println!();
    println!("• Documentation often assumes Red Hat / OpenShift familiarity.");
    println!("  Standalone Kubernetes users sometimes find the docs Red Hat-");
    println!("  centric to the point of friction.");
    println!();
    println!("• Strong fit for existing Red Hat shops; awkward sell for");
    println!("  greenfield cloud-native teams that aren't already OpenShift");
    println!("  customers.");
}

fn run_threescale(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (SlateOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
        "apicast" => { print_apicast(); 0 }
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
        .unwrap_or_else(|| "threescale".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_threescale(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/threescale"), "threescale"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("threescale.exe"), "threescale"); }
    #[test] fn t_help() { assert_eq!(run_threescale(&[], "threescale"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_threescale(&["xx".to_string()], "threescale"), 2); }
}
