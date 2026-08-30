//! Slate OS PCI Device Lister
//!
//! Lists PCI devices by reading from /sys/bus/pci/ or /proc/pci.
//! Includes a built-in vendor/device ID database for common hardware.
//!
//! # Usage
//!
//! ```text
//! lspci                    List all PCI devices (summary)
//! lspci -v                 Verbose (show capabilities, BARs)
//! lspci -vv                Very verbose (full config space dump)
//! lspci -n                 Show numeric IDs instead of names
//! lspci -nn                Show both names and numeric IDs
//! lspci -s <slot>          Filter by slot (bus:dev.fn)
//! lspci -d <vendor:device> Filter by vendor:device ID
//! lspci -k                 Show kernel driver in use
//! lspci -t                 Tree view (bus hierarchy)
//! lspci --json             JSON output
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// PCI device structure
// ============================================================================

#[derive(Clone)]
struct PciDevice {
    bus: u8,
    device: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    class_code: u8,
    subclass: u8,
    prog_if: u8,
    revision: u8,
    subsys_vendor: u16,
    subsys_device: u16,
    irq: u8,
    driver: String,
    bars: Vec<BarInfo>,
}

#[derive(Clone)]
struct BarInfo {
    index: u8,
    base: u64,
    size: u64,
    is_io: bool,
    is_prefetchable: bool,
}

impl PciDevice {
    fn slot_str(&self) -> String {
        format!("{:02x}:{:02x}.{}", self.bus, self.device, self.function)
    }
}

// ============================================================================
// PCI class database
// ============================================================================

fn class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x00, 0x00) => "Non-VGA unclassified device",
        (0x00, 0x01) => "VGA compatible unclassified device",
        (0x01, 0x00) => "SCSI storage controller",
        (0x01, 0x01) => "IDE interface",
        (0x01, 0x05) => "ATA controller",
        (0x01, 0x06) => "SATA controller",
        (0x01, 0x07) => "Serial Attached SCSI controller",
        (0x01, 0x08) => "NVMe controller",
        (0x01, _) => "Mass storage controller",
        (0x02, 0x00) => "Ethernet controller",
        (0x02, 0x80) => "Network controller",
        (0x02, _) => "Network controller",
        (0x03, 0x00) => "VGA compatible controller",
        (0x03, 0x01) => "XGA controller",
        (0x03, 0x02) => "3D controller",
        (0x03, _) => "Display controller",
        (0x04, 0x00) => "Multimedia video controller",
        (0x04, 0x01) => "Multimedia audio controller",
        (0x04, 0x03) => "Audio device",
        (0x04, _) => "Multimedia controller",
        (0x05, 0x00) => "RAM memory",
        (0x05, _) => "Memory controller",
        (0x06, 0x00) => "Host bridge",
        (0x06, 0x01) => "ISA bridge",
        (0x06, 0x04) => "PCI bridge",
        (0x06, 0x07) => "CardBus bridge",
        (0x06, _) => "Bridge",
        (0x07, 0x00) => "Serial controller",
        (0x07, 0x01) => "Parallel controller",
        (0x07, _) => "Communication controller",
        (0x08, 0x00) => "PIC",
        (0x08, 0x01) => "DMA controller",
        (0x08, 0x02) => "Timer",
        (0x08, 0x03) => "RTC controller",
        (0x08, _) => "System peripheral",
        (0x09, _) => "Input device controller",
        (0x0A, _) => "Docking station",
        (0x0B, _) => "Processor",
        (0x0C, 0x00) => "FireWire controller",
        (0x0C, 0x03) => "USB controller",
        (0x0C, 0x05) => "SMBus controller",
        (0x0C, _) => "Serial bus controller",
        (0x0D, _) => "Wireless controller",
        (0x0E, _) => "Intelligent controller",
        (0x0F, _) => "Satellite communication controller",
        (0x10, _) => "Encryption controller",
        (0x11, _) => "Signal processing controller",
        (0x12, _) => "Processing accelerator",
        (0xFF, _) => "Unassigned class",
        _ => "Unknown device",
    }
}

