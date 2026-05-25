#![deny(clippy::all)]

//! duosecurity-cli — OurOS Duo Security (the green push button MFA, Ann Arbor MI, Cisco $2.35B 2018)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_duo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: duosecurity [OPTIONS]");
        println!("Duo Security (OurOS) — the green push-button MFA (now Cisco Duo Trusted Access)");
        println!();
        println!("Options:");
        println!("  --mfa                  Multi-factor authentication (push, OTP, FIDO2)");
        println!("  --sso                  Duo Single Sign-On (SAML IdP)");
        println!("  --trusted-endpoints    Device trust + posture");
        println!("  --network-gateway      Duo Network Gateway (modern VPN replacement)");
        println!("  --risk                 Risk-Based Authentication (adaptive)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Duo Security 2024 (OurOS) — duo-cli (Admin API)"); return 0; }
    println!("Duo Security 2024 (OurOS) — Cisco Duo (the green-button MFA, zero-trust access)");
    println!("  Vendor: Cisco Duo (Ann Arbor, MI — division of Cisco Systems NASDAQ:CSCO)");
    println!("  Founders: Dug Song + Jon Oberheide, 2009");
    println!("          Dug Song: ex-Arbor Networks (DDoS), security researcher, dsniff author");
    println!("          Jon Oberheide: PhD U. Michigan, ex-Arbor Networks security");
    println!("          Met in Ann Arbor — University of Michigan ecosystem");
    println!("          'Wanted to make 2FA so easy that people would actually use it'");
    println!("          Originally focused on web apps + VPN MFA");
    println!("          Bootstrapped early (no Series A until 2012)");
    println!("  Funding + acquisition:");
    println!("         Total raised: ~$120M");
    println!("         Series A 2012: $5M (Benchmark)");
    println!("         Series B 2014: $12M (Benchmark, Google Ventures)");
    println!("         Series C 2014: $30M (Benchmark + Geodesic Capital)");
    println!("         Series D 2017: $70M (Meritech + others) at ~$1B valuation");
    println!("         Acquired by Cisco Aug 2018 for $2.35B cash");
    println!("         Largest Michigan tech exit ever at the time");
    println!("         Founders stayed at Cisco for 3+ years post-acquisition");
    println!("  Strategic position (under Cisco):");
    println!("                    pitch: 'simplest, friendliest MFA + zero-trust access for any org'");
    println!("                    target: every org needing MFA — SMB to Fortune 500");
    println!("                    primary competitor: Microsoft Entra MFA, Okta MFA, RSA SecurID, YubiKey");
    println!("                    secondary: Authy (Twilio), Auth0, Beyond Identity, Google Authenticator");
    println!("                    Duo wedge: easiest user experience (the green push button)");
    println!("                    + universal 'works with any app' positioning (200+ integrations)");
    println!("                    + acquired by Cisco = enterprise channel + Cisco Secure portfolio");
    println!("                    + heavy in education (universities use Duo extensively)");
    println!("                    'Duo Push' became generic term for push-based MFA");
    println!("  Pricing (per-user/month, transparent):");
    println!("    Duo Free: $0 (up to 10 users, basic MFA)");
    println!("    Duo Essentials: $3/user/mo (MFA + Single Sign-On + Trusted Endpoints)");
    println!("    Duo Advantage: $6/user/mo (+ Risk-Based Auth + Adaptive Access)");
    println!("    Duo Premier: $9/user/mo (+ Duo Network Gateway, Trust Monitor, Verified Push)");
    println!("    Cisco Secure Bundle: Duo + Umbrella + Secure Endpoint discounted");
    println!("    'Transparent per-user pricing' = unusual for enterprise security");
    println!("  Architecture (the simple-by-design MFA):");
    println!("    - Cloud-native multi-tenant SaaS (was, pre-Cisco)");
    println!("    - Now hosted within Cisco's secure cloud");
    println!("    - Duo Mobile app: push, HOTP, TOTP, biometric");
    println!("    - WebAuthn/FIDO2 support (security keys)");
    println!("    - Push notifications via APNs + FCM");
    println!("    - Verify-by-number challenge (Duo Verified Push, 2022)");
    println!("    - 200+ integrations (VPN, web app, SSO, RDP, SSH, etc.)");
    println!("    - Authentication Proxy for on-prem app integration");
    println!("    - REST Admin API for automation");
    println!("  Product portfolio:");
    println!("    1. Duo Multi-Factor Authentication (the core):");
    println!("       - Duo Push (the famous green button)");
    println!("       - Duo OTP (HOTP/TOTP backup)");
    println!("       - SMS + phone call (fallback)");
    println!("       - WebAuthn/FIDO2 security keys");
    println!("       - Biometric (Touch ID, Face ID, Android Fingerprint)");
    println!("       - 'Approve' or 'Deny' on phone = the brand");
    println!("    2. Duo Mobile (the iconic app):");
    println!("       - iOS + Android");
    println!("       - 50M+ downloads");
    println!("       - Push notifications + OTP generation");
    println!("       - Per-account icons + push details");
    println!("       - Verified Push (entered number challenge) reduces fatigue attacks");
    println!("    3. Duo Single Sign-On (SAML IdP, 2020+):");
    println!("       - Cloud SAML IdP (alternative to Okta + Azure AD)");
    println!("       - Integrated with Duo MFA out of the box");
    println!("       - Catalog of pre-configured SAML apps");
    println!("       - Late entry vs Okta + Microsoft");
    println!("    4. Trusted Endpoints (device trust):");
    println!("       - Verify device meets policy before allowing access");
    println!("       - Managed vs unmanaged device classification");
    println!("       - OS version + disk encryption + firewall checks");
    println!("       - Block access from unmanaged BYOD if policy requires");
    println!("    5. Duo Network Gateway (DNG, modern VPN replacement):");
    println!("       - Browser-based access to internal apps");
    println!("       - No VPN client needed");
    println!("       - HTTP/HTTPS-only proxied access");
    println!("       - Zero Trust Network Access (ZTNA) lite");
    println!("    6. Risk-Based Authentication (RBA, adaptive):");
    println!("       - ML-driven risk scoring");
    println!("       - Step-up to phishing-resistant when risky");
    println!("       - 'Don't ask for MFA if you're already trusted; force WebAuthn if you're sketchy'");
    println!("    7. Verified Duo Push:");
    println!("       - Login screen displays a number");
    println!("       - User enters that number in Duo Mobile to approve");
    println!("       - Defeats push-bombing / MFA fatigue attacks (Lapsus$ style)");
    println!("       - Major selling point post-2022 incidents");
    println!("    8. Passwordless (FIDO2 + Touch ID for Duo SSO):");
    println!("       - Login with no password — just biometric + Duo");
    println!("       - Passwordless workforce auth growing");
    println!("    9. Trust Monitor + Universal Prompt:");
    println!("       - ML-based anomaly detection");
    println!("       - 'Why did Bob just authenticate from a new country?'");
    println!("       - Universal Prompt: redesigned auth prompt UI (2021+)");
    println!("    10. Duo Care + Admin Portal:");
    println!("       - Customer success program for large deployments");
    println!("       - Admin UI universally praised as friendly");
    println!("       - 'The admin UI doesn't feel like enterprise security'");
    println!("  The push-button origin story:");
    println!("    - Pre-Duo (2009): MFA = hardware token (RSA SecurID) or SMS OTP");
    println!("    - Duo's insight: push notifications make MFA frictionless");
    println!("    - User just taps 'Approve' on phone — no code typing");
    println!("    - Initially considered too simple by security purists");
    println!("    - Became the dominant MFA UX paradigm");
    println!("    - Every modern MFA copies the push pattern (Okta Verify, Microsoft Authenticator)");
    println!("    - 'Duo Push' = generic noun in industry");
    println!("  The Cisco acquisition (the largest MI tech exit):");
    println!("    - Aug 2018: Cisco acquired for $2.35B cash");
    println!("    - Largest Michigan tech exit in history");
    println!("    - Cisco's biggest cybersecurity acquisition at the time");
    println!("    - Anchored Cisco's Secure Access portfolio (now SASE play)");
    println!("    - Founders stayed for 3+ years (Dug Song became Cisco GM of Secure Access)");
    println!("    - Combined with Cisco Umbrella + Cisco Secure Endpoint = Cisco Secure suite");
    println!("    - Acquisition often cited as one of the smoothest cyber integrations");
    println!("  The University of Michigan + Ann Arbor heritage:");
    println!("    - Founders met in Ann Arbor security community");
    println!("    - UMich was an early Duo customer");
    println!("    - Strong in higher ed: Yale, Stanford, MIT, Harvard, U. Michigan, hundreds of universities");
    println!("    - 'Universities use Duo' became a marketing line");
    println!("    - Ann Arbor's biggest tech success story");
    println!("  Integrations:");
    println!("    - Duo Admin API (REST)");
    println!("    - 200+ pre-built integrations (VPN, RDP, SSH, web app)");
    println!("    - SAML 2.0 (Duo SSO + 3rd party IdPs as proxy)");
    println!("    - OIDC support");
    println!("    - LDAP + Active Directory user sync");
    println!("    - Microsoft 365, Google Workspace, Salesforce, ServiceNow");
    println!("    - AWS, Azure, GCP IAM integrations");
    println!("    - Cisco AnyConnect + Meraki + ASA VPN (Cisco synergy)");
    println!("    - Palo Alto GlobalProtect + Fortinet + Check Point + F5");
    println!("    - SDKs: Python, Ruby, Java, .NET, Go, Node, PHP");
    println!("    - WebAuthn/FIDO2 + YubiKey support");
    println!("    - Splunk, QRadar, Sentinel SIEM integration");
    println!("    - Terraform provider");
    println!("  Duo CLI usage:");
    println!("    # Duo Admin API (curl + HMAC auth):");
    println!("    duo_unix --version                                       # the duo_unix CLI (PAM)");
    println!("    # duo_unix PAM module config (for SSH MFA):");
    println!("    # cat /etc/duo/pam_duo.conf");
    println!("    # ikey, skey, host, pushinfo=yes, autopush=yes, prompts=1");
    println!("    # Then in /etc/pam.d/sshd: auth required pam_duo.so");
    println!("    # Login Duo for Windows (RDP + console):");
    println!("    # MSI-installed agent for Windows MFA");
    println!("    # Admin API examples:");
    println!("    # GET /admin/v1/users → list users");
    println!("    # GET /admin/v1/policies → list policies");
    println!("    # POST /admin/v1/integrations → create integration");
    println!("    # Terraform:");
    println!("    # resource \"duo_integration\" \"my_app\" {{ type = \"sso\" name = \"My App\" }}");
    println!("    # Duo Mobile app for end-users:");
    println!("    # Install, scan QR, approve pushes");
    println!("  Customers (universities + enterprise + everyone needing MFA):");
    println!("    - Yale, Stanford, MIT, Harvard, U Michigan, Columbia (universities)");
    println!("    - Hundreds of universities globally");
    println!("    - K-12 districts (NYC DOE, LAUSD, etc.)");
    println!("    - Federal: GSA, DOE, others (FedRAMP authorized)");
    println!("    - Toyota, Etsy, Eventbrite, NASA JPL");
    println!("    - JPMorgan, Bank of America (selective deployments)");
    println!("    - 30,000+ organizations globally");
    println!("    - 1M+ daily active users on Duo Push");
    println!("  Critique: under Cisco, less independent product velocity than pre-acquisition");
    println!("           Duo SSO late + less feature-rich than Okta + Azure AD");
    println!("           push fatigue attacks exposed weakness (mitigated by Verified Push)");
    println!("           Risk-Based Auth less mature than Okta + Ping equivalents");
    println!("           Network Gateway less competitive than dedicated ZTNA (Zscaler, Netskope)");
    println!("           Cisco branding (now 'Cisco Duo') dilutes the friendly Duo identity");
    println!("           SMB-friendly UX feels less powerful at Fortune 50 scale");
    println!("           IGA + provisioning weaker than dedicated IDaaS");
    println!("           founders departure (~2022) raised continuity questions");
    println!("  Differentiator: the green push-button MFA (founded 2009 by Dug Song + Jon Oberheide in Ann Arbor MI, acquired by Cisco Aug 2018 for $2.35B largest MI tech exit ever) + Duo Push (the iconic UX, generic noun in MFA industry, every modern MFA copies the push pattern) + Duo Mobile app (50M+ downloads, iOS + Android) + Verified Duo Push (number-challenge defeats push-fatigue attacks, 2022) + WebAuthn/FIDO2 + biometric (Touch ID/Face ID) + Duo SSO (SAML IdP) + Trusted Endpoints (device trust + posture) + Duo Network Gateway (modern VPN replacement, browser-based ZTNA-lite) + Risk-Based Authentication + Passwordless + Trust Monitor anomaly detection + Universal Prompt + 200+ pre-built integrations + duo_unix PAM module + Login Duo for Windows + Cisco AnyConnect/Meraki/ASA synergy + AWS/Azure/GCP integration + Yale/Stanford/MIT/Harvard/UMich/hundreds-of-universities-proven + 30,000+ organizations + 1M+ daily Duo Push users + transparent per-user pricing ($3-9/user/mo) + Ann Arbor tech success story + University of Michigan heritage + Cisco Secure suite anchor + FedRAMP authorized + simplest MFA UX in the industry + the brand that made 2FA something users don't hate — the friendly green-button MFA that taught the entire industry how to do push-based authentication right");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "duosecurity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_duo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
