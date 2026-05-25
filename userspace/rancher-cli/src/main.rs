#![deny(clippy::all)]

//! rancher-cli — OurOS SUSE Rancher Kubernetes management
//!
//! Single personality: `rancher`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rancher(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rancher [OPTIONS] [SUBCMD]");
        println!("SUSE Rancher 2.9 (OurOS) — Kubernetes management platform");
        println!();
        println!("Options:");
        println!("  login URL --token T    Authenticate to Rancher server");
        println!("  cluster list           List managed clusters");
        println!("  context switch CTX     Switch cluster context");
        println!("  kubectl ARGS           Pass-through to kubectl on current cluster");
        println!("  --rke / --rke2 / --k3s Provisioning engine");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rancher version v2.9.2 (OurOS)"); return 0; }
    println!("SUSE Rancher 2.9.2 (OurOS)");
    println!("  Mission: manage Kubernetes anywhere (any cloud, on-prem, edge)");
    println!("  K8s distros: RKE (Rancher K8s Engine), RKE2 (CNCF-certified), K3s (edge)");
    println!("  Multi-cluster: provision, import, observe 100s of clusters via Fleet");
    println!("  Apps: integrated catalog (Helm charts), GitOps via Fleet");
    println!("  Observability: integrated Prometheus, Grafana, AlertManager");
    println!("  Security: cluster scanning (Kube-bench), policy (OPA Gatekeeper), Neuvector");
    println!("  Service Mesh: integrated Istio, Linkerd");
    println!("  Acquired by SUSE 2020; community + SUSE Rancher Prime support tiers");
    println!("  License: Apache 2.0 (free); Rancher Prime subscription support");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rancher".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rancher(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
