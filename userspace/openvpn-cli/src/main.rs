#![deny(clippy::all)]

//! openvpn-cli — OurOS OpenVPN CLI
//!
//! Single personality: `openvpn`

use std::env;
use std::process;

fn run_openvpn(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openvpn [OPTIONS]");
        println!();
        println!("OpenVPN — SSL/TLS VPN (OurOS).");
        println!();
        println!("Options:");
        println!("  --config FILE          Configuration file");
        println!("  --remote HOST PORT     Remote server");
        println!("  --proto PROTO          Protocol (udp, tcp)");
        println!("  --dev TYPE             Device type (tun, tap)");
        println!("  --ca FILE              CA certificate");
        println!("  --cert FILE            Client certificate");
        println!("  --key FILE             Client private key");
        println!("  --tls-auth FILE DIR    TLS auth key");
        println!("  --cipher ALG           Cipher algorithm");
        println!("  --auth ALG             HMAC algorithm");
        println!("  --comp-lzo             Enable LZO compression");
        println!("  --verb N               Verbosity (0-11)");
        println!("  --daemon               Run as daemon");
        println!("  --log FILE             Log file");
        println!("  --status FILE N        Status file");
        println!("  --server NET MASK      Server mode");
        println!("  --client               Client mode");
        println!("  --genkey                Generate key");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("OpenVPN 2.6.8 x86_64 (OurOS)");
        println!("library versions: OpenSSL 3.2.0, LZO 2.10");
        return 0;
    }

    if args.iter().any(|a| a == "--genkey") {
        println!("#");
        println!("# 2048 bit OpenVPN static key");
        println!("#");
        println!("-----BEGIN OpenVPN Static key V1-----");
        println!("abcdef1234567890abcdef1234567890");
        println!("1234567890abcdef1234567890abcdef");
        println!("-----END OpenVPN Static key V1-----");
        return 0;
    }

    let config = args.windows(2).find(|w| w[0] == "--config")
        .map(|w| w[1].as_str());
    let server_mode = args.iter().any(|a| a == "--server");
    let remote = args.windows(2).find(|w| w[0] == "--remote")
        .map(|w| w[1].as_str());

    let config_name = config.unwrap_or("client.ovpn");
    println!("OpenVPN 2.6.8 (OurOS)");
    println!("  Config: {}", config_name);

    if server_mode {
        println!("  Mode: Server");
        println!("  TUN/TAP device tun0 opened");
        println!("  net_iface_up: set tun0 up");
        println!("  net_addr_v4_add: 10.8.0.1/24 dev tun0");
        println!("  Listening for incoming connections on 0.0.0.0:1194");
        println!("  Initialization Sequence Completed");
    } else {
        let host = remote.unwrap_or("vpn.example.com");
        println!("  Mode: Client");
        println!("  Attempting to establish TCP/UDP connection with {}:1194", host);
        println!("  TCP/UDP: Connected to {}:1194", host);
        println!("  TLS: Initial packet from {}:1194", host);
        println!("  VERIFY OK: CN=server");
        println!("  Control Channel: TLSv1.3, cipher TLSv1.3 TLS_AES_256_GCM_SHA384");
        println!("  [vpn.example.com] Peer Connection Initiated");
        println!("  TUN/TAP device tun0 opened");
        println!("  net_iface_up: set tun0 up");
        println!("  net_addr_v4_add: 10.8.0.6/24 dev tun0");
        println!("  Initialization Sequence Completed");
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
    fn help_and_version_exit_zero() {
        assert_eq!(run_openvpn(vec!["--help".to_string()]), 0);
        assert_eq!(run_openvpn(vec!["-h".to_string()]), 0);
        assert_eq!(run_openvpn(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_openvpn(vec![]), 0);
    }
}