/// Common vendor name lookup.
fn vendor_name(id: u16) -> &'static str {
    match id {
        0x1234 => "QEMU",
        0x8086 => "Intel Corporation",
        0x1022 => "Advanced Micro Devices [AMD]",
        0x1002 => "Advanced Micro Devices [AMD/ATI]",
        0x10DE => "NVIDIA Corporation",
        0x14E4 => "Broadcom Inc.",
        0x168C => "Qualcomm Atheros",
        0x8087 => "Intel Corporation (Wireless)",
        0x1AF4 => "Red Hat (Virtio)",
        0x1B36 => "Red Hat (QEMU Virtual)",
        0x15AD => "VMware",
        0x1AB8 => "Parallels",
        0x80EE => "Oracle VirtualBox",
        0x1D6B => "Linux Foundation",
        0x1A03 => "ASPEED Technology",
        0x10EC => "Realtek Semiconductor",
        0x1969 => "Qualcomm Atheros (Killer)",
        0x14C3 => "MediaTek",
        0x1B73 => "Fresco Logic",
        0x1217 => "O2 Micro",
        _ => "",
    }
}

/// Common device name lookup for QEMU/virtio.
fn device_name(vendor: u16, device: u16) -> &'static str {
    match (vendor, device) {
        // QEMU
        (0x1234, 0x1111) => "QEMU Virtual VGA",
        // Virtio
        (0x1AF4, 0x1000) => "Virtio network device",
        (0x1AF4, 0x1001) => "Virtio block device",
        (0x1AF4, 0x1002) => "Virtio memory balloon",
        (0x1AF4, 0x1003) => "Virtio console",
        (0x1AF4, 0x1004) => "Virtio SCSI",
        (0x1AF4, 0x1005) => "Virtio RNG",
        (0x1AF4, 0x1009) => "Virtio filesystem",
        (0x1AF4, 0x1041) => "Virtio network device (modern)",
        (0x1AF4, 0x1042) => "Virtio block device (modern)",
        (0x1AF4, 0x1043) => "Virtio console (modern)",
        (0x1AF4, 0x1044) => "Virtio RNG (modern)",
        (0x1AF4, 0x1045) => "Virtio memory balloon (modern)",
        (0x1AF4, 0x1048) => "Virtio SCSI (modern)",
        (0x1AF4, 0x1049) => "Virtio filesystem (modern)",
        (0x1AF4, 0x1050) => "Virtio GPU",
        (0x1AF4, 0x1052) => "Virtio input",
        (0x1AF4, 0x1053) => "Virtio socket",
        // QEMU virtual
        (0x1B36, 0x0001) => "QEMU PCI-PCI bridge",
        (0x1B36, 0x0002) => "QEMU PCI serial port",
        (0x1B36, 0x0003) => "QEMU PCI parallel port",
        (0x1B36, 0x0004) => "QEMU PCI test device",
        (0x1B36, 0x0005) => "QEMU PCI Rocker switch",
        (0x1B36, 0x000D) => "QEMU XHCI Host Controller",
        (0x1B36, 0x0100) => "QEMU PCIe Root Port",
        // Intel common
        (0x8086, 0x100E) => "82540EM Gigabit Ethernet",
        (0x8086, 0x10D3) => "82574L Gigabit Ethernet",
        (0x8086, 0x1503) => "82579V Gigabit Ethernet",
        (0x8086, 0x1539) => "I211 Gigabit Ethernet",
        (0x8086, 0x15B8) => "Ethernet Connection (2) I219-V",
        (0x8086, 0x2922) => "82801IR/IO/IH SATA Controller (AHCI)",
        (0x8086, 0x29C0) => "Express DRAM Controller",
        (0x8086, 0x2918) => "LPC Interface Controller",
        (0x8086, 0x2934) => "USB UHCI Controller",
        (0x8086, 0x293A) => "USB2 EHCI Controller",
        _ => "",
    }
}

// ============================================================================
// /sys/bus/pci scanner
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_hex_file(path: &str) -> Option<u64> {
    read_file(path).and_then(|s| {
        let s = s.trim_start_matches("0x");
        u64::from_str_radix(s, 16).ok()
    })
}

