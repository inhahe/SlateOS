#![deny(clippy::all)]

//! beyondidentity-cli — SlateOS Beyond Identity (passwordless phishing-resistant MFA, Netscape co-founders)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: beyondidentity [OPTIONS]");
        println!("Beyond Identity (SlateOS) — passwordless phishing-resistant MFA on device-bound credentials");
        println!();
        println!("Options:");
        println!("  --workforce            Beyond Identity Workforce (passwordless SSO + MFA)");
        println!("  --customers            Beyond Identity Customers (CIAM passwordless)");
        println!("  --secure-workforce     Phishing-resistant workforce MFA");
        println!("  --secure-customers     CIAM with passwordless + DeviceTrust");
        println!("  --secure-devops        DevOps passwordless SSH + Git signing");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Beyond Identity 2024 (SlateOS) — beyondidentity-cli (REST + bi-cli)"); return 0; }
    println!("Beyond Identity 2024 (SlateOS) — Passwordless Phishing-Resistant Auth (Netscape Pedigree)");
    println!("  Vendor: Beyond Identity, Inc. (New York, NY — private)");
    println!("  Founders: Jim Clark + TJ Jermoluk, 2020");
    println!("          Jim Clark: Netscape co-founder (Netscape Navigator, 1994 IPO), Silicon Graphics co-founder, Healtheon");
    println!("          TJ Jermoluk: ex-CEO @Home Network (cable broadband internet pioneer), ex-Healtheon");
    println!("          Started Beyond Identity to 'fix authentication's fundamental problems'");
    println!("          'Same team that brought the web to the masses, now fixing how the web logs you in'");
    println!("          Industry credibility: Jim Clark = legendary serial entrepreneur, $billions exits");
    println!("          NYC HQ, distributed engineering team");
    println!("  Funding:");
    println!("         Total raised: ~$205M+");
    println!("         Series A Jul 2020: $30M (Koch Disruptive Technologies + others)");
    println!("         Series B Sep 2020: $75M (Koch Industries + New Enterprise Associates)");
    println!("         Series C Feb 2022: $100M (Evolution Equity Partners + existing)");
    println!("         Valuation undisclosed but ~$1B+ range");
    println!("         Koch Disruptive (Koch Industries' VC) early investor — unusual");
    println!("  Strategic position: 'eliminate passwords entirely via device-bound cryptographic keys':");
    println!("                    pitch: 'no passwords, no OTPs, no shared secrets — only device-bound keys + biometric'");
    println!("                    target: workforce + CIAM at security-forward enterprises");
    println!("                    primary competitor: Okta + Duo, Microsoft Authenticator, YubiKey, Hypr");
    println!("                    secondary: Auth0, Stytch, Clerk for CIAM; SailPoint + Ping for workforce");
    println!("                    BI wedge: device-bound cryptographic credentials = phishing-resistant by design");
    println!("                    + no passwords on the wire ever");
    println!("                    + no OTPs (which are phishable)");
    println!("                    + biometric unlocks the local credential (TEE/Secure Enclave/TPM)");
    println!("                    + 'Phishing-resistant' is the FBI/CISA-recommended MFA category");
    println!("                    Jim Clark's pedigree (Netscape) drives credibility");
    println!("                    'No more passwords. No more phishing.'");
    println!("  Pricing (subscription-based, per-user):");
    println!("    Beyond Identity Free: $0 (limited users, dev testing)");
    println!("    Secure Workforce Essentials: ~$3/user/mo (passwordless MFA + SSO)");
    println!("    Secure Workforce Advanced: ~$6/user/mo (+ DeviceTrust, risk policies)");
    println!("    Secure Workforce Enterprise: ~$10+/user/mo (advanced + auditing)");
    println!("    Secure Customers (CIAM): per-MAU pricing");
    println!("    typical enterprise deals: 6-figure annual contracts");
    println!("    competing with Okta + Duo's $3-9/user pricing");
    println!("  Architecture (the crypto + device-bound model):");
    println!("    - Device-bound private key generated in Secure Enclave / TPM / TEE");
    println!("    - Public key registered with Beyond Identity service");
    println!("    - Authentication = sign challenge with device-bound private key");
    println!("    - Biometric (Touch ID / Face ID / Windows Hello) unlocks the local key");
    println!("    - No password ever transmitted");
    println!("    - No shared secret to phish");
    println!("    - Asymmetric cryptography: even if BI servers breached, can't impersonate users");
    println!("    - DeviceTrust: collect device posture + factor into auth decisions");
    println!("    - SAML/OIDC IdP for federation with existing SSO");
    println!("  Product portfolio:");
    println!("    1. Secure Workforce (the flagship):");
    println!("       - Passwordless MFA for employees");
    println!("       - SAML/OIDC SSO IdP");
    println!("       - DeviceTrust + device posture");
    println!("       - Adaptive policies");
    println!("       - Beyond Identity Authenticator app + browser extension");
    println!("    2. Secure Customers (CIAM):");
    println!("       - Passwordless registration + login for consumer apps");
    println!("       - SDK for embedding in mobile + web apps");
    println!("       - No password reset hell");
    println!("       - Used by: fintech apps, healthcare portals, dating apps");
    println!("    3. Secure DevOps:");
    println!("       - Passwordless SSH (replaces SSH keys + passwords)");
    println!("       - Git commit signing with device-bound keys");
    println!("       - 'No more SSH key file on disk'");
    println!("       - Replaces: traditional SSH + GPG signing keys");
    println!("    4. DeviceTrust (device posture + trust):");
    println!("       - Verify device meets policy");
    println!("       - OS version, encryption, AV/EDR, firewall checks");
    println!("       - Block auth from non-compliant devices");
    println!("       - Continuous evaluation (re-check during session)");
    println!("    5. Phishing-Resistant Authentication:");
    println!("       - FBI + CISA recommend phishing-resistant MFA (2022 guidance)");
    println!("       - BI's approach satisfies the recommendation");
    println!("       - Hardware security keys + WebAuthn also qualify");
    println!("       - BI's 'invisible' phishing-resistance — no extra hardware");
    println!("    6. Risk-Based Authentication:");
    println!("       - Risk score from: device, location, time, behavior");
    println!("       - Allow / step-up / deny based on risk");
    println!("       - 'Continuous authentication' — re-evaluate during session");
    println!("    7. Universal Passkeys:");
    println!("       - BI was early on FIDO2 + passkeys");
    println!("       - Cross-device passkey sync support");
    println!("       - Compatible with Apple + Google passkey ecosystems");
    println!("    8. Beyond Identity Authenticator:");
    println!("       - Mobile app + desktop app");
    println!("       - Local key storage in Secure Enclave / TPM");
    println!("       - Biometric unlock");
    println!("       - Push approval for SSO flows");
    println!("    9. Browser Extension:");
    println!("       - For seamless SSO into web apps");
    println!("       - Auto-detects login pages, presents passwordless flow");
    println!("       - Chrome + Edge + Firefox + Safari");
    println!("    10. Admin Console + APIs:");
    println!("       - Cloud-based admin UI");
    println!("       - REST APIs for automation");
    println!("       - SCIM 2.0 provisioning support");
    println!("       - Terraform provider");
    println!("  The phishing-resistance argument:");
    println!("    - Most MFA (push, OTP, SMS, even some hardware tokens) is phishable");
    println!("    - 2022 Lapsus$ + Uber breach: MFA fatigue / push bombing");
    println!("    - 2022 Okta breach (via support contractor): MFA didn't help");
    println!("    - FBI + CISA + NSA guidance: 'use phishing-resistant MFA'");
    println!("    - Only WebAuthn/FIDO2 + device-bound credentials qualify");
    println!("    - BI implements this without separate hardware key");
    println!("    - 'Phishing-resistant by design, not by user discipline'");
    println!("  The Jim Clark pedigree:");
    println!("    - Silicon Graphics co-founder (3D graphics workstations, 1980s-90s)");
    println!("    - Netscape Communications co-founder (Netscape Navigator browser, 1994 IPO defined dotcom)");
    println!("    - Healtheon co-founder (online health insurance, IPO 1999)");
    println!("    - myCFO / Shutterfly + others");
    println!("    - $billions in personal exits");
    println!("    - 'Founded 4 billion-dollar companies' — rare credibility");
    println!("    - Active angel + serial entrepreneur into his 70s");
    println!("  The TJ Jermoluk story:");
    println!("    - CEO of @Home Network 1995-1999 (cable broadband internet pioneer)");
    println!("    - @Home delivered fast internet over cable for the first time");
    println!("    - Healtheon CEO with Jim Clark");
    println!("    - Long-time Jim Clark partner");
    println!("    - 'Clark + Jermoluk' = legendary 90s entrepreneur duo");
    println!("  Integrations:");
    println!("    - REST Admin API");
    println!("    - SAML 2.0 + OIDC IdP for federation");
    println!("    - SCIM 2.0 for user provisioning");
    println!("    - Workday + AD as identity sources");
    println!("    - Okta + Microsoft Entra co-existence (BI as auth method)");
    println!("    - WebAuthn/FIDO2 compatibility");
    println!("    - Browser extensions (Chrome, Edge, Firefox, Safari)");
    println!("    - Mobile SDKs (iOS, Android)");
    println!("    - Web SDK (JS)");
    println!("    - SSH integration for passwordless SSH");
    println!("    - GitHub + GitLab for commit signing");
    println!("    - SIEM integration (Splunk, Sentinel)");
    println!("    - MDM integrations for DeviceTrust (Jamf, Intune, Workspace ONE)");
    println!("    - Terraform provider");
    println!("  Beyond Identity CLI usage:");
    println!("    # bi-cli (workforce admin):");
    println!("    bi-cli login --realm <tenant>");
    println!("    bi-cli users list");
    println!("    bi-cli policies list");
    println!("    bi-cli applications list");
    println!("    bi-cli devices list --user <email>");
    println!("    # REST API:");
    println!("    curl -H 'Authorization: Bearer <token>' \\");
    println!("         https://api.beyondidentity.com/v1/tenants/<tenant>/identities");
    println!("    # Secure DevOps (passwordless SSH):");
    println!("    bi-ssh login                                             # initialize device-bound SSH key");
    println!("    ssh user@host                                            # auto-authenticated via BI");
    println!("    # Git commit signing:");
    println!("    git config --global gpg.format ssh");
    println!("    git config --global user.signingKey <BI-key>");
    println!("    git commit -S -m 'signed commit'                        # signed with BI device key");
    println!("    # Customers SDK (mobile/web embed):");
    println!("    # JS: window.beyondIdentity.bind() / window.beyondIdentity.authenticate()");
    println!("  Customers (security-forward enterprises):");
    println!("    - Snowflake (post-2023 breach, accelerated phishing-resistant MFA adoption)");
    println!("    - Cornell University, NYU (higher ed)");
    println!("    - Various fintechs + healthcare");
    println!("    - US Federal (gov agencies — FedRAMP authorized)");
    println!("    - Manufacturing + critical infra (Koch portfolio companies)");
    println!("    - Customer count private (still earlier stage)");
    println!("    - Growing list of security-conscious tech companies");
    println!("  Critique: less mature ecosystem vs Okta + Duo (smaller integration catalog)");
    println!("           DeviceTrust requires endpoint agent install (friction)");
    println!("           CIAM market vs Auth0 + Stytch + Clerk (BI catching up)");
    println!("           passwordless UX still requires user adjustment (no more typing passwords)");
    println!("           browser extension requirement annoying for some flows");
    println!("           Apple + Google native passkeys = competing approach");
    println!("           pricing comparable to Okta but Okta has bigger feature surface");
    println!("           founders mostly act as figureheads/board (TJ + Jim not day-to-day CEOs)");
    println!("           Koch Industries early investment = political optics among progressive enterprises");
    println!("  Differentiator: passwordless phishing-resistant MFA built on device-bound cryptographic credentials (founded 2020 by Jim Clark Netscape co-founder + TJ Jermoluk @Home Network CEO, NYC, $205M+ raised including Koch Disruptive + NEA + Evolution Equity) + device-bound private key in Secure Enclave/TPM/TEE (never leaves device) + asymmetric crypto (no shared secret to phish) + biometric unlock local key (Touch ID/Face ID/Windows Hello) + NO PASSWORD ever transmitted + Secure Workforce (passwordless SSO + MFA) + Secure Customers (CIAM passwordless) + Secure DevOps (passwordless SSH + Git commit signing) + DeviceTrust (device posture + continuous evaluation) + Risk-Based Authentication + Universal Passkeys (FIDO2 + cross-device sync) + BI Authenticator (mobile + desktop) + browser extensions + Snowflake/Cornell/NYU/Koch portfolio-proven + FBI + CISA + NSA recommended phishing-resistant MFA category + WebAuthn/FIDO2 compatible + Terraform provider + SCIM 2.0 + Jim Clark's $billions-in-exits pedigree (Netscape Navigator + Silicon Graphics + Healtheon + Shutterfly) + TJ Jermoluk's @Home Network heritage + 'phishing-resistant by design, not by user discipline' + post-Lapsus$/Uber/Okta-breach era validation — the passwordless authentication platform from the team that gave the web Netscape Navigator, now eliminating the passwords that they helped popularize via the same browser");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "beyondidentity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/beyondidentity"), "beyondidentity");
        assert_eq!(basename(r"C:\bin\beyondidentity.exe"), "beyondidentity.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("beyondidentity.exe"), "beyondidentity");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bi(&["--help".to_string()], "beyondidentity"), 0);
        assert_eq!(run_bi(&["-h".to_string()], "beyondidentity"), 0);
        let _ = run_bi(&["--version".to_string()], "beyondidentity");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bi(&[], "beyondidentity");
    }
}
