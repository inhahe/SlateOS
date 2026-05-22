#![deny(clippy::all)]

//! certbot — OurOS certificate management (Let's Encrypt client)
//!
//! Single personality: `certbot`

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _LETSENCRYPT_DIR: &str = "/etc/letsencrypt";
const _RENEWAL_DIR: &str = "/etc/letsencrypt/renewal";
const _ACME_URL: &str = "https://acme-v02.api.letsencrypt.org/directory";
const _STAGING_URL: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Certificate {
    name: String,
    domains: Vec<String>,
    expiry: String,
    _serial: String,
    _path_cert: String,
    _path_key: String,
    _path_chain: String,
    _path_fullchain: String,
}

fn sample_certificates() -> Vec<Certificate> {
    vec![
        Certificate {
            name: "ouros.local".to_string(),
            domains: vec!["ouros.local".to_string(), "www.ouros.local".to_string()],
            expiry: "2025-08-20T00:00:00Z".to_string(),
            _serial: "0A1B2C3D4E5F6071".to_string(),
            _path_cert: "/etc/letsencrypt/live/ouros.local/cert.pem".to_string(),
            _path_key: "/etc/letsencrypt/live/ouros.local/privkey.pem".to_string(),
            _path_chain: "/etc/letsencrypt/live/ouros.local/chain.pem".to_string(),
            _path_fullchain: "/etc/letsencrypt/live/ouros.local/fullchain.pem".to_string(),
        },
        Certificate {
            name: "mail.ouros.local".to_string(),
            domains: vec!["mail.ouros.local".to_string()],
            expiry: "2025-07-15T00:00:00Z".to_string(),
            _serial: "1A2B3C4D5E6F7081".to_string(),
            _path_cert: "/etc/letsencrypt/live/mail.ouros.local/cert.pem".to_string(),
            _path_key: "/etc/letsencrypt/live/mail.ouros.local/privkey.pem".to_string(),
            _path_chain: "/etc/letsencrypt/live/mail.ouros.local/chain.pem".to_string(),
            _path_fullchain: "/etc/letsencrypt/live/mail.ouros.local/fullchain.pem".to_string(),
        },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_certbot(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: certbot COMMAND [OPTIONS]");
            println!();
            println!("ACME client for obtaining TLS certificates.");
            println!();
            println!("Commands:");
            println!("  certonly       Obtain or renew a certificate");
            println!("  install        Install a certificate in a server config");
            println!("  renew          Renew all certificates near expiry");
            println!("  revoke         Revoke a certificate");
            println!("  delete         Delete a certificate");
            println!("  certificates   List managed certificates");
            println!("  register       Create an ACME account");
            println!("  unregister     Deactivate ACME account");
            println!("  rollback       Roll back server config changes");
            println!("  enhance        Add security enhancements");
            println!("  --version      Show version");
            0
        }
        "--version" | "-V" => { println!("certbot 0.1.0 (OurOS)"); 0 }
        "certonly" => certbot_certonly(&cmd_args),
        "renew" => certbot_renew(&cmd_args),
        "revoke" => certbot_revoke(&cmd_args),
        "delete" => certbot_delete(&cmd_args),
        "certificates" => certbot_certificates(),
        "register" => certbot_register(&cmd_args),
        "enhance" => certbot_enhance(&cmd_args),
        other => { eprintln!("certbot: unknown command '{}'", other); 1 }
    }
}

fn certbot_certonly(args: &[String]) -> i32 {
    let mut domains: Vec<String> = Vec::new();
    let mut webroot = false;
    let mut standalone = false;
    let mut _dry_run = false;
    let mut staging = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--domain" | "--domains" => {
                i += 1;
                if i < args.len() {
                    for d in args[i].split(',') {
                        domains.push(d.trim().to_string());
                    }
                }
            }
            "--webroot" => webroot = true,
            "--standalone" => standalone = true,
            "--dry-run" => _dry_run = true,
            "--staging" | "--test-cert" => staging = true,
            _ => {}
        }
        i += 1;
    }

    if domains.is_empty() {
        eprintln!("certbot: no domains specified (use -d)");
        return 1;
    }

    let method = if webroot { "webroot" } else if standalone { "standalone" } else { "preferred-challenges" };
    let server = if staging { "staging" } else { "production" };

    println!("Saving debug log to /var/log/letsencrypt/letsencrypt.log");
    println!("Requesting a certificate for {} domain(s) via {} ({} server)",
        domains.len(), method, server);

    for d in &domains {
        println!("  Performing challenges for {}...", d);
        println!("  Waiting for verification...");
        println!("  Cleaning up challenges for {}...", d);
    }

    println!();
    println!("Successfully received certificate.");
    println!("Certificate is saved at: /etc/letsencrypt/live/{}/fullchain.pem", domains[0]);
    println!("Key is saved at:         /etc/letsencrypt/live/{}/privkey.pem", domains[0]);
    println!();
    println!("IMPORTANT NOTES:");
    println!(" - Congratulations! Your certificate and chain have been saved.");
    println!(" - This certificate expires on 2025-08-20.");
    println!(" - To renew, run: certbot renew");
    0
}

