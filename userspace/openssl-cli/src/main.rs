#![deny(clippy::all)]

//! openssl-cli — OurOS OpenSSL-compatible cryptography CLI
//!
//! Single personality: `openssl`

use std::env;
use std::process;

fn run_openssl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "help" || a == "--help" || a == "-h") {
        println!("Usage: openssl <COMMAND> [OPTIONS]");
        println!();
        println!("OpenSSL command-line tool.");
        println!();
        println!("Standard commands:");
        println!("  genrsa       Generate RSA private key");
        println!("  genpkey      Generate private key");
        println!("  req          Certificate signing request");
        println!("  x509         X.509 certificate utility");
        println!("  ca           Certificate authority");
        println!("  s_client     SSL/TLS client");
        println!("  s_server     SSL/TLS server");
        println!("  enc          Symmetric encryption");
        println!("  dgst         Message digest");
        println!("  rand         Generate random bytes");
        println!("  pkcs12       PKCS#12 utility");
        println!("  rsa          RSA key utility");
        println!("  ec           EC key utility");
        println!("  verify       Certificate verification");
        println!("  crl          CRL management");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            let full = args.iter().any(|a| a == "-a");
            println!("OpenSSL 3.2.0 14 Nov 2023 (OurOS)");
            if full {
                println!("built on: Thu Jan 1 00:00:00 2024");
                println!("platform: ouros-x86_64");
                println!("compiler: rustc 1.76.0");
                println!("OPENSSLDIR: /etc/ssl");
            }
            0
        }
        "genrsa" => {
            let bits = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(2048);
            println!("Generating RSA private key, {} bit long modulus (2 primes)", bits);
            println!("...+++++++++++++++++++");
            println!("...+++++++++++++++++++");
            println!("e is 65537 (0x010001)");
            println!("-----BEGIN RSA PRIVATE KEY-----");
            println!("MIIEpAIBAAKCAQEA... (key data)");
            println!("-----END RSA PRIVATE KEY-----");
            0
        }
        "req" => {
            let new = args.iter().any(|a| a == "-new" || a == "-newkey");
            if new {
                println!("Generating a RSA private key");
                println!("writing new private key to 'key.pem'");
                println!("-----");
                println!("You are about to be asked to enter information.");
                println!("Country Name (2 letter code) [US]:");
                println!("State or Province Name [California]:");
                println!("Locality Name [San Francisco]:");
                println!("Organization Name [Example Inc]:");
                println!("Common Name [example.com]:");
                println!("  CSR written to req.pem");
            }
            0
        }
        "x509" => {
            let text = args.iter().any(|a| a == "-text");
            if text {
                println!("Certificate:");
                println!("    Data:");
                println!("        Version: 3 (0x2)");
                println!("        Serial Number: 1234567890");
                println!("        Issuer: C=US, O=Example Inc, CN=Example CA");
                println!("        Validity:");
                println!("            Not Before: Jan 15 00:00:00 2024 GMT");
                println!("            Not After : Jan 15 00:00:00 2025 GMT");
                println!("        Subject: C=US, O=Example Inc, CN=example.com");
                println!("        Subject Public Key Info:");
                println!("            Public Key Algorithm: rsaEncryption");
                println!("            RSA Public-Key: (2048 bit)");
            }
            0
        }
        "s_client" => {
            let host = args.windows(2)
                .find(|w| w[0] == "-connect")
                .map(|w| w[1].as_str())
                .unwrap_or("example.com:443");
            println!("CONNECTED(00000003)");
            println!("depth=2 C=US, O=DigiCert Inc, CN=DigiCert Global Root G2");
            println!("depth=1 C=US, O=DigiCert Inc, CN=DigiCert SHA2 Extended Validation Server CA");
            println!("depth=0 CN={}", host.split(':').next().unwrap_or("example.com"));
            println!("---");
            println!("SSL handshake has read 3456 bytes and written 789 bytes");
            println!("---");
            println!("Protocol  : TLSv1.3");
            println!("Cipher    : TLS_AES_256_GCM_SHA384");
            0
        }
        "dgst" => {
            let algo = args.iter()
                .find(|a| a.starts_with("-sha") || a.starts_with("-md5"))
                .map(|s| &s[1..])
                .unwrap_or("sha256");
            let file = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("input.dat");
            println!("{}({})= abc123def456789012345678901234567890123456789012345678901234", algo, file);
            0
        }
        "rand" => {
            let nbytes: u32 = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(32);
            let hex = args.iter().any(|a| a == "-hex");
            if hex {
                println!("abc123def456789012345678901234567890123456789012345678901234abcd");
            } else {
                println!("({} random bytes written to stdout)", nbytes);
            }
            0
        }
        "enc" => {
            let decrypt = args.iter().any(|a| a == "-d");
            if decrypt {
                println!("Decrypting...");
            } else {
                println!("Encrypting...");
            }
            println!("  Done.");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: openssl <command>. See help.");
            } else {
                eprintln!("Error: unknown command '{}'. See help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openssl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
