#![deny(clippy::all)]

//! okta-cli — SlateOS Okta (the SSO/IAM market leader, $80B at peak)
//!
//! Single personality: `okta`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_okta(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: okta [OPTIONS]");
        println!("Okta Workforce Identity Cloud (SlateOS) — Enterprise SSO/MFA/IAM");
        println!();
        println!("Options:");
        println!("  --workforce            Workforce Identity Cloud (employees)");
        println!("  --customer             Customer Identity Cloud (CIAM, ex-Auth0)");
        println!("  --sso                  Single Sign-On (SAML + OIDC)");
        println!("  --mfa                  Adaptive MFA / FastPass passwordless");
        println!("  --lifecycle            Lifecycle Management (SCIM provisioning)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Okta Workforce Identity Cloud 2024 (SlateOS)"); return 0; }
    println!("Okta Workforce Identity Cloud 2024 (SlateOS)");
    println!("  Vendor: Okta, Inc. (San Francisco — NASDAQ:OKTA)");
    println!("  Founders: Todd McKinnon (ex-Salesforce SVP Engineering) + Frederic Kerrest");
    println!("  Founded: 2009; IPO 2017");
    println!("  Funding (pre-IPO): Andreessen Horowitz, Sequoia, Greylock + others");
    println!("  Peak market cap: ~$45B (Feb 2021), ~$80B momentary peak");
    println!("                  declined sharply post-2022 SaaS correction + breach reputation hits");
    println!("  Scale: 18,000+ customer companies");
    println!("        ~6,800 employees");
    println!("        FY2024 revenue $2.3B");
    println!("  Major acquisition: Auth0 May 2021 for $6.5B");
    println!("                     repositioned as 'Customer Identity Cloud'");
    println!("  Security incidents:");
    println!("    - LAPSUS$ breach Jan 2022 (intrusion via Sitel subcontractor)");
    println!("    - Customer support breach Oct 2023 (HAR file access tokens stolen)");
    println!("    - Several smaller incidents — Okta's reputation as 'identity for identity providers' damaged");
    println!("    - Cloudflare, BeyondTrust, 1Password all victims via Okta")    ;
    println!("  Pricing: SSO from $2/user/mo, Adaptive MFA $6/user/mo, Lifecycle Management $4/user/mo");
    println!("          enterprise typically $15-25/user/mo for full bundle");
    println!("  Workforce products:");
    println!("    - SSO (SAML, OIDC, WS-Fed) with 7,000+ app catalog (Okta Integration Network)");
    println!("    - Universal Directory (consolidated identity store, AD + LDAP + cloud)");
    println!("    - Adaptive MFA (push, biometric, FIDO2, TOTP, SMS, voice, OTP)");
    println!("    - FastPass (passwordless via Okta Verify app)");
    println!("    - Lifecycle Management (auto-provision + deprovision via SCIM)");
    println!("    - Access Gateway (on-prem app SSO without re-architecting)");
    println!("    - Advanced Server Access (SSH/RDP ephemeral certificates)");
    println!("    - Privileged Access (PAM features)");
    println!("    - Identity Governance (access reviews, SoD policies)");
    println!("    - Identity Threat Protection (ITP) — behavior-based session risk scoring");
    println!("  Customer Identity Cloud (Auth0):");
    println!("    - B2C / B2B login flows (developer-friendly SDKs)");
    println!("    - Social login (Google, Facebook, Apple, etc.)");
    println!("    - Universal Login + branded sign-in pages");
    println!("    - Actions (extension hooks at login/registration)");
    println!("  Culture: 'Oktane' annual conference, Identity Day, 'identity-first security' marketing");
    println!("  Critique: pricing pressure from cheaper competitors (Microsoft Entra ID bundled in M365)");
    println!("           breach fatigue — every 6-12 months a new incident");
    println!("           Auth0 integration into Okta UX still imperfect");
    println!("  Differentiator: largest neutral identity provider — works with everyone (Microsoft/Google/Apple)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "okta".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_okta(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_okta};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/okta"), "okta");
        assert_eq!(basename(r"C:\bin\okta.exe"), "okta.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("okta.exe"), "okta");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_okta(&["--help".to_string()], "okta"), 0);
        assert_eq!(run_okta(&["-h".to_string()], "okta"), 0);
        let _ = run_okta(&["--version".to_string()], "okta");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_okta(&[], "okta");
    }
}
