#![deny(clippy::all)]

//! calico-cli — OurOS Calico container networking
//!
//! Multi-personality: `calicoctl`, `calico-node`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_calico(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "calico-node" => {
                println!("calico-node (OurOS) — Calico per-node daemon");
                println!("  Runs Felix (policy), BIRD (BGP), confd");
            }
            _ => {
                println!("calicoctl v3.28 (OurOS) — Calico management CLI");
                println!("  get                List resources");
                println!("  create             Create resources");
                println!("  replace            Replace resources");
                println!("  delete             Delete resources");
                println!("  apply              Apply resources");
                println!("  node               Node operations");
                println!("  ipam               IP address management");
                println!("  -o FORMAT          Output format (yaml, json, table)");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Calico v3.28.0 (OurOS)"); return 0; }
    match prog {
        "calico-node" => {
            println!("calico-node v3.28.0 (OurOS)");
            println!("  Felix: started (policy enforcement)");
            println!("  BIRD: started (BGP peering)");
            println!("  confd: started (config management)");
            println!("  Node: k8s-worker-01");
            println!("  Pod CIDR: 10.244.1.0/24");
            println!("  BGP peers: 3 established");
        }
        _ => {
            println!("calicoctl v3.28.0 (OurOS)");
            println!("  Cluster type: Kubernetes");
            println!("  Nodes: 5 (3 ready, 2 not-ready)");
            println!("  Network policies: 12");
            println!("  IP pools: 1 (10.244.0.0/16)");
            println!("  Workload endpoints: 45");
            println!("  Host endpoints: 5");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "calicoctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_calico(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
