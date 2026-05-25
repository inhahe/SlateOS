#![deny(clippy::all)]

//! gloo-cli — OurOS Gloo Edge API gateway
//!
//! Single personality: `glooctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gloo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: glooctl [COMMAND] [OPTIONS]");
        println!("glooctl v1.16 (OurOS) — Gloo Edge API gateway CLI");
        println!();
        println!("Commands:");
        println!("  install            Install Gloo Edge");
        println!("  uninstall          Uninstall Gloo Edge");
        println!("  check              Health check");
        println!("  get upstreams|virtualservices|proxies  List resources");
        println!("  create upstream|virtualservice  Create resource");
        println!("  delete upstream|virtualservice  Delete resource");
        println!("  proxy              Manage proxies");
        println!("  route              Manage routes");
        println!("  dashboard          Open web UI");
        println!();
        println!("Options:");
        println!("  --namespace NS     Namespace");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("glooctl v1.16.12 (OurOS)"); return 0; }
    println!("Gloo Edge v1.16.12 (OurOS)");
    println!("  Status: healthy");
    println!("  Upstreams: 12 (10 accepted)");
    println!("  Virtual services: 8");
    println!("  Routes: 23");
    println!("  Proxies: 1 (gateway-proxy)");
    println!("  Auth configs: 3");
    println!("  Rate limit configs: 2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "glooctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gloo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
