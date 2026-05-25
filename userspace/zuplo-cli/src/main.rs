#![deny(clippy::all)]
//! zuplo-cli — OurOS Zuplo programmable edge API gateway personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Zuplo programmable edge API gateway.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Zuplo's story — ex-Microsoft API team, 2020");
    println!("    products        Gateway, dev portal, monetization, AI gateway");
    println!("    programmable    TypeScript-native gateway philosophy");
    println!("    edge            Why Cloudflare Workers as the runtime");
    println!("    pricing         Free tier and growth pricing");
    println!("    customers       Notable Zuplo deployments");
    println!("    differentiator  Code-first, Git-deployed, edge-native");
    println!("    critique        Honest limitations");
    println!("    help / version");
}

fn print_about() {
    println!("Zuplo — programmable API gateway that lives at the edge.");
    println!();
    println!("Founded 2020 in Seattle by Josh Twist (CEO), Adrian Hall, and");
    println!("Nate Taggart. Josh and Adrian were principal engineers on the");
    println!("Microsoft Azure API Management team for years — Josh founded");
    println!("Azure Mobile Services before that. They left Microsoft believing");
    println!("that the next-generation API gateway should be:");
    println!("  • Code-first, not XML/YAML/UI-first");
    println!("  • TypeScript-native (because that's where the API developers are)");
    println!("  • Edge-deployed (sub-50ms latency from anywhere)");
    println!("  • Git-deployed (CI/CD, PRs, code review of policy changes)");
    println!("  • Open-source friendly and self-hostable when needed");
    println!();
    println!("Funding: $9M seed Apr 2022 led by Bain Capital Ventures with");
    println!("participation from Microsoft's M12 fund. $20M Series A Jun 2024");
    println!("led by Bain with Microsoft, Vertex, others — bringing total to");
    println!("~$29M. Headquartered Seattle, fully remote engineering.");
    println!();
    println!("Strong product-led growth via the free tier and developer-first");
    println!("documentation. The 'AI gateway' positioning in 2024 (after the");
    println!("LLM-API explosion) significantly accelerated adoption.");
}

fn print_products() {
    println!("Zuplo product surface:");
    println!();
    println!("• API Gateway — flagship");
    println!("    Cloudflare Workers-based edge runtime. TypeScript modules");
    println!("    composed via a routes.json configuration. Policies as code:");
    println!("    auth (API key, JWT, OAuth, mTLS), rate limit, transform,");
    println!("    validation (against OpenAPI / JSON Schema), CORS, caching,");
    println!("    redirects. Custom inline TypeScript handlers for arbitrary");
    println!("    logic.");
    println!();
    println!("• Developer Portal");
    println!("    Auto-generated from your OpenAPI spec. Branded, themeable,");
    println!("    self-service API key issuance. MDX-based custom pages.");
    println!("    Hosts on Zuplo's CDN with custom domain support.");
    println!();
    println!("• Monetization (Stripe-integrated)");
    println!("    Define usage plans, pricing tiers, metered billing in code.");
    println!("    Zuplo issues API keys against Stripe customers, meters usage,");
    println!("    and triggers Stripe Billing for invoicing. No separate");
    println!("    billing system required.");
    println!();
    println!("• AI Gateway (2024)");
    println!("    LLM-specific policies: token counting, prompt logging,");
    println!("    semantic caching, prompt injection scanning, content");
    println!("    filtering, model routing/failover across OpenAI/Anthropic/");
    println!("    Bedrock/Vertex. Streams responses, supports SSE.");
    println!();
    println!("• Self-Hosted Edition");
    println!("    Run Zuplo on your own infrastructure for sovereignty or");
    println!("    air-gap requirements. Docker-based. Less polished than the");
    println!("    SaaS but available for enterprise contracts.");
}

