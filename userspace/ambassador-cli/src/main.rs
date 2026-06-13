#![deny(clippy::all)]
//! ambassador-cli — SlateOS Ambassador Labs Edge Stack personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Ambassador Labs Edge Stack / Emissary-ingress.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Datawire to Ambassador Labs evolution");
    println!("    products        Edge Stack, Emissary, Telepresence, Blackbird");
    println!("    emissary        Emissary-ingress (CNCF Incubating)");
    println!("    telepresence    Local k8s dev with Telepresence");
    println!("    pricing         OSS + Edge Stack tiers");
    println!("    customers       Notable users");
    println!("    differentiator  Envoy-based K8s-native ingress");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Ambassador Labs — Envoy-based Kubernetes ingress and edge.");
    println!();
    println!("The company started as Datawire, founded in 2014 in Boston by");
    println!("Richard Li (CEO), Rafael Schloming, and a small team. Earlier");
    println!("history: Richard had been VP of Engineering at Red Hat for the");
    println!("Cloud BU and earlier at JBoss. Rafael was a core developer of");
    println!("Apache Qpid and AMQP at Red Hat.");
    println!();
    println!("Datawire's initial product was a microservices runtime; they");
    println!("pivoted in 2017 to focus on Ambassador, an open-source Envoy-");
    println!("based API gateway designed natively for Kubernetes. The thesis:");
    println!("Kubernetes was eating the world, but the existing API gateway");
    println!("vendors (Kong, Apigee, NGINX) had not been designed for the");
    println!("Kubernetes operational model — declarative CRDs, GitOps, label-");
    println!("based routing, sidecar proxies.");
    println!();
    println!("Rebranded as Ambassador Labs in 2020 to reflect the broader");
    println!("portfolio (Telepresence, Edge Stack, Blackbird API Studio).");
    println!();
    println!("Funding: Series A ~$5M 2017 led by Matrix Partners. Series B");
    println!("~$28M 2019 led by Insight. Series C ~$75M 2021 led by Insight");
    println!("and Eight Roads, $50M Series D ~2023. Total raised ~$160M.");
    println!("HQ Boston with remote engineering. Ambassador's Emissary-ingress");
    println!("(the OSS core of Edge Stack) is a CNCF Incubating project as of");
    println!("2021, donated by Datawire/Ambassador Labs.");
}

fn print_products() {
    println!("Ambassador Labs product portfolio:");
    println!();
    println!("• Ambassador Edge Stack");
    println!("    Commercial, comprehensive API gateway for Kubernetes. Built");
    println!("    on Envoy via Emissary-ingress, adds developer portal, OAuth");
    println!("    integrations, rate limiting service, edge policy console,");
    println!("    Filter chain, OPA integration, advanced rate limiting,");
    println!("    Service Preview, and Argo CD integration. Free tier exists.");
    println!();
    println!("• Emissary-ingress");
    println!("    Open-source Apache 2.0 Envoy-based ingress for Kubernetes.");
    println!("    The unbundled core of Edge Stack. CNCF Incubating since 2021.");
    println!("    Pure CRD-driven configuration — no admin API or DB.");
    println!();
    println!("• Telepresence");
    println!("    Local development for Kubernetes. Intercept traffic from a");
    println!("    cluster-deployed service to your laptop, with personal");
    println!("    intercepts (only your traffic gets routed to your dev box).");
    println!("    CNCF Sandbox project. Open-source core, commercial cloud.");
    println!();
    println!("• Blackbird API Development (newer)");
    println!("    API design + mocking + testing platform. Auto-generates");
    println!("    backend code from an OpenAPI spec, deploys mocks for");
    println!("    frontend development before the backend is ready.");
    println!();
    println!("• Ambassador Cloud");
    println!("    Hosted control plane for managing Edge Stack and");
    println!("    Telepresence across clusters. Subscription model.");
}

