#![deny(clippy::all)]

//! nutanix-cli — OurOS Nutanix Cloud Platform (HCI + AHV hypervisor)
//!
//! Single personality: `nutanix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ntnx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nutanix [OPTIONS]");
        println!("Nutanix Cloud Platform AOS 7.0 (OurOS) — HCI + AHV hypervisor + DBaaS");
        println!();
        println!("Options:");
        println!("  --prism                Prism (web UI + Prism Central multi-cluster)");
        println!("  --ahv                  AHV — Nutanix's own KVM-based hypervisor (free)");
        println!("  --files                Nutanix Files (NFS/SMB scale-out NAS)");
        println!("  --objects              Nutanix Objects (S3-compatible)");
        println!("  --era                  Nutanix Era/NDB (DBaaS)");
        println!("  --karbon               Nutanix Karbon Kubernetes Service");
        println!("  --cli                  acli/ncli command-line tools");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nutanix AOS 7.0 / AHV 20230302.1019 / Prism Central pc.2024.3 (OurOS)"); return 0; }
    println!("Nutanix AOS 7.0 (OurOS)");
    println!("  Vendor: Nutanix Inc. (San Jose, CA; founded 2009; NASDAQ:NTNX)");
    println!("  Founders: Dheeraj Pandey, Mohit Aron (also Cohesity founder), Ajeet Singh");
    println!("  Category creator: hyperconverged infrastructure (HCI) — compute + storage in one node");
    println!("  Architecture: Controller VM (CVM) on each node, distributed storage fabric (DSF)");
    println!("  AHV: Nutanix's own hypervisor (KVM-based), free with AOS — no extra license");
    println!("  Multi-hypervisor: also runs on VMware ESXi, Microsoft Hyper-V");
    println!("  Storage: distributed shared-nothing — no SAN/NAS needed");
    println!("  Software: Acropolis (clusterware), Prism (UI), Calm (automation), Flow (microseg),");
    println!("            Era (DBaaS), Files (NAS), Objects (S3), Move (migration), Karbon (K8s)");
    println!("  Editions: AOS Starter, Pro, Ultimate; per-CPU or per-VM");
    println!("  Cloud: Nutanix Cloud Clusters (NC2) — bare-metal AWS/Azure with Nutanix on top");
    println!("  Migration wave: significant VMware refugees post-Broadcom 2024");
    println!("  Strengths: simplicity (one-click upgrades), AHV included, hardware-agnostic");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nutanix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ntnx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ntnx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nutanix"), "nutanix");
        assert_eq!(basename(r"C:\bin\nutanix.exe"), "nutanix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nutanix.exe"), "nutanix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ntnx(&["--help".to_string()], "nutanix"), 0);
        assert_eq!(run_ntnx(&["-h".to_string()], "nutanix"), 0);
        assert_eq!(run_ntnx(&["--version".to_string()], "nutanix"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ntnx(&[], "nutanix"), 0);
    }
}