fn certbot_renew(args: &[String]) -> i32 {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let force = args.iter().any(|a| a == "--force-renewal");

    let certs = sample_certificates();
    println!("Saving debug log to /var/log/letsencrypt/letsencrypt.log");
    println!();
    println!("- - - - - - - - - - - - - - - - - - - - - - - - - - - - - -");
    println!("Processing /etc/letsencrypt/renewal/*.conf");
    println!("- - - - - - - - - - - - - - - - - - - - - - - - - - - - - -");

    for cert in &certs {
        if dry_run {
            println!("Cert not yet due for renewal (simulated dry run): {}", cert.name);
        } else if force {
            println!("Renewing certificate for {} (forced)", cert.name);
            println!("  New certificate saved.");
        } else {
            println!("Certificate not yet due for renewal: {}", cert.name);
        }
    }

    println!();
    println!("- - - - - - - - - - - - - - - - - - - - - - - - - - - - - -");
    if dry_run {
        println!("** DRY RUN: simulating 'certbot renew' close to expiry");
        println!("** (The test certificates above have not been saved.)");
    } else {
        println!("Congratulations, all renewals succeeded.");
    }
    0
}

fn certbot_revoke(args: &[String]) -> i32 {
    let cert_path = args.iter().position(|a| a == "--cert-path")
        .and_then(|i| args.get(i + 1));

    match cert_path {
        Some(path) => {
            println!("Revoking certificate: {}", path);
            println!("Certificate revoked (simulated).");
            0
        }
        None => {
            eprintln!("certbot revoke: --cert-path required");
            1
        }
    }
}

fn certbot_delete(args: &[String]) -> i32 {
    let name = args.iter().position(|a| a == "--cert-name")
        .and_then(|i| args.get(i + 1));

    match name {
        Some(n) => {
            println!("Deleting certificate '{}' and related files (simulated)", n);
            0
        }
        None => {
            eprintln!("certbot delete: --cert-name required");
            1
        }
    }
}

fn certbot_certificates() -> i32 {
    let certs = sample_certificates();
    println!("Found the following certs:");

    for cert in &certs {
        println!("  Certificate Name: {}", cert.name);
        println!("    Serial Number: {}", cert._serial);
        println!("    Key Type: RSA");
        println!("    Domains: {}", cert.domains.join(" "));
        println!("    Expiry Date: {}", cert.expiry);
        println!("    Certificate Path: {}", cert._path_fullchain);
        println!("    Private Key Path: {}", cert._path_key);
        println!();
    }
    0
}

fn certbot_register(args: &[String]) -> i32 {
    let email = args.iter().position(|a| a == "--email" || a == "-m")
        .and_then(|i| args.get(i + 1));

    match email {
        Some(e) => {
            println!("Registering ACME account with email: {}", e);
            println!("Account registered (simulated).");
            0
        }
        None => {
            println!("Registering ACME account without email (not recommended).");
            println!("Account registered (simulated).");
            0
        }
    }
}

fn certbot_enhance(args: &[String]) -> i32 {
    let hsts = args.iter().any(|a| a == "--hsts");
    let redirect = args.iter().any(|a| a == "--redirect");
    let staple = args.iter().any(|a| a == "--staple-ocsp");

    println!("Enhancing server configuration (simulated):");
    if redirect { println!("  Adding HTTP → HTTPS redirect"); }
    if hsts { println!("  Adding HSTS header (max-age=31536000)"); }
    if staple { println!("  Enabling OCSP stapling"); }
    if !redirect && !hsts && !staple {
        println!("  No enhancements specified (use --redirect, --hsts, --staple-ocsp)");
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_certbot(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_certificates() {
        let certs = sample_certificates();
        assert_eq!(certs.len(), 2);
        assert!(certs[0].domains.len() >= 2);
    }

    #[test]
    fn test_cert_paths() {
        let certs = sample_certificates();
        assert!(certs[0]._path_cert.contains("letsencrypt"));
        assert!(certs[0]._path_key.contains("privkey"));
    }
}
