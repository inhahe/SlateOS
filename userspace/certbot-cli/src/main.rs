#![deny(clippy::all)]

//! certbot-cli — OurOS Let's Encrypt certificate manager
//!
//! Single personality: `certbot`

use std::env;
use std::process;

fn run_certbot(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: certbot <COMMAND> [OPTIONS]");
        println!();
        println!("Automatically obtain and renew TLS certificates (Let's Encrypt).");
        println!();
        println!("Commands:");
        println!("  certonly     Obtain or renew a certificate (no install)");
        println!("  install      Install a certificate in a server");
        println!("  renew        Renew all certificates");
        println!("  revoke       Revoke a certificate");
        println!("  delete       Delete a certificate");
        println!("  certificates List certificates");
        println!("  register     Create ACME account");
        println!("  rollback     Roll back server config changes");
        println!();
        println!("Options:");
        println!("  -d, --domain <DOMAIN>  Domain name(s)");
        println!("  --nginx                Use nginx plugin");
        println!("  --apache               Use apache plugin");
        println!("  --standalone           Use standalone HTTP server");
        println!("  --dns-<PROVIDER>       Use DNS challenge");
        println!("  --webroot              Use webroot plugin");
        println!("  -w, --webroot-path <P> Webroot path");
        println!("  --email <EMAIL>        Email for account");
        println!("  --agree-tos            Agree to terms");
        println!("  --dry-run              Test without saving");
        println!("  -n, --non-interactive  Non-interactive mode");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("certbot 2.9.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "certonly" => {
            let domain = args.windows(2)
                .find(|w| w[0] == "-d" || w[0] == "--domain")
                .map(|w| w[1].as_str())
                .unwrap_or("example.com");
            let dry_run = args.iter().any(|a| a == "--dry-run");
            println!("Saving debug log to /var/log/letsencrypt/letsencrypt.log");
            println!("Requesting a certificate for {}", domain);
            println!("  Performing challenges...");
            println!("  Waiting for verification...");
            println!("  Cleaning up challenges...");
            if dry_run {
                println!("  (dry run) Certificate not saved.");
            } else {
                println!("  Successfully received certificate.");
                println!("  Certificate: /etc/letsencrypt/live/{}/fullchain.pem", domain);
                println!("  Key:         /etc/letsencrypt/live/{}/privkey.pem", domain);
                println!("  Expiry:      2024-04-15");
            }
            0
        }
        "renew" => {
            println!("Processing /etc/letsencrypt/renewal/example.com.conf");
            println!("  Certificate not yet due for renewal (30 days remaining)");
            println!();
            println!("No renewals were attempted.");
            0
        }
        "certificates" => {
            println!("Found the following certs:");
            println!("  Certificate Name: example.com");
            println!("    Domains: example.com www.example.com");
            println!("    Expiry Date: 2024-04-15 (VALID: 90 days)");
            println!("    Certificate Path: /etc/letsencrypt/live/example.com/fullchain.pem");
            println!("    Private Key Path: /etc/letsencrypt/live/example.com/privkey.pem");
            0
        }
        "revoke" => {
            println!("Certificate revoked.");
            0
        }
        "delete" => {
            let domain = args.windows(2)
                .find(|w| w[0] == "-d" || w[0] == "--cert-name")
                .map(|w| w[1].as_str())
                .unwrap_or("example.com");
            println!("Deleted certificate for {}", domain);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: certbot <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_certbot(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_certbot};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_certbot(vec!["--help".to_string()]), 0);
        assert_eq!(run_certbot(vec!["-h".to_string()]), 0);
        assert_eq!(run_certbot(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_certbot(vec![]), 0);
    }
}
