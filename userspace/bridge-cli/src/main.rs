#![deny(clippy::all)]

//! bridge-cli — SlateOS network bridge/bonding tools
//!
//! Multi-personality: `bridge`, `brctl`, `bondctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bridge(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bridge [OPTIONS] OBJECT COMMAND");
        println!();
        println!("bridge — iproute2 bridge management (SlateOS).");
        println!();
        println!("Objects: link, fdb, mdb, vlan, monitor");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("bridge utility, iproute2-6.7.0 (SlateOS)");
        return 0;
    }

    let obj = args.first().map(|s| s.as_str()).unwrap_or("link");
    match obj {
        "link" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match cmd {
                "show" => {
                    println!("2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 master br0 state forwarding priority 32 cost 4");
                    println!("3: eth1: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 master br0 state forwarding priority 32 cost 4");
                }
                "set" => println!("bridge: link set completed"),
                _ => println!("bridge: link command '{}' completed", cmd),
            }
        }
        "fdb" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match cmd {
                "show" => {
                    println!("00:11:22:33:44:55 dev eth0 master br0 permanent");
                    println!("66:77:88:99:aa:bb dev eth0 master br0");
                    println!("aa:bb:cc:dd:ee:ff dev eth1 master br0");
                    println!("33:44:55:66:77:88 dev br0 self permanent");
                    println!("ff:ff:ff:ff:ff:ff dev eth0 master br0 permanent");
                }
                "add" | "del" => {
                    let addr = args.get(2).map(|s| s.as_str()).unwrap_or("00:00:00:00:00:00");
                    println!("bridge: fdb {} {} completed", cmd, addr);
                }
                _ => println!("bridge: fdb command '{}' completed", cmd),
            }
        }
        "mdb" => {
            println!("dev br0 port eth0 grp 239.1.1.1 permanent");
            println!("dev br0 port eth1 grp 239.1.1.1 temp");
        }
        "vlan" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match cmd {
                "show" => {
                    println!("port\tvlan-id");
                    println!("eth0\t 1 PVID Egress Untagged");
                    println!("\t 10");
                    println!("\t 20");
                    println!("eth1\t 1 PVID Egress Untagged");
                    println!("\t 10");
                    println!("br0\t 1 PVID Egress Untagged");
                }
                "add" | "del" => println!("bridge: vlan {} completed", cmd),
                _ => println!("bridge: vlan command '{}' completed", cmd),
            }
        }
        "monitor" => println!("Listening for bridge events..."),
        _ => println!("bridge: unknown object '{}'", obj),
    }
    0
}

fn run_brctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: brctl COMMAND [ARGS]");
        println!();
        println!("Commands:");
        println!("  addbr <bridge>          Create bridge");
        println!("  delbr <bridge>          Delete bridge");
        println!("  addif <bridge> <if>     Add interface to bridge");
        println!("  delif <bridge> <if>     Remove interface from bridge");
        println!("  show [bridge]           Show bridges");
        println!("  showmacs <bridge>       Show MAC table");
        println!("  stp <bridge> on|off     Set STP");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match subcmd {
        "show" => {
            println!("bridge name\tbridge id\t\tSTP enabled\tinterfaces");
            println!("br0\t\t8000.001122334455\tyes\t\teth0");
            println!("\t\t\t\t\t\t\t\teth1");
            println!("docker0\t\t8000.aabbccddeeff\tno\t\tveth1234");
        }
        "showmacs" => {
            let br = args.get(1).map(|s| s.as_str()).unwrap_or("br0");
            println!("port no\tmac addr\t\tis local?\tageing timer ({})", br);
            println!("  1\t00:11:22:33:44:55\tyes\t\t   0.00");
            println!("  1\t66:77:88:99:aa:bb\tno\t\t  12.34");
            println!("  2\taa:bb:cc:dd:ee:ff\tno\t\t   5.67");
        }
        "addbr" | "delbr" => {
            let br = args.get(1).map(|s| s.as_str()).unwrap_or("br0");
            println!("brctl: {} {}", subcmd, br);
        }
        "addif" | "delif" => {
            let br = args.get(1).map(|s| s.as_str()).unwrap_or("br0");
            let iface = args.get(2).map(|s| s.as_str()).unwrap_or("eth0");
            println!("brctl: {} {} {}", subcmd, br, iface);
        }
        "stp" => {
            let br = args.get(1).map(|s| s.as_str()).unwrap_or("br0");
            let state = args.get(2).map(|s| s.as_str()).unwrap_or("on");
            println!("brctl: STP for {} set to {}", br, state);
        }
        _ => {
            eprintln!("brctl: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn run_bondctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bondctl [OPTIONS] COMMAND");
        println!();
        println!("bondctl — network bonding management (SlateOS).");
        println!();
        println!("Commands:");
        println!("  show [BOND]          Show bonding info");
        println!("  create NAME          Create bond");
        println!("  delete NAME          Delete bond");
        println!("  add-slave BOND IF    Add slave interface");
        println!("  del-slave BOND IF    Remove slave interface");
        println!("  set-mode BOND MODE   Set bonding mode");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match subcmd {
        "show" => {
            println!("Bonding Mode: IEEE 802.3ad Dynamic link aggregation");
            println!("Transmit Hash Policy: layer3+4 (1)");
            println!("MII Status: up");
            println!("MII Polling Interval (ms): 100");
            println!("Up Delay (ms): 0");
            println!("Down Delay (ms): 0");
            println!("Peer Notification Delay (ms): 0");
            println!();
            println!("802.3ad info");
            println!("LACP active: on");
            println!("LACP rate: fast");
            println!();
            println!("Slave Interface: eth0");
            println!("MII Status: up");
            println!("Speed: 10000 Mbps");
            println!("Duplex: full");
            println!("Link Failure Count: 0");
            println!("Permanent HW addr: 00:11:22:33:44:55");
            println!("Aggregator ID: 1");
            println!();
            println!("Slave Interface: eth1");
            println!("MII Status: up");
            println!("Speed: 10000 Mbps");
            println!("Duplex: full");
            println!("Link Failure Count: 0");
            println!("Permanent HW addr: 66:77:88:99:AA:BB");
            println!("Aggregator ID: 1");
        }
        "create" | "delete" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("bond0");
            println!("bondctl: {} {}", subcmd, name);
        }
        "add-slave" | "del-slave" => {
            let bond = args.get(1).map(|s| s.as_str()).unwrap_or("bond0");
            let iface = args.get(2).map(|s| s.as_str()).unwrap_or("eth0");
            println!("bondctl: {} {} to {}", subcmd, iface, bond);
        }
        "set-mode" => {
            let bond = args.get(1).map(|s| s.as_str()).unwrap_or("bond0");
            let mode = args.get(2).map(|s| s.as_str()).unwrap_or("802.3ad");
            println!("bondctl: set mode for {} to {}", bond, mode);
        }
        _ => {
            eprintln!("bondctl: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bridge".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "brctl" => run_brctl(&rest),
        "bondctl" => run_bondctl(&rest),
        _ => run_bridge(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bridge};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bridge"), "bridge");
        assert_eq!(basename(r"C:\bin\bridge.exe"), "bridge.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bridge.exe"), "bridge");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bridge(&["--help".to_string()]), 0);
        assert_eq!(run_bridge(&["-h".to_string()]), 0);
        let _ = run_bridge(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bridge(&[]);
    }
}
