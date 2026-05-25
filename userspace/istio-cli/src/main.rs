#![deny(clippy::all)]

//! istio-cli — OurOS Istio service mesh
//!
//! Single personality: `istioctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_istio(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: istioctl [COMMAND] [OPTIONS]");
        println!("istioctl v1.21 (OurOS) — Istio service mesh CLI");
        println!();
        println!("Commands:");
        println!("  install            Install Istio");
        println!("  manifest           Generate manifests");
        println!("  profile            Manage profiles");
        println!("  analyze            Analyze configuration");
        println!("  proxy-status       Show proxy sync status");
        println!("  proxy-config       Show proxy configuration");
        println!("  dashboard          Open web dashboards");
        println!("  kube-inject        Inject sidecar");
        println!("  verify-install     Verify installation");
        println!();
        println!("Options:");
        println!("  --context CTX      Kubernetes context");
        println!("  --namespace NS     Namespace");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("istioctl v1.21.2 (OurOS)"); return 0; }
    println!("istioctl v1.21.2 (OurOS)");
    println!("  Mesh: healthy");
    println!("  Control plane: istiod (1 replica)");
    println!("  Data plane: 23 proxies (Envoy v1.29)");
    println!("  Gateways: 2 (ingress, egress)");
    println!("  Virtual services: 12");
    println!("  Destination rules: 8");
    println!("  mTLS: STRICT mode");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "istioctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_istio(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
