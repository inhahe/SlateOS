#![deny(clippy::all)]

//! lspci-cli — OurOS PCI device lister
//!
//! Multi-personality: `lspci`, `setpci`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lspci(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lspci [OPTIONS]");
        println!();
        println!("lspci — list PCI devices (OurOS).");
        println!();
        println!("Options:");
        println!("  -v              Verbose");
        println!("  -vv             Very verbose");
        println!("  -vvv            Even more verbose");
        println!("  -n              Show numeric IDs");
        println!("  -nn             Show both textual and numeric IDs");
        println!("  -k              Show kernel drivers");
        println!("  -t              Show tree");
        println!("  -s [[DOMAIN:]BUS:]SLOT  Show only selected devices");
        println!("  -d [VENDOR]:[DEVICE]    Show only matching vendor/device");
        println!("  -D              Show PCI domain");
        println!("  -mm             Machine readable (verbose)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("lspci version 3.11.1 (OurOS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v" || a == "-vv" || a == "-vvv");
    let numeric = args.iter().any(|a| a == "-n");
    let both = args.iter().any(|a| a == "-nn");
    let tree = args.iter().any(|a| a == "-t");
    let kernel = args.iter().any(|a| a == "-k");

    if tree {
        println!("-[0000:00]-+-00.0  Intel Corporation Device 4668");
        println!("           +-01.0-[01]----00.0  NVIDIA Corporation AD102");
        println!("           +-02.0  Intel Corporation Device 4680");
        println!("           +-14.0  Intel Corporation Device 7ae0");
        println!("           +-17.0  Intel Corporation Device 7ae8");
        println!("           +-1f.0  Intel Corporation Device 7a04");
        println!("           +-1f.3  Intel Corporation Device 7ad0");
        println!("           +-1f.4  Intel Corporation Device 7aa3");
        println!("           \\-1f.5  Intel Corporation Device 7ae4");
        return 0;
    }

    let devices = [
        ("00:00.0", "Host bridge", "Intel Corporation", "12th/13th Gen Core Processor Host Bridge", "0600", "8086:4668", ""),
        ("00:02.0", "VGA compatible controller", "Intel Corporation", "UHD Graphics 770", "0300", "8086:4680", "i915"),
        ("00:14.0", "USB controller", "Intel Corporation", "Alder Lake USB 3.2 xHCI", "0c03", "8086:7ae0", "xhci_hcd"),
        ("00:17.0", "SATA controller", "Intel Corporation", "Alder Lake SATA AHCI", "0106", "8086:7ae8", "ahci"),
        ("00:1f.0", "ISA bridge", "Intel Corporation", "Z690 Chipset LPC/eSPI", "0601", "8086:7a04", ""),
        ("00:1f.3", "Audio device", "Intel Corporation", "Alder Lake HD Audio", "0403", "8086:7ad0", "snd_hda_intel"),
        ("00:1f.4", "SMBus", "Intel Corporation", "Alder Lake SMBus", "0c05", "8086:7aa3", "i801_smbus"),
        ("00:1f.5", "Serial bus controller", "Intel Corporation", "Alder Lake SPI Controller", "0c80", "8086:7ae4", "intel-spi"),
        ("01:00.0", "VGA compatible controller", "NVIDIA Corporation", "GeForce RTX 4090", "0300", "10de:2684", "nvidia"),
        ("02:00.0", "Network controller", "Intel Corporation", "Wi-Fi 6E AX211", "0280", "8086:51f0", "iwlwifi"),
        ("03:00.0", "Ethernet controller", "Intel Corporation", "I225-V 2.5GbE", "0200", "8086:15f3", "igc"),
        ("04:00.0", "Non-Volatile memory controller", "Samsung Electronics", "980 PRO NVMe SSD", "0108", "144d:a80a", "nvme"),
    ];

    for (addr, class, vendor, name, class_id, ids, driver) in &devices {
        if numeric {
            println!("{} {} [{}]: {} [{}]", addr, class, class_id, ids, ids);
        } else if both {
            println!("{} {} [{}]: {} {} [{}]", addr, class, class_id, vendor, name, ids);
        } else {
            println!("{} {}: {} {}", addr, class, vendor, name);
        }
        if verbose {
            println!("\tSubsystem: {} {}", vendor, name);
            println!("\tFlags: bus master, fast devsel, latency 0");
            println!("\tMemory at fb000000 (64-bit, non-prefetchable) [size=16M]");
        }
        if kernel && !driver.is_empty() {
            println!("\tKernel driver in use: {}", driver);
        }
    }
    0
}

fn run_setpci(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: setpci [OPTIONS] DEVICE REGISTER[=VALUE]");
        println!();
        println!("setpci — configure PCI devices (OurOS).");
        println!();
        println!("Options:");
        println!("  -v              Verbose");
        println!("  -s DEVICE       Device selector");
        println!("  -d VENDOR:DEV   Device filter");
        println!("  -G              Enable register guess mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("setpci version 3.11.1 (OurOS)");
        return 0;
    }

    println!("setpci: configuration space access simulated (OurOS)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lspci".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "setpci" => run_setpci(&rest),
        _ => run_lspci(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lspci};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lspci"), "lspci");
        assert_eq!(basename(r"C:\bin\lspci.exe"), "lspci.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lspci.exe"), "lspci");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lspci(&["--help".to_string()]), 0);
        assert_eq!(run_lspci(&["-h".to_string()]), 0);
        let _ = run_lspci(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lspci(&[]);
    }
}