/// Scan /sys/bus/pci/devices/ for PCI devices.
fn scan_sysfs() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    let pci_path = "/sys/bus/pci/devices";
    let entries = match fs::read_dir(pci_path) {
        Ok(e) => e,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let dev_path = format!("{pci_path}/{name}");

        // Parse BDF from directory name (0000:BB:DD.F).
        let bdf = parse_bdf(&name);
        let (bus, device, function) = match bdf {
            Some(b) => b,
            None => continue,
        };

        let vendor_id = read_hex_file(&format!("{dev_path}/vendor"))
            .unwrap_or(0) as u16;
        let device_id = read_hex_file(&format!("{dev_path}/device"))
            .unwrap_or(0) as u16;
        let class_val = read_hex_file(&format!("{dev_path}/class"))
            .unwrap_or(0) as u32;
        let revision = read_hex_file(&format!("{dev_path}/revision"))
            .unwrap_or(0) as u8;
        let subsys_vendor = read_hex_file(&format!("{dev_path}/subsystem_vendor"))
            .unwrap_or(0) as u16;
        let subsys_device = read_hex_file(&format!("{dev_path}/subsystem_device"))
            .unwrap_or(0) as u16;
        let irq = read_hex_file(&format!("{dev_path}/irq"))
            .unwrap_or(0) as u8;

        let class_code = ((class_val >> 16) & 0xFF) as u8;
        let subclass = ((class_val >> 8) & 0xFF) as u8;
        let prog_if = (class_val & 0xFF) as u8;

        let driver = read_file(&format!("{dev_path}/driver/name"))
            .unwrap_or_default();

        // Read BARs.
        let bars = read_bars(&dev_path);

        devices.push(PciDevice {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision,
            subsys_vendor,
            subsys_device,
            irq,
            driver,
            bars,
        });
    }

    // Sort by BDF.
    devices.sort_by(|a, b| {
        (a.bus, a.device, a.function).cmp(&(b.bus, b.device, b.function))
    });

    devices
}

/// Scan /proc/pci as fallback.
fn scan_proc_pci() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    let content = match read_file("/proc/pci") {
        Some(c) => c,
        None => return devices,
    };

    // /proc/pci format (our kernel):
    // BB:DD.F vendor=XXXX device=XXXX class=XXYYZZ rev=XX irq=N driver=name
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let bdf = match parse_bdf(parts[0]) {
            Some(b) => b,
            None => continue,
        };

        let mut dev = PciDevice {
            bus: bdf.0,
            device: bdf.1,
            function: bdf.2,
            vendor_id: 0,
            device_id: 0,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            revision: 0,
            subsys_vendor: 0,
            subsys_device: 0,
            irq: 0,
            driver: String::new(),
            bars: Vec::new(),
        };

        for part in parts.iter().skip(1) {
            if let Some((key, val)) = part.split_once('=') {
                match key {
                    "vendor" => dev.vendor_id = u16::from_str_radix(val, 16).unwrap_or(0),
                    "device" => dev.device_id = u16::from_str_radix(val, 16).unwrap_or(0),
                    "class" => {
                        let c = u32::from_str_radix(val, 16).unwrap_or(0);
                        dev.class_code = ((c >> 16) & 0xFF) as u8;
                        dev.subclass = ((c >> 8) & 0xFF) as u8;
                        dev.prog_if = (c & 0xFF) as u8;
                    }
                    "rev" => dev.revision = u8::from_str_radix(val, 16).unwrap_or(0),
                    "irq" => dev.irq = val.parse().unwrap_or(0),
                    "driver" => dev.driver = val.to_string(),
                    _ => {}
                }
            }
        }

        devices.push(dev);
    }

    devices
}

fn parse_bdf(s: &str) -> Option<(u8, u8, u8)> {
    // Formats: "BB:DD.F" or "DDDD:BB:DD.F"
    let s = if s.len() > 7 {
        // Skip domain prefix.
        s.rsplit_once(':')
            .map(|(prefix, _)| {
                // Actually, for DDDD:BB:DD.F we need different parsing.
                // Let's just take the last BB:DD.F part.
                let _ = prefix;
                s
            })
            .unwrap_or(s)
    } else {
        s
    };

    // Try DDDD:BB:DD.F format.
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 3 {
        // domain:bus:dev.fn
        let bus = u8::from_str_radix(parts[1], 16).ok()?;
        let (dev_str, fn_str) = parts[2].split_once('.')?;
        let device = u8::from_str_radix(dev_str, 16).ok()?;
        let function: u8 = fn_str.parse().ok()?;
        return Some((bus, device, function));
    }

    if parts.len() == 2 {
        // bus:dev.fn
        let bus = u8::from_str_radix(parts[0], 16).ok()?;
        let (dev_str, fn_str) = parts[1].split_once('.')?;
        let device = u8::from_str_radix(dev_str, 16).ok()?;
        let function: u8 = fn_str.parse().ok()?;
        return Some((bus, device, function));
    }

    None
}