fn print_emissary() {
    println!("Emissary-ingress — the open-source core.");
    println!();
    println!("Emissary is a Kubernetes-native ingress controller built on");
    println!("Envoy. CNCF Incubating, Apache 2.0. Originally the open-source");
    println!("'Ambassador' project before Edge Stack was extracted as the");
    println!("commercial product.");
    println!();
    println!("Architecture:");
    println!("  • Envoy as the data plane (sidecarless ingress mode)");
    println!("  • Emissary control plane: watches Kubernetes API for CRDs,");
    println!("    translates to Envoy xDS configuration, pushes via gRPC ADS");
    println!("  • Pure CRD-driven config — no etcd-of-its-own, no separate DB");
    println!();
    println!("Key CRDs:");
    println!("  • Mapping: route definitions (similar to Ingress but richer)");
    println!("  • Host: TLS certificates, hostname patterns");
    println!("  • TLSContext: TLS configuration");
    println!("  • AuthService: external authorization plugin");
    println!("  • RateLimitService: external rate limiting plugin");
    println!("  • TracingService: distributed tracing config");
    println!("  • LogService: external logging");
    println!("  • Module: global configuration");
    println!("  • Filter / FilterPolicy: pre/post processing chain (Edge Stack)");
    println!();
    println!("Capabilities:");
    println!("  • L7 routing: hostname, path, header, method, query, weights");
    println!("  • Canary deployments (weighted routes)");
    println!("  • Circuit breakers, retries, timeouts (Envoy native)");
    println!("  • TLS termination, mTLS, SNI");
    println!("  • gRPC, WebSockets, HTTP/2, HTTP/3 support");
    println!("  • Cross-Origin Resource Sharing (CORS)");
    println!("  • Service Mesh integration (Linkerd, Istio, Consul Connect)");
    println!();
    println!("Emissary is comparable to Contour (another Envoy-based ingress)");
    println!("and to the Kubernetes Gateway API ingress implementations.");
}

fn print_telepresence() {
    println!("Telepresence — local Kubernetes development.");
    println!();
    println!("Problem: developing microservices on Kubernetes requires either");
    println!("running the whole stack locally (expensive, slow, divergent from");
    println!("production) or constant push-build-deploy cycles to a shared");
    println!("dev cluster (10-minute feedback loops, painful).");
    println!();
    println!("Telepresence's solution: redirect traffic from a cluster-deployed");
    println!("service to a process running on your laptop, transparently.");
    println!("Your laptop becomes a participant in the cluster network, with");
    println!("DNS resolution for Service names, ability to call other cluster");
    println!("services, and traffic intercepted from the real ingress.");
    println!();
    println!("Modes:");
    println!("  • Global intercept (OSS): all traffic to a service is routed");
    println!("    to your laptop. Useful for solo development.");
    println!("  • Personal intercept (commercial cloud feature): only traffic");
    println!("    matching a header (e.g., your dev cookie) is routed to your");
    println!("    laptop; the rest still goes to the cluster pod. Multiple");
    println!("    developers can intercept the same service simultaneously");
    println!("    without colliding.");
    println!();
    println!("Implementation:");
    println!("  • Traffic Agent sidecar injected into the intercepted Pod");
    println!("  • Userspace VPN tunnel from laptop to cluster (sshfuse/ kubectl");
    println!("    port-forward style)");
    println!("  • DNS resolver on laptop intercepts cluster service names");
    println!();
    println!("CNCF Sandbox project. Telepresence v2 (2021+) rewrote the core");
    println!("in Go after Telepresence v1 (Python + Twisted) hit scaling");
    println!("limits.");
}

fn print_pricing() {
    println!("Ambassador Labs pricing:");
    println!();
    println!("• Emissary-ingress — Free (Apache 2.0, CNCF)");
    println!();
    println!("• Telepresence OSS — Free (Apache 2.0)");
    println!();
    println!("• Edge Stack Community — Free");
    println!("    Edge Stack with 5 connected services limit, community support.");
    println!("    Enough for evaluation and small clusters.");
    println!();
    println!("• Edge Stack Enterprise — Subscription (contact sales)");
    println!("    Unlimited services, advanced features (developer portal,");
    println!("    OAuth/OIDC filter, OPA policy engine, advanced rate limiting),");
    println!("    24/7 support, SLA. Indicative: low-to-mid five figures USD");
    println!("    per cluster per year.");
    println!();
    println!("• Telepresence Pro / Enterprise — Subscription");
    println!("    Personal intercepts (the multi-developer mode), SSO");
    println!("    integration, audit logs, premium support. Per-user pricing.");
    println!();
    println!("• Blackbird API Dev — Subscription tiers");
    println!("    Hobby free, Team paid, Enterprise custom.");
}

