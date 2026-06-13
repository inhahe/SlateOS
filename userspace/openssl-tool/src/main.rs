#![deny(clippy::all)]
//! openssl-tool — personality CLI for OpenSSL, the de-facto open-source
//! TLS / SSL / crypto toolkit and library.
//!
//! Forked from the original SSLeay (Eric A. Young and Tim J. Hudson,
//! Australia, started 1995). Renamed OpenSSL in late 1998 when Young
//! and Hudson moved to RSA Security and a new project home was set up.
//! The library powers an overwhelming majority of HTTPS endpoints —
//! it is the canonical implementation behind Apache, nginx, OpenSSH's
//! crypto, Python's `ssl`, Node's TLS, PHP, Ruby, Perl, and most
//! commercial appliances. The `openssl` CLI binary is the standard
//! tool for generating keys, signing certificates, debugging TLS,
//! and computing hashes.
//!
//! Famous incidents: Heartbleed (CVE-2014-0160, April 2014) exposed
//! private memory from any vulnerable server and drove the formation
//! of the Core Infrastructure Initiative. The Linux Foundation now
//! sponsors part of OpenSSL development through OpenSSL Software
//! Foundation. FIPS 140-2 / 140-3 validation is provided by the
//! OpenSSL FIPS Object Module — separately reviewed and certified.
//! Apache-2.0 licensed since OpenSSL 3.0 (Sept 2021), replacing the
//! historical dual SSLeay + OpenSSL license that was source of decades
//! of license-compatibility headaches.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — OpenSSL TLS/crypto-toolkit personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         SSLeay 1995 → OpenSSL 1998; the de-facto TLS toolkit");
    println!("    history       Young & Hudson, RSA move, OCI, OSF sponsorship");
    println!("    library       libcrypto + libssl architecture and major APIs");
    println!("    cli           `openssl` subcommands: genrsa, req, x509, s_client...");
    println!("    tls           SSL/TLS protocol versions, ciphers, ALPN, SNI");
    println!("    fips          FIPS 140-2 / 140-3 module and validated builds");
    println!("    heartbleed    CVE-2014-0160 — what it was, who it hit, fallout");
    println!("    license       SSLeay+OpenSSL dual license → Apache-2.0 in 3.0");
    println!("    versions      1.0.x → 1.1.x → 3.x → 3.x LTS series and EOL dates");
    println!("    alternatives  LibreSSL, BoringSSL, mbedTLS, wolfSSL, rustls");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() {
    println!("openssl-tool 0.1.0 (SlateOS personality CLI)");
}

fn run_about() {
    println!("OpenSSL — open-source TLS, SSL, and general-purpose crypto toolkit.");
    println!("  Origin:     SSLeay, Eric A. Young + Tim J. Hudson, AU, 1995.");
    println!("  Renamed:    OpenSSL late 1998 after Young/Hudson joined RSA Security.");
    println!("  License:    Apache-2.0 since 3.0 (2021); previously dual SSLeay+OpenSSL.");
    println!("  Composition: two libraries (libcrypto, libssl) + one CLI (`openssl`).");
    println!("  Ubiquity:    the dominant TLS implementation on Linux/BSD/embedded.");
    println!("               Apache httpd, nginx, OpenSSH (libcrypto), Python `ssl`,");
    println!("               Node `tls`, PHP `openssl`, Ruby, Perl, curl, wget, git,");
    println!("               and most commercial network appliances link against it.");
    println!("  Governance:  OpenSSL Software Foundation + OpenSSL Management Committee.");
}

fn run_history() {
    println!("Project history.");
    println!("  1995: SSLeay 0.5 — Young & Hudson begin a free SSL implementation");
    println!("        because the only options (RSA BSAFE, etc.) were commercial.");
    println!("  Aug 1998: Young & Hudson hired by RSA Security (Australia office).");
    println!("        SSLeay development by the originators halts.");
    println!("  Dec 1998: project re-founded as OpenSSL; Mark Cox, Ralf Engelschall,");
    println!("        Dr. Stephen Henson, Ben Laurie, Bodo Möller, Ulf Möller,");
    println!("        Holger Reif, Paul C. Sutton form the initial core team.");
    println!("  Apr 2014: Heartbleed disclosure (CVE-2014-0160) — single largest");
    println!("        reputational event in OpenSSL history.");
    println!("  May 2014: Core Infrastructure Initiative founded by the Linux");
    println!("        Foundation in direct response; OpenSSL receives sustained");
    println!("        funding for the first time. Two full-time developers hired.");
    println!("  Sep 2021: OpenSSL 3.0 — Apache-2.0 license relicense + provider arch.");
    println!("  Today:    development by the OpenSSL Software Foundation team.");
}

