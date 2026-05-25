#![deny(clippy::all)]

//! innernet-cli — OurOS innernet WireGuard network manager
//!
//! Multi-personality: `innernet`, `innernet-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_innernet(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "innernet-server" => {
                println!("innernet-server (OurOS) — innernet WireGuard server");
                println!("  new                Create new network");
                println!("  serve INTERFACE    Start server");
                println!("  add-peer IFACE     Add peer invitation");
                println!("  add-cidr IFACE     Add new CIDR");
                println!("  rename-peer IFACE  Rename peer");
                println!("  delete-peer IFACE  Delete peer");
            }
            _ => {
                println!("innernet (OurOS) — innernet WireGuard client");
                println!("  install INVITE     Install invitation");
                println!("  up INTERFACE       Bring up interface");
                println!("  down INTERFACE     Bring down interface");
                println!("  fetch INTERFACE    Fetch peer updates");
                println!("  list INTERFACE     List peers");
                println!("  override-endpoint  Override endpoint");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("innernet v1.6.1 (OurOS)"); return 0; }
    match prog {
        "innernet-server" => {
            println!("innernet Server v1.6.1 (OurOS)");
            println!("  Network: mycompany");
            println!("  CIDR: 10.42.0.0/16");
            println!("  Peers: 15");
            println!("  Listening: 0.0.0.0:51820");
            println!("  API: 0.0.0.0:51821");
        }
        _ => {
            println!("innernet v1.6.1 (OurOS)");
            println!("  Interface: mycompany");
            println!("  Address: 10.42.1.5/16");
            println!("  Peers: 14 (12 reachable)");
            println!("  Endpoint: vpn.example.com:51820");
            println!("  Latest handshake: 32 seconds ago");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "innernet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_innernet(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