fn print_customers() {
    println!("Ambassador Labs customer references (public):");
    println!();
    println!("  • AT&T — Edge Stack for telco K8s deployments");
    println!("  • Microsoft — internal teams using Telepresence");
    println!("  • Cisco — Edge Stack in customer-facing K8s products");
    println!("  • Ticketmaster — Telepresence for development workflows");
    println!("  • Bose — Edge Stack for connected devices APIs");
    println!("  • The New York Times — K8s ingress for editorial systems");
    println!("  • PayPal — internal API gateway");
    println!("  • Lifion (ADP) — Telepresence in dev workflow");
    println!("  • Bloomberg — Telepresence for terminal microservices dev");
    println!("  • Comcast — K8s ingress at scale");
    println!();
    println!("Pattern: Kubernetes-native shops where Envoy is the ingress");
    println!("technology of choice and the team values declarative CRD-driven");
    println!("operations. Telepresence has independent adoption — many use it");
    println!("without Edge Stack.");
}

fn print_differentiator() {
    println!("Why teams pick Ambassador:");
    println!();
    println!("• Kubernetes-native from day one. CRDs, GitOps-friendly,");
    println!("  declarative. No admin API or DB to manage outside K8s.");
    println!();
    println!("• Envoy data plane. Same battle-tested proxy that powers Istio,");
    println!("  Consul Connect, AWS App Mesh, Google Cloud Service Mesh.");
    println!("  Excellent performance, observability, and protocol support.");
    println!();
    println!("• Telepresence is genuinely useful, almost regardless of");
    println!("  whether you use Edge Stack. Many K8s teams adopt Telepresence");
    println!("  for dev workflow improvement without touching the API gateway.");
    println!();
    println!("• CNCF citizenship. Emissary is Incubating; Telepresence is");
    println!("  Sandbox. Open governance, contribution paths, no rugpull risk.");
    println!();
    println!("• Smooth Envoy upgrade path. Edge Stack tracks Envoy releases");
    println!("  closely, so you get HTTP/3, eBPF features, etc., faster than");
    println!("  with proxies that fork heavily.");
    println!();
    println!("vs. Kong: Kong has more polished dev portal, more plugins,");
    println!("  Konnect cloud control plane. Ambassador is more K8s-native");
    println!("  and Envoy-based. Different philosophies.");
    println!();
    println!("vs. NGINX Ingress: Ambassador is more feature-rich (canary,");
    println!("  rate limiting, OAuth filters), declarative-CRD-first. NGINX");
    println!("  Ingress is older and more ubiquitous but less expressive.");
    println!();
    println!("vs. Istio Gateway: Istio gives you ingress + east-west mesh;");
    println!("  Ambassador focuses on ingress only. Istio is more complex");
    println!("  to operate; Ambassador is simpler if you don't need mesh.");
    println!();
    println!("vs. Contour: Both Envoy-based, both CNCF. Contour is leaner");
    println!("  and laser-focused; Ambassador Edge Stack adds many features.");
}

fn print_critique() {
    println!("Honest critique of Ambassador:");
    println!();
    println!("• Edge Stack is Kubernetes-only. If you run mixed VM + K8s");
    println!("  infrastructure, Edge Stack can't serve as the unified API");
    println!("  gateway. Pure K8s shops only.");
    println!();
    println!("• Documentation has been uneven. Edge Stack docs and Emissary");
    println!("  docs sometimes diverge, with the open-source side getting");
    println!("  less attention than the commercial product.");
    println!();
    println!("• Developer portal is functional but less polished than");
    println!("  competitors (Kong Insomnia, Apigee, Zuplo). Custom branding");
    println!("  requires significant CSS/HTML work.");
    println!();
    println!("• Monetization is not a first-class feature. If you sell APIs");
    println!("  per-call, Edge Stack doesn't provide billing primitives —");
    println!("  you integrate Stripe yourself.");
    println!();
    println!("• Brand transitions confuse customers. 'Datawire Ambassador →");
    println!("  Ambassador Labs → Emissary-ingress + Edge Stack' has been a");
    println!("  series of renames that left docs and Stack Overflow answers");
    println!("  referring to obsolete names.");
    println!();
    println!("• Telepresence v1 → v2 was a hard reset. Old TP1 features and");
    println!("  workflows don't all map cleanly. Some users stayed on TP1");
    println!("  longer than the team would prefer.");
    println!();
    println!("• Closer-to-the-metal ingress (Envoy directly or Contour) can");
    println!("  be lighter operationally if you don't need the Edge Stack");
    println!("  bells and whistles.");
}

fn run_ambassador(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "emissary" => { print_emissary(); 0 }
        "telepresence" | "tp" => { print_telepresence(); 0 }
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
        .unwrap_or_else(|| "ambassador".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_ambassador(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/ambassador"), "ambassador"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("ambassador.exe"), "ambassador"); }
    #[test] fn t_help() { assert_eq!(run_ambassador(&[], "ambassador"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_ambassador(&["xx".to_string()], "ambassador"), 2); }
}
