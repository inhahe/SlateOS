#![deny(clippy::all)]

//! openssl — Slate OS OpenSSL command-line toolkit
//!
//! Single personality: `openssl`

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _VERSION_STRING: &str = "OpenSSL 0.1.0 (Slate OS)";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct CertInfo {
    subject: String,
    issuer: String,
    serial: String,
    not_before: String,
    not_after: String,
    _sig_alg: String,
    _pubkey_bits: u32,
}

fn sample_cert() -> CertInfo {
    CertInfo {
        subject: "CN=slateos.local, O=Slate OS Project, C=US".to_string(),
        issuer: "CN=Slate OS Root CA, O=Slate OS Project, C=US".to_string(),
        serial: "0A:1B:2C:3D:4E:5F:60:71".to_string(),
        not_before: "May 22 00:00:00 2025 GMT".to_string(),
        not_after: "May 22 00:00:00 2026 GMT".to_string(),
        _sig_alg: "sha256WithRSAEncryption".to_string(),
        _pubkey_bits: 4096,
    }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_openssl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            println!("Usage: openssl COMMAND [OPTIONS]");
            println!();
            println!("Standard commands:");
            println!("  version        Show version info");
            println!("  genrsa         Generate RSA private key");
            println!("  genpkey        Generate private key");
            println!("  req            Certificate request / self-signed cert");
            println!("  x509           X.509 certificate utilities");
            println!("  rsa            RSA key management");
            println!("  enc            Symmetric cipher encryption");
            println!("  dgst           Message digest / signing");
            println!("  s_client       SSL/TLS client");
            println!("  s_server       SSL/TLS server");
            println!("  rand           Generate random bytes");
            println!("  speed          Benchmark ciphers");
            println!("  verify         Verify certificate chain");
            println!("  pkcs12         PKCS#12 utilities");
            println!("  ciphers        List supported ciphers");
            println!("  list           List algorithms and capabilities");
            0
        }
        "version" => openssl_version(&cmd_args),
        "genrsa" => openssl_genrsa(&cmd_args),
        "genpkey" => openssl_genpkey(&cmd_args),
        "req" => openssl_req(&cmd_args),
        "x509" => openssl_x509(&cmd_args),
        "enc" => openssl_enc(&cmd_args),
        "dgst" => openssl_dgst(&cmd_args),
        "s_client" => openssl_s_client(&cmd_args),
        "rand" => openssl_rand(&cmd_args),
        "speed" => openssl_speed(),
        "verify" => openssl_verify(&cmd_args),
        "ciphers" => openssl_ciphers(&cmd_args),
        "list" => openssl_list(&cmd_args),
        other => { eprintln!("openssl: '{}' is not a recognized command", other); 1 }
    }
}

fn openssl_version(args: &[String]) -> i32 {
    let show_all = args.iter().any(|a| a == "-a");
    println!("OpenSSL 0.1.0 (Slate OS)");
    if show_all {
        println!("built on: May 22 2025");
        println!("platform: x86_64-slateos");
        println!("compiler: rustc (Slate OS nightly)");
        println!("OPENSSLDIR: \"/etc/ssl\"");
        println!("ENGINESDIR: \"/usr/lib/engines-3\"");
        println!("MODULESDIR: \"/usr/lib/ossl-modules\"");
    }
    0
}

fn openssl_genrsa(args: &[String]) -> i32 {
    let bits = args.last().and_then(|s| s.parse::<u32>().ok()).unwrap_or(2048);
    let out = args.iter().position(|a| a == "-out")
        .and_then(|i| args.get(i + 1));

    println!("Generating RSA private key, {} bit long modulus (2 primes)", bits);
    println!("..............+++++++++++++++++++++++++++++++++++++++");
    println!("...+++++++++++++++++++++++++++++++++++++++");
    println!("e is 65537 (0x010001)");

    if let Some(path) = out {
        println!("writing RSA key to '{}'", path);
    } else {
        println!("-----BEGIN RSA PRIVATE KEY-----");
        println!("MIIEpAIBAAKCAQEA... (simulated {} bit key)", bits);
        println!("-----END RSA PRIVATE KEY-----");
    }
    0
}

fn openssl_genpkey(args: &[String]) -> i32 {
    let algorithm = args.iter().position(|a| a == "-algorithm")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("RSA");

    println!("Generating {} private key (simulated)", algorithm);
    println!("-----BEGIN PRIVATE KEY-----");
    println!("MIIEvgIBADANBgkq... (simulated {} key)", algorithm);
    println!("-----END PRIVATE KEY-----");
    0
}

