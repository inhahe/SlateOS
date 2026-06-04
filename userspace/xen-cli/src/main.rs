#![deny(clippy::all)]

//! xen-cli — OurOS Xen Project hypervisor
//!
//! Single personality: `xen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xen(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xen [OPTIONS]");
        println!("Xen Project 4.19 (OurOS) — Open-source type-1 hypervisor");
        println!();
        println!("Options:");
        println!("  --xl                   xl toolstack (default since Xen 4.1)");
        println!("  --xen-create-image     xen-tools VM provisioning");
        println!("  --pv                   Paravirtualization (PV) — modified guest kernels");
        println!("  --hvm                  Hardware Virtual Machine (full virtualization)");
        println!("  --pvh                  PVH — modern PV mode (HVM container, no QEMU emul)");
        println!("  --xenstore             xenstore — config/state key-value store");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xen 4.19.0 (OurOS)"); return 0; }
    println!("Xen Project 4.19.0 (OurOS)");
    println!("  Vendor: Xen Project (under Linux Foundation since 2013)");
    println!("  Originated: 2003 at Cambridge University (Ian Pratt + students)");
    println!("  XenSource → Citrix 2007 (XenServer) → Citrix Hypervisor → XCP-ng fork");
    println!("  Type: Type-1 (bare-metal microkernel) — dom0 (privileged Linux) + domU guests");
    println!("  Modes: PV (paravirtual, custom kernel), HVM (full virt + QEMU device model),");
    println!("         PVH (modern, hybrid — no QEMU, hardware virt for memory/CPU)");
    println!("  Used by: AWS EC2 (was the original EC2 hypervisor, now mostly Nitro),");
    println!("           Citrix XenServer/Hypervisor, XCP-ng, OracleVM Server, Qubes OS,");
    println!("           BSDi, OpenXT (security-focused)");
    println!("  Adopters: Verizon Cloud, Rackspace, large telcos (NFV)");
    println!("  Strengths: ARM support (Xen ARM Cortex-A), real-time variant, MISRA-C for safety");
    println!("  Toolstacks: xl (current), libvirt+libxl, XAPI (Xen Orchestra/XCP-ng API)");
    println!("  Security: small TCB, formal verification work (CertiKOS-style), used in Qubes");
    println!("  Mostly behind-the-scenes — KVM has overtaken Xen as default Linux hypervisor");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xen".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xen(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xen};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xen"), "xen");
        assert_eq!(basename(r"C:\bin\xen.exe"), "xen.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xen.exe"), "xen");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xen(&["--help".to_string()], "xen"), 0);
        assert_eq!(run_xen(&["-h".to_string()], "xen"), 0);
        let _ = run_xen(&["--version".to_string()], "xen");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xen(&[], "xen");
    }
}
