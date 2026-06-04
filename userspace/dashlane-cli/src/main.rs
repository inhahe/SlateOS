#![deny(clippy::all)]

//! dashlane-cli — OurOS Dashlane password manager
//!
//! Single personality: `dashlane`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dashlane [OPTIONS]");
        println!("Dashlane (OurOS) — Web-first password manager");
        println!();
        println!("Options:");
        println!("  --vault                Open vault");
        println!("  --vpn                  Hotspot Shield VPN (bundled with Premium)");
        println!("  --dark-web             Dark Web Insights");
        println!("  --password-health      Password Health (weakness analyzer)");
        println!("  --autofill-engine      Autofill engine (auto-fill credentials/cards)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dashlane 6.2459.0 (OurOS)"); return 0; }
    println!("Dashlane 6.2459.0 (OurOS)");
    println!("  Vendor: Dashlane SAS (Paris/New York, founded 2009)");
    println!("  Crypto: AES-256, Argon2id (modern KDF), zero-knowledge architecture");
    println!("  Architecture pivot: 2022 sunset native apps, web-only");
    println!("  Web extension + web vault are the only client now (no installable desktop)");
    println!("  Features: vault, autofill, password generator, password health, secure notes,");
    println!("            credit/ID monitoring (US), dark web alerts, VPN (Hotspot Shield)");
    println!("  Plans: Free (limited), Premium $4.99/mo, Friends & Family $7.49/mo (10 users)");
    println!("  Business: Starter, Team, Business — SSO, SCIM, U2F, encrypted sharing");
    println!("  Passkeys: full passkey support (FIDO2) added 2023 — first PM to ship");
    println!("  Mobile: iOS, Android (still native there, autofill providers)");
    println!("  Differentiator: passwordless authentication, focus on consumer breach alerts");
    println!("  Funding: $200M+ raised, valued $1B+");
    println!("  Audits: third-party SOC 2, regular pentest disclosures");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dashlane".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dashlane"), "dashlane");
        assert_eq!(basename(r"C:\bin\dashlane.exe"), "dashlane.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dashlane.exe"), "dashlane");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dl(&["--help".to_string()], "dashlane"), 0);
        assert_eq!(run_dl(&["-h".to_string()], "dashlane"), 0);
        let _ = run_dl(&["--version".to_string()], "dashlane");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dl(&[], "dashlane");
    }
}
