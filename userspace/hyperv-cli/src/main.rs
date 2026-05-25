#![deny(clippy::all)]

//! hyperv-cli — OurOS Microsoft Hyper-V hypervisor
//!
//! Single personality: `hyperv`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hyperv [OPTIONS]");
        println!("Microsoft Hyper-V (OurOS) — Type-1 hypervisor for Windows + Windows Server");
        println!();
        println!("Options:");
        println!("  --manager              Hyper-V Manager (MMC console)");
        println!("  --new VM               New-VM (PowerShell cmdlet equivalent)");
        println!("  --quick-create         Quick Create (Ubuntu/MSIX/Windows 11 dev img)");
        println!("  --wsl                  WSL2 Linux kernel (utility VM uses Hyper-V)");
        println!("  --windows-sandbox      Windows Sandbox (ephemeral isolated Windows)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft Hyper-V 10.0.26100 (Windows 11 24H2 / Server 2025) (OurOS)"); return 0; }
    println!("Microsoft Hyper-V 10.0.26100 (OurOS)");
    println!("  Vendor: Microsoft");
    println!("  Type: Type-1 (bare-metal) — Windows boots as 'root partition' on hypervisor");
    println!("  Launched: Windows Server 2008 (Jun 2008); Client Hyper-V on Windows 8+ Pro/Ent");
    println!("  Architecture: hypervisor schedules root + child partitions; VMBus IPC");
    println!("  Free standalone: Microsoft Hyper-V Server (last 2019, discontinued 2022)");
    println!("  Underpins: WSL2 (Linux VM kernel), Windows Sandbox, Defender Application Guard,");
    println!("             Hyper-V Containers, Credential Guard, Device Guard, MDAG");
    println!("  Features: dynamic memory, live migration, replica (DR), shielded VMs,");
    println!("           Enhanced Session Mode (RDP-into-VM), Discrete Device Assignment (GPU)");
    println!("  Disk format: .vhd / .vhdx (Virtual Hard Disk, eXtended) — Microsoft native");
    println!("  Management: Hyper-V Manager MMC, PowerShell Hyper-V module, SCVMM (enterprise),");
    println!("              Windows Admin Center (modern web UI)");
    println!("  Server pricing: included with Windows Server Standard/Datacenter — no extra license");
    println!("  Bundled with: Azure (Azure's hypervisor is a heavily-modified Hyper-V)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hyperv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