fn print_programmable() {
    println!("Zuplo's code-first philosophy.");
    println!();
    println!("Traditional API gateways express logic as configuration: YAML");
    println!("policies, XML flows, UI-driven rules engines. The argument for");
    println!("config-first: anyone can edit without learning to code.");
    println!();
    println!("Zuplo's counter-argument: at scale, config eats code anyway. You");
    println!("end up with thousand-line YAML files referencing JavaScript");
    println!("expression-language snippets, multi-stage transforms, and");
    println!("conditional policy chains. The config becomes a worse programming");
    println!("language than the language you'd have used.");
    println!();
    println!("Zuplo's bet: lead with TypeScript. Routes are defined in a");
    println!("routes.json that maps URL patterns to TypeScript handlers and");
    println!("composable middleware. Every policy is a TypeScript function.");
    println!("Custom logic is a TypeScript module. The 'config' is the code.");
    println!();
    println!("Benefits:");
    println!("  • Standard tooling: VS Code, ESLint, Prettier, Jest, GitHub");
    println!("  • Pull-request review of gateway policy changes");
    println!("  • npm packages: import any library that runs on Workers");
    println!("  • Type-checked: catch broken routes at deploy, not runtime");
    println!("  • Easy testing: unit-test policy chains in Jest");
    println!("  • One language for backend + gateway logic");
    println!();
    println!("Tradeoffs:");
    println!("  • Non-developers can't edit policies (no clicky UI)");
    println!("  • TypeScript is the only first-class language");
    println!("  • Compute model is Workers — V8 isolates, not Node");
}

fn print_edge() {
    println!("Why Zuplo built on Cloudflare Workers.");
    println!();
    println!("Most API gateways are stateful Java/Go applications you deploy");
    println!("to your VPC or to a vendor's regional managed K8s. Latency is");
    println!("regional: anywhere from 10ms (in-region) to 200ms (cross-region).");
    println!();
    println!("Zuplo runs on Cloudflare Workers — V8 isolates deployed to ~300");
    println!("Cloudflare PoPs worldwide. Every API request hits a Worker");
    println!("within ~30ms of the client. Implications:");
    println!();
    println!("  • Cold start: ~5ms vs. 100-1000ms for container-based gateways");
    println!("  • Auth latency: API key validation happens at the edge, before");
    println!("    your origin sees the request. Invalid keys get rejected at");
    println!("    the closest PoP. DDoS protection comes for free.");
    println!("  • Rate limiting: globally consistent counters via Cloudflare");
    println!("    Durable Objects. Hard to do this well with regional gateways.");
    println!("  • Cost: pay-per-request not pay-per-VM. Idle costs near zero.");
    println!();
    println!("Constraints:");
    println!("  • 50ms CPU budget per request on the free Workers tier (more");
    println!("    on paid tiers, but never as much as a full Node process)");
    println!("  • No persistent connections to origin (each request is");
    println!("    independent)");
    println!("  • Limited Node-API compatibility — npm packages must run on");
    println!("    the Workers runtime");
    println!();
    println!("Zuplo packages the Workers complexity behind a friendly DX.");
    println!("You write code; Zuplo handles deployment, secrets, custom domains,");
    println!("multi-environment promotion, and rollback.");
}

fn print_pricing() {
    println!("Zuplo pricing (USD, 2025):");
    println!();
    println!("• Hobby — Free");
    println!("    1 environment, 1M requests/month, basic policies, community");
    println!("    support. Generous enough for personal projects and prototypes.");
    println!();
    println!("• Builder — $25/month per project");
    println!("    Multiple environments, 1.5M requests included + $0.50 per");
    println!("    additional 1K, custom domain, MFA, role-based access.");
    println!();
    println!("• Business — $400+/month");
    println!("    SLA, premium support, audit logs, SSO, higher rate limits,");
    println!("    advanced features (custom auth providers, Connected Apps).");
    println!();
    println!("• Enterprise — custom");
    println!("    Dedicated support, self-hosted option, custom contracts.");
    println!();
    println!("Notable: Zuplo's pricing is dramatically below Apigee/MuleSoft/");
    println!("Kong Konnect for equivalent scale. The free tier alone covers");
    println!("workloads that would cost hundreds of dollars on competitors.");
}

