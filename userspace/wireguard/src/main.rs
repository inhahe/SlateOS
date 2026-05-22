#![deny(clippy::all)]

//! wireguard — OurOS WireGuard VPN management
//!
//! Multi-personality binary for WireGuard VPN tunnels.
//! Detected via argv[0]:
//!
//! - `wg` (default) — WireGuard configuration tool
//! - `wg-quick` — quick WireGuard interface setup/teardown

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _WG_CONF_DIR: &str = "/etc/wireguard";
const _WG_RUN_DIR: &str = "/var/run/wireguard";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct WgInterface {
    name: String,
    private_key: String,
    public_key: String,
    listen_port: u16,
    _fwmark: Option<u32>,
    peers: Vec<WgPeer>,
}

#[derive(Clone, Debug)]
struct WgPeer {
    public_key: String,
    _preshared_key: Option<String>,
    endpoint: Option<String>,
    allowed_ips: Vec<String>,
    latest_handshake: u64,
    transfer_rx: u64,
    transfer_tx: u64,
    persistent_keepalive: Option<u16>,
}

#[derive(Clone, Debug)]
struct _WgQuickConfig {
    _interface: _WgQuickInterface,
    _peers: Vec<_WgQuickPeer>,
}

#[derive(Clone, Debug)]
struct _WgQuickInterface {
    _private_key: String,
    _listen_port: Option<u16>,
    _address: Vec<String>,
    _dns: Vec<String>,
    _mtu: Option<u32>,
    _table: Option<String>,
    _pre_up: Vec<String>,
    _post_up: Vec<String>,
    _pre_down: Vec<String>,
    _post_down: Vec<String>,
}

#[derive(Clone, Debug)]
struct _WgQuickPeer {
    _public_key: String,
    _preshared_key: Option<String>,
    _endpoint: Option<String>,
    _allowed_ips: Vec<String>,
    _persistent_keepalive: Option<u16>,
}

// ── Simulated data ────────────────────────────────────────────────────

fn simulated_interfaces() -> Vec<WgInterface> {
    vec![
        WgInterface {
            name: "wg0".to_string(),
            private_key: "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=".to_string(),
            public_key: "HIgo9xNzJMWLKASShiTqIybxR0V1tB1ZbFCP9d0RvEY=".to_string(),
            listen_port: 51820,
            _fwmark: None,
            peers: vec![
                WgPeer {
                    public_key: "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=".to_string(),
                    _preshared_key: None,
                    endpoint: Some("198.51.100.1:51820".to_string()),
                    allowed_ips: vec!["10.0.0.2/32".to_string(), "192.168.88.0/24".to_string()],
                    latest_handshake: 1716000000,
                    transfer_rx: 124_456_789,
                    transfer_tx: 98_765_432,
                    persistent_keepalive: Some(25),
                },
                WgPeer {
                    public_key: "TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2LM9F28=".to_string(),
                    _preshared_key: None,
                    endpoint: Some("203.0.113.50:51820".to_string()),
                    allowed_ips: vec!["10.0.0.3/32".to_string()],
                    latest_handshake: 1715999000,
                    transfer_rx: 56_789_012,
                    transfer_tx: 34_567_890,
                    persistent_keepalive: None,
                },
            ],
        },
    ]
}

fn format_transfer(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.2} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.2} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_handshake_age(ts: u64) -> String {
    if ts == 0 {
        return "never".to_string();
    }
    // Simulated "age" from a reference time
    let age_secs: u64 = 120; // pretend 2 minutes ago
    if age_secs < 60 {
        format!("{} seconds ago", age_secs)
    } else if age_secs < 3600 {
        format!("{} minute(s), {} second(s) ago", age_secs / 60, age_secs % 60)
    } else {
        format!("{} hour(s) ago", age_secs / 3600)
    }
}

// ── wg personality ────────────────────────────────────────────────────

fn run_wg(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "show".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: wg <command> [<args>]");
            println!();
            println!("WireGuard configuration utility.");
            println!();
            println!("Commands:");
            println!("  show [IFACE]          Show interface status (default)");
            println!("  showconf IFACE        Show configuration in wg format");
            println!("  set IFACE [OPTS]      Set interface configuration");
            println!("  setconf IFACE FILE    Set config from file");
            println!("  addconf IFACE FILE    Append config from file");
            println!("  syncconf IFACE FILE   Sync config from file (add new, remove old)");
            println!("  genkey                Generate private key");
            println!("  genpsk                Generate preshared key");
            println!("  pubkey                Derive public key from private");
            println!("  --version             Show version");
            0
        }
        "--version" | "version" => {
            println!("wireguard-tools 0.1.0 (OurOS)");
            0
        }
        "show" => wg_show(&cmd_args),
        "showconf" => wg_showconf(&cmd_args),
        "set" => wg_set(&cmd_args),
        "setconf" => wg_setconf(&cmd_args),
        "addconf" => wg_addconf(&cmd_args),
        "syncconf" => wg_syncconf(&cmd_args),
        "genkey" => wg_genkey(),
        "genpsk" => wg_genpsk(),
        "pubkey" => wg_pubkey(),
        other => {
            // Could be interface name for "wg show <iface>"
            let mut show_args = vec![other.to_string()];
            show_args.extend(cmd_args);
            wg_show(&show_args)
        }
    }
}

