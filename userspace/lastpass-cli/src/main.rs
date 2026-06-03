#![deny(clippy::all)]

//! lastpass-cli — OurOS LastPass password manager
//!
//! Single personality: `lastpass`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lastpass [OPTIONS]");
        println!("LastPass (OurOS) — GoTo (LogMeIn) password manager");
        println!();
        println!("Options:");
        println!("  --vault                Open vault");
        println!("  --generate             Generate password");
        println!("  --security-dashboard   Security Dashboard (dark web monitoring)");
        println!("  --emergency-access     Emergency Access (recovery via designee)");
        println!("  --teams                LastPass Teams / Business / Enterprise");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LastPass 4.137.0 (OurOS)"); return 0; }
    println!("LastPass 4.137.0 (OurOS)");
    println!("  Owner: GoTo (formerly LogMeIn — acquired LastPass 2015 for $125M)");
    println!("  Founded: 2008 by Marvin Liao, Joe Siegrist, Bilal Hameed");
    println!("  Spun off from GoTo Sep 2024 (private, separate company)");
    println!("  Crypto: AES-256-CBC, PBKDF2-SHA256 (100K iter default, was 5K pre-2018)");
    println!("  Features: vault, autofill, password generator, secure notes, dark web monitor,");
    println!("            credit monitoring (US only), emergency access, sharing");
    println!("  Plans: Free (1 device class), Premium $36/yr, Families $48/yr (6 users)");
    println!("  Business: Teams $4/user/mo, Business $7/user/mo, MFA + SSO add-ons");
    println!("  2022 BREACH (severe): customer vault data exfiltrated (encrypted), source code");
    println!("                       stolen, unencrypted URLs leaked — recommended mass migration");
    println!("  Recovery iterations bumped to 600K post-breach (Aug 2023)");
    println!("  Migration drain: many users moved to Bitwarden / 1Password / Proton Pass");
    println!("  Mobile + browser: extensions for Chrome/Firefox/Safari/Edge/Brave");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lastpass".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lastpass"), "lastpass");
        assert_eq!(basename(r"C:\bin\lastpass.exe"), "lastpass.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lastpass.exe"), "lastpass");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lp(&["--help".to_string()], "lastpass"), 0);
        assert_eq!(run_lp(&["-h".to_string()], "lastpass"), 0);
        assert_eq!(run_lp(&["--version".to_string()], "lastpass"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lp(&[], "lastpass"), 0);
    }
}
