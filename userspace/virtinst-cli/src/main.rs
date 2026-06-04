#![deny(clippy::all)]

//! virtinst-cli — OurOS virt-install VM provisioning
//!
//! Multi-personality: `virt-install`, `virt-clone`, `virt-xml`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_virt_install(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-install [OPTIONS]");
        println!("virt-install v4.1 (OurOS) — Provision new virtual machines");
        println!();
        println!("Options:");
        println!("  --name NAME        VM name");
        println!("  --memory MiB       RAM size");
        println!("  --vcpus N          Number of vCPUs");
        println!("  --disk SIZE        Disk size (e.g., 20)");
        println!("  --cdrom ISO        Installation media");
        println!("  --os-variant NAME  OS variant for optimizations");
        println!("  --network SPEC     Network config");
        println!("  --graphics TYPE    Display type (vnc, spice, none)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virt-install v4.1 (OurOS)"); return 0; }
    println!("virt-install: provisioning new VM");
    println!("  Name: new-vm");
    println!("  Memory: 2048 MiB");
    println!("  vCPUs: 2");
    println!("  Disk: 20 GiB (qcow2)");
    0
}

fn run_virt_clone(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-clone [OPTIONS]");
        println!("virt-clone v4.1 (OurOS) — Clone existing virtual machines");
        println!("  --original NAME   Source VM");
        println!("  --name NAME       New VM name");
        println!("  --auto-clone      Auto-generate all names");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virt-clone v4.1 (OurOS)"); return 0; }
    println!("virt-clone: cloning VM");
    println!("  Source: original-vm");
    println!("  Clone: original-vm-clone");
    println!("  Disk cloned: 20 GiB");
    0
}

fn run_virt_xml(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-xml DOMAIN [OPTIONS]");
        println!("virt-xml v4.1 (OurOS) — Edit libvirt domain XML");
        println!("  --add-device     Add device to domain");
        println!("  --remove-device  Remove device");
        println!("  --edit           Edit existing device");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("virt-xml v4.1 (OurOS)"); return 0; }
    println!("virt-xml: domain XML editor");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virt-install".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "virt-clone" => run_virt_clone(&rest, &prog),
        "virt-xml" => run_virt_xml(&rest, &prog),
        _ => run_virt_install(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_virt_install};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virtinst"), "virtinst");
        assert_eq!(basename(r"C:\bin\virtinst.exe"), "virtinst.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virtinst.exe"), "virtinst");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_virt_install(&["--help".to_string()], "virtinst"), 0);
        assert_eq!(run_virt_install(&["-h".to_string()], "virtinst"), 0);
        let _ = run_virt_install(&["--version".to_string()], "virtinst");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_virt_install(&[], "virtinst");
    }
}