fn read_bars(dev_path: &str) -> Vec<BarInfo> {
    let mut bars = Vec::new();

    // Read resource file which has BAR info.
    let resource = match read_file(&format!("{dev_path}/resource")) {
        Some(r) => r,
        None => return bars,
    };

    for (i, line) in resource.lines().enumerate() {
        if i >= 6 {
            break; // Only 6 BARs.
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let start = u64::from_str_radix(
                parts[0].trim_start_matches("0x"),
                16,
            ).unwrap_or(0);
            let end = u64::from_str_radix(
                parts[1].trim_start_matches("0x"),
                16,
            ).unwrap_or(0);
            let flags = u64::from_str_radix(
                parts[2].trim_start_matches("0x"),
                16,
            ).unwrap_or(0);

            if start == 0 && end == 0 {
                continue;
            }

            let size = if end > start { end - start + 1 } else { 0 };
            let is_io = (flags & 0x1) != 0;
            let is_prefetchable = (flags & 0x8) != 0;

            bars.push(BarInfo {
                index: i as u8,
                base: start,
                size,
                is_io,
                is_prefetchable,
            });
        }
    }

    bars
}

// ============================================================================
// Display
// ============================================================================

struct Config {
    verbose: u8,     // 0=summary, 1=verbose, 2=very verbose
    numeric: u8,     // 0=names, 1=numeric, 2=both
    show_driver: bool,
    tree: bool,
    json: bool,
    filter_slot: Option<String>,
    filter_vendor: Option<u16>,
    filter_device: Option<u16>,
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}G", bytes / 1_073_741_824)
    } else if bytes >= 1_048_576 {
        format!("{}M", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{bytes}")
    }
}

fn display_device_summary(dev: &PciDevice, config: &Config) {
    let slot = dev.slot_str();
    let class_str = class_name(dev.class_code, dev.subclass);

    let vendor_str = vendor_name(dev.vendor_id);
    let device_str = device_name(dev.vendor_id, dev.device_id);

    match config.numeric {
        1 => {
            // Numeric only.
            println!(
                "{slot} Class {:02x}{:02x}: {:04x}:{:04x}",
                dev.class_code, dev.subclass,
                dev.vendor_id, dev.device_id,
            );
        }
        2 => {
            // Both names and numeric.
            let vname = if vendor_str.is_empty() {
                format!("Unknown vendor {:04x}", dev.vendor_id)
            } else {
                format!("{vendor_str} [{:04x}]", dev.vendor_id)
            };
            let dname = if device_str.is_empty() {
                format!("Device {:04x}", dev.device_id)
            } else {
                format!("{device_str} [{:04x}]", dev.device_id)
            };
            println!("{slot} {class_str} [{:02x}{:02x}]: {vname} {dname}",
                dev.class_code, dev.subclass);
        }
        _ => {
            // Names only (default).
            let vname = if vendor_str.is_empty() {
                format!("Vendor {:04x}", dev.vendor_id)
            } else {
                vendor_str.to_string()
            };
            let dname = if device_str.is_empty() {
                format!("Device {:04x}", dev.device_id)
            } else {
                device_str.to_string()
            };
            println!("{slot} {class_str}: {vname} {dname}");
        }
    }
}

fn display_device_verbose(dev: &PciDevice, config: &Config) {
    display_device_summary(dev, config);

    println!("\tSubsystem: {:04x}:{:04x}", dev.subsys_vendor, dev.subsys_device);
    println!("\tRevision:  {:02x}", dev.revision);

    if dev.prog_if != 0 {
        println!("\tProgIf:    {:02x}", dev.prog_if);
    }

    if dev.irq > 0 {
        println!("\tIRQ:       {}", dev.irq);
    }

    if !dev.driver.is_empty() {
        println!("\tDriver:    {}", dev.driver);
    }

    // BARs.
    for bar in &dev.bars {
        let bar_type = if bar.is_io { "I/O" } else { "Memory" };
        let prefetch = if bar.is_prefetchable { " [prefetchable]" } else { "" };
        println!(
            "\tBAR{}: {} at {:#010x} [size={}]{}",
            bar.index, bar_type, bar.base, format_size(bar.size), prefetch,
        );
    }

    println!();
}

fn display_tree(devices: &[PciDevice]) {
    // Group by bus.
    let mut buses: Vec<u8> = devices.iter().map(|d| d.bus).collect();
    buses.sort();
    buses.dedup();

    for bus in &buses {
        println!("Bus {:02x}", bus);
        let bus_devs: Vec<&PciDevice> = devices.iter()
            .filter(|d| d.bus == *bus)
            .collect();

        for (i, dev) in bus_devs.iter().enumerate() {
            let prefix = if i == bus_devs.len() - 1 { "└──" } else { "├──" };
            let vname = vendor_name(dev.vendor_id);
            let dname = device_name(dev.vendor_id, dev.device_id);
            let name = if !dname.is_empty() {
                dname.to_string()
            } else if !vname.is_empty() {
                format!("{vname} Device {:04x}", dev.device_id)
            } else {
                format!("{:04x}:{:04x}", dev.vendor_id, dev.device_id)
            };
            println!("  {prefix} {:02x}.{} {}", dev.device, dev.function, name);
        }
    }
}

