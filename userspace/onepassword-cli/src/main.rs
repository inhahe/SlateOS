#![deny(clippy::all)]

//! onepassword-cli — Slate OS 1Password (AgileBits) password manager
//!
//! Personalities: `onepassword`, `1password`, `op`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_op(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: 1password [OPTIONS]");
        println!("1Password 8 (Slate OS) — AgileBits password manager");
        println!();
        println!("Options:");
        println!("  --vault NAME           Open vault");
        println!("  --unlock               Unlock with master password / biometrics");
        println!("  --generate             Generate password / passphrase / one-time code");
        println!("  --watchtower           Watchtower (breach + weak password monitoring)");
        println!("  --ssh                  1Password SSH agent (keys stored encrypted)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("1Password 8.10.50 (Slate OS)"); return 0; }
    println!("1Password 8.10.50 (Slate OS)");
    println!("  Vendor: AgileBits Inc. (Toronto, Canada, founded 2005)");
    println!("  Founders: Roustem Karimov, Dave Teare");
    println!("  Crypto: AES-256-GCM, PBKDF2-HMAC-SHA256 (100K iter), Secret Key + master pw");
    println!("  Secret Key: 34-character second-factor stored on device — ensures cloud blob");
    println!("              is not crackable even if AgileBits is breached");
    println!("  1Password 8: Electron rewrite (2022), replaced native Mac app — controversial");
    println!("  Hosting: 1Password.com cloud (no more local-only vaults since v8)");
    println!("  Features: web/Windows/Mac/Linux/iOS/Android, browser extension (1Password X),");
    println!("            SSH agent integration (sign Git commits, replace ssh-add)");
    println!("  Plans: Individual $2.99/mo, Families $4.99/mo (5 users), Teams $7.99/user/mo");
    println!("  Business: Business $7.99/user, Enterprise (custom) — SSO, SCIM, audit");
    println!("  Developer Tools: shell plugin, CLI (op), Kubernetes Operator, Terraform provider");
    println!("  Strengths: best-in-class UX, Secret Key model, audit reports clean");
    println!("  Competitors: Bitwarden (FOSS), Dashlane, Keeper, LastPass (post-breach decline)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "1password".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_op(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_op};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/onepassword"), "onepassword");
        assert_eq!(basename(r"C:\bin\onepassword.exe"), "onepassword.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("onepassword.exe"), "onepassword");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_op(&["--help".to_string()], "onepassword"), 0);
        assert_eq!(run_op(&["-h".to_string()], "onepassword"), 0);
        let _ = run_op(&["--version".to_string()], "onepassword");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_op(&[], "onepassword");
    }
}
