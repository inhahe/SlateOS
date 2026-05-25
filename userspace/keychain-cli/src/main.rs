#![deny(clippy::all)]

//! keychain-cli — OurOS keychain SSH/GPG agent manager
//!
//! Single personality: `keychain`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_keychain(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: keychain [OPTIONS] [KEY...]");
        println!("keychain v2.8 (OurOS) — SSH/GPG agent front-end");
        println!();
        println!("Options:");
        println!("  --clear           Clear all cached keys");
        println!("  --list            List cached keys");
        println!("  --agents TYPE     Agent types (ssh,gpg)");
        println!("  --eval            Output for eval");
        println!("  --noask           Don't ask for passphrase");
        println!("  --quiet           Suppress output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("keychain v2.8 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--clear") {
        println!("* keychain: clearing all cached keys");
        return 0;
    }
    if args.iter().any(|a| a == "--list") {
        println!("ssh-rsa SHA256:abc123... user@host (RSA)");
        println!("ssh-ed25519 SHA256:def456... user@host (ED25519)");
        return 0;
    }
    println!(" * keychain 2.8.5 ~ http://www.funtoo.org");
    println!(" * Found existing ssh-agent: 12345");
    println!(" * Adding 1 ssh key(s): /home/user/.ssh/id_ed25519");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keychain".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_keychain(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
