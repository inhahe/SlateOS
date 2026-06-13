#![deny(clippy::all)]

//! vmware-cli — SlateOS VMware Workstation Pro / Fusion / vSphere
//!
//! Single personality: `vmware`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vmw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vmware [OPTIONS]");
        println!("VMware Workstation Pro 17 (SlateOS) — Type-2 hypervisor (Linux/Win host)");
        println!();
        println!("Options:");
        println!("  --new                  Create new VM");
        println!("  --open VMX             Open .vmx file");
        println!("  --player               VMware Workstation Player (free, non-commercial)");
        println!("  --fusion               VMware Fusion (macOS host, Apple Silicon supported)");
        println!("  --esxi                 VMware ESXi (type-1 bare metal hypervisor)");
        println!("  --vcenter              vCenter Server (datacenter orchestration)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VMware Workstation 17.6.2 build-24409262 (SlateOS)"); return 0; }
    println!("VMware Workstation Pro 17.6.2 build-24409262 (SlateOS)");
    println!("  Vendor: VMware LLC (Palo Alto, CA), founded 1998");
    println!("  History: Acquired by Dell-EMC 2016; Broadcom acquired VMware Nov 2023 ($61B)");
    println!("  Broadcom era: drastic licensing changes — perpetual licenses sunset,");
    println!("                Workstation Pro/Fusion Pro made FREE for personal use (May 2024)");
    println!("  Products: Workstation Pro (Win/Linux), Fusion (macOS), Player, ESXi (bare metal),");
    println!("            vSphere, vCenter, NSX (SDN), vSAN (HCI storage), Aria (cloud mgmt),");
    println!("            Horizon (VDI), Tanzu (Kubernetes)");
    println!("  Engines: ESXi (proprietary VMkernel), Workstation/Fusion (paravirt + hardware HVM)");
    println!("  Formats: .vmx (VM config), .vmdk (disk), .ovf/.ova (open virt format)");
    println!("  Strengths: enterprise hypervisor king, mature ecosystem, hardware compat");
    println!("  Concern: Broadcom price hikes triggered Proxmox/Nutanix migration wave 2024");
    println!("  Differentiator: snapshots, linked clones, Unity (seamless app integration)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vmware".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vmw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vmw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vmware"), "vmware");
        assert_eq!(basename(r"C:\bin\vmware.exe"), "vmware.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vmware.exe"), "vmware");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vmw(&["--help".to_string()], "vmware"), 0);
        assert_eq!(run_vmw(&["-h".to_string()], "vmware"), 0);
        let _ = run_vmw(&["--version".to_string()], "vmware");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vmw(&[], "vmware");
    }
}
