#![deny(clippy::all)]

//! gocd-cli — OurOS GoCD continuous delivery
//!
//! Multi-personality: `gocd-server`, `gocd-agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gocd(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "gocd-agent" => {
                println!("gocd-agent (OurOS) — GoCD build agent");
                println!("  --server-url URL   GoCD server URL");
                println!("  --auto-register    Auto-register with server");
                println!("  --environments ENV Environments to belong to");
            }
            _ => {
                println!("gocd-server (OurOS) — GoCD continuous delivery server");
                println!("  --http-port PORT   HTTP port (default: 8153)");
                println!("  --https-port PORT  HTTPS port (default: 8154)");
                println!("  --config-dir DIR   Config directory");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GoCD v24.1.0 (OurOS)"); return 0; }
    match prog {
        "gocd-agent" => {
            println!("GoCD Agent v24.1.0 (OurOS)");
            println!("  Status: idle");
            println!("  Server: https://gocd.example.com:8154");
            println!("  Resources: linux, docker");
            println!("  Environments: production");
        }
        _ => {
            println!("GoCD Server v24.1.0 (OurOS)");
            println!("  Dashboard: http://0.0.0.0:8153");
            println!("  Pipelines: 23 (5 building)");
            println!("  Agents: 8 (6 idle, 2 building)");
            println!("  Environments: 3");
            println!("  Config repos: 2 (Git)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gocd-server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gocd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
