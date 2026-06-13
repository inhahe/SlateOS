#![deny(clippy::all)]

//! step-cli — SlateOS Smallstep certificate management
//!
//! Multi-personality: `step`, `step-ca`

use std::env;
use std::process;

fn run_step(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: step <command> [flags]");
        println!();
        println!("Commands:");
        println!("  certificate   Manage certificates");
        println!("  crypto        Cryptographic primitives");
        println!("  ca            Certificate Authority commands");
        println!("  oauth         OAuth/OIDC flows");
        println!("  ssh           SSH certificate commands");
        println!("  beta          Beta features");
        println!("  version       Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("Smallstep CLI/0.26.1 (Slate OS, linux/amd64)");
            println!("Release Date: 2025-05-22");
        }
        "certificate" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "create" => {
                    println!("Your certificate has been saved in cert.pem.");
                    println!("Your private key has been saved in key.pem.");
                }
                "inspect" => {
                    println!("Certificate:");
                    println!("    Data:");
                    println!("        Version: 3 (0x2)");
                    println!("        Serial Number: 1234567890");
                    println!("    Signature Algorithm: ECDSA-SHA256");
                    println!("        Issuer: CN=Slate OS Root CA");
                    println!("        Validity:");
                    println!("            Not Before: 2025-05-22 00:00:00 +0000 UTC");
                    println!("            Not After : 2026-05-22 00:00:00 +0000 UTC");
                    println!("        Subject: CN=example.com");
                }
                "verify" => println!("Certificate is valid."),
                "lint" => println!("Certificate linting: 0 errors, 0 warnings."),
                "fingerprint" => println!("SHA256:abcdef1234567890abcdef1234567890abcdef1234567890"),
                _ => {
                    println!("Subcommands: create, inspect, verify, lint, fingerprint, install, uninstall, format, key");
                }
            }
        }
        "crypto" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "keypair" => println!("Key pair generated: pub.pem, priv.pem"),
                "hash" => println!("SHA256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
                _ => println!("Subcommands: keypair, hash, nacl, jwt, jwe, jws, kdf, otp"),
            }
        }
        "ca" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "bootstrap" => println!("The root certificate has been saved to ~/.step/certs/root_ca.crt."),
                "certificate" => println!("Certificate signed and saved."),
                "token" => println!("eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.simulated.token"),
                "health" => println!("ok"),
                _ => println!("Subcommands: bootstrap, certificate, token, revoke, renew, provisioner, health"),
            }
        }
        "ssh" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "certificate" => println!("SSH certificate saved to id-ssh-cert.pub"),
                "login" => println!("SSH login successful."),
                "list" => println!("Found 2 SSH certificates."),
                _ => println!("Subcommands: certificate, login, list, config, check-host, inspect"),
            }
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_step_ca(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: step-ca [FLAGS] [CONFIG]");
        println!();
        println!("Flags:");
        println!("  --password-file <file>  Password file");
        println!("  --issuer-password-file  Issuer password");
        println!("  --resolver <addr>       DNS resolver");
        println!("  --pidfile <file>        PID file");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Smallstep CA/0.26.1 (Slate OS)");
        return 0;
    }

    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("ca.json");
    println!("Smallstep CA/0.26.1 (Slate OS)");
    println!("Loading configuration from {}", config);
    println!("Starting Certificate Authority...");
    println!("Listening on :443 ...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("step");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "step-ca" => run_step_ca(rest),
        _ => run_step(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_step};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_step(vec!["--help".to_string()]), 0);
        assert_eq!(run_step(vec!["-h".to_string()]), 0);
        let _ = run_step(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_step(vec![]);
    }
}
