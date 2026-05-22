//! OurOS hardware information utility.
//!
//! Multi-personality binary providing:
//! - **hwinfo** — comprehensive hardware inventory
//! - **lshw** — list hardware (simplified)
//!
//! Probes system hardware by reading /sys, /proc, and DMI tables
//! to produce a detailed hardware inventory report.

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct HwDevice {
    class: String,
    description: String,
    vendor: String,
    model: String,
    _bus_type: String,
    _bus_id: String,
    driver: String,
    properties: BTreeMap<String, String>,
}

impl HwDevice {
    fn new(class: &str) -> Self {
        Self {
            class: class.to_string(),
            description: String::new(),
            vendor: String::new(),
            model: String::new(),
            _bus_type: String::new(),
            _bus_id: String::new(),
            driver: String::new(),
            properties: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum OutputFormat {
    Normal,
    Short,
    Json,
    Xml,
}

#[derive(Clone, Debug)]
struct HwInfoOptions {
    format: OutputFormat,
    filter_class: Option<String>,
    show_all: bool,
    _log_file: Option<String>,
}

impl Default for HwInfoOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Normal,
            filter_class: None,
            show_all: false,
            _log_file: None,
        }
    }
}

// ============================================================================
// Hardware probing
// ============================================================================

fn probe_cpu() -> Vec<HwDevice> {
    let mut devices = Vec::new();
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();

    let mut current_props = BTreeMap::new();
    let mut cpu_count = 0u32;

    for line in cpuinfo.lines() {
        if line.is_empty() {
            if !current_props.is_empty() {
                let mut dev = HwDevice::new("cpu");
                dev.vendor = current_props
                    .get("vendor_id")
                    .cloned()
                    .unwrap_or_default();
                dev.model = current_props
                    .get("model name")
                    .cloned()
                    .unwrap_or_default();
                dev.description = format!("CPU #{cpu_count}");
                dev.properties = current_props.clone();
                devices.push(dev);
                current_props.clear();
                cpu_count += 1;
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            current_props.insert(
                key.trim().to_string(),
                value.trim().to_string(),
            );
        }
    }

    // Handle last entry.
    if !current_props.is_empty() {
        let mut dev = HwDevice::new("cpu");
        dev.vendor = current_props
            .get("vendor_id")
            .cloned()
            .unwrap_or_default();
        dev.model = current_props
            .get("model name")
            .cloned()
            .unwrap_or_default();
        dev.description = format!("CPU #{cpu_count}");
        dev.properties = current_props;
        devices.push(dev);
    }

    // Fallback if /proc/cpuinfo not available.
    if devices.is_empty() {
        let mut dev = HwDevice::new("cpu");
        dev.description = "Processor".to_string();
        dev.vendor = "Unknown".to_string();
        dev.model = "Unknown CPU".to_string();
        devices.push(dev);
    }

    devices
}

fn probe_memory() -> Vec<HwDevice> {
    let mut dev = HwDevice::new("memory");
    dev.description = "System Memory".to_string();

    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    for line in meminfo.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            dev.properties.insert(key.to_string(), value.to_string());

            if key == "MemTotal" {
                dev.model = format!("RAM: {value}");
            }
        }
    }

    if dev.model.is_empty() {
        dev.model = "System Memory".to_string();
    }

    vec![dev]
}

fn probe_pci() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") {
        for entry in entries.flatten() {
            let path = entry.path();
            let bus_id = entry.file_name().to_string_lossy().to_string();

            let vendor_id = read_sysfs_trim(&path.join("vendor"));
            let device_id = read_sysfs_trim(&path.join("device"));
            let class_code = read_sysfs_trim(&path.join("class"));
            let driver = read_sysfs_link_name(&path.join("driver"));

            let pci_class = classify_pci_device(&class_code);

            let mut dev = HwDevice::new(&pci_class);
            dev.description = format!("PCI Device {bus_id}");
            dev.vendor = vendor_id.clone();
            dev.model = device_id.clone();
            dev._bus_type = "pci".to_string();
            dev._bus_id = bus_id;
            dev.driver = driver;
            dev.properties.insert("vendor_id".to_string(), vendor_id);
            dev.properties.insert("device_id".to_string(), device_id);
            dev.properties.insert("class".to_string(), class_code);

            devices.push(dev);
        }
    }

    devices
}

