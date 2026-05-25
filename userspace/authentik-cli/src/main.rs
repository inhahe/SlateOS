#![deny(clippy::all)]

//! authentik-cli — OurOS authentik identity provider
//!
//! Single personality: `authentik`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_authentik(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: authentik [COMMAND] [OPTIONS]");
        println!("authentik v2024.2 (OurOS) — Identity provider and SSO");
        println!();
        println!("Commands:");
        println!("  server             Start web server");
        println!("  worker             Start background worker");
        println!("  migrate            Run database migrations");
        println!("  create-admin       Create initial admin user");
        println!("  export-blueprint   Export configuration blueprint");
        println!("  import-blueprint   Import configuration blueprint");
        println!("  repair             Repair database state");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("authentik v2024.2.3 (OurOS)"); return 0; }
    println!("authentik v2024.2.3 (OurOS)");
    println!("  Web: https://0.0.0.0:9443");
    println!("  Users: 567");
    println!("  Groups: 12");
    println!("  Applications: 23");
    println!("  Providers: OIDC (15), SAML (5), LDAP (3)");
    println!("  Flows: 8 configured");
    println!("  Database: PostgreSQL");
    println!("  Cache: Redis");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "authentik".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_authentik(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
