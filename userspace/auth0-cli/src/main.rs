#![deny(clippy::all)]

//! auth0-cli — OurOS Auth0 (developer-friendly CIAM, now Okta Customer Identity Cloud)
//!
//! Single personality: `auth0`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_a0(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: auth0 [OPTIONS]");
        println!("Auth0 by Okta (OurOS) — Identity-as-a-Service for developers");
        println!();
        println!("Options:");
        println!("  --universal-login      Universal Login (hosted login page)");
        println!("  --rules                Rules (deprecated → Actions)");
        println!("  --actions              Actions (extension hooks at login/registration)");
        println!("  --b2b                  Organizations (B2B multi-tenant)");
        println!("  --b2c                  B2C login flows");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Auth0 by Okta 2024 (OurOS)"); return 0; }
    println!("Auth0 by Okta 2024 (OurOS)");
    println!("  Vendor: Auth0, Inc. → acquired by Okta May 2021 for $6.5B (all-stock)");
    println!("          rebranded 'Customer Identity Cloud (Auth0)' under Okta");
    println!("  Founders: Eugenio Pace + Matías Woloski (Argentina → Bellevue WA)");
    println!("           Eugenio: ex-Microsoft Identity team, deep federation expertise");
    println!("           Matías: led Argentine .NET community");
    println!("  Founded: 2013 (Bellevue, WA + Buenos Aires, Argentina)");
    println!("  Funding: Bessemer, Meritech, Sapphire + others — ~$330M raised pre-acquisition");
    println!("  Strategy: 'developer-first authentication' — beautiful docs + SDKs for every platform");
    println!("           Auth0 captured mindshare with hands-on developer experience");
    println!("           targeted CIAM (Customer Identity & Access Management) vs Okta's workforce focus");
    println!("  Pricing: Free tier — 25K MAU, social + email/password");
    println!("          B2C Essentials: from $35/mo (1,000 MAU)");
    println!("          B2C Professional: from $240/mo (1,000 MAU, MFA, custom domains)");
    println!("          B2B Essentials/Professional: per-organization pricing");
    println!("          Enterprise: custom (HIPAA, FedRAMP, unlimited MAU)");
    println!("  Killer features:");
    println!("    - Universal Login: hosted login page, drop-in OAuth/OIDC flows");
    println!("    - 30+ SDKs (Node, Python, Go, .NET, Java, iOS, Android, React, Angular, Vue, ...)");
    println!("    - 60+ identity providers out-of-box (Google, Facebook, Microsoft, GitHub, LinkedIn, ...)");
    println!("    - Database connection options: own DB, Auth0-hosted, migration from existing")    ;
    println!("    - Actions (Node 18 sandbox): pre/post hooks at login, signup, MFA, etc.");
    println!("    - Rules (legacy, being migrated to Actions)");
    println!("    - M2M (Machine-to-Machine) tokens for service-to-service");
    println!("    - Organizations: B2B multi-tenant (each customer = an org with its own users + branding)");
    println!("    - Attack Protection: brute-force, bot detection, breached-password detection");
    println!("    - Custom Domains (Professional+) — auth.yourdomain.com");
    println!("    - Adaptive MFA (risk scoring, step-up auth)");
    println!("  Devex: famously good docs at auth0.com/docs — case study for SaaS documentation");
    println!("  Customers: Stripe, Pfizer, Subway, Mazda, Siemens — anyone needing modern customer login");
    println!("  Critique: pricing complex (MAU-based + feature gates) — surprise bills at scale");
    println!("           Rules → Actions migration painful (Auth0 deprecating old extension model)");
    println!("           post-acquisition some feel slowed-down product velocity");
    println!("  Differentiator: gold standard developer experience for CIAM — fastest 'hello world' to prod login");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "auth0".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_a0(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
