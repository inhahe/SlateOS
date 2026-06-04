#![deny(clippy::all)]

//! virt-manager-cli — OurOS Virtual Machine Manager
//!
//! Single personality: `virt-manager`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_virt_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-manager [OPTIONS]");
        println!("virt-manager v4.1 (OurOS) — Virtual Machine Manager");
        println!();
        println!("Options:");
        println!("  -c URI          Connect to hypervisor URI");
        println!("  --show-domain-creator  Open new VM wizard");
        println!("  --show-domain-editor NAME  Open VM settings");
        println!("  --show-domain-console NAME Open VM console");
        println!("  --version       Show version");
        println!();
        println!("GUI for managing KVM/QEMU/libvirt virtual machines.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virt-manager v4.1 (OurOS)"); return 0; }
    println!("virt-manager: Virtual Machine Manager");
    println!("  Connection: qemu:///system");
    println!("  VMs: 3 defined (1 running, 2 shutoff)");
    println!("  Storage pools: 1 (default)");
    println!("  Networks: 1 (default NAT)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virt-manager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_virt_manager(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_virt_manager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virt-manager"), "virt-manager");
        assert_eq!(basename(r"C:\bin\virt-manager.exe"), "virt-manager.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virt-manager.exe"), "virt-manager");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_virt_manager(&["--help".to_string()], "virt-manager"), 0);
        assert_eq!(run_virt_manager(&["-h".to_string()], "virt-manager"), 0);
        let _ = run_virt_manager(&["--version".to_string()], "virt-manager");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_virt_manager(&[], "virt-manager");
    }
}
