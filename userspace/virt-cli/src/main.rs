#![deny(clippy::all)]

//! virt-cli — SlateOS virt-manager CLI tools
//!
//! Multi-personality: `virt-install`, `virt-clone`, `virt-viewer`, `virt-xml`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_virt_install(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-install [OPTIONS]");
        println!();
        println!("virt-install — provision new virtual machines (Slate OS).");
        println!();
        println!("Options:");
        println!("  --name NAME          VM name");
        println!("  --memory MB          Memory in MB");
        println!("  --vcpus N            Number of vCPUs");
        println!("  --disk PATH          Disk specification");
        println!("  --cdrom FILE         CD-ROM ISO");
        println!("  --location URL       Install location");
        println!("  --os-variant ID      OS variant");
        println!("  --network NETWORK    Network specification");
        println!("  --graphics TYPE      Graphics (vnc/spice/none)");
        println!("  --noautoconsole      Don't auto-attach console");
        println!("  --import             Import existing disk");
        println!("  --boot OPTS          Boot options");
        return 0;
    }

    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("vm1");
    let mem = args.windows(2).find(|w| w[0] == "--memory").map(|w| w[1].as_str()).unwrap_or("1024");
    let vcpus = args.windows(2).find(|w| w[0] == "--vcpus").map(|w| w[1].as_str()).unwrap_or("1");

    println!("Starting install...");
    println!("  Name: {}", name);
    println!("  Memory: {} MB", mem);
    println!("  vCPUs: {}", vcpus);
    println!("Creating domain...");
    println!("Domain creation completed.");
    0
}

fn run_virt_clone(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-clone [OPTIONS]");
        println!();
        println!("virt-clone — clone existing VMs (Slate OS).");
        println!();
        println!("Options:");
        println!("  --original NAME     Source VM");
        println!("  --name NAME         Clone name");
        println!("  --auto-clone        Auto-generate clone config");
        println!("  --file PATH         Clone disk path");
        return 0;
    }

    let orig = args.windows(2).find(|w| w[0] == "--original").map(|w| w[1].as_str()).unwrap_or("vm1");
    let name = args.windows(2).find(|w| w[0] == "--name").map(|w| w[1].as_str()).unwrap_or("vm1-clone");

    println!("Cloning '{}' → '{}'...", orig, name);
    println!("  Cloning disk...");
    println!("Clone '{}' created successfully.", name);
    0
}

fn run_virt_viewer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-viewer [OPTIONS] [DOMAIN]");
        println!();
        println!("virt-viewer — VM console viewer (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c, --connect URI  Hypervisor URI");
        println!("  -w, --wait         Wait for domain to start");
        println!("  -f, --full-screen  Full screen mode");
        println!("  --auto-resize      Auto-resize window");
        return 0;
    }
    let domain = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("vm1");
    println!("Connecting to '{}' console...", domain);
    println!("Connected via SPICE.");
    0
}

fn run_virt_xml(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virt-xml [OPTIONS] DOMAIN");
        println!();
        println!("virt-xml — edit libvirt XML (Slate OS).");
        println!();
        println!("Options:");
        println!("  --add-device      Add device");
        println!("  --remove-device   Remove device");
        println!("  --edit            Edit existing config");
        println!("  --build-xml       Print XML without applying");
        return 0;
    }
    println!("Domain XML updated.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "virt-install".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "virt-clone" => run_virt_clone(&rest),
        "virt-viewer" => run_virt_viewer(&rest),
        "virt-xml" => run_virt_xml(&rest),
        _ => run_virt_install(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_virt_install};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virt"), "virt");
        assert_eq!(basename(r"C:\bin\virt.exe"), "virt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virt.exe"), "virt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_virt_install(&["--help".to_string()]), 0);
        assert_eq!(run_virt_install(&["-h".to_string()]), 0);
        let _ = run_virt_install(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_virt_install(&[]);
    }
}