fn wg_show(args: &[String]) -> i32 {
    let iface_filter = args.first().map(|s| s.as_str());
    let interfaces = simulated_interfaces();

    for iface in &interfaces {
        if let Some(filter) = iface_filter {
            if filter != "all" && filter != iface.name {
                continue;
            }
        }

        println!("interface: {}", iface.name);
        println!("  public key: {}", iface.public_key);
        println!("  private key: (hidden)");
        println!("  listening port: {}", iface.listen_port);
        println!();

        for peer in &iface.peers {
            println!("peer: {}", peer.public_key);
            if let Some(ref ep) = peer.endpoint {
                println!("  endpoint: {}", ep);
            }
            println!("  allowed ips: {}", peer.allowed_ips.join(", "));
            if peer.latest_handshake > 0 {
                println!("  latest handshake: {}", format_handshake_age(peer.latest_handshake));
            }
            println!("  transfer: {} received, {} sent",
                format_transfer(peer.transfer_rx),
                format_transfer(peer.transfer_tx));
            if let Some(ka) = peer.persistent_keepalive {
                println!("  persistent keepalive: every {} seconds", ka);
            }
            println!();
        }
    }
    0
}

fn wg_showconf(args: &[String]) -> i32 {
    let iface_name = match args.first() {
        Some(name) => name.as_str(),
        None => {
            eprintln!("wg: showconf requires an interface name");
            return 1;
        }
    };

    let interfaces = simulated_interfaces();
    let iface = match interfaces.iter().find(|i| i.name == iface_name) {
        Some(i) => i,
        None => {
            eprintln!("wg: interface '{}' not found", iface_name);
            return 1;
        }
    };

    println!("[Interface]");
    println!("ListenPort = {}", iface.listen_port);
    println!("PrivateKey = {}", iface.private_key);
    println!();

    for peer in &iface.peers {
        println!("[Peer]");
        println!("PublicKey = {}", peer.public_key);
        if let Some(ref ep) = peer.endpoint {
            println!("Endpoint = {}", ep);
        }
        println!("AllowedIPs = {}", peer.allowed_ips.join(", "));
        if let Some(ka) = peer.persistent_keepalive {
            println!("PersistentKeepalive = {}", ka);
        }
        println!();
    }
    0
}

fn wg_set(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("wg: set requires an interface name and options");
        return 1;
    }
    println!("wg: set {} (simulated)", args.join(" "));
    println!("Configuration updated");
    0
}

fn wg_setconf(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("wg: setconf requires INTERFACE and FILE");
        return 1;
    }
    println!("wg: setconf {} from {} (simulated)", args[0], args[1]);
    println!("Configuration applied");
    0
}

fn wg_addconf(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("wg: addconf requires INTERFACE and FILE");
        return 1;
    }
    println!("wg: addconf {} from {} (simulated)", args[0], args[1]);
    println!("Configuration appended");
    0
}

fn wg_syncconf(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("wg: syncconf requires INTERFACE and FILE");
        return 1;
    }
    println!("wg: syncconf {} from {} (simulated)", args[0], args[1]);
    println!("Configuration synchronized");
    0
}

fn wg_genkey() -> i32 {
    // Simulated base64-encoded 256-bit key
    println!("oK56DE9Ue9zK76rAc8pBl6opph+1v36lm7cXXsQKrQM=");
    0
}

fn wg_genpsk() -> i32 {
    println!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    0
}

fn wg_pubkey() -> i32 {
    // Would read private key from stdin and derive public key
    println!("Cr07fjAbdhi7bz0KRM/kXSqpqI4MKGqRtqrNAEOj80U=");
    0
}

// ── wg-quick personality ──────────────────────────────────────────────