fn run_library() {
    println!("Library architecture.");
    println!("  libcrypto — primitive crypto: hashes (SHA-2/3, BLAKE2), HMAC,");
    println!("              symmetric ciphers (AES, ChaCha20-Poly1305), asymmetric");
    println!("              (RSA, DSA, ECDSA, EdDSA, ECDH, X25519, X448), KDFs,");
    println!("              ASN.1/DER, X.509 parsing, PEM, BIO abstract I/O.");
    println!("              May be linked without libssl by tools that just need crypto");
    println!("              (OpenSSH famously links libcrypto without libssl).");
    println!("  libssl    — SSL/TLS protocol implementation built on libcrypto.");
    println!("              SSL_CTX_new, SSL_new, SSL_connect/accept/read/write,");
    println!("              session resumption, ALPN, SNI, OCSP stapling, post-handshake");
    println!("              auth, TLS 1.3 0-RTT.");
    println!("  EVP envelope: high-level uniform API on top of the algorithm engines.");
    println!("  Providers (3.0+): plug-in crypto backends — default, legacy, fips.");
    println!("                    Replaces the older ENGINE API for HSM offload.");
    println!("  ENGINE API: still present for backwards compat; deprecated for new code.");
}

fn run_cli() {
    println!("`openssl` CLI cheat sheet — the canonical day-to-day subcommands.");
    println!("  Key gen:");
    println!("    openssl genrsa -out key.pem 4096");
    println!("    openssl genpkey -algorithm ED25519 -out key.pem");
    println!("    openssl ecparam -genkey -name prime256v1 -out ec.pem");
    println!("  CSRs and certs:");
    println!("    openssl req -new -key key.pem -out csr.pem");
    println!("    openssl x509 -req -in csr.pem -signkey key.pem -out cert.pem");
    println!("    openssl x509 -in cert.pem -text -noout       # human-readable dump");
    println!("  TLS debugging:");
    println!("    openssl s_client -connect host:443 -servername host");
    println!("    openssl s_server -cert cert.pem -key key.pem -accept 4433");
    println!("    openssl ciphers -v 'HIGH:!aNULL:!MD5'");
    println!("  Hashes / MACs / encoding:");
    println!("    openssl dgst -sha256 file");
    println!("    openssl mac -macopt key:abcd -in file -digest sha256 HMAC");
    println!("    openssl base64 -in file -out file.b64");
    println!("    openssl enc -aes-256-gcm -in file -out file.enc -pbkdf2");
    println!("  Random / time / formats:");
    println!("    openssl rand -hex 32");
    println!("    openssl pkcs12 -export -in cert.pem -inkey key.pem -out bundle.p12");
}

fn run_tls() {
    println!("TLS protocol coverage.");
    println!("  SSL 2.0/3.0 — removed (POODLE, DROWN).");
    println!("  TLS 1.0/1.1 — deprecated (RFC 8996, Mar 2021); still buildable but");
    println!("                disabled by default in modern distros.");
    println!("  TLS 1.2     — RFC 5246, the long-standing baseline.");
    println!("  TLS 1.3     — RFC 8446 (Aug 2018); supported since OpenSSL 1.1.1.");
    println!("  Cipher suites: full TLS 1.3 AEAD set (AES-128/256-GCM, ChaCha20-Poly1305).");
    println!("  Key exchange:  X25519, X448, secp256r1/384r1/521r1, DH/ECDHE.");
    println!("  Auth:          RSA, ECDSA, Ed25519, Ed448, RSA-PSS.");
    println!("  Extensions:    SNI, ALPN, OCSP, session tickets, 0-RTT, PHA, ESNI/ECH (3.2+).");
    println!("  Post-quantum:  hybrid kyber-X25519 KEM via OQS provider plug-in (3.x).");
}

fn run_fips() {
    println!("FIPS validation.");
    println!("  OpenSSL FIPS Object Module — a separately-built libcrypto subset");
    println!("  that has been validated by a NIST-accredited lab against");
    println!("  FIPS 140-2 (and now 140-3).");
    println!("  OpenSSL 1.0.2: classic FIPS Object Module 2.0 (long-running, end-of-validity 2026).");
    println!("  OpenSSL 3.0:   FIPS provider — validation cert #4282 (2022).");
    println!("  Activation:    /etc/ssl/openssl.cnf provider section + .fipsinstall config.");
    println!("                 Once enabled, only FIPS-approved algorithms are available;");
    println!("                 anything else returns EVP_R_OPERATION_NOT_SUPPORTED.");
    println!("  Customers:     US federal agencies (FedRAMP), DoD, regulated finance,");
    println!("                 and any vendor with a FIPS-mode customer requirement.");
}

