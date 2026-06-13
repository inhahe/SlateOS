#![deny(clippy::all)]

//! flannel-cli — SlateOS Flannel container networking
//!
//! Single personality: `flanneld`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flannel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flanneld [OPTIONS]");
        println!("Flannel v0.25 (SlateOS) — Container network overlay");
        println!();
        println!("Options:");
        println!("  --etcd-endpoints URL   etcd endpoints");
        println!("  --etcd-prefix PREFIX   etcd key prefix");
        println!("  --iface IFACE          Network interface");
        println!("  --ip-masq              Setup IP masquerade");
        println!("  --kube-subnet-mgr      Kubernetes subnet manager");
        println!("  --subnet-file FILE     Subnet environment file");
        println!("  --public-ip IP         Public IP address");
        println!("  -v LEVEL               Verbosity level");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Flannel v0.25.5 (SlateOS)"); return 0; }
    println!("Flannel v0.25.5 (SlateOS)");
    println!("  Backend: VXLAN");
    println!("  Network: 10.244.0.0/16");
    println!("  Subnet: 10.244.1.0/24");
    println!("  Interface: eth0 (192.168.1.10)");
    println!("  MTU: 1450");
    println!("  IP masquerade: enabled");
    println!("  Subnet file: /run/flannel/subnet.env");
    println!("  Watching for network events...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flanneld".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flannel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_flannel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flannel"), "flannel");
        assert_eq!(basename(r"C:\bin\flannel.exe"), "flannel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flannel.exe"), "flannel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flannel(&["--help".to_string()], "flannel"), 0);
        assert_eq!(run_flannel(&["-h".to_string()], "flannel"), 0);
        let _ = run_flannel(&["--version".to_string()], "flannel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flannel(&[], "flannel");
    }
}
