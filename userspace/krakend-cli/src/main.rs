#![deny(clippy::all)]
//! krakend-cli — SlateOS KrakenD high-performance API gateway personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — KrakenD high-performance Go-based API gateway.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           KrakenD's Spanish origin, stateless design");
    println!("    products        Community Edition, Enterprise, Designer");
    println!("    architecture    Pure stateless, declarative, aggregation-first");
    println!("    performance     Benchmark numbers and what makes KrakenD fast");
    println!("    aggregation     Backend response merging and shaping");
    println!("    pricing         CE free / Enterprise paid");
    println!("    customers       Notable users");
    println!("    differentiator  Why pick KrakenD over Kong/Tyk/NGINX");
    println!("    critique        Honest limitations");
    println!("    help / version");
}

fn print_about() {
    println!("KrakenD — declarative high-performance API gateway.");
    println!();
    println!("Created by Daniel Lopez Ridruejo and David Coallier in 2016 in");
    println!("Barcelona, Spain. The project began as an internal tool at");
    println!("Bonial.com (a German shopping-deals startup) where the team");
    println!("needed an aggregation gateway capable of fanning out to dozens of");
    println!("microservices per request and merging the responses into a");
    println!("single payload — at high RPS — without becoming the bottleneck.");
    println!();
    println!("Open-sourced in 2017 as KrakenD Community Edition (Apache 2.0).");
    println!("The company KrakenD S.L. was incorporated in 2018 to support the");
    println!("growing Enterprise Edition with additional security policies,");
    println!("vault integration, advanced rate limiting, telemetry, and SLAs.");
    println!();
    println!("KrakenD remains primarily bootstrapped, funded by Enterprise");
    println!("Edition subscriptions and consulting. Smaller than Kong/Tyk in");
    println!("headcount but with a focused, well-regarded engineering culture.");
    println!("HQ Barcelona, fully remote team across EU.");
    println!();
    println!("The CNCF accepted KrakenD as a Sandbox project in 2024,");
    println!("recognizing its growing role in cloud-native API gateway space.");
}

fn print_products() {
    println!("KrakenD product line:");
    println!();
    println!("• KrakenD Community Edition (CE)");
    println!("    Apache 2.0 open source. The full gateway runtime, all");
    println!("    aggregation, transformation, rate-limit, basic JWT auth.");
    println!("    Production-grade for many use cases. No paid features hidden");
    println!("    behind a 'community edition is crippled' wall.");
    println!();
    println!("• KrakenD Enterprise Edition (EE)");
    println!("    Commercial subscription. Adds:");
    println!("    - Advanced security: bot detection, OPA/Rego integration,");
    println!("      automatic JWK rotation, ECC-asymmetric JWT");
    println!("    - Geofencing and ASN/IP intelligence");
    println!("    - Vault, AWS Secrets Manager, GCP Secret Manager integration");
    println!("    - Advanced rate limiting (sliding-window, distributed)");
    println!("    - WebSocket/SSE support, gRPC server reflection");
    println!("    - Audit logs, OpenTelemetry, dedicated support, LTS");
    println!();
    println!("• KrakenDesigner");
    println!("    Web-based visual config builder. Generates the JSON config");
    println!("    that KrakenD runtimes consume. Reduces hand-editing of large");
    println!("    configs while keeping the declarative-config-is-source-of-");
    println!("    truth principle.");
    println!();
    println!("• Flexible Config (FC)");
    println!("    Templating system that lets you generate the runtime");
    println!("    configuration from smaller modular files. Avoids monolithic");
    println!("    JSON blobs for large APIs.");
    println!();
    println!("• KrakenD Studio (Enterprise, newer)");
    println!("    Cloud console for managing fleets of KrakenD gateways,");
    println!("    rolling config changes, observing traffic.");
}

fn print_architecture() {
    println!("KrakenD architecture — stateless, declarative, no surprises.");
    println!();
    println!("Core design principles:");
    println!();
    println!("• 100% stateless. Every gateway instance is identical and");
    println!("  interchangeable. No database, no admin API that mutates state.");
    println!("  Config is JSON — load it at boot, serve traffic, exit cleanly.");
    println!("  Add/remove instances behind a load balancer without coordination.");
    println!();
    println!("• Declarative configuration. A single JSON file (or modular FC");
    println!("  templates) describes every endpoint, backend, transformation,");
    println!("  policy. No runtime API to change behavior — change config,");
    println!("  restart (or hot-reload), done. Versioned in Git, code-reviewed.");
    println!();
    println!("• No control plane required. Unlike Kong (which needs a database");
    println!("  + admin API for non-DB-less mode), KrakenD has no control plane");
    println!("  for the base runtime. The optional KrakenD Studio is observability,");
    println!("  not control.");
    println!();
    println!("• Written in Go. ~10MB static binary, single-binary deployment.");
    println!("  No JVM, no Node, no dependencies. Runs as a sidecar or as");
    println!("  cluster ingress with equal ease.");
    println!();
    println!("• Aggregation-first. KrakenD was designed to fan out to N");
    println!("  backends per request and merge results. Most gateways treat");
    println!("  this as an afterthought; KrakenD treats it as the default.");
    println!();
    println!("Runtime flow per request:");
    println!("  1. Match endpoint pattern");
    println!("  2. Apply input policies (auth, rate-limit, CORS)");
    println!("  3. Decompose request into N backend calls");
    println!("  4. Fan out backends in parallel (with timeouts, retries, CB)");
    println!("  5. Filter/transform each backend response");
    println!("  6. Merge responses according to merge strategy");
    println!("  7. Apply output policies (compression, headers)");
    println!("  8. Return aggregated response");
}

