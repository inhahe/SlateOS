#![deny(clippy::all)]

//! proxmox-cli — OurOS Proxmox VE / Backup Server / Mail Gateway
//!
//! Single personality: `proxmox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pmx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: proxmox [OPTIONS]");
        println!("Proxmox VE 8.3 (OurOS) — Open-source virtualization platform");
        println!();
        println!("Options:");
        println!("  --ve                   Proxmox Virtual Environment (KVM + LXC + Ceph)");
        println!("  --bs                   Proxmox Backup Server (deduplication backup)");
        println!("  --mg                   Proxmox Mail Gateway (anti-spam/AV)");
        println!("  --datacenter           Multi-node cluster view");
        println!("  --pveam                pveam — appliance/template manager");
        println!("  --pct                  pct — LXC container CLI");
        println!("  --qm                   qm — QEMU/KVM VM CLI");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Proxmox Virtual Environment 8.3.1 / pve-manager 8.3.1 (OurOS)"); return 0; }
    println!("Proxmox VE 8.3.1 (OurOS)");
    println!("  Vendor: Proxmox Server Solutions GmbH (Vienna, Austria; founded 2005)");
    println!("  License: AGPL-3.0 (entire stack; no proprietary edition)");
    println!("  Base: Debian 12 (Bookworm) — full Debian under the hood");
    println!("  Kernel: Ubuntu LTS kernel (better ZFS + hardware support than Debian default)");
    println!("  Virtualization: KVM (full VMs) + LXC (Linux containers) in unified UI");
    println!("  Storage: ZFS, Ceph (built-in distributed), NFS, iSCSI, LVM, Gluster, dir, BTRFS");
    println!("  Clustering: built-in cluster (no extra license), Corosync for membership,");
    println!("              live migration, HA manager, fencing");
    println!("  Web UI: HTML5 (no Flash/Java), no separate vCenter equivalent needed");
    println!("  Networking: SDN preview, OVS, Linux bridge, VLANs, VXLAN, frr (FRRouting)");
    println!("  Backup: integrated VM backup, Proxmox Backup Server for dedup + verify");
    println!("  Subscription: no-sub (testing repo) free, Community $115/yr/CPU,");
    println!("                Basic $355/yr/CPU, Standard $545, Premium $1090 — for stable repo + support");
    println!("  Migration wave: huge influx from VMware after Broadcom 2024 price hikes");
    println!("  Strengths: zero-cost feature set, sane defaults, open standards (QEMU/KVM/LXC)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "proxmox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pmx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pmx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proxmox"), "proxmox");
        assert_eq!(basename(r"C:\bin\proxmox.exe"), "proxmox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proxmox.exe"), "proxmox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pmx(&["--help".to_string()], "proxmox"), 0);
        assert_eq!(run_pmx(&["-h".to_string()], "proxmox"), 0);
        let _ = run_pmx(&["--version".to_string()], "proxmox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pmx(&[], "proxmox");
    }
}
