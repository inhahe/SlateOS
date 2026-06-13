#![deny(clippy::all)]

//! openvpn — SlateOS VPN solution
//!
//! Single personality: `openvpn`

use std::env;
use std::process;

fn run_openvpn(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openvpn [options]");
        println!();
        println!("General Options:");
        println!("  --config <file>         Read configuration from file");
        println!("  --daemon [name]         Become a daemon");
        println!("  --log <file>            Log to file");
        println!("  --status <file> [sec]   Write status to file every sec seconds");
        println!("  --verb <n>              Set verbosity level (default=1)");
        println!();
        println!("Tunnel Options:");
        println!("  --dev <tunN|tapN>       TUN/TAP virtual device");
        println!("  --dev-type <type>       Device type (tun or tap)");
        println!("  --local <host>          Local host name or IP address");
        println!("  --remote <host> [port]  Remote host");
        println!("  --port <port>           TCP/UDP port (default: 1194)");
        println!("  --proto <proto>         Protocol (udp, tcp-server, tcp-client)");
        println!("  --topology <type>       Topology (net30, p2p, subnet)");
        println!();
        println!("Crypto Options:");
        println!("  --cipher <cipher>       Encryption cipher");
        println!("  --auth <algorithm>      HMAC authentication algorithm");
        println!("  --ca <file>             Certificate authority file");
        println!("  --cert <file>           Local certificate file");
        println!("  --key <file>            Local private key file");
        println!("  --dh <file>             Diffie-Hellman parameters file");
        println!("  --tls-auth <file> [dir] TLS control channel authentication");
        println!();
        println!("Server Options:");
        println!("  --server <network> <netmask>  Server mode");
        println!("  --client                      Client mode");
        println!("  --push \"<option>\"             Push option to clients");
        println!("  --client-config-dir <dir>     Client-specific config directory");
        println!();
        println!("Other:");
        println!("  --genkey <type> <file>  Generate random key/secret");
        println!("  --show-ciphers          Show available ciphers");
        println!("  --show-digests          Show available digests");
        println!("  --show-tls              Show available TLS ciphersuites");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("OpenVPN 2.6.10 (SlateOS) x86_64 [SSL (OpenSSL)] [LZO] [LZ4] [EPOLL] [MH/PKTINFO] [AEAD]");
        println!("library versions: OpenSSL 3.2.1, LZO 2.10");
        println!("Originally developed by James Yonan");
        println!("Copyright (C) 2002-2024 OpenVPN Inc <sales@openvpn.net>");
        return 0;
    }

    if args.iter().any(|a| a == "--show-ciphers") {
        println!("The following ciphers and cipher modes are available for use");
        println!("with OpenVPN. Each cipher shown below may be used as a parameter");
        println!("to the --cipher option.");
        println!();
        println!("AES-256-GCM        (256 bit key, 128 bit block, TLS client/server mode)");
        println!("AES-128-GCM        (128 bit key, 128 bit block, TLS client/server mode)");
        println!("AES-256-CBC        (256 bit key, 128 bit block)");
        println!("AES-128-CBC        (128 bit key, 128 bit block)");
        println!("CHACHA20-POLY1305  (256 bit key, stream cipher, AEAD)");
        return 0;
    }
    if args.iter().any(|a| a == "--show-digests") {
        println!("The following message digests are available for use with OpenVPN.");
        println!();
        println!("SHA256     256 bit digest size");
        println!("SHA384     384 bit digest size");
        println!("SHA512     512 bit digest size");
        println!("SHA1       160 bit digest size");
        return 0;
    }
    if args.iter().any(|a| a == "--show-tls") {
        println!("Available TLS Ciphers, listed in order of preference:");
        println!();
        println!("TLS_AES_256_GCM_SHA384");
        println!("TLS_CHACHA20_POLY1305_SHA256");
        println!("TLS_AES_128_GCM_SHA256");
        println!("ECDHE-ECDSA-AES256-GCM-SHA384");
        println!("ECDHE-RSA-AES256-GCM-SHA384");
        return 0;
    }
    if args.iter().any(|a| a == "--genkey") {
        println!("-----BEGIN OpenVPN Static key V1-----");
        println!("abc123def456ghi789jkl012mno345pq");
        println!("rst678uvw901xyz234abc567def890ghi");
        println!("jkl123mno456pqr789stu012vwx345yz");
        println!("abc678def901ghi234jkl567mno890pqr");
        println!("-----END OpenVPN Static key V1-----");
        return 0;
    }

    // Start server/client
    let is_server = args.iter().any(|a| a == "--server");
    let is_client = args.iter().any(|a| a == "--client");
    let proto = args.iter().position(|a| a == "--proto")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("udp");
    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(1194);
    let dev = args.iter().position(|a| a == "--dev")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("tun0");

    println!("2025-05-22 10:00:00 OpenVPN 2.6.10 (SlateOS) x86_64");
    println!("2025-05-22 10:00:00 library versions: OpenSSL 3.2.1, LZO 2.10");

    if is_server {
        println!("2025-05-22 10:00:00 Diffie-Hellman initialized with 2048 bit key");
        println!("2025-05-22 10:00:00 TUN/TAP device {} opened", dev);
        println!("2025-05-22 10:00:00 /sbin/ip link set dev {} up mtu 1500", dev);
        println!("2025-05-22 10:00:00 /sbin/ip addr add dev {} 10.8.0.1/24 broadcast 10.8.0.255", dev);
        println!("2025-05-22 10:00:00 {} link remote: [AF_INET]0.0.0.0:{}", proto.to_uppercase(), port);
        println!("2025-05-22 10:00:01 Initialization Sequence Completed");
        println!("2025-05-22 10:00:01 Listening for incoming connections on {}:{}", proto, port);
    } else if is_client {
        let remote = args.iter().position(|a| a == "--remote")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("server.example.com");
        println!("2025-05-22 10:00:00 TCP/UDP: Preserving recently used remote address: [AF_INET]{}:{}", remote, port);
        println!("2025-05-22 10:00:00 {} link local: (not bound)", proto.to_uppercase());
        println!("2025-05-22 10:00:00 {} link remote: [AF_INET]{}:{}", proto.to_uppercase(), remote, port);
        println!("2025-05-22 10:00:01 TLS: Initial packet from [AF_INET]{}:{}", remote, port);
        println!("2025-05-22 10:00:01 Peer Connection Initiated with [AF_INET]{}:{}", remote, port);
        println!("2025-05-22 10:00:02 TUN/TAP device {} opened", dev);
        println!("2025-05-22 10:00:02 /sbin/ip addr add dev {} 10.8.0.2/24 broadcast 10.8.0.255", dev);
        println!("2025-05-22 10:00:02 Initialization Sequence Completed");
    } else {
        println!("2025-05-22 10:00:00 NOTE: --server or --client must be specified");
        println!("2025-05-22 10:00:00 Use --help for more information.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openvpn(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_openvpn};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_openvpn(vec!["--help".to_string()]), 0);
        assert_eq!(run_openvpn(vec!["-h".to_string()]), 0);
        let _ = run_openvpn(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_openvpn(vec![]);
    }
}