fn print_performance() {
    println!("KrakenD performance — the headline differentiator.");
    println!();
    println!("Public benchmarks (from KrakenD docs + community comparisons):");
    println!();
    println!("  • KrakenD: ~25K-60K req/sec per instance on commodity hardware");
    println!("    (4 vCPU, 8GB RAM) for typical aggregation workloads");
    println!("  • Pass-through proxy (no transformations): >100K req/sec");
    println!("  • Latency overhead vs. direct backend call: ~0.2-1ms p99");
    println!("  • Memory footprint: ~30-100MB at steady state per instance");
    println!();
    println!("Why KrakenD is fast:");
    println!();
    println!("  • Pure Go, no GC pauses long enough to matter. Compiled binary,");
    println!("    no warm-up needed.");
    println!("  • Stateless: no DB lookups in the hot path. Config is in");
    println!("    memory, parsed at boot.");
    println!("  • Concurrent backend fan-out using goroutines, not thread pools.");
    println!("    Doing 10 parallel backend calls costs ~10KB of stack, not");
    println!("    10 OS threads.");
    println!("  • Zero-allocation paths in the hot loop (where possible).");
    println!("    JSON parsing optimized with custom decoders.");
    println!("  • Optional Lua/Martian/CEL scripting compiled at load time,");
    println!("    not interpreted per-request.");
    println!();
    println!("Comparisons (rough, vendor-dependent):");
    println!("  • KrakenD vs. Kong: KrakenD is faster on aggregation workloads");
    println!("    by a factor of 2-3x in published comparisons. Kong is faster");
    println!("    on simple proxy with Lua plugins disabled.");
    println!("  • KrakenD vs. NGINX+Lua: comparable raw throughput; KrakenD");
    println!("    wins on aggregation/merging tasks where NGINX would need");
    println!("    extensive Lua.");
    println!("  • KrakenD vs. Tyk: similar throughput; Tyk has more enterprise");
    println!("    feature breadth at the cost of complexity.");
}

fn print_aggregation() {
    println!("KrakenD aggregation — the gateway's superpower.");
    println!();
    println!("Pattern: a frontend wants user-profile data that lives in 3");
    println!("microservices (user-service, orders-service, preferences-service).");
    println!("Without aggregation, the frontend makes 3 calls, deals with");
    println!("3 failure modes, and the mobile experience is laggy on bad");
    println!("networks. With KrakenD, the frontend makes 1 call. KrakenD");
    println!("fans out to 3 backends in parallel, merges, returns one payload.");
    println!();
    println!("KrakenD aggregation features:");
    println!();
    println!("  • Parallel fan-out with per-backend timeout and circuit breakers");
    println!("  • Filtering: include only specific fields from each response");
    println!("    (whitelist) or exclude (blacklist). Reduces payload size.");
    println!("  • Field flattening, prefixing, renaming on merge");
    println!("  • Failure tolerance: continue with partial data if a non-");
    println!("    critical backend fails (configurable per-backend)");
    println!("  • Different content types per backend (JSON, XML, gRPC, GraphQL)");
    println!("    auto-converted before merge");
    println!("  • Static content injection (e.g., version, ttl, server time)");
    println!("  • Conditional backends: skip a backend based on request headers");
    println!("    or query parameters");
    println!("  • Sequential backends: backend B depends on backend A's output");
    println!("    (chained backends with response→request templating)");
    println!();
    println!("This pattern is sometimes called 'Backend for Frontend' (BFF) or");
    println!("'API composition.' KrakenD implements it without you writing");
    println!("custom BFF code — it's declarative configuration.");
}

fn print_pricing() {
    println!("KrakenD pricing:");
    println!();
    println!("• Community Edition — Free (Apache 2.0)");
    println!("    Full gateway runtime. No artificial limits on requests,");
    println!("    endpoints, or instances. Self-host on any infrastructure.");
    println!("    Community support via GitHub, Slack, Discord.");
    println!();
    println!("• Enterprise Edition — Subscription (contact sales)");
    println!("    Per-instance or per-environment pricing. Indicative:");
    println!("    ~€10K-50K/year for typical enterprise deployments,");
    println!("    scaling up for high-instance-count deployments. Includes");
    println!("    24/7 support with SLA, LTS releases, and the advanced");
    println!("    security/observability features.");
    println!();
    println!("• KrakenD Studio — Subscription, add-on to EE");
    println!("    Centralized fleet management and observability.");
    println!();
    println!("Pricing is notably below Apigee, MuleSoft, and Kong Konnect");
    println!("Enterprise for equivalent throughput. The CE → EE ladder is");
    println!("realistic — CE is genuinely production-grade, so you only");
    println!("upgrade when you need specific Enterprise features.");
}

