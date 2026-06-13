#![deny(clippy::all)]

//! jumpcloud-cli — SlateOS JumpCloud (Directory-as-a-Service — cloud AD alternative)
//!
//! Single personality: `jumpcloud`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_jc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jumpcloud [OPTIONS]");
        println!("JumpCloud (Slate OS) — Open Directory Platform (cloud AD replacement)");
        println!();
        println!("Options:");
        println!("  --directory            JumpCloud Directory (users + groups + device identity)");
        println!("  --sso                  SSO + SCIM provisioning");
        println!("  --mfa                  MFA (push, TOTP, FIDO2)");
        println!("  --mdm                  MDM (Mac/Win/iOS/Android/Linux)");
        println!("  --rmm                  Remote monitoring + remote assist");
        println!("  --patch-management     Patch management for OS + apps");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("JumpCloud 2024 (Slate OS)"); return 0; }
    println!("JumpCloud 2024 (Slate OS)");
    println!("  Vendor: JumpCloud, Inc. (Louisville, Colorado — founded 2012)");
    println!("  Founders: Rajat Bhargava (chairman; serial entrepreneur — StillSecure, MyMail) + Larry Middle");
    println!("  Funding: General Atlantic, Sapphire Ventures, BlackRock + others");
    println!("          $400M+ raised");
    println!("          $2.6B valuation (Sep 2021)");
    println!("  Strategy: 'replace Active Directory + Okta + Intune + JAMF with one platform'");
    println!("           targets SMBs and mid-market who can't run on-prem AD");
    println!("           strong with cross-OS environments (Mac + Linux + Windows)");
    println!("  Scale: 200,000+ companies");
    println!("        most are 10-1000 employees");
    println!("        ~1,000 employees, profitable");
    println!("  Pricing:");
    println!("    Free for 10 users + 10 devices (full platform)");
    println!("    À la carte $2/user/mo for SSO, $2/user/mo MFA, $4/user/mo device mgmt, etc.");
    println!("    'Platform' bundle ~$15-25/user/mo for everything");
    println!("    Per-package pricing rare — most upgrade to Platform");
    println!("  Core platform pillars:");
    println!("    1. Directory: users, groups, password sync (G Workspace + M365 + others)");
    println!("    2. Device Management: MDM/MAM for Mac, Win, iOS, Android, Linux, ChromeOS");
    println!("    3. Access Management: SSO, MFA, SCIM provisioning to apps");
    println!("    4. Network: cloud RADIUS, cloud LDAP, password manager");
    println!("    5. Security: passwordless, conditional access, ZTNA gateways");
    println!("    6. Patch + Software Management");
    println!("    7. Reports + Insights");
    println!("    8. AI Assistant: agentic 'JumpCloud GO' for admin tasks");
    println!("  Killer features:");
    println!("    - Cross-OS device management — even Linux endpoints get full MDM");
    println!("    - Cloud LDAP / RADIUS (no on-prem servers needed)");
    println!("    - Password Manager built into directory (vs requiring 1Password/Bitwarden separately)");
    println!("    - Cloud-native Active Directory bridge (sync AD on-prem ↔ JumpCloud cloud)");
    println!("  Customers: 200K+ — mostly SMB-mid-market with hybrid Mac/Linux/Win shops");
    println!("            popular with dev-tooling companies + design agencies");
    println!("  Critique: feature breadth → depth uneven (some modules less polished than dedicated tools)");
    println!("           sales-led pricing despite SMB positioning");
    println!("           less recognized than Okta in enterprise circles");
    println!("  Differentiator: one platform for AD + SSO + MDM + RADIUS + Patch — full SMB IT stack");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "jumpcloud".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_jc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/jumpcloud"), "jumpcloud");
        assert_eq!(basename(r"C:\bin\jumpcloud.exe"), "jumpcloud.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("jumpcloud.exe"), "jumpcloud");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jc(&["--help".to_string()], "jumpcloud"), 0);
        assert_eq!(run_jc(&["-h".to_string()], "jumpcloud"), 0);
        let _ = run_jc(&["--version".to_string()], "jumpcloud");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jc(&[], "jumpcloud");
    }
}