fn run_wg_quick(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: wg-quick <up|down|strip> <INTERFACE|CONFIG_FILE>");
            println!();
            println!("Quickly set up or tear down a WireGuard interface.");
            println!();
            println!("Commands:");
            println!("  up IFACE      Bring up a WireGuard interface");
            println!("  down IFACE    Bring down a WireGuard interface");
            println!("  strip IFACE   Strip wg-quick specific options from config");
            println!("  save IFACE    Save running config to file");
            0
        }
        "up" => wg_quick_up(&cmd_args),
        "down" => wg_quick_down(&cmd_args),
        "strip" => wg_quick_strip(&cmd_args),
        "save" => wg_quick_save(&cmd_args),
        other => {
            eprintln!("wg-quick: unknown command '{}'", other);
            1
        }
    }
}

fn wg_quick_up(args: &[String]) -> i32 {
    let iface = match args.first() {
        Some(name) => name.as_str(),
        None => {
            eprintln!("wg-quick: up requires an interface name");
            return 1;
        }
    };

    println!("[#] ip link add {} type wireguard", iface);
    println!("[#] wg setconf {} /dev/fd/63", iface);
    println!("[#] ip -4 address add 10.0.0.1/24 dev {}", iface);
    println!("[#] ip link set mtu 1420 up dev {}", iface);
    println!("[#] ip -4 route add 192.168.88.0/24 dev {}", iface);
    println!("[#] resolvconf -a {} -m 0 -x", iface);
    println!();
    println!("wg-quick: {} is up", iface);
    0
}

fn wg_quick_down(args: &[String]) -> i32 {
    let iface = match args.first() {
        Some(name) => name.as_str(),
        None => {
            eprintln!("wg-quick: down requires an interface name");
            return 1;
        }
    };

    println!("[#] wg showconf {}", iface);
    println!("[#] resolvconf -d {}", iface);
    println!("[#] ip -4 route delete 192.168.88.0/24 dev {}", iface);
    println!("[#] ip link delete dev {}", iface);
    println!();
    println!("wg-quick: {} is down", iface);
    0
}

fn wg_quick_strip(args: &[String]) -> i32 {
    let iface = match args.first() {
        Some(name) => name.as_str(),
        None => {
            eprintln!("wg-quick: strip requires an interface name");
            return 1;
        }
    };

    // Show config with wg-quick-specific options stripped
    println!("[Interface]");
    println!("ListenPort = 51820");
    println!("PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
    println!();
    println!("[Peer]");
    println!("PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=");
    println!("Endpoint = 198.51.100.1:51820");
    println!("AllowedIPs = 10.0.0.2/32, 192.168.88.0/24");
    println!("PersistentKeepalive = 25");
    let _ = iface; // used for config file lookup
    0
}

fn wg_quick_save(args: &[String]) -> i32 {
    let iface = match args.first() {
        Some(name) => name.as_str(),
        None => {
            eprintln!("wg-quick: save requires an interface name");
            return 1;
        }
    };

    println!("wg-quick: saving {} to {}/{}.conf (simulated)", iface, _WG_CONF_DIR, iface);
    println!("Configuration saved");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("wg");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "wg-quick" => run_wg_quick(rest),
        _ => run_wg(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_interfaces() {
        let ifaces = simulated_interfaces();
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name, "wg0");
        assert_eq!(ifaces[0].listen_port, 51820);
        assert_eq!(ifaces[0].peers.len(), 2);
    }

    #[test]
    fn test_peer_data() {
        let ifaces = simulated_interfaces();
        let peer = &ifaces[0].peers[0];
        assert!(peer.endpoint.is_some());
        assert!(!peer.allowed_ips.is_empty());
        assert!(peer.transfer_rx > 0);
        assert!(peer.persistent_keepalive.is_some());
    }

    #[test]
    fn test_format_transfer() {
        assert_eq!(format_transfer(500), "500 B");
        assert_eq!(format_transfer(1024), "1.00 KiB");
        assert_eq!(format_transfer(1_048_576), "1.00 MiB");
        assert_eq!(format_transfer(1_073_741_824), "1.00 GiB");
    }

    #[test]
    fn test_format_handshake_age() {
        assert_eq!(format_handshake_age(0), "never");
        let s = format_handshake_age(1716000000);
        assert!(s.contains("minute") || s.contains("second") || s.contains("hour"));
    }

    #[test]
    fn test_peer_without_keepalive() {
        let ifaces = simulated_interfaces();
        let peer = &ifaces[0].peers[1];
        assert!(peer.persistent_keepalive.is_none());
    }

    #[test]
    fn test_private_key_present() {
        let ifaces = simulated_interfaces();
        assert!(!ifaces[0].private_key.is_empty());
        assert!(!ifaces[0].public_key.is_empty());
    }
}
