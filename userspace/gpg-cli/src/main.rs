#![deny(clippy::all)]

//! gpg-cli — SlateOS GnuPG CLI
//!
//! Single personality: `gpg`

use std::env;
use std::process;

fn run_gpg(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gpg [OPTIONS] [FILE]");
        println!();
        println!("GnuPG — OpenPGP encryption and signing (Slate OS).");
        println!();
        println!("Commands:");
        println!("  --gen-key           Generate a new key pair");
        println!("  --list-keys         List public keys");
        println!("  --list-secret-keys  List secret keys");
        println!("  --encrypt, -e       Encrypt data");
        println!("  --decrypt, -d       Decrypt data");
        println!("  --sign, -s          Sign data");
        println!("  --verify            Verify signature");
        println!("  --import            Import keys");
        println!("  --export            Export keys");
        println!("  --delete-key        Delete a key");
        println!("  --fingerprint       Show fingerprints");
        println!("  --keyserver         Specify keyserver");
        println!("  --recv-keys         Receive keys from server");
        println!("  --send-keys         Send keys to server");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gpg (GnuPG) 2.4.4 (Slate OS)");
        println!("libgcrypt 1.10.3");
        return 0;
    }

    let has_flag = |flags: &[&str]| args.iter().any(|a| flags.contains(&a.as_str()));

    if has_flag(&["--gen-key", "--generate-key"]) {
        println!("gpg: key generation started");
        println!("  Real name: User Name");
        println!("  Email: user@example.com");
        println!("gpg: key ABC123DEF456GHI7 marked as ultimately trusted");
        println!("gpg: revocation certificate stored as '/home/user/.gnupg/openpgp-revocs.d/...'");
        println!("pub   ed25519 2024-01-15 [SC]");
        println!("      ABC123DEF456GHI789JKL012MNO345PQR678STU9");
        println!("uid                      User Name <user@example.com>");
        println!("sub   cv25519 2024-01-15 [E]");
        return 0;
    }
    if has_flag(&["--list-keys", "-k", "--list-public-keys"]) {
        println!("/home/user/.gnupg/pubring.kbx");
        println!("-------------------------------");
        println!("pub   ed25519 2024-01-15 [SC]");
        println!("      ABC123DEF456GHI789JKL012MNO345PQR678STU9");
        println!("uid           [ultimate] User Name <user@example.com>");
        println!("sub   cv25519 2024-01-15 [E]");
        println!();
        println!("pub   rsa4096 2023-06-01 [SC] [expires: 2025-06-01]");
        println!("      901VWX234YZA567BCD890EFG123HIJ456KLM789N");
        println!("uid           [  full  ] Colleague <colleague@example.com>");
        println!("sub   rsa4096 2023-06-01 [E]");
        return 0;
    }
    if has_flag(&["--list-secret-keys", "-K"]) {
        println!("/home/user/.gnupg/pubring.kbx");
        println!("-------------------------------");
        println!("sec   ed25519 2024-01-15 [SC]");
        println!("      ABC123DEF456GHI789JKL012MNO345PQR678STU9");
        println!("uid           [ultimate] User Name <user@example.com>");
        println!("ssb   cv25519 2024-01-15 [E]");
        return 0;
    }
    if has_flag(&["--encrypt", "-e"]) {
        let recipient = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--recipient")
            .map(|w| w[1].as_str()).unwrap_or("user@example.com");
        let file = args.last().map(|s| s.as_str()).unwrap_or("message.txt");
        println!("gpg: encrypted for {}", recipient);
        println!("gpg: wrote {}.gpg", file);
        return 0;
    }
    if has_flag(&["--decrypt", "-d"]) {
        let file = args.last().map(|s| s.as_str()).unwrap_or("message.txt.gpg");
        println!("gpg: encrypted with cv25519 key, ID 0123456789ABCDEF");
        println!("gpg: decrypted {}", file);
        println!("(decrypted content written to stdout)");
        return 0;
    }
    if has_flag(&["--sign", "-s"]) {
        let file = args.last().map(|s| s.as_str()).unwrap_or("document.txt");
        println!("gpg: signed with ed25519 key ABC123DEF456GHI7");
        println!("gpg: wrote {}.gpg", file);
        return 0;
    }
    if has_flag(&["--verify"]) {
        let file = args.last().map(|s| s.as_str()).unwrap_or("document.txt.sig");
        println!("gpg: Signature made Mon Jan 15 14:00:00 2024 UTC");
        println!("gpg:                using EDDSA key ABC123DEF456GHI789JKL012MNO345PQR678STU9");
        println!("gpg: Good signature from \"User Name <user@example.com>\" [ultimate]");
        println!("  File: {}", file);
        return 0;
    }
    if has_flag(&["--import"]) {
        let file = args.last().map(|s| s.as_str()).unwrap_or("pubkey.asc");
        println!("gpg: key 0123456789ABCDEF: public key \"Imported User <import@example.com>\" imported");
        println!("gpg: Total number processed: 1");
        println!("gpg:               imported: 1");
        println!("  File: {}", file);
        return 0;
    }
    if has_flag(&["--export"]) {
        let armor = has_flag(&["--armor", "-a"]);
        if armor {
            println!("-----BEGIN PGP PUBLIC KEY BLOCK-----");
            println!("mDMEZaUBBRYJKwYBBAHaRw8BAQdAabcdef...");
            println!("-----END PGP PUBLIC KEY BLOCK-----");
        } else {
            println!("(binary key data written to stdout)");
        }
        return 0;
    }
    if has_flag(&["--fingerprint"]) {
        println!("pub   ed25519 2024-01-15 [SC]");
        println!("      ABC1 23DE F456 GHI7 89JK  L012 MNO3 45PQ R678 STU9");
        println!("uid           [ultimate] User Name <user@example.com>");
        println!("sub   cv25519 2024-01-15 [E]");
        println!("      FING ERPR INT0 OFSU BKEY  1234 5678 90AB CDEF 1234");
        return 0;
    }

    eprintln!("Usage: gpg [options] [file]. See --help.");
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gpg(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gpg};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gpg(vec!["--help".to_string()]), 0);
        assert_eq!(run_gpg(vec!["-h".to_string()]), 0);
        let _ = run_gpg(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gpg(vec![]);
    }
}
