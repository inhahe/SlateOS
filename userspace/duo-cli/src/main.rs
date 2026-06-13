#![deny(clippy::all)]

//! duo-cli — SlateOS Duo Security (the green-screen push-notification MFA, now Cisco-owned)
//!
//! Single personality: `duo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_duo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: duo [OPTIONS]");
        println!("Cisco Duo (Slate OS) — Multi-factor authentication & zero-trust access");
        println!();
        println!("Options:");
        println!("  --mfa                  Multi-factor authentication");
        println!("  --device-trust          Device Trust (posture check)");
        println!("  --duo-network-gateway   Reverse-proxy zero-trust gateway");
        println!("  --essentials            Duo Essentials ($3/user/mo)");
        println!("  --advantage             Duo Advantage ($6/user/mo)");
        println!("  --premier               Duo Premier ($9/user/mo)");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cisco Duo 2024 (Slate OS)"); return 0; }
    println!("Cisco Duo 2024 (Slate OS)");
    println!("  Vendor: Duo Security, Inc. → acquired by Cisco Oct 2018 for $2.35B");
    println!("          now Cisco's Duo product line under Cisco Secure Access portfolio");
    println!("  Founders: Dug Song + Jon Oberheide (Ann Arbor, MI — founded 2009)");
    println!("           Dug Song: prolific security researcher (DSniff, fragroute, libdnet, Arbor Networks)");
    println!("           Jon Oberheide: known for Android security research");
    println!("  History: bootstrapped + Silicon Valley funded — Benchmark, Geodesic, Index + others");
    println!("          IPO never happened — Cisco swooped before");
    println!("          Cisco acquisition was largest Michigan tech exit at the time");
    println!("  Strategy: 'democratize security' — push-button MFA accessible to everyone");
    println!("           strong with Higher Ed (the green push notification on student/staff phones)");
    println!("           expanded into Trusted Access (zero-trust + device posture)");
    println!("  Scale: 30,000+ customer organizations");
    println!("        ~2M+ end users on Duo Push daily");
    println!("        7 of 10 largest US universities use Duo");
    println!("  Pricing: Free up to 10 users (5 apps), Essentials $3/user/mo, Advantage $6, Premier $9");
    println!("  Killer features:");
    println!("    - Duo Push: one-tap MFA approval (mobile push) — UX gold standard, the original 'tap to approve'");
    println!("    - Universal Prompt: modernized UI replacing Traditional Prompt (2022+)");
    println!("    - Device Health: check OS version, screen lock, FDE, MDM enrollment before allowing access");
    println!("    - Risk-Based Authentication: skip MFA on known good context, step-up on anomalies");
    println!("    - Phishing-resistant FIDO2/WebAuthn (security keys + passkeys)");
    println!("    - Verified Push: requires entering 3-digit code from login screen (defeats push fatigue/MFA bombing)");
    println!("    - Duo Network Gateway (DNG): zero-trust reverse proxy for on-prem apps");
    println!("    - Duo Single Sign-On (SAML IdP, basic vs Okta/Entra)");
    println!("    - Duo Passport: passwordless SSO");
    println!("    - SIEM log forwarding, admin API, ~150 integrations");
    println!("  Trust + reputation: Duo has avoided major breaches; identity provider that hasn't been LAPSUS$'d");
    println!("  Customers: Yale, Duke, Stanford, UC Berkeley (higher ed lead), Etsy, Twilio, Lyft, US Federal");
    println!("  Cultural angle: Ann Arbor pride — Dug Song is U of M alum, Duo HQ in downtown AA");
    println!("  Critique: SSO module weaker than dedicated Okta/Entra ID");
    println!("           became 'just another part of Cisco' post-acquisition — less standalone identity");
    println!("  Differentiator: simplest 'add MFA in an afternoon' product — push UX defined the category");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "duo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_duo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_duo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/duo"), "duo");
        assert_eq!(basename(r"C:\bin\duo.exe"), "duo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("duo.exe"), "duo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_duo(&["--help".to_string()], "duo"), 0);
        assert_eq!(run_duo(&["-h".to_string()], "duo"), 0);
        let _ = run_duo(&["--version".to_string()], "duo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_duo(&[], "duo");
    }
}
