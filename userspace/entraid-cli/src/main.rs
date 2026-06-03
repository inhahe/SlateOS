#![deny(clippy::all)]

//! entraid-cli — OurOS Microsoft Entra ID (formerly Azure AD — the M365 identity backbone)
//!
//! Single personality: `entraid`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: entraid [OPTIONS]");
        println!("Microsoft Entra ID (OurOS) — Cloud identity & access management");
        println!();
        println!("Options:");
        println!("  --p1                    Entra ID P1 (Conditional Access, included with M365 E3)");
        println!("  --p2                    Entra ID P2 (Identity Protection + PIM, M365 E5)");
        println!("  --b2c                   Entra External ID (CIAM, formerly Azure AD B2C)");
        println!("  --conditional-access    Conditional Access policies");
        println!("  --pim                    Privileged Identity Management");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft Entra ID 2024 (OurOS)"); return 0; }
    println!("Microsoft Entra ID 2024 (OurOS)");
    println!("  Vendor: Microsoft Corporation (Redmond, WA — NASDAQ:MSFT)");
    println!("  History: started as 'Windows Azure Active Directory' 2010 (auth for Azure cloud)");
    println!("          renamed 'Azure Active Directory' 2014");
    println!("          renamed 'Microsoft Entra ID' July 2023 (part of broader 'Entra' identity family)");
    println!("  Branding context: Entra is Microsoft's identity portfolio name now covering:");
    println!("    - Entra ID (was Azure AD)");
    println!("    - Entra External ID (CIAM, was Azure AD B2C)");
    println!("    - Entra ID Governance");
    println!("    - Entra Permissions Management (cloud infrastructure entitlement mgmt, CIEM)");
    println!("    - Entra Verified ID (decentralized identity / verifiable credentials)");
    println!("    - Entra Internet Access + Entra Private Access (Secure Service Edge — Microsoft's SSE)");
    println!("    - Microsoft Defender for Identity (was Azure ATP)");
    println!("  Pricing: Free tier included with M365 / Azure (basic SSO, 50K MAU)");
    println!("          Entra ID P1: $6/user/mo (Conditional Access, dynamic groups, password writeback) — in M365 E3");
    println!("          Entra ID P2: $9/user/mo (Identity Protection risk policies, PIM JIT admin) — in M365 E5");
    println!("          Microsoft Entra ID Governance: $7/user/mo (access reviews, entitlement mgmt)");
    println!("  Scale: 720,000+ tenants, 1.4B+ identities");
    println!("        the largest IdP on the planet (bundled with M365)");
    println!("  Core features:");
    println!("    - SSO to 10,000+ apps in App Gallery (SAML, OIDC, password-vaulted)");
    println!("    - Conditional Access (rich policy engine — location, device, app, risk, user, group)");
    println!("    - MFA: phone, SMS, email, OATH, push (Microsoft Authenticator), FIDO2");
    println!("    - Passwordless: Windows Hello for Business, FIDO2 security keys, phone sign-in");
    println!("    - Self-Service Password Reset (SSPR) + writeback to AD on-prem");
    println!("    - Hybrid: Entra Connect Sync (formerly Azure AD Connect) sync with on-prem AD");
    println!("    - Privileged Identity Management (PIM) — JIT eligible role activation");
    println!("    - Identity Protection (risk detection: leaked creds, anonymous IP, atypical travel)");
    println!("    - Entitlement Management (access packages, automatic provisioning)");
    println!("    - Cross-tenant access settings (B2B collaboration controls)");
    println!("    - Verified ID (Microsoft's verifiable credentials product)");
    println!("    - Microsoft Authenticator app (push + TOTP + companion app for password sync)");
    println!("  Strategy: bundled with M365 → 'free' for most enterprises → massive default-IdP advantage");
    println!("           Microsoft pricing pressure has hurt Okta + standalone IAM vendors significantly");
    println!("  Critique:");
    println!("    - Conditional Access policy interactions can become a maze (CA debugger only partial relief)");
    println!("    - Token theft (cookie pass-the-cookie) attacks high-profile — Storm-0558 May 2023 breached MS-issued GovCloud tokens");
    println!("    - Microsoft itself was breached repeatedly 2023-2024 (Storm-0558, Midnight Blizzard)");
    println!("    - Sometimes overlapping product lines (Entra Permissions vs Defender for Cloud Apps)");
    println!("  Differentiator: bundled-with-M365 economic moat + deepest Windows/Office/Azure integration");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "entraid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eid(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/entraid"), "entraid");
        assert_eq!(basename(r"C:\bin\entraid.exe"), "entraid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("entraid.exe"), "entraid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_eid(&["--help".to_string()], "entraid"), 0);
        assert_eq!(run_eid(&["-h".to_string()], "entraid"), 0);
        assert_eq!(run_eid(&["--version".to_string()], "entraid"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_eid(&[], "entraid"), 0);
    }
}
