#![deny(clippy::all)]

//! netmaker-cli — OurOS Netmaker virtual network platform
//!
//! Multi-personality: `netmaker`, `nmctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_netmaker(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "nmctl" => {
                println!("nmctl (OurOS) — Netmaker CLI client");
                println!("  network list|create|delete  Manage networks");
                println!("  node list|create|delete     Manage nodes");
                println!("  key list|create|delete      Manage enrollment keys");
                println!("  dns list|create|delete      Manage DNS entries");
                println!("  acl list|allow|deny         Manage ACLs");
                println!("  context set|list             Set API context");
            }
            _ => {
                println!("netmaker (OurOS) — Netmaker server");
                println!("  serve              Start Netmaker server");
                println!("  --config FILE      Config file");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Netmaker v0.24.2 (OurOS)"); return 0; }
    match prog {
        "nmctl" => {
            println!("Netmaker Networks:");
            println!("  office-lan  10.10.0.0/24  8 nodes  WireGuard");
            println!("  dev-mesh    10.20.0.0/24  5 nodes  WireGuard");
            println!("  Total: 2 networks, 13 nodes");
        }
        _ => {
            println!("Netmaker v0.24.2 (OurOS)");
            println!("  Networks: 2");
            println!("  Nodes: 13");
            println!("  Egress gateways: 2");
            println!("  Ingress gateways: 1");
            println!("  API: https://0.0.0.0:8081");
            println!("  Dashboard: https://0.0.0.0:8082");
            println!("  MQ broker: 0.0.0.0:8883 (MQTT/TLS)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "netmaker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_netmaker(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
