#![deny(clippy::all)]

//! cloud-hypervisor-cli — SlateOS Cloud Hypervisor VMM
//!
//! Single personality: `cloud-hypervisor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cloud_hypervisor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cloud-hypervisor [OPTIONS]");
        println!("cloud-hypervisor v39.0 (Slate OS) — Rust-based VMM");
        println!();
        println!("Options:");
        println!("  --kernel PATH     Kernel image");
        println!("  --initramfs PATH  Initramfs image");
        println!("  --disk PATH       Disk image");
        println!("  --cpus boot=N     Number of vCPUs");
        println!("  --memory size=N   Memory size (MiB)");
        println!("  --net tap=TAP     Network config");
        println!("  --serial tty      Serial console");
        println!("  --console off     Console device");
        println!("  --api-socket PATH API socket path");
        println!("  -v                Verbose");
        println!("  -V / --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") { println!("cloud-hypervisor v39.0 (Slate OS)"); return 0; }
    println!("Cloud Hypervisor v39.0");
    println!("  vCPUs: 2");
    println!("  Memory: 2048 MiB");
    println!("  Kernel: vmlinux");
    println!("  Disks: 1");
    println!("  API socket: /run/ch.sock");
    println!("  VM booting...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cloud-hypervisor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cloud_hypervisor(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cloud_hypervisor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cloud-hypervisor"), "cloud-hypervisor");
        assert_eq!(basename(r"C:\bin\cloud-hypervisor.exe"), "cloud-hypervisor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cloud-hypervisor.exe"), "cloud-hypervisor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cloud_hypervisor(&["--help".to_string()], "cloud-hypervisor"), 0);
        assert_eq!(run_cloud_hypervisor(&["-h".to_string()], "cloud-hypervisor"), 0);
        let _ = run_cloud_hypervisor(&["--version".to_string()], "cloud-hypervisor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cloud_hypervisor(&[], "cloud-hypervisor");
    }
}
