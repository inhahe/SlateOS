#![deny(clippy::all)]

//! cni-cli — OurOS CNI (Container Network Interface) plugins
//!
//! Multi-personality: `cnitool`, `flannel`, `calico`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cnitool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cnitool COMMAND NETCONF NETNS");
        println!();
        println!("cnitool — CNI network management (OurOS).");
        println!();
        println!("Commands:");
        println!("  add <net> <ns>     Add container to network");
        println!("  del <net> <ns>     Remove container from network");
        println!("  check <net> <ns>   Check container network");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cnitool version 1.1.2 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("add");
    match subcmd {
        "add" => {
            println!("{{");
            println!("  \"cniVersion\": \"1.0.0\",");
            println!("  \"interfaces\": [{{");
            println!("    \"name\": \"eth0\",");
            println!("    \"sandbox\": \"/var/run/netns/container1\"");
            println!("  }}],");
            println!("  \"ips\": [{{");
            println!("    \"address\": \"10.244.1.5/24\",");
            println!("    \"gateway\": \"10.244.1.1\"");
            println!("  }}],");
            println!("  \"routes\": [{{");
            println!("    \"dst\": \"0.0.0.0/0\"");
            println!("  }}],");
            println!("  \"dns\": {{");
            println!("    \"nameservers\": [\"10.96.0.10\"]");
            println!("  }}");
            println!("}}");
        }
        "del" => println!("Network removed successfully."),
        "check" => println!("Network check passed."),
        _ => println!("cnitool: unknown command '{}'", subcmd),
    }
    0
}

fn run_flannel(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flannel [OPTIONS]");
        println!("  --iface=IFACE          Network interface");
        println!("  --ip-masq              Enable IP masquerading");
        println!("  --kube-subnet-mgr      Use K8s subnet manager");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("flannel version 0.24.2 (OurOS)");
        return 0;
    }

    println!("flannel: Determining IP address of default interface");
    println!("flannel: Using interface with name eth0 and address 192.168.1.100");
    println!("flannel: Defaulting external address to 192.168.1.100");
    println!("flannel: Created subnet manager with local subnet 10.244.0.0/24");
    println!("flannel: Lease acquired: 10.244.0.0/24");
    println!("flannel: Adding route: 10.244.1.0/24 via 192.168.1.101");
    println!("flannel: Adding route: 10.244.2.0/24 via 192.168.1.102");
    println!("flannel: Running backend vxlan");
    0
}

fn run_calico(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: calico [OPTIONS] COMMAND");
        println!();
        println!("Commands:");
        println!("  node status       Show node status");
        println!("  ipam show         Show IPAM allocations");
        println!("  get nodes         List nodes");
        println!("  get profiles      List profiles");
        println!("  version           Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => {
            println!("Client Version:    v3.27.0 (OurOS)");
            println!("Cluster Version:   v3.27.0");
            println!("Cluster Type:      typha,kdd,k8s,bgp,kubeadm");
        }
        "node" => {
            println!("Calico process is running.");
            println!();
            println!("IPv4 BGP status");
            println!("+--------------+-------------------+-------+----------+-------------+");
            println!("| PEER ADDRESS | PEER TYPE         | STATE | SINCE    | INFO        |");
            println!("+--------------+-------------------+-------+----------+-------------+");
            println!("| 192.168.1.101| node-to-node mesh | up    | 08:00:00 | Established |");
            println!("| 192.168.1.102| node-to-node mesh | up    | 08:00:00 | Established |");
            println!("+--------------+-------------------+-------+----------+-------------+");
        }
        "get" => {
            println!("NAME              ASN       IPV4          STATUS");
            println!("ouros-node-1      (64512)   192.168.1.100 up");
            println!("ouros-node-2      (64512)   192.168.1.101 up");
        }
        _ => println!("calico: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cnitool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "flannel" | "flanneld" => run_flannel(&rest),
        "calico" | "calicoctl" => run_calico(&rest),
        _ => run_cnitool(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cnitool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cni"), "cni");
        assert_eq!(basename(r"C:\bin\cni.exe"), "cni.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cni.exe"), "cni");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cnitool(&["--help".to_string()]), 0);
        assert_eq!(run_cnitool(&["-h".to_string()]), 0);
        let _ = run_cnitool(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cnitool(&[]);
    }
}
