#![deny(clippy::all)]

//! hwinfo-cli — SlateOS hardware information tool
//!
//! Multi-personality: `hwinfo`, `lshw`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_hwinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hwinfo [OPTIONS]");
        println!();
        println!("hwinfo — probe for hardware (Slate OS).");
        println!();
        println!("Options:");
        println!("  --short        Short listing");
        println!("  --cpu          CPU info");
        println!("  --disk         Disk info");
        println!("  --gfxcard      Graphics card info");
        println!("  --memory       Memory info");
        println!("  --netcard      Network card info");
        println!("  --sound        Sound device info");
        println!("  --usb          USB device info");
        println!("  --all          All hardware info");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("hwinfo version 21.82 (Slate OS)");
        return 0;
    }

    let short = args.iter().any(|a| a == "--short");
    let cpu = args.iter().any(|a| a == "--cpu");
    let disk = args.iter().any(|a| a == "--disk");
    let gfx = args.iter().any(|a| a == "--gfxcard");
    let memory = args.iter().any(|a| a == "--memory");
    let netcard = args.iter().any(|a| a == "--netcard");
    let show_all = args.iter().any(|a| a == "--all") || (!cpu && !disk && !gfx && !memory && !netcard);

    if short {
        if show_all || cpu {
            println!("cpu:");
            println!("  Intel(R) Core(TM) i9-13900K");
        }
        if show_all || gfx {
            println!("graphics card:");
            println!("  NVIDIA GeForce RTX 4090");
            println!("  Intel UHD Graphics 770");
        }
        if show_all || disk {
            println!("disk:");
            println!("  /dev/sda  Samsung SSD 870 EVO 1TB");
            println!("  /dev/nvme0n1  Samsung 980 PRO 2TB");
        }
        if show_all || netcard {
            println!("network:");
            println!("  Intel I225-V 2.5GbE");
            println!("  Intel Wi-Fi 6E AX211");
        }
        if show_all || memory {
            println!("memory:");
            println!("  Main Memory: 64GB");
        }
        return 0;
    }

    if show_all || cpu {
        println!("01: None 00.0: 10103 CPU");
        println!("  [Created at cpu.464]");
        println!("  Model: \"Intel(R) Core(TM) i9-13900K\"");
        println!("  Vendor: pci 0x8086 \"Intel Corporation\"");
        println!("  Device: cpu \"Intel(R) Core(TM) i9-13900K\"");
        println!("  Config Status: cfg=yes, avail=yes, need=no, active=yes");
        println!();
    }
    if show_all || gfx {
        println!("02: PCI 01.0: 0300 VGA compatible controller");
        println!("  [Created at pci.378]");
        println!("  Model: \"NVIDIA GeForce RTX 4090\"");
        println!("  Vendor: pci 0x10de \"NVIDIA Corporation\"");
        println!("  SubVendor: pci 0x10de \"NVIDIA Corporation\"");
        println!("  Device: pci 0x2684 \"AD102 [GeForce RTX 4090]\"");
        println!("  Driver: \"nvidia\"");
        println!("  Memory Range: 0xfb000000-0xfbffffff (non-prefetchable)");
        println!("  Config Status: cfg=yes, avail=yes, need=no, active=yes");
        println!();
    }
    if show_all || disk {
        println!("03: SCSI 00.0: 10600 Disk");
        println!("  Model: \"Samsung SSD 870 EVO 1TB\"");
        println!("  Device File: /dev/sda");
        println!("  Size: 931 GB (1000204886016 bytes)");
        println!("  Config Status: cfg=yes, avail=yes, need=no, active=yes");
        println!();
    }
    0
}

fn run_lshw(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lshw [OPTIONS]");
        println!();
        println!("lshw — list hardware (Slate OS).");
        println!();
        println!("Options:");
        println!("  -short         Short listing");
        println!("  -class CLASS   Only show CLASS");
        println!("  -json          JSON output");
        println!("  -xml           XML output");
        println!("  -html          HTML output");
        println!("  -businfo       Bus information");
        println!("  -sanitize      Sanitize output (hide serials)");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("lshw B.02.19.2 (Slate OS)");
        return 0;
    }

    let short = args.iter().any(|a| a == "-short");
    let json = args.iter().any(|a| a == "-json");

    if json {
        println!("{{");
        println!("  \"id\": \"computer\",");
        println!("  \"class\": \"system\",");
        println!("  \"description\": \"Desktop Computer\",");
        println!("  \"product\": \"System Product Name\",");
        println!("  \"vendor\": \"System manufacturer\"");
        println!("}}");
        return 0;
    }

    if short {
        println!("H/W path         Device  Class       Description");
        println!("=====================================================");
        println!("/0                       system       System Product Name");
        println!("/0/0                     memory       64GiB System Memory");
        println!("/0/0/0                   memory       32GiB DIMM DDR5 5600 MHz");
        println!("/0/0/1                   memory       32GiB DIMM DDR5 5600 MHz");
        println!("/0/1                     processor    13th Gen Intel(R) Core(TM) i9-13900K");
        println!("/0/100                   bridge       12th/13th Gen Core Host Bridge");
        println!("/0/100/2                 display      UHD Graphics 770");
        println!("/0/100/1/0               display      GeForce RTX 4090");
        println!("/1               sda     disk         Samsung SSD 870 EVO 1TB");
        println!("/2               nvme0n1 disk         Samsung 980 PRO 2TB");
        println!("/3               eth0    network      I225-V 2.5GbE");
    } else {
        println!("computer");
        println!("    description: Desktop Computer");
        println!("    product: System Product Name");
        println!("    vendor: System manufacturer");
        println!("    width: 64 bits");
        println!("    capabilities: dmi-3.6.0 smbios-3.6.0 vsyscall32");
        println!("  *-cpu");
        println!("       description: CPU");
        println!("       product: 13th Gen Intel(R) Core(TM) i9-13900K");
        println!("       vendor: Intel Corp.");
        println!("       physical id: 1");
        println!("       bus info: cpu@0");
        println!("       size: 3GHz");
        println!("       capacity: 5.8GHz");
        println!("       width: 64 bits");
        println!("       capabilities: x86-64 fpu vme de pse tsc msr pae mce cx8 apic");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "hwinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lshw" => run_lshw(&rest),
        _ => run_hwinfo(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hwinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hwinfo"), "hwinfo");
        assert_eq!(basename(r"C:\bin\hwinfo.exe"), "hwinfo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hwinfo.exe"), "hwinfo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hwinfo(&["--help".to_string()]), 0);
        assert_eq!(run_hwinfo(&["-h".to_string()]), 0);
        let _ = run_hwinfo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hwinfo(&[]);
    }
}
