#![deny(clippy::all)]

//! onelogin-cli — SlateOS OneLogin (Workforce IAM, now part of One Identity / Quest Software)
//!
//! Single personality: `onelogin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ol(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onelogin [OPTIONS]");
        println!("OneLogin by One Identity (SlateOS) — Workforce IAM");
        println!();
        println!("Options:");
        println!("  --sso                  SSO (SAML + OIDC + WS-Fed)");
        println!("  --mfa                  Multi-factor authentication");
        println!("  --vigilance-ai         OneLogin Vigilance AI (anomaly detection)");
        println!("  --smart-factor         SmartFactor Authentication (risk-based)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OneLogin 2024 (SlateOS)"); return 0; }
    println!("OneLogin 2024 (SlateOS)");
    println!("  Vendor: OneLogin → acquired by One Identity (Quest Software, Clearlake Capital) Oct 2021");
    println!("          rebranded part of One Identity portfolio");
    println!("  Founders: Thomas Pedersen (Danish — also founded Zendesk, Snaplogic alum)");
    println!("            Christian Pedersen (brother)");
    println!("  Founded: 2009, San Francisco");
    println!("  History: one of the earliest standalone SaaS IAM vendors (alongside Okta)");
    println!("          competed with Okta + Centrify (now Delinea) + Ping Identity throughout the 2010s");
    println!("          ~$170M raised, never IPO'd");
    println!("          acquired by One Identity (private equity-backed Quest Software arm) 2021");
    println!("  Security incidents:");
    println!("    - May 2017 breach: customer data accessed via compromised AWS keys (cleartext password storage controversy)");
    println!("    - 2019 lesser incident — both hurt brand vs Okta during critical growth window");
    println!("  Pricing: Starter $4/user/mo, Enterprise $8/user/mo");
    println!("          add-ons for MFA, VPN, advanced features");
    println!("  Features:");
    println!("    - SSO with 6,000+ pre-integrated apps");
    println!("    - SmartFactor Authentication (adaptive MFA — looks at device, geo, behavior)");
    println!("    - Vigilance AI: anomaly detection on auth events");
    println!("    - OneLogin Desktop Pro (Mac/Win SSO at OS login)");
    println!("    - Provisioning (SCIM, just-in-time, custom)");
    println!("    - Trusted IdP federation (chain another IdP behind OneLogin)");
    println!("    - VLD (Virtual LDAP Directory)");
    println!("    - Cloud Directory (sync with AD/LDAP)");
    println!("    - Workflows (no-code automation)");
    println!("  Vs Okta: lower pricing, similar feature surface, smaller app catalog");
    println!("          chose enterprise mid-market focus vs Okta's wider net");
    println!("  Customers: TUI Group, Pandora Jewelry, Steelcase, ~5,500 mid-market enterprises");
    println!("  Critique: post-acquisition product velocity questionable");
    println!("           Microsoft Entra ID + Okta squeezed the middle of the market");
    println!("           brand less recognized than during 2010s peak");
    println!("  Differentiator: cheaper than Okta with SmartFactor adaptive MFA built in");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "onelogin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ol(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ol};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/onelogin"), "onelogin");
        assert_eq!(basename(r"C:\bin\onelogin.exe"), "onelogin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("onelogin.exe"), "onelogin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ol(&["--help".to_string()], "onelogin"), 0);
        assert_eq!(run_ol(&["-h".to_string()], "onelogin"), 0);
        let _ = run_ol(&["--version".to_string()], "onelogin");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ol(&[], "onelogin");
    }
}
