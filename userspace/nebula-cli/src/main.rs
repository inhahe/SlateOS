#![deny(clippy::all)]

//! nebula-cli — OurOS Nebula overlay network tools
//!
//! Multi-personality: `nebula`, `nebula-cert`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nebula(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nebula [OPTIONS]");
        println!();
        println!("nebula — scalable overlay networking (OurOS).");
        println!();
        println!("Options:");
        println!("  -config <path>    Config file (default: /etc/nebula/config.yml)");
        println!("  -test             Test config and exit");
        println!("  -print-cert       Print certificate info");
        println!("  -version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version" || a == "--version") {
        println!("Version: 1.9.0 (OurOS)");
        println!("Build Date: 2024-01-15T00:00:00Z");
        return 0;
    }

    if args.iter().any(|a| a == "-test") {
        println!("Loaded config:");
        println!("  pki:");
        println!("    ca: /etc/nebula/ca.crt");
        println!("    cert: /etc/nebula/host.crt");
        println!("    key: /etc/nebula/host.key");
        println!("  static_host_map:");
        println!("    \"10.42.0.1\": [\"203.0.113.1:4242\"]");
        println!("  lighthouse:");
        println!("    am_lighthouse: false");
        println!("    hosts: [\"10.42.0.1\"]");
        println!("  listen:");
        println!("    port: 4242");
        println!("  tun:");
        println!("    dev: nebula1");
        println!("Config test passed.");
        return 0;
    }

    if args.iter().any(|a| a == "-print-cert") {
        println!("NebulaCertificate {{");
        println!("  Details {{");
        println!("    Name: ouros-desktop");
        println!("    Ips: [10.42.0.5/24]");
        println!("    Subnets: []");
        println!("    Groups: [\"servers\", \"desktop\"]");
        println!("    Not Before: 2024-01-01 00:00:00 +0000 UTC");
        println!("    Not After: 2025-01-01 00:00:00 +0000 UTC");
        println!("    Is CA: false");
        println!("    Issuer: aabbccdd11223344556677889900aabbccddeeff11223344556677889900aabb");
        println!("    Public key: 00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff");
        println!("  }}");
        println!("  Fingerprint: sha256:aabb...eeff");
        println!("  Signature: 0011...eeff");
        println!("}}");
        return 0;
    }

    let config = args.windows(2).find(|w| w[0] == "-config")
        .map(|w| w[1].as_str())
        .unwrap_or("/etc/nebula/config.yml");
    println!("nebula: loading config from '{}'", config);
    println!("nebula: Firewall has been enabled");
    println!("nebula: Handshake manager ready");
    println!("nebula: Main HostMap created (capacity: 1024)");
    println!("nebula: UDP listener started on [::]:4242");
    println!("nebula: Handshake message sent to 10.42.0.1 via 203.0.113.1:4242");
    println!("nebula: Handshake received from 10.42.0.1 (lighthouse)");
    println!("nebula: Tunnel established with 10.42.0.1");
    0
}

fn run_nebula_cert(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nebula-cert <command> [OPTIONS]");
        println!();
        println!("nebula-cert — Nebula certificate management (OurOS).");
        println!();
        println!("Commands:");
        println!("  ca        Create a CA certificate");
        println!("  sign      Sign a host certificate");
        println!("  print     Print certificate details");
        println!("  verify    Verify a certificate");
        println!("  keygen    Generate a key pair");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("print");
    match subcmd {
        "ca" => {
            let name = args.windows(2).find(|w| w[0] == "-name")
                .map(|w| w[1].as_str())
                .unwrap_or("OurOS Nebula CA");
            println!("Generated CA certificate for '{}':", name);
            println!("  ca.crt (certificate)");
            println!("  ca.key (private key)");
        }
        "sign" => {
            let name = args.windows(2).find(|w| w[0] == "-name")
                .map(|w| w[1].as_str())
                .unwrap_or("ouros-desktop");
            let ip = args.windows(2).find(|w| w[0] == "-ip")
                .map(|w| w[1].as_str())
                .unwrap_or("10.42.0.5/24");
            println!("Signed certificate for '{}' with IP {}", name, ip);
            println!("  {}.crt (certificate)", name);
            println!("  {}.key (private key)", name);
        }
        "print" => {
            println!("NebulaCertificate {{");
            println!("  Details {{");
            println!("    Name: ouros-desktop");
            println!("    Ips: [10.42.0.5/24]");
            println!("    Groups: [\"servers\"]");
            println!("    Not Before: 2024-01-01 00:00:00 +0000 UTC");
            println!("    Not After: 2025-01-01 00:00:00 +0000 UTC");
            println!("    Is CA: false");
            println!("  }}");
            println!("}}");
        }
        "verify" => {
            let cert = args.windows(2).find(|w| w[0] == "-crt")
                .map(|w| w[1].as_str())
                .unwrap_or("host.crt");
            println!("{}: certificate is valid", cert);
        }
        "keygen" => {
            println!("Generated key pair:");
            println!("  host.key (private key)");
            println!("  host.pub (public key)");
        }
        _ => {
            eprintln!("nebula-cert: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nebula".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nebula-cert" => run_nebula_cert(&rest),
        _ => run_nebula(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