fn run_heartbleed() {
    println!("Heartbleed — CVE-2014-0160, disclosed Apr 7 2014.");
    println!("  Bug:        a missing bounds check on the TLS heartbeat extension");
    println!("              (RFC 6520) let a remote attacker request the server to");
    println!("              echo up to 64 KiB of adjacent process memory per request.");
    println!("  Introduced: OpenSSL 1.0.1 (Mar 2012) by a single committer adding");
    println!("              heartbeat support; missed in review.");
    println!("  Impact:     private TLS keys, session cookies, passwords, and emails");
    println!("              leaked from a stunning fraction of the global HTTPS fleet.");
    println!("              Yahoo Mail, Akamai, Stripe, the Canadian tax authority,");
    println!("              the UK Mumsnet, OpenVPN gateways, network appliances — all");
    println!("              issued mass rekey + revocation campaigns.");
    println!("  Disclosure: Codenomicon (Finland) and a Google researcher independently");
    println!("              found it within days of each other.");
    println!("  Fallout:    Linux Foundation's Core Infrastructure Initiative created");
    println!("              to fund OpenSSL + other under-funded critical projects.");
    println!("              LibreSSL forked from OpenSSL by OpenBSD a week later.");
    println!("              BoringSSL forked by Google later in 2014.");
}

fn run_license() {
    println!("Licensing history.");
    println!("  Until 3.0:  dual license — original SSLeay license + the OpenSSL license.");
    println!("              Famous incompatibility with GPLv2 (advertising clause).");
    println!("              Distros worked around it via the system-library-exception");
    println!("              and one-off GPL+OpenSSL-exception clauses in linked apps.");
    println!("  3.0 (2021): full relicense to Apache-2.0. Required individually");
    println!("              re-contacting hundreds of historical contributors over");
    println!("              several years to obtain re-license consent. Among the");
    println!("              largest open-source relicensing efforts ever completed.");
    println!("  Effect:     OpenSSL 3.x is GPLv2-compatible; downstream apps no longer");
    println!("              need the GPL-exception clause when linking against libssl.");
}

fn run_versions() {
    println!("Major versions and support status.");
    println!("  1.0.0  (2010-03) — first 1.x; long EOL.");
    println!("  1.0.1  (2012-03) — TLS 1.1/1.2, the Heartbleed branch. EOL Dec 2016.");
    println!("  1.0.2  (2015-01) — long-lived; FIPS 2.0 module home; ext-support EOL 2020.");
    println!("  1.1.0  (2016-08) — opaque structs, deprecation of SSLv2/3, removed EGD.");
    println!("  1.1.1  (2018-09) — first stable TLS 1.3; EOL Sept 2023.");
    println!("  3.0.0  (2021-09) — Apache-2.0 license, provider arch, FIPS in tree.");
    println!("                     LTS (5-year support, until Sept 2026).");
    println!("  3.1.x  (2023-03) — provider/FIPS hardening, non-LTS.");
    println!("  3.2.x  (2023-11) — QUIC client (RFC 9000) support.");
    println!("  3.3.x  (2024-04) — QUIC client expansion + more.");
    println!("  3.4.x  (2024-10) — ML-KEM/ML-DSA introduction; non-LTS.");
    println!("  Cadence: feature releases ~6 months; LTS designated by the OMC.");
}

fn run_alternatives() {
    println!("Notable forks and competitors.");
    println!("  LibreSSL    — OpenBSD fork (Theo de Raadt et al.), Apr 2014, post-");
    println!("                Heartbleed. Default in OpenBSD; ports in others.");
    println!("                Aggressive code-removal + portable subset.");
    println!("  BoringSSL   — Google fork, 2014. Drives Chrome, Android, Cloudflare's");
    println!("                edge. No API/ABI stability guarantee — explicitly an");
    println!("                internal fork that others use at their own risk.");
    println!("  AWS-LC      — Amazon's fork of BoringSSL with FIPS validation,");
    println!("                drop-in libcrypto/libssl ABI for AL2/AL2023.");
    println!("  mbedTLS     — small-footprint TLS, originally PolarSSL, owned by Arm.");
    println!("                Popular for embedded + IoT; Apache-2.0.");
    println!("  wolfSSL     — embedded + FIPS-validated commercial TLS, GPL/commercial.");
    println!("  s2n        — Amazon's pure-TLS implementation (libcrypto-agnostic).");
    println!("  rustls      — pure-Rust TLS on `ring`/`aws-lc-rs` crypto, ISRG funded.");
    println!("                Used in curl (`--tls-backend rustls`), Hyper, Cloudflare Quiche.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "openssl-tool".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "history" => run_history(),
        "library" => run_library(),
        "cli" => run_cli(),
        "tls" => run_tls(),
        "fips" => run_fips(),
        "heartbleed" => run_heartbleed(),
        "license" => run_license(),
        "versions" => run_versions(),
        "alternatives" => run_alternatives(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs_all_subcommands() {
        run_about();
        run_history();
        run_library();
        run_cli();
        run_tls();
        run_fips();
        run_heartbleed();
        run_license();
        run_versions();
        run_alternatives();
    }

    #[test]
    fn help_and_version() {
        print_help("openssl-tool");
        print_version();
    }
}