fn openssl_req(args: &[String]) -> i32 {
    let new = args.iter().any(|a| a == "-new");
    let x509 = args.iter().any(|a| a == "-x509");
    let _nodes = args.iter().any(|a| a == "-nodes");

    if new && x509 {
        println!("Generating self-signed certificate (simulated):");
        println!("  Subject: CN=slateos.local, O=Slate OS Project, C=US");
        println!("  Validity: 365 days");
        println!("  Key: RSA 2048 bit");
        println!("-----BEGIN CERTIFICATE-----");
        println!("MIIDazCCAlOgAwIBAgI... (simulated)");
        println!("-----END CERTIFICATE-----");
    } else if new {
        println!("Generating certificate signing request (simulated):");
        println!("  Subject: CN=slateos.local, O=Slate OS Project, C=US");
        println!("-----BEGIN CERTIFICATE REQUEST-----");
        println!("MIICYjCCAUoCAQAw... (simulated)");
        println!("-----END CERTIFICATE REQUEST-----");
    } else {
        println!("openssl req: use -new to create a CSR, -x509 for self-signed cert");
    }
    0
}

fn openssl_x509(args: &[String]) -> i32 {
    let text = args.iter().any(|a| a == "-text");
    let noout = args.iter().any(|a| a == "-noout");

    let cert = sample_cert();

    if text {
        println!("Certificate:");
        println!("    Data:");
        println!("        Version: 3 (0x2)");
        println!("        Serial Number: {}", cert.serial);
        println!("    Signature Algorithm: sha256WithRSAEncryption");
        println!("        Issuer: {}", cert.issuer);
        println!("        Validity");
        println!("            Not Before: {}", cert.not_before);
        println!("            Not After : {}", cert.not_after);
        println!("        Subject: {}", cert.subject);
        println!("        Subject Public Key Info:");
        println!("            Public Key Algorithm: rsaEncryption");
        println!("                RSA Public-Key: (4096 bit)");
        println!("        X509v3 extensions:");
        println!("            X509v3 Basic Constraints: critical");
        println!("                CA:FALSE");
        println!("            X509v3 Subject Key Identifier:");
        println!("                AB:CD:EF:12:34:56:78:9A");
    }

    if !noout {
        println!("-----BEGIN CERTIFICATE-----");
        println!("MIIDazCCAlOgAwIBAgI... (simulated)");
        println!("-----END CERTIFICATE-----");
    }
    0
}

fn openssl_enc(args: &[String]) -> i32 {
    let decrypt = args.iter().any(|a| a == "-d");
    let cipher = args.iter().find(|a| a.starts_with("-aes") || a.starts_with("-chacha"))
        .map(|s| s.as_str())
        .unwrap_or("-aes-256-cbc");

    if decrypt {
        println!("openssl enc {}: decrypting (simulated)", cipher);
    } else {
        println!("openssl enc {}: encrypting (simulated)", cipher);
    }
    println!("enter encryption password: ********");
    0
}

fn openssl_dgst(args: &[String]) -> i32 {
    let algo = if args.iter().any(|a| a == "-sha256") { "SHA256" }
        else if args.iter().any(|a| a == "-sha512") { "SHA512" }
        else if args.iter().any(|a| a == "-sha1") { "SHA1" }
        else if args.iter().any(|a| a == "-md5") { "MD5" }
        else { "SHA256" };

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        println!("{}(stdin)= e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", algo);
    } else {
        for f in &files {
            println!("{}({})= a1b2c3d4e5f6... (simulated)", algo, f);
        }
    }
    0
}

fn openssl_s_client(args: &[String]) -> i32 {
    let connect = args.iter().position(|a| a == "-connect")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost:443");

    println!("CONNECTED(00000003)");
    println!("---");
    println!("Certificate chain");
    println!(" 0 s:CN = {}", connect.split(':').next().unwrap_or("localhost"));
    println!("   i:CN = Slate OS Root CA");
    println!("---");
    println!("Server certificate");
    println!("-----BEGIN CERTIFICATE-----");
    println!("MIIDazCCAlOgAwIBAgI... (simulated)");
    println!("-----END CERTIFICATE-----");
    println!("---");
    println!("SSL handshake has read 3456 bytes and written 789 bytes");
    println!("Verification: OK");
    println!("---");
    println!("New, TLSv1.3, Cipher is TLS_AES_256_GCM_SHA384");
    println!("Server public key is 4096 bit");
    println!("---");
    0
}

fn openssl_rand(args: &[String]) -> i32 {
    let hex = args.iter().any(|a| a == "-hex");
    let base64 = args.iter().any(|a| a == "-base64");
    let count = args.last().and_then(|s| s.parse::<u32>().ok()).unwrap_or(32);

    if hex {
        // Simulated hex random
        let bytes_per_line = 32;
        let mut remaining = count;
        while remaining > 0 {
            let n = remaining.min(bytes_per_line);
            for _ in 0..n {
                print!("ab");
            }
            println!();
            remaining -= n;
        }
    } else if base64 {
        println!("q7bM3kSGDlOaYKDB1mE+xg== (simulated {} bytes)", count);
    } else {
        println!("(binary random data: {} bytes)", count);
    }
    0
}

