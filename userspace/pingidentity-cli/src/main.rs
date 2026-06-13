#![deny(clippy::all)]

//! pingidentity-cli — SlateOS Ping Identity (federated enterprise IAM, ForgeRock merged in)
//!
//! Single personality: `pingidentity`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ping(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pingidentity [OPTIONS]");
        println!("Ping Identity (SlateOS) — Enterprise IAM, PingOne + PingFederate + ForgeRock");
        println!();
        println!("Options:");
        println!("  --pingone              PingOne (cloud IAM)");
        println!("  --pingfederate         PingFederate (SAML/OIDC federation, on-prem or hybrid)");
        println!("  --pingid               PingID (MFA)");
        println!("  --davinci              PingOne DaVinci (no-code identity orchestration)");
        println!("  --forgerock            ForgeRock products (merged 2023)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ping Identity 2024 (SlateOS)"); return 0; }
    println!("Ping Identity 2024 (SlateOS)");
    println!("  Vendor: Ping Identity Corporation (Denver, CO)");
    println!("          taken private by Thoma Bravo Oct 2022 for $2.8B");
    println!("          acquired ForgeRock Aug 2023 for $2.3B (Thoma Bravo also owned that)");
    println!("          merged operations under Ping Identity brand");
    println!("  Founders: Andre Durand (Denver, 2002)");
    println!("           Durand: still Founder + 'visionary' role, identity industry fixture");
    println!("  History: pre-cloud IAM pioneer (PingFederate on-prem SAML 2.0 since mid-2000s)");
    println!("          early thought leader on identity federation standards (SAML, WS-*, OIDC)");
    println!("          IPO 2019 (NYSE:PING) → take-private 2022");
    println!("  Pricing: enterprise — custom (typically $50K-$1M+/year deployments)");
    println!("  Products (post-ForgeRock merger):");
    println!("    PingOne — cloud IAM platform");
    println!("      - PingOne SSO (Workforce + Customer)");
    println!("      - PingOne MFA");
    println!("      - PingOne DaVinci (no-code identity workflow orchestration)");
    println!("      - PingOne Authorize (fine-grained authorization)");
    println!("      - PingOne Risk + Fraud + Protect");
    println!("      - PingOne Verify (identity proofing — government ID + selfie)");
    println!("      - PingOne Cloud Directory (Universal Directory equivalent)");
    println!("    PingFederate — federation gateway (still widely deployed on-prem)");
    println!("    PingAccess — application gateway");
    println!("    PingDirectory — LDAP-compatible directory");
    println!("    PingID — MFA");
    println!("    PingCentral — admin console");
    println!("    ForgeRock Identity Cloud + Access Management + Identity Management + Directory Services");
    println!("  Strategy:");
    println!("    - Hybrid on-prem + cloud architecture (vs Okta's pure cloud)");
    println!("    - Highly regulated industries: large banks, telcos, healthcare, government");
    println!("    - Identity orchestration (DaVinci): no-code drag-and-drop login flows");
    println!("  Customers: 50%+ Fortune 100, half the world's largest banks, US federal agencies");
    println!("  Critique: complex product portfolio (PingOne + PingFed + PingAccess + PingDirectory + PingID + ForgeRock)");
    println!("           higher implementation cost than Okta/Auth0");
    println!("           still recovering from ForgeRock integration overhead (2023-2024)");
    println!("  Differentiator: depth + hybrid + complex regulated enterprise — where Okta can't go");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pingidentity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ping(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ping};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pingidentity"), "pingidentity");
        assert_eq!(basename(r"C:\bin\pingidentity.exe"), "pingidentity.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pingidentity.exe"), "pingidentity");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ping(&["--help".to_string()], "pingidentity"), 0);
        assert_eq!(run_ping(&["-h".to_string()], "pingidentity"), 0);
        let _ = run_ping(&["--version".to_string()], "pingidentity");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ping(&[], "pingidentity");
    }
}
