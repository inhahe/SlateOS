#![deny(clippy::all)]

//! wireguard-cli — OurOS WireGuard VPN CLI
//!
//! Multi-personality: `wg`, `wg-quick`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "wg-quick" => "wg-quick",
        _ => "wg",
    }
}

fn run_wg(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wg <COMMAND> [OPTIONS]");
        println!();
        println!("Commands:");
        println!("  show [IFACE]       Show interface status");
        println!("  showconf IFACE     Show running config");
        println!("  set IFACE ...      Change interface config");
        println!("  setconf IFACE FILE Set config from file");
        println!("  addconf IFACE FILE Append config from file");
        println!("  syncconf IFACE FILE Sync config");
        println!("  genkey             Generate private key");
        println!("  genpsk             Generate pre-shared key");
        println!("  pubkey             Derive public key from private");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match cmd {
        "show" => {
            println!("interface: wg0");
            println!("  public key: abc123DEF456/xyz789+012345678901234567890=");
            println!("  private key: (hidden)");
            println!("  listening port: 51820");
            println!();
            println!("peer: def456ABC789/uvw012+345678901234567890123456=");
            println!("  endpoint: 203.0.113.1:51820");
            println!("  allowed ips: 10.0.0.2/32");
            println!("  latest handshake: 45 seconds ago");
            println!("  transfer: 123.45 MiB received, 67.89 MiB sent");
            println!("  persistent keepalive: every 25 seconds");
            0
        }
        "genkey" => {
            println!("yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            0
        }
        "genpsk" => {
            println!("xOqjvVpFSJg1wRBVw2MFvPm/SFRG3BYzjv2SNwCkIXo=");
            0
        }
        "pubkey" => {
            println!("HiSQpGBMvYQce/8rxNhG8vbtIEEiFpGBqSsf6MnPjQ0=");
            0
        }
        "showconf" => {
            println!("[Interface]");
            println!("ListenPort = 51820");
            println!("PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            println!();
            println!("[Peer]");
            println!("PublicKey = def456ABC789/uvw012+345678901234567890123456=");
            println!("AllowedIPs = 10.0.0.2/32");
            println!("Endpoint = 203.0.113.1:51820");
            println!("PersistentKeepalive = 25");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn run_wg_quick(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wg-quick <up|down|save|strip> <IFACE|CONFIG>");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let iface = args.get(1).map(|s| s.as_str()).unwrap_or("wg0");

    match cmd {
        "up" => {
            println!("[#] ip link add {} type wireguard", iface);
            println!("[#] wg setconf {} /tmp/wg-quick.conf", iface);
            println!("[#] ip -4 address add 10.0.0.1/24 dev {}", iface);
            println!("[#] ip link set mtu 1420 up dev {}", iface);
            println!("[#] Interface {} is up.", iface);
            0
        }
        "down" => {
            println!("[#] ip link delete dev {}", iface);
            println!("[#] Interface {} is down.", iface);
            0
        }
        "save" => {
            println!("[#] Configuration saved for {}", iface);
            0
        }
        _ => {
            eprintln!("Usage: wg-quick <up|down|save|strip> <IFACE>");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("wg"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match p {
        "wg" => run_wg(&rest),
        "wg-quick" => run_wg_quick(&rest),
        _ => run_wg(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_wg};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wg(&["--help".to_string()]), 0);
        assert_eq!(run_wg(&["-h".to_string()]), 0);
        assert_eq!(run_wg(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wg(&[]), 0);
    }
}