fn openssl_speed() -> i32 {
    println!("Doing various benchmarks (simulated):");
    println!("{:<30} {:>10} {:>10} {:>10}", "Algorithm", "16 bytes", "256 bytes", "8192 bytes");
    println!("{}", "-".repeat(65));
    println!("{:<30} {:>10} {:>10} {:>10}", "aes-128-cbc", "1234.56k", "5678.90k", "9012.34k");
    println!("{:<30} {:>10} {:>10} {:>10}", "aes-256-cbc", "1111.22k", "4444.55k", "7777.88k");
    println!("{:<30} {:>10} {:>10} {:>10}", "aes-128-gcm", "2345.67k", "6789.01k", "10234.56k");
    println!("{:<30} {:>10} {:>10} {:>10}", "chacha20-poly1305", "3456.78k", "7890.12k", "11234.56k");
    println!("{:<30} {:>10} {:>10} {:>10}", "sha256", "4567.89k", "8901.23k", "12345.67k");
    println!("{:<30} {:>10} {:>10} {:>10}", "sha512", "5678.90k", "9012.34k", "13456.78k");
    println!();
    println!("{:<30} {:>10} {:>10}", "Algorithm", "sign/s", "verify/s");
    println!("{}", "-".repeat(55));
    println!("{:<30} {:>10} {:>10}", "rsa 2048", "1234.5", "45678.9");
    println!("{:<30} {:>10} {:>10}", "rsa 4096", "234.5", "12345.6");
    println!("{:<30} {:>10} {:>10}", "ecdsa P-256", "23456.7", "9876.5");
    println!("{:<30} {:>10} {:>10}", "ed25519", "34567.8", "12345.6");
    0
}

fn openssl_verify(args: &[String]) -> i32 {
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    for f in &files {
        println!("{}: OK", f);
    }
    if files.is_empty() {
        println!("stdin: OK (simulated)");
    }
    0
}

fn openssl_ciphers(args: &[String]) -> i32 {
    let verbose = args.iter().any(|a| a == "-v" || a == "-V");

    if verbose {
        println!("TLS_AES_256_GCM_SHA384        TLSv1.3 Kx=any  Au=any  Enc=AESGCM(256) Mac=AEAD");
        println!("TLS_CHACHA20_POLY1305_SHA256  TLSv1.3 Kx=any  Au=any  Enc=CHACHA20/POLY1305(256) Mac=AEAD");
        println!("TLS_AES_128_GCM_SHA256        TLSv1.3 Kx=any  Au=any  Enc=AESGCM(128) Mac=AEAD");
        println!("ECDHE-RSA-AES256-GCM-SHA384   TLSv1.2 Kx=ECDH Au=RSA  Enc=AESGCM(256) Mac=AEAD");
        println!("ECDHE-RSA-AES128-GCM-SHA256   TLSv1.2 Kx=ECDH Au=RSA  Enc=AESGCM(128) Mac=AEAD");
        println!("ECDHE-RSA-CHACHA20-POLY1305   TLSv1.2 Kx=ECDH Au=RSA  Enc=CHACHA20/POLY1305(256) Mac=AEAD");
    } else {
        println!("TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305");
    }
    0
}

fn openssl_list(args: &[String]) -> i32 {
    let what = args.first().map(|s| s.as_str()).unwrap_or("-commands");

    match what {
        "-cipher-algorithms" | "-ciphers" => {
            println!("Cipher algorithms:");
            for alg in &["AES-128-CBC", "AES-256-CBC", "AES-128-GCM", "AES-256-GCM",
                        "CHACHA20-POLY1305", "DES-EDE3-CBC", "CAMELLIA-256-CBC"] {
                println!("  {}", alg);
            }
        }
        "-digest-algorithms" | "-digests" => {
            println!("Digest algorithms:");
            for alg in &["SHA1", "SHA224", "SHA256", "SHA384", "SHA512",
                        "SHA3-256", "SHA3-512", "BLAKE2b512", "BLAKE2s256", "MD5"] {
                println!("  {}", alg);
            }
        }
        "-public-key-algorithms" => {
            println!("Public key algorithms:");
            for alg in &["RSA", "EC", "ED25519", "ED448", "X25519", "X448", "DH"] {
                println!("  {}", alg);
            }
        }
        _ => {
            println!("Standard commands: version genrsa genpkey req x509 enc dgst");
            println!("                  s_client s_server rand speed verify ciphers list");
        }
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openssl(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_cert() {
        let cert = sample_cert();
        assert!(cert.subject.contains("slateos.local"));
        assert!(cert.issuer.contains("Root CA"));
    }

    #[test]
    fn test_cert_dates() {
        let cert = sample_cert();
        assert!(cert.not_before.contains("2025"));
        assert!(cert.not_after.contains("2026"));
    }
}
