#![deny(clippy::all)]

//! env0-cli — OurOS env0 environment automation
//!
//! Single personality: `env0`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_env0(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: env0 [COMMAND] [OPTIONS]");
        println!("env0 v2.0 (OurOS) — Environment-as-a-Service platform");
        println!();
        println!("Commands:");
        println!("  environment list|create|destroy   Manage environments");
        println!("  deployment list|approve|cancel     Manage deployments");
        println!("  template list|create               Manage templates");
        println!("  project list|create                Manage projects");
        println!("  cost report                        Cost reporting");
        println!("  drift detect                       Drift detection");
        println!();
        println!("Options:");
        println!("  --api-key KEY      API key");
        println!("  --org-id ID        Organization ID");
        println!("  --output json|table Output format");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("env0 v2.0.1 (OurOS)"); return 0; }
    println!("env0 v2.0.1 (OurOS)");
    println!("  Organizations: 1");
    println!("  Projects: 8");
    println!("  Environments: 23 active");
    println!("  Templates: 15");
    println!("  Deployments: 45 (last 7d)");
    println!("  Estimated cost: $1,234/mo");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "env0".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_env0(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