fn print_customers() {
    println!("Zuplo customer references:");
    println!();
    println!("  • Auth0 (Okta) — uses Zuplo for partner API gateway");
    println!("  • RudderStack — customer data platform, partner APIs");
    println!("  • Cohere — early AI gateway adopter for LLM APIs");
    println!("  • Mintlify — docs platform partner integrations");
    println!("  • Hookdeck — webhook infrastructure (peer integration)");
    println!("  • Knock — notifications platform partner APIs");
    println!("  • Liveblocks — real-time collaboration API gateway");
    println!("  • Postman — partner ecosystem APIs");
    println!("  • Vercel — internal API gateway use cases");
    println!();
    println!("Pattern: developer-tools companies, AI infrastructure startups,");
    println!("API-first SaaS that need monetization + dev portal without");
    println!("building it. Strongest in the Vercel/Cloudflare/edge-native");
    println!("ecosystem.");
}

fn print_differentiator() {
    println!("Why teams pick Zuplo:");
    println!();
    println!("• Code-first. Policies, routes, transforms — all TypeScript.");
    println!("  Versioned in Git, code-reviewed via PRs, deployed via CI/CD.");
    println!();
    println!("• Edge-deployed by default. ~300 PoPs globally via Cloudflare");
    println!("  Workers. Cold start ~5ms. Auth/rate-limit at the edge.");
    println!();
    println!("• Best-in-class AI gateway. Released early in the LLM wave with");
    println!("  semantic caching, prompt injection scanning, multi-model");
    println!("  routing/failover, token metering. Active feature roadmap.");
    println!();
    println!("• Monetization built-in. Stripe-integrated rate plans + metered");
    println!("  billing. You don't need a separate monetization product.");
    println!();
    println!("• Free tier covers real production workloads. 1M requests/month");
    println!("  is enough for many B2B SaaS apps.");
    println!();
    println!("• Excellent developer experience. CLI, GitHub integration,");
    println!("  VS Code extension, fast feedback loop. Docs are well-written.");
    println!();
    println!("vs. Kong: Zuplo is dramatically simpler for greenfield teams;");
    println!("  Kong has more enterprise features, plugins, and on-prem maturity.");
    println!();
    println!("vs. Apigee/Mulesoft: Zuplo is 10-50x cheaper at small scale and");
    println!("  has modern DX; Apigee has deeper enterprise capability.");
    println!();
    println!("vs. AWS API Gateway: Zuplo is not locked to AWS, has a real");
    println!("  developer portal, and offers monetization out of the box.");
    println!();
    println!("vs. KrakenD/Tyk: Zuplo's edge-native model and TypeScript-first");
    println!("  approach is distinct; the others are Go-based and self-hosted-first.");
}

fn print_critique() {
    println!("Honest critique of Zuplo:");
    println!();
    println!("• Cloudflare Workers lock-in (mostly). The runtime model is");
    println!("  Workers-shaped: V8 isolates, request-scoped state, no");
    println!("  long-lived connections. Self-hosted Zuplo Docker exists but");
    println!("  is less battle-tested than the SaaS.");
    println!();
    println!("• TypeScript-only for custom logic. If you need a Python or");
    println!("  Go transform, you'll have to call out to a backend service.");
    println!();
    println!("• Younger product, smaller installed base than Kong/Apigee/");
    println!("  MuleSoft. Some enterprise checkboxes (FedRAMP, ISO 27017,");
    println!("  detailed audit logs) are still being added.");
    println!();
    println!("• Workers CPU limits matter for heavy transforms. Big payload");
    println!("  rewrites or XML processing may run into the per-request CPU");
    println!("  ceiling on lower tiers.");
    println!();
    println!("• Plugin ecosystem is smaller than Kong's. The 'inline TypeScript'");
    println!("  philosophy somewhat obviates plugins, but third-party reusable");
    println!("  policy modules are limited.");
    println!();
    println!("• Documentation, while good, sometimes lags new features.");
    println!("  Roadmap velocity is high and AI gateway docs evolve weekly.");
    println!();
    println!("• Enterprise sales motion still maturing. Large procurement");
    println!("  cycles can be slower than with established vendors.");
}

fn run_zuplo(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "programmable" | "code" => { print_programmable(); 0 }
        "edge" => { print_edge(); 0 }
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
        .unwrap_or_else(|| "zuplo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_zuplo(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/zuplo"), "zuplo"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("zuplo.exe"), "zuplo"); }
    #[test] fn t_help() { assert_eq!(run_zuplo(&[], "zuplo"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_zuplo(&["xx".to_string()], "zuplo"), 2); }
}