fn print_customers() {
    println!("KrakenD customer references (public):");
    println!();
    println!("  • Vodafone — telco partner APIs");
    println!("  • Booking.com — internal aggregation gateway");
    println!("  • Cabify — Spanish ride-hailing, mobile-app aggregation");
    println!("  • Telefónica — multi-country API platform");
    println!("  • Mediapro — sports broadcasting APIs");
    println!("  • Spanish public health systems (autonomous-community APIs)");
    println!("  • Bonial.com — original deployment (origin story)");
    println!("  • SoundCloud — partner/embed APIs (community references)");
    println!("  • Schibsted — Norwegian media group");
    println!("  • multiple European retailers and fintechs");
    println!();
    println!("Pattern: high-throughput aggregation use cases where the");
    println!("alternative is writing a custom BFF service per frontend.");
    println!("Strong adoption in EU and LATAM markets. Common pairing with");
    println!("Kubernetes ingress (Istio/Linkerd) for service-to-service plus");
    println!("KrakenD for client-facing aggregation.");
}

fn print_differentiator() {
    println!("Why teams pick KrakenD:");
    println!();
    println!("• Performance. If your gateway is the bottleneck and you need");
    println!("  to fix it without throwing 10x hardware at it, KrakenD's");
    println!("  Go-based stateless design wins benchmarks.");
    println!();
    println!("• Stateless operations. Add/remove instances behind a load");
    println!("  balancer with no coordination. Perfect for autoscaling Kubernetes");
    println!("  deployments. No 'control plane' to lose data on.");
    println!();
    println!("• Aggregation as a first-class feature. The whole point of");
    println!("  building an API gateway in 2025 (vs. just using an ingress)");
    println!("  is composition, and KrakenD does composition best-in-class.");
    println!();
    println!("• Declarative config in Git. No admin API drift. What's in");
    println!("  the JSON is exactly what's running.");
    println!();
    println!("• Apache 2.0 OSS without 'community edition is fake' games. CE");
    println!("  is genuinely usable in production; EE is purely additive.");
    println!();
    println!("• Low operational cost. Single Go binary, no DB, no JVM.");
    println!("  Container images are tiny (~20MB). Cold start is instant.");
    println!();
    println!("vs. Kong: KrakenD is faster on aggregation, Go-based, stateless,");
    println!("  no DB. Kong has a richer plugin ecosystem and Konnect cloud.");
    println!();
    println!("vs. Tyk: similar OSS positioning. KrakenD wins on aggregation");
    println!("  primitives; Tyk wins on developer portal and GraphQL universal");
    println!("  data graph.");
    println!();
    println!("vs. NGINX+Lua: KrakenD provides aggregation, JWT, transforms,");
    println!("  rate limiting out of the box; NGINX would require Lua coding.");
    println!();
    println!("vs. Apigee/MuleSoft: KrakenD is 10-100x cheaper, no vendor");
    println!("  lock-in. Lacks monetization and developer-portal depth.");
}

fn print_critique() {
    println!("Honest critique of KrakenD:");
    println!();
    println!("• No built-in developer portal. The dev portal is on the EE");
    println!("  roadmap but not core. Teams pair KrakenD with separate");
    println!("  documentation tools (Mintlify, Redoc, Bump.sh).");
    println!();
    println!("• No monetization features. If you need to bill consumers per");
    println!("  API call, you'll need an external system. Apigee, Kong");
    println!("  Konnect, Zuplo all have monetization built in.");
    println!();
    println!("• No admin UI for runtime configuration. Some teams want");
    println!("  clicky UIs to edit policies. KrakenDesigner mitigates this");
    println!("  but is Designer-time, not runtime — the philosophy is");
    println!("  configuration-as-code, period.");
    println!();
    println!("• Plugin ecosystem is smaller than Kong's. KrakenD has");
    println!("  extensibility (Lua scripting, Martian DSL, Go-plugin builds)");
    println!("  but the community marketplace of pre-built plugins is thin.");
    println!();
    println!("• Brand recognition lower in North America. European adoption");
    println!("  is strong; US enterprise sales motion is smaller.");
    println!();
    println!("• Hot-reload of configuration sometimes requires full restart");
    println!("  for major topology changes. Stateless design means restarts");
    println!("  are cheap but you do need a rolling restart strategy.");
    println!();
    println!("• Documentation is good for getting started but advanced");
    println!("  topics (custom Go-plugin development, OPA integration");
    println!("  patterns, complex aggregation graphs) sometimes require");
    println!("  asking on Discord rather than reading docs.");
}

fn run_krakend(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
        "performance" | "perf" => { print_performance(); 0 }
        "aggregation" | "agg" => { print_aggregation(); 0 }
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
        .unwrap_or_else(|| "krakend".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_krakend(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/krakend"), "krakend"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("krakend.exe"), "krakend"); }
    #[test] fn t_help() { assert_eq!(run_krakend(&[], "krakend"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_krakend(&["xx".to_string()], "krakend"), 2); }
}
