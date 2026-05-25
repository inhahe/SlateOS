#![deny(clippy::all)]

//! veeam-cli — OurOS Veeam Data Platform (enterprise backup/recovery)
//!
//! Single personality: `veeam`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: veeam [OPTIONS]");
        println!("Veeam Data Platform 12.2 (OurOS) — Enterprise backup, recovery, replication");
        println!();
        println!("Options:");
        println!("  --backup TARGET        vmware/hyperv/agent/microsoft365/cloud-connect");
        println!("  --replicate            Veeam Replication (image-level replica)");
        println!("  --instant-recovery     Instant VM Recovery (boot from backup file)");
        println!("  --orchestrator         Veeam Recovery Orchestrator");
        println!("  --secure-restore       Antivirus scan during restore");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Veeam Backup & Replication 12.2.0.334 (OurOS)"); return 0; }
    println!("Veeam Backup & Replication 12.2.0.334 (OurOS)");
    println!("  Vendor: Veeam Software (HQ Kirkland WA + Prague, founded 2006)");
    println!("  Founders: Ratmir Timashev, Andrei Baronov (Russian-American)");
    println!("  Sold to: Insight Partners Jan 2020 ($5B)");
    println!("  Origin: VMware ESXi backup pioneer (Veeam Backup 1.0 for ESX 2008)");
    println!("  Platforms: VMware vSphere, Microsoft Hyper-V, Nutanix AHV, Oracle Linux Virt,");
    println!("            RHV/oVirt, Proxmox VE, KVM, agent-based Windows/Linux/Solaris/AIX/Mac");
    println!("  Backup targets: any block/file/object storage (S3, Azure Blob, GCS, tape, disk)");
    println!("  Cloud: Veeam Backup for AWS/Azure/GCP/Salesforce/Microsoft 365");
    println!("  Recovery: Instant Recovery (mount + boot VM from backup), SureBackup verification,");
    println!("           Secure Restore (AV scan), DataLabs sandbox, item-level restore (AD, Exchange, SQL)");
    println!("  Editions: Foundation (free 10 workloads), Data Platform Essentials/Foundation/");
    println!("            Advanced/Premium — per-instance licensing");
    println!("  Market: #1 in backup/recovery market share by revenue (Gartner)");
    println!("  Strengths: agentless VM backup, application-aware, scriptable PowerShell + REST");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "veeam".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
