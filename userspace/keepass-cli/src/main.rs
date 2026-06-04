#![deny(clippy::all)]

//! keepass-cli — OurOS KeePass open-source password manager
//!
//! Single personality: `keepass`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: keepass [OPTIONS] [DATABASE]");
        println!("KeePass 2.57 (OurOS) — File-based open-source password manager");
        println!();
        println!("Options:");
        println!("  DATABASE               Path to .kdbx file");
        println!("  --keyfile FILE         Use a key file in addition to master password");
        println!("  --new                  Create new database");
        println!("  --generate             Password generator");
        println!("  --autotype             Auto-Type (send keystrokes to target window)");
        println!("  --plugin               Load plugin");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("KeePass 2.57 (OurOS)"); return 0; }
    println!("KeePass 2.57 (OurOS)");
    println!("  Vendor: Dominik Reichl (single author, KeePass Password Safe Foundation)");
    println!("  License: GPL-2.0-or-later (free, open source)");
    println!("  Stack: .NET (KeePass 2.x), C++ (KeePass 1.x classic)");
    println!("  Database: .kdbx file (KeePass Database eXtended) — local file, you own storage");
    println!("  Crypto: AES-256 (default), ChaCha20 (option); KDFs: AES-KDF, Argon2id");
    println!("  Key composition: master password, key file, Windows User Account, YubiKey HMAC-SHA1");
    println!("  No cloud: sync is your responsibility (Dropbox/Nextcloud/Syncthing/USB)");
    println!("  Features: Auto-Type (send creds to any window), groups, attachments, custom icons,");
    println!("            triggers, plugins (KeeAnywhere, KeePassNatMsg, KeePassRPC for browsers)");
    println!("  Forks: KeePassXC (cross-platform Qt rewrite), KeePassDX (Android),");
    println!("         KeeWeb (web-based), MacPass (macOS), Strongbox (iOS/macOS)");
    println!("  Browser integration: requires plugin (KeePassXC has native messaging built-in)");
    println!("  Audit: included in EU FOSSA bug bounty program — well-vetted");
    println!("  Adoption: high among sysadmins, privacy advocates, and offline-first users");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keepass".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/keepass"), "keepass");
        assert_eq!(basename(r"C:\bin\keepass.exe"), "keepass.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("keepass.exe"), "keepass");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kp(&["--help".to_string()], "keepass"), 0);
        assert_eq!(run_kp(&["-h".to_string()], "keepass"), 0);
        let _ = run_kp(&["--version".to_string()], "keepass");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kp(&[], "keepass");
    }
}
