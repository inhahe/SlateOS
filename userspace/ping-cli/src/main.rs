#![deny(clippy::all)]

//! ping-cli — OurOS Ping Identity (enterprise IDaaS, Denver CO, Thoma Bravo, merged with ForgeRock 2023)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ping(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ping [OPTIONS]");
        println!("Ping Identity (OurOS) — enterprise identity (PingOne + PingFederate + PingAccess + PingAuthorize)");
        println!();
        println!("Options:");
        println!("  --pingone              PingOne Cloud (IDaaS unified platform)");
        println!("  --pingfederate         PingFederate (SAML/OIDC federation server)");
        println!("  --pingaccess           PingAccess (web access management)");
        println!("  --pingauthorize        PingAuthorize (dynamic authorization)");
        println!("  --pingid               PingID (MFA)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ping Identity 2024 (OurOS) — ping-cli (PingOne admin)"); return 0; }
    println!("Ping Identity 2024 (OurOS) — Enterprise IDaaS Platform (Thoma Bravo / ForgeRock merger)");
    println!("  Vendor: Ping Identity Corporation (Denver, CO — private under Thoma Bravo)");
    println!("  Founders: Andre Durand, 2002 (CEO until 2023, founder-led for 21 years)");
    println!("          Andre Durand: serial entrepreneur, founded Ping Identity from scratch");
    println!("          Originally focused on identity federation (SAML)");
    println!("          'Ping' name = a ping between identity providers (federation reference)");
    println!("          Boulder/Denver Colorado deep-tech scene");
    println!("          IPO'd NYSE:PING 2019 at $15/share (raised $187.5M)");
    println!("          Acquired private by Thoma Bravo Oct 2022 for $2.8B ($28.50/share)");
    println!("          Andre Durand stepped down post-acquisition; new CEO appointed");
    println!("  Strategic move (Aug 2023):");
    println!("    - Thoma Bravo announced merger of Ping Identity + ForgeRock");
    println!("    - Combined entity: largest pure-play enterprise IDaaS outside Okta + Microsoft");
    println!("    - Headcount ~3000 combined, revenue ~$1B combined");
    println!("    - Andre Durand returned as CEO of combined entity");
    println!("    - Branding continues as Ping Identity (ForgeRock products being integrated)");
    println!("    - Joins SailPoint (also Thoma Bravo) in identity portfolio");
    println!("    - Thoma Bravo: largest software PE firm, building identity vertical");
    println!("  Strategic position: 'enterprise IDaaS for large complex orgs':");
    println!("                    pitch: 'identity for the Fortune 500 — federation, AuthN, AuthZ, MFA, fraud'");
    println!("                    target: Fortune 1000, government, healthcare, financial services");
    println!("                    primary competitor: Okta, Microsoft Entra ID (Azure AD), IBM Security Verify");
    println!("                    secondary: Auth0 (now Okta), SailPoint (IGA), CyberArk (PAM)");
    println!("                    Ping wedge: federation heritage + complex use cases + on-prem + cloud hybrid");
    println!("                    + workforce + customer + B2B identity coverage");
    println!("                    + post-ForgeRock merger = largest enterprise IDaaS portfolio");
    println!("                    'For complex Fortune 500 IT, Ping handles what Okta can't'");
    println!("  Pricing (enterprise, opaque):");
    println!("    PingOne for Customers Essentials: ~$0.20/MAU/mo (consumer identity)");
    println!("    PingOne for Workforce Essentials: ~$3/user/mo (employee SSO)");
    println!("    PingOne MFA: ~$3-5/user/mo add-on");
    println!("    PingOne for Customers Plus / Premium: ~$0.50-1+/MAU/mo");
    println!("    PingFederate on-prem: per-server licensing, 6-figure enterprise deals");
    println!("    PingAccess on-prem: per-server licensing");
    println!("    typically 6-7 figure annual contracts for large deployments");
    println!("    'Enterprise PE-owned pricing' = procurement-heavy");
    println!("  Architecture (the multi-product stack):");
    println!("    - PingOne: cloud-native multi-tenant IDaaS");
    println!("    - PingFederate: self-hosted federation server (Java, SAML/OIDC IdP + SP)");
    println!("    - PingAccess: reverse-proxy web access management (Java)");
    println!("    - PingAuthorize: dynamic policy decision point (XACML / dynamic authorization)");
    println!("    - PingDirectory: LDAP-compatible directory server (Java, from UnboundID acq 2016)");
    println!("    - PingID: MFA (push, OTP, FIDO2, biometric)");
    println!("    - PingIntelligence for APIs: API security + bot detection");
    println!("    - DaVinci: identity orchestration low-code platform (Singular Key acq 2021)");
    println!("    - 'Each product for a specific identity use case, integrate as needed'");
    println!("  Product portfolio (the merged Ping + ForgeRock kitchen sink):");
    println!("    1. PingOne (the cloud unified platform):");
    println!("       - Multi-tenant cloud IDaaS");
    println!("       - Workforce + Customer + B2B SSO");
    println!("       - SAML + OIDC + OAuth 2.0");
    println!("       - Unified admin console");
    println!("       - PingOne for Customers, for Workforce, MFA, Risk, Fraud bundles");
    println!("    2. PingFederate (the federation server, on-prem/private cloud):");
    println!("       - Self-hosted SAML + OIDC IdP and SP");
    println!("       - Adaptive auth policies");
    println!("       - The product that made Ping famous (federation heritage)");
    println!("       - Used by: complex enterprise where SaaS IDaaS doesn't fit");
    println!("    3. PingAccess (web access management):");
    println!("       - Reverse proxy + access policies");
    println!("       - Replaces legacy SiteMinder / Oracle Access Manager");
    println!("       - WAM market: shrinking but still significant install base");
    println!("    4. PingAuthorize (dynamic authorization):");
    println!("       - Externalized authorization (PDP/PEP architecture)");
    println!("       - JSON Pointer-based policies");
    println!("       - GraphQL + REST integration");
    println!("       - 'Beyond RBAC — attribute-based + risk-based authorization'");
    println!("    5. PingID (MFA):");
    println!("       - Push notifications, OTP, FIDO2 keys, biometric");
    println!("       - Adaptive risk-based MFA");
    println!("       - 'Goodbye SMS OTP, hello FIDO2'");
    println!("    6. PingDirectory (LDAP directory server):");
    println!("       - From UnboundID acquisition 2016");
    println!("       - Enterprise LDAP directory");
    println!("       - Supports billions of identities");
    println!("       - Common for: telcos, large consumer-identity stores");
    println!("    7. DaVinci (identity orchestration):");
    println!("       - Low-code flows for identity journeys");
    println!("       - Acquired Singular Key 2021");
    println!("       - 'Drag-and-drop multi-step auth + identity workflows'");
    println!("    8. PingIntelligence for APIs:");
    println!("       - API security: bot detection, anomaly detection");
    println!("       - ML-based behavioral analysis");
    println!("       - Complements API gateways (Apigee, Kong, etc.)");
    println!("    9. PingCentral (admin):");
    println!("       - Multi-PingFederate / PingAccess management");
    println!("       - Self-service for app onboarding teams");
    println!("    10. ForgeRock integration (post-merger):");
    println!("       - ForgeRock Identity Cloud → integrating with PingOne");
    println!("       - OpenAM, OpenIDM, OpenDJ → integrating with PingFederate, PingDirectory");
    println!("       - Combined roadmap announced 2024");
    println!("  The Andre Durand return (the founder comeback):");
    println!("    - Founded Ping 2002, ran it for 20 years through IPO");
    println!("    - Sold to Thoma Bravo 2022, stepped down");
    println!("    - Thoma Bravo brought him back as CEO of merged Ping+ForgeRock entity 2023");
    println!("    - Rare in PE-backed enterprise software: original founder returns");
    println!("    - Signals Thoma Bravo's commitment to building, not just margin-extracting");
    println!("  The federation heritage:");
    println!("    - 'Ping' literally references the SAML federation handshake");
    println!("    - PingFederate (originally 'PingFederate Server') was the flagship for 15 years");
    println!("    - When SaaS adoption boomed, Ping pivoted to PingOne cloud");
    println!("    - Still: PingFederate on-prem revenues remain significant");
    println!("    - 'For enterprises with hybrid + on-prem identity, Ping is the choice'");
    println!("  Integrations:");
    println!("    - PingOne admin UI + API");
    println!("    - PingFederate admin UI + REST API");
    println!("    - PingAccess CLI + REST API");
    println!("    - PingID SDK for mobile apps");
    println!("    - 1000+ integrations (Salesforce, Workday, ServiceNow, Office 365, AWS, GCP, Azure)");
    println!("    - SCIM 2.0 for user provisioning");
    println!("    - SAML 2.0 + OIDC + OAuth 2.0 + FIDO2/WebAuthn");
    println!("    - Active Directory + LDAP source integration");
    println!("    - SIEM integrations (Splunk, QRadar, Sentinel)");
    println!("    - Identity governance: integrates with SailPoint (sister Thoma Bravo company)");
    println!("    - Terraform provider for IaC");
    println!("  Ping CLI usage:");
    println!("    # PingOne admin via REST (most common):");
    println!("    curl -H 'Authorization: Bearer <token>' https://api.pingone.com/v1/environments");
    println!("    # Terraform PingOne provider:");
    println!("    # provider \"pingone\" {{ client_id = \"...\" client_secret = \"...\" }}");
    println!("    # PingFederate admin CLI (older):");
    println!("    pf-admin-cli list connections                            # SAML connections");
    println!("    pf-admin-cli export connection my-sp                     # export SP config");
    println!("    # DaVinci flow editor (web GUI)");
    println!("    # PingID admin web console");
    println!("  Customers (Fortune 500 enterprise + government):");
    println!("    - Bank of America, Citi, Wells Fargo (US banks)");
    println!("    - Allianz, AXA, Generali (insurance)");
    println!("    - Cisco, IBM, HP (tech enterprises)");
    println!("    - US Federal: HHS, DHS, GSA (FedRAMP authorized)");
    println!("    - State of Texas, State of California (state government)");
    println!("    - NHS UK, German Bundesagentur (EU government)");
    println!("    - Walmart, Target, Lowes (retail)");
    println!("    - Boeing, Lockheed (aerospace)");
    println!("    - ~50% of Fortune 100 use Ping somewhere");
    println!("  Critique: complex product matrix (PingOne vs PingFederate vs PingAccess confusion)");
    println!("           on-prem + cloud + merger = lots of overlap to rationalize");
    println!("           less Okta-shiny in dev DX (Ping is enterprise-IT-led, not dev-led)");
    println!("           PingOne UI rough compared to Okta + Microsoft");
    println!("           Thoma Bravo PE = revenue + margin focus, not aggressive innovation");
    println!("           ForgeRock merger = integration mess for next 2-3 years");
    println!("           licensing complexity remains (PE-owned)");
    println!("           customer identity less developer-friendly than Auth0 + Clerk + Stytch");
    println!("           hybrid on-prem + SaaS deployment = more ops burden");
    println!("  Differentiator: 22-year enterprise IDaaS pioneer (founded 2002 by Andre Durand in Denver CO, IPO NYSE:PING 2019, Thoma Bravo acquisition Oct 2022 $2.8B, Andre Durand returned as CEO Aug 2023 of merged Ping+ForgeRock entity) + most comprehensive identity portfolio (PingOne cloud IDaaS + PingFederate on-prem federation + PingAccess WAM + PingAuthorize dynamic authorization + PingDirectory LDAP + PingID MFA + PingIntelligence API security + DaVinci identity orchestration low-code + post-ForgeRock ForgeRock Identity Cloud + OpenAM + OpenIDM + OpenDJ integration) + the federation heritage ('Ping' = SAML federation handshake) + workforce + customer + B2B identity coverage + UnboundID directory acquisition + Singular Key DaVinci orchestration acquisition + Thoma Bravo identity vertical play (with SailPoint sister portfolio) + 1000+ app integrations + Bank of America/Citi/Wells/Allianz/AXA/Cisco/IBM/HHS/DHS-proven + FedRAMP authorized + ~50% of Fortune 100 + 6-7 figure enterprise deals + JSON Pointer policy authorization + Andre Durand founder comeback (rare in PE-owned enterprise software) — the most enterprise-grade IDaaS portfolio after the ForgeRock merger, the choice for complex Fortune 500 hybrid identity that's beyond Okta's SaaS comfort zone");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ping".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ping(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ping};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ping"), "ping");
        assert_eq!(basename(r"C:\bin\ping.exe"), "ping.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ping.exe"), "ping");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ping(&["--help".to_string()], "ping"), 0);
        assert_eq!(run_ping(&["-h".to_string()], "ping"), 0);
        let _ = run_ping(&["--version".to_string()], "ping");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ping(&[], "ping");
    }
}