fn probe_usb() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
        for entry in entries.flatten() {
            let path = entry.path();
            let bus_id = entry.file_name().to_string_lossy().to_string();

            // Skip hub interfaces (contain ':').
            if bus_id.contains(':') {
                continue;
            }

            let vendor = read_sysfs_trim(&path.join("manufacturer"));
            let product = read_sysfs_trim(&path.join("product"));
            let id_vendor = read_sysfs_trim(&path.join("idVendor"));
            let id_product = read_sysfs_trim(&path.join("idProduct"));
            let bcd_class = read_sysfs_trim(&path.join("bDeviceClass"));

            let usb_class = classify_usb_device(&bcd_class);

            let mut dev = HwDevice::new(&usb_class);
            dev.description = if product.is_empty() {
                format!("USB Device {bus_id}")
            } else {
                product.clone()
            };
            dev.vendor = vendor;
            dev.model = product;
            dev._bus_type = "usb".to_string();
            dev._bus_id = bus_id;
            dev.properties.insert("idVendor".to_string(), id_vendor);
            dev.properties.insert("idProduct".to_string(), id_product);

            devices.push(dev);
        }
    }

    devices
}

fn probe_block() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();

            // Skip loop/ram devices.
            if name.starts_with("loop") || name.starts_with("ram") {
                continue;
            }

            let model = read_sysfs_trim(&path.join("device/model"));
            let vendor = read_sysfs_trim(&path.join("device/vendor"));
            let size_str = read_sysfs_trim(&path.join("size"));
            let removable = read_sysfs_trim(&path.join("removable"));
            let rotational = read_sysfs_trim(&path.join("queue/rotational"));

            let size_sectors: u64 = size_str.parse().unwrap_or(0);
            let size_bytes = size_sectors * 512;
            let size_gb = size_bytes / (1024 * 1024 * 1024);

            let disk_type = if removable == "1" {
                "removable"
            } else if rotational == "0" {
                "SSD"
            } else {
                "HDD"
            };

            let mut dev = HwDevice::new("disk");
            dev.description = format!("/dev/{name} ({disk_type}, {size_gb} GB)");
            dev.vendor = vendor;
            dev.model = model;
            dev.properties
                .insert("size_bytes".to_string(), size_bytes.to_string());
            dev.properties
                .insert("type".to_string(), disk_type.to_string());

            devices.push(dev);
        }
    }

    devices
}

fn probe_network() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();

            // Skip loopback.
            if name == "lo" {
                continue;
            }

            let mac = read_sysfs_trim(&path.join("address"));
            let mtu = read_sysfs_trim(&path.join("mtu"));
            let speed = read_sysfs_trim(&path.join("speed"));
            let driver = read_sysfs_link_name(&path.join("device/driver"));
            let operstate = read_sysfs_trim(&path.join("operstate"));

            let net_type = if path.join("wireless").exists() {
                "wireless"
            } else {
                "ethernet"
            };

            let mut dev = HwDevice::new("network");
            dev.description = format!("{name} ({net_type})");
            dev.model = name.clone();
            dev.driver = driver;
            dev.properties.insert("mac".to_string(), mac);
            dev.properties.insert("mtu".to_string(), mtu);
            dev.properties.insert("speed".to_string(), speed);
            dev.properties.insert("operstate".to_string(), operstate);
            dev.properties.insert("type".to_string(), net_type.to_string());

            devices.push(dev);
        }
    }

    devices
}

fn probe_display() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    // DRM subsystem.
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let path = entry.path();

            let mut dev = HwDevice::new("display");
            dev.description = format!("Graphics card: {name}");
            dev.driver = read_sysfs_link_name(&path.join("device/driver"));
            dev.vendor = read_sysfs_trim(&path.join("device/vendor"));
            dev.model = read_sysfs_trim(&path.join("device/device"));

            devices.push(dev);
        }
    }

    // Framebuffer fallback.
    if devices.is_empty() {
        if let Ok(entries) = fs::read_dir("/sys/class/graphics") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("fb") {
                    let path = entry.path();
                    let fb_name = read_sysfs_trim(&path.join("name"));

                    let mut dev = HwDevice::new("display");
                    dev.description = format!("Framebuffer: {fb_name}");
                    dev.model = fb_name;
                    devices.push(dev);
                }
            }
        }
    }

    devices
}

fn probe_audio() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    // ALSA sound cards.
    if let Ok(content) = fs::read_to_string("/proc/asound/cards") {
        for line in content.lines() {
            let line = line.trim();
            // Lines like: " 0 [Intel          ]: HDA-Intel - HDA Intel PCH"
            if line.is_empty() || !line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                continue;
            }
            let mut dev = HwDevice::new("sound");
            dev.description = line.to_string();
            dev.model = line.to_string();
            devices.push(dev);
        }
    }

    devices
}