fn display_json(devices: &[PciDevice]) {
    println!("[");
    for (i, dev) in devices.iter().enumerate() {
        let comma = if i < devices.len() - 1 { "," } else { "" };
        let vname = vendor_name(dev.vendor_id);
        let dname = device_name(dev.vendor_id, dev.device_id);
        let cname = class_name(dev.class_code, dev.subclass);
        println!(
            "  {{\"slot\":\"{}\",\"vendor_id\":\"{:04x}\",\"device_id\":\"{:04x}\",\
             \"vendor\":\"{vname}\",\"device\":\"{dname}\",\
             \"class\":\"{:02x}{:02x}\",\"class_name\":\"{cname}\",\
             \"revision\":\"{:02x}\",\"driver\":\"{}\",\"irq\":{}}}{comma}",
            dev.slot_str(), dev.vendor_id, dev.device_id,
            dev.class_code, dev.subclass,
            dev.revision, dev.driver, dev.irq,
        );
    }
    println!("]");
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("Slate OS PCI Device Lister v0.1.0");
    println!();
    println!("List PCI devices and their configuration.");
    println!();
    println!("USAGE:");
    println!("  lspci [options]");
    println!();
    println!("OPTIONS:");
    println!("  -v            Verbose output (BARs, IRQ, driver)");
    println!("  -vv           Very verbose output");
    println!("  -n            Show numeric vendor/device IDs");
    println!("  -nn           Show both names and numeric IDs");
    println!("  -k            Show kernel driver in use");
    println!("  -t            Tree view (bus hierarchy)");
    println!("  -s <slot>     Filter by slot (BB:DD.F)");
    println!("  -d <v:d>      Filter by vendor:device ID (hex)");
    println!("  --json        JSON output");
    println!("  --help, -h    Show this help");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        verbose: 0,
        numeric: 0,
        show_driver: false,
        tree: false,
        json: false,
        filter_slot: None,
        filter_vendor: None,
        filter_device: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" => { config.verbose = 1; i += 1; }
            "-vv" => { config.verbose = 2; i += 1; }
            "-n" => { config.numeric = 1; i += 1; }
            "-nn" => { config.numeric = 2; i += 1; }
            "-k" => { config.show_driver = true; i += 1; }
            "-t" | "--tree" => { config.tree = true; i += 1; }
            "--json" => { config.json = true; i += 1; }
            "-s" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -s requires a slot (BB:DD.F)");
                    process::exit(1);
                }
                config.filter_slot = Some(args[i + 1].clone());
                i += 2;
            }
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -d requires vendor:device");
                    process::exit(1);
                }
                if let Some((v, d)) = args[i + 1].split_once(':') {
                    config.filter_vendor = u16::from_str_radix(v, 16).ok();
                    config.filter_device = u16::from_str_radix(d, 16).ok();
                }
                i += 2;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                process::exit(1);
            }
        }
    }

    // Scan for devices.
    let mut devices = scan_sysfs();
    if devices.is_empty() {
        devices = scan_proc_pci();
    }

    if devices.is_empty() {
        eprintln!("No PCI devices found (is /sys/bus/pci or /proc/pci available?)");
        process::exit(1);
    }

    // Apply filters.
    if let Some(ref slot) = config.filter_slot {
        devices.retain(|d| d.slot_str().contains(slot.as_str()));
    }
    if let Some(v) = config.filter_vendor {
        devices.retain(|d| d.vendor_id == v);
    }
    if let Some(d) = config.filter_device {
        devices.retain(|d_dev| d_dev.device_id == d);
    }

    // Display.
    if config.json {
        display_json(&devices);
    } else if config.tree {
        display_tree(&devices);
    } else if config.verbose > 0 {
        for dev in &devices {
            display_device_verbose(dev, &config);
        }
    } else {
        for dev in &devices {
            display_device_summary(dev, &config);
            if config.show_driver && !dev.driver.is_empty() {
                println!("\tKernel driver in use: {}", dev.driver);
            }
        }
    }
}
