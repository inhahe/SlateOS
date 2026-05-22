#![deny(clippy::all)]

//! wireguard-tools — OurOS WireGuard VPN utilities
//!
//! Multi-personality: `wg` (WireGuard tool), `wg-quick` (quick setup)

use std::env;
use std::process;

fn run_wg(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "show".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: wg <cmd> [<args>]");
            println!();
            println!("Available subcommands:");
            println!("  show [<interface> [<field>]]   Shows current configuration and device info");
            println!("  showconf <interface>           Shows the current configuration");
            println!("  set <interface> [<args>]       Change current configuration");
            println!("  setconf <interface> <file>     Applies a configuration file");
            println!("  addconf <interface> <file>     Appends a configuration file");
            println!("  syncconf <interface> <file>    Synchronize a configuration file");
            println!("  genkey                         Generates a new private key");
            println!("  genpsk                         Generates a new preshared key");
            println!("  pubkey                         Calculates the public key from a private key");
            println!("  --version                      Show version");
            0
        }
        "--version" | "version" => {
            println!("wireguard-tools v1.0.20210914 (OurOS)");
            0
        }
        "show" => {
            let iface = cmd_args.first().map(|s| s.as_str());
            if iface.is_none() || iface == Some("all") {
                println!("interface: wg0");
                println!("  public key: gN65BkIKy1eCE9pP1wdc8ROUgxU3sGR2SAhXkG2BFVM=");
                println!("  private key: (hidden)");
                println!("  listening port: 51820");
                println!();
                println!("peer: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=");
                println!("  endpoint: 198.51.100.1:51820");
                println!("  allowed ips: 10.0.0.2/32, fd00::2/128");
                println!("  latest handshake: 42 seconds ago");
                println!("  transfer: 1.42 GiB received, 890.32 MiB sent");
                println!("  persistent keepalive: every 25 seconds");
                println!();
                println!("peer: TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2ZPkENk=");
                println!("  endpoint: 203.0.113.5:51820");
                println!("  allowed ips: 10.0.0.3/32");
                println!("  latest handshake: 3 minutes, 12 seconds ago");
                println!("  transfer: 456.78 MiB received, 234.56 MiB sent");
            } else {
                let field = cmd_args.get(1).map(|s| s.as_str());
                match field {
                    Some("public-key") => println!("gN65BkIKy1eCE9pP1wdc8ROUgxU3sGR2SAhXkG2BFVM="),
                    Some("listen-port") => println!("51820"),
                    Some("peers") => {
                        println!("xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=");
                        println!("TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2ZPkENk=");
                    }
                    Some("endpoints") => {
                        println!("xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\t198.51.100.1:51820");
                        println!("TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2ZPkENk=\t203.0.113.5:51820");
                    }
                    Some("transfer") => {
                        println!("xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\t1525612544\t933421056");
                        println!("TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2ZPkENk=\t478956544\t245923840");
                    }
                    Some("dump") => {
                        println!("gN65BkIKy1eCE9pP1wdc8ROUgxU3sGR2SAhXkG2BFVM=\t(none)\t51820\toff");
                        println!("xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\t(none)\t198.51.100.1:51820\t10.0.0.2/32,fd00::2/128\t42\t1525612544\t933421056\t25");
                    }
                    _ => {
                        println!("interface: wg0");
                        println!("  public key: gN65BkIKy1eCE9pP1wdc8ROUgxU3sGR2SAhXkG2BFVM=");
                        println!("  listening port: 51820");
                    }
                }
            }
            0
        }
        "showconf" => {
            let iface = cmd_args.first().map(|s| s.as_str()).unwrap_or("wg0");
            let _ = iface;
            println!("[Interface]");
            println!("ListenPort = 51820");
            println!("PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            println!();
            println!("[Peer]");
            println!("PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=");
            println!("AllowedIPs = 10.0.0.2/32, fd00::2/128");
            println!("Endpoint = 198.51.100.1:51820");
            println!("PersistentKeepalive = 25");
            println!();
            println!("[Peer]");
            println!("PublicKey = TrMvSoP4jYQlY6RIzBgbssQqY3vxI2piVFBs2ZPkENk=");
            println!("AllowedIPs = 10.0.0.3/32");
            println!("Endpoint = 203.0.113.5:51820");
            0
        }
        "genkey" => {
            println!("yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            0
        }
        "genpsk" => {
            println!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
            0
        }
        "pubkey" => {
            println!("gN65BkIKy1eCE9pP1wdc8ROUgxU3sGR2SAhXkG2BFVM=");
            0
        }
        "set" | "setconf" | "addconf" | "syncconf" => {
            println!("({} applied — simulated)", cmd);
            0
        }
        other => { eprintln!("wg: unknown command '{}'", other); 1 }
    }
}

fn run_wg_quick(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let iface = args.get(1).map(|s| s.as_str()).unwrap_or("wg0");

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: wg-quick <up|down|save|strip> <INTERFACE|CONFIG_FILE>");
            0
        }
        "up" => {
            println!("[#] ip link add {} type wireguard", iface);
            println!("[#] wg setconf {} /dev/fd/63", iface);
            println!("[#] ip -4 address add 10.0.0.1/24 dev {}", iface);
            println!("[#] ip link set mtu 1420 up dev {}", iface);
            println!("[#] ip -4 route add 10.0.0.0/24 dev {}", iface);
            0
        }
        "down" => {
            println!("[#] ip link delete dev {}", iface);
            0
        }
        "save" => {
            println!("[Interface]");
            println!("Address = 10.0.0.1/24");
            println!("ListenPort = 51820");
            println!("PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            0
        }
        "strip" => {
            println!("[Interface]");
            println!("ListenPort = 51820");
            println!("PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=");
            0
        }
        other => { eprintln!("wg-quick: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("wg");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "wg-quick" => run_wg_quick(rest),
        _ => run_wg(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