fn probe_input() -> Vec<HwDevice> {
    let mut devices = Vec::new();

    if let Ok(content) = fs::read_to_string("/proc/bus/input/devices") {
        let mut current_name = String::new();
        let mut current_phys = String::new();

        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("N: Name=") {
                current_name = rest.trim_matches('"').to_string();
            } else if let Some(rest) = line.strip_prefix("P: Phys=") {
                current_phys = rest.to_string();
            } else if line.is_empty() && !current_name.is_empty() {
                let input_type = if current_name.to_lowercase().contains("keyboard") {
                    "keyboard"
                } else if current_name.to_lowercase().contains("mouse")
                    || current_name.to_lowercase().contains("trackpad")
                {
                    "mouse"
                } else {
                    "input"
                };

                let mut dev = HwDevice::new(input_type);
                dev.description = current_name.clone();
                dev.model = current_name.clone();
                dev.properties
                    .insert("phys".to_string(), current_phys.clone());

                devices.push(dev);
                current_name.clear();
                current_phys.clear();
            }
        }
    }

    devices
}

// ============================================================================
// Sysfs helpers
// ============================================================================

fn read_sysfs_trim(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_sysfs_link_name(path: &Path) -> String {
    fs::read_link(path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default()
}

fn classify_pci_device(class_code: &str) -> String {
    // PCI class codes (first two hex digits).
    let code = class_code
        .strip_prefix("0x")
        .unwrap_or(class_code);
    if code.len() >= 4 {
        match &code[..4] {
            "0300" | "0301" | "0302" => "display".to_string(),
            "0200" | "0201" => "network".to_string(),
            "0100" | "0101" | "0104" | "0106" => "storage".to_string(),
            "0403" | "0401" => "sound".to_string(),
            "0c03" => "usb-controller".to_string(),
            "0600" | "0601" | "0604" => "bridge".to_string(),
            "0280" => "wireless".to_string(),
            _ => {
                if code.len() >= 2 {
                    match &code[..2] {
                        "03" => "display".to_string(),
                        "02" => "network".to_string(),
                        "01" => "storage".to_string(),
                        "04" => "multimedia".to_string(),
                        "06" => "bridge".to_string(),
                        "0c" => "serial-bus".to_string(),
                        "05" => "memory-controller".to_string(),
                        _ => "other".to_string(),
                    }
                } else {
                    "other".to_string()
                }
            }
        }
    } else {
        "other".to_string()
    }
}

fn classify_usb_device(class: &str) -> String {
    match class {
        "09" => "usb-hub".to_string(),
        "03" => "input".to_string(),
        "08" => "usb-storage".to_string(),
        "02" => "network".to_string(),
        "01" => "audio".to_string(),
        "0e" => "camera".to_string(),
        "07" => "printer".to_string(),
        "06" => "imaging".to_string(),
        _ => "usb".to_string(),
    }
}

// ============================================================================
// Output formatting
// ============================================================================

fn print_device_normal(dev: &HwDevice, index: usize) {
    println!("--- Device #{index} ---");
    println!("  Class:       {}", dev.class);
    println!("  Description: {}", dev.description);
    if !dev.vendor.is_empty() {
        println!("  Vendor:      {}", dev.vendor);
    }
    if !dev.model.is_empty() {
        println!("  Model:       {}", dev.model);
    }
    if !dev.driver.is_empty() {
        println!("  Driver:      {}", dev.driver);
    }
    for (key, value) in &dev.properties {
        println!("  {key}: {value}");
    }
    println!();
}

fn print_device_short(dev: &HwDevice) {
    let desc = if dev.model.is_empty() {
        &dev.description
    } else {
        &dev.model
    };
    println!("  {:>16}: {desc}", dev.class);
}

fn print_devices_json(devices: &[HwDevice]) {
    println!("[");
    for (i, dev) in devices.iter().enumerate() {
        println!("  {{");
        println!("    \"class\": \"{}\",", dev.class);
        println!("    \"description\": \"{}\",", escape_json(&dev.description));
        println!("    \"vendor\": \"{}\",", escape_json(&dev.vendor));
        println!("    \"model\": \"{}\",", escape_json(&dev.model));
        println!("    \"driver\": \"{}\"", escape_json(&dev.driver));
        if i + 1 < devices.len() {
            println!("  }},");
        } else {
            println!("  }}");
        }
    }
    println!("]");
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// ============================================================================
// Probe all
// ============================================================================

fn probe_all() -> Vec<HwDevice> {
    let mut all = Vec::new();
    all.extend(probe_cpu());
    all.extend(probe_memory());
    all.extend(probe_pci());
    all.extend(probe_usb());
    all.extend(probe_block());
    all.extend(probe_network());
    all.extend(probe_display());
    all.extend(probe_audio());
    all.extend(probe_input());
    all
}

// ============================================================================
// hwinfo personality
// ============================================================================

fn hwinfo_main(args: &[String]) -> i32 {
    let mut opts = HwInfoOptions::default();

    for arg in args {
        match arg.as_str() {
            "--all" => opts.show_all = true,
            "--short" => opts.format = OutputFormat::Short,
            "--json" => opts.format = OutputFormat::Json,
            "--xml" => opts.format = OutputFormat::Xml,
            "--cpu" => opts.filter_class = Some("cpu".to_string()),
            "--memory" | "--ram" => opts.filter_class = Some("memory".to_string()),
            "--disk" | "--storage" => opts.filter_class = Some("disk".to_string()),
            "--network" | "--netcard" => opts.filter_class = Some("network".to_string()),
            "--display" | "--gfxcard" => opts.filter_class = Some("display".to_string()),
            "--sound" => opts.filter_class = Some("sound".to_string()),
            "--usb" => opts.filter_class = Some("usb".to_string()),
            "--input" | "--keyboard" | "--mouse" => opts.filter_class = Some("input".to_string()),
            "--pci" => opts.filter_class = Some("pci".to_string()),
            "--help" | "-h" => {
                println!("Usage: hwinfo [options]");
                println!();
                println!("Probe and display hardware information.");
                println!();
                println!("Output:");
                println!("  --short   Short listing");
                println!("  --json    JSON output");
                println!("  --xml     XML output");
                println!("  --all     Show all details");
                println!();
                println!("Filters:");
                println!("  --cpu, --memory, --disk, --network");
                println!("  --display, --sound, --usb, --input");
                println!();
                println!("  -h, --help    Display this help");
                println!("  --version     Display version");
                return 0;
            }
            "--version" => {
                println!("hwinfo (OurOS) {VERSION}");
                return 0;
            }
            _ => {}
        }
    }

    let all_devices = probe_all();

    let devices: Vec<&HwDevice> = if let Some(ref class) = opts.filter_class {
        all_devices.iter().filter(|d| d.class == *class).collect()
    } else {
        all_devices.iter().collect()
    };

    match opts.format {
        OutputFormat::Short => {
            println!("Hardware summary ({} devices):", devices.len());
            for dev in &devices {
                print_device_short(dev);
            }
        }
        OutputFormat::Json => {
            let owned: Vec<HwDevice> = devices.into_iter().cloned().collect();
            print_devices_json(&owned);
        }
        OutputFormat::Xml => {
            println!("<?xml version=\"1.0\"?>");
            println!("<hwinfo>");
            for dev in &devices {
                println!("  <device class=\"{}\">", dev.class);
                println!("    <description>{}</description>", dev.description);
                println!("    <vendor>{}</vendor>", dev.vendor);
                println!("    <model>{}</model>", dev.model);
                println!("    <driver>{}</driver>", dev.driver);
                println!("  </device>");
            }
            println!("</hwinfo>");
        }
        OutputFormat::Normal => {
            println!("=== Hardware Information ===");
            println!("{} devices found", devices.len());
            println!();
            for (i, dev) in devices.iter().enumerate() {
                print_device_normal(dev, i);
            }
        }
    }

    0
}

// ============================================================================
// lshw personality
// ============================================================================

fn lshw_main(args: &[String]) -> i32 {
    let mut format = OutputFormat::Normal;
    let mut filter_class: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-short" => format = OutputFormat::Short,
            "-json" => format = OutputFormat::Json,
            "-xml" => format = OutputFormat::Xml,
            "-class" => { /* Next arg is the class */ }
            "--help" | "-h" => {
                println!("Usage: lshw [-short] [-json] [-xml] [-class CLASS]");
                println!();
                println!("List hardware.");
                return 0;
            }
            "--version" => {
                println!("lshw (OurOS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                // Could be class name after -class.
                filter_class = Some(s.to_string());
            }
            _ => {}
        }
    }

    let all_devices = probe_all();
    let devices: Vec<&HwDevice> = if let Some(ref class) = filter_class {
        all_devices.iter().filter(|d| d.class == *class).collect()
    } else {
        all_devices.iter().collect()
    };

    match format {
        OutputFormat::Short => {
            println!(
                "{:<20} {:<16} {:<30}",
                "H/W path", "Class", "Description"
            );
            println!("{}", "=".repeat(66));
            for (i, dev) in devices.iter().enumerate() {
                println!(
                    "/sys/device/{:<8} {:<16} {}",
                    i,
                    dev.class,
                    if dev.model.is_empty() {
                        &dev.description
                    } else {
                        &dev.model
                    }
                );
            }
        }
        OutputFormat::Json => {
            let owned: Vec<HwDevice> = devices.into_iter().cloned().collect();
            print_devices_json(&owned);
        }
        OutputFormat::Xml => {
            println!("<?xml version=\"1.0\"?>");
            println!("<list>");
            for dev in &devices {
                println!("  <node class=\"{}\">", dev.class);
                println!("    <description>{}</description>", dev.description);
                println!("  </node>");
            }
            println!("</list>");
        }
        OutputFormat::Normal => {
            for dev in &devices {
                println!("  *-{}", dev.class);
                println!("       description: {}", dev.description);
                if !dev.vendor.is_empty() {
                    println!("       vendor: {}", dev.vendor);
                }
                if !dev.model.is_empty() {
                    println!("       product: {}", dev.model);
                }
                if !dev.driver.is_empty() {
                    println!("       configuration: driver={}", dev.driver);
                }
                println!();
            }
        }
    }

    0
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("hwinfo");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "lshw" => lshw_main(&rest),
        _ => hwinfo_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_pci_display() {
        assert_eq!(classify_pci_device("0x0300"), "display");
        assert_eq!(classify_pci_device("0x0301"), "display");
        assert_eq!(classify_pci_device("0x0302"), "display");
    }

    #[test]
    fn test_classify_pci_network() {
        assert_eq!(classify_pci_device("0x0200"), "network");
    }

    #[test]
    fn test_classify_pci_storage() {
        assert_eq!(classify_pci_device("0x0106"), "storage");
    }

    #[test]
    fn test_classify_pci_sound() {
        assert_eq!(classify_pci_device("0x0403"), "sound");
    }

    #[test]
    fn test_classify_pci_bridge() {
        assert_eq!(classify_pci_device("0x0600"), "bridge");
    }

    #[test]
    fn test_classify_pci_unknown() {
        assert_eq!(classify_pci_device("0xff00"), "other");
    }

    #[test]
    fn test_classify_usb_hub() {
        assert_eq!(classify_usb_device("09"), "usb-hub");
    }

    #[test]
    fn test_classify_usb_input() {
        assert_eq!(classify_usb_device("03"), "input");
    }

    #[test]
    fn test_classify_usb_storage() {
        assert_eq!(classify_usb_device("08"), "usb-storage");
    }

    #[test]
    fn test_classify_usb_unknown() {
        assert_eq!(classify_usb_device("ff"), "usb");
    }

    #[test]
    fn test_hw_device_new() {
        let dev = HwDevice::new("cpu");
        assert_eq!(dev.class, "cpu");
        assert!(dev.description.is_empty());
        assert!(dev.vendor.is_empty());
    }

    #[test]
    fn test_probe_cpu_returns_something() {
        let cpus = probe_cpu();
        assert!(!cpus.is_empty());
    }

    #[test]
    fn test_probe_memory_returns_something() {
        let mem = probe_memory();
        assert!(!mem.is_empty());
        assert_eq!(mem[0].class, "memory");
    }

    #[test]
    fn test_probe_all_returns_something() {
        let all = probe_all();
        assert!(!all.is_empty());
    }

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("hello"), "hello");
        assert_eq!(escape_json("a\"b"), "a\\\"b");
        assert_eq!(escape_json("a\\b"), "a\\\\b");
        assert_eq!(escape_json("a\nb"), "a\\nb");
    }

    #[test]
    fn test_output_format_eq() {
        assert_eq!(OutputFormat::Normal, OutputFormat::Normal);
        assert_ne!(OutputFormat::Short, OutputFormat::Json);
    }

    #[test]
    fn test_default_options() {
        let opts = HwInfoOptions::default();
        assert_eq!(opts.format, OutputFormat::Normal);
        assert!(opts.filter_class.is_none());
        assert!(!opts.show_all);
    }
}
