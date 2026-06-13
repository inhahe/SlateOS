//! Slate OS USB Device Lister
//!
//! Lists USB devices by reading from `/sys/bus/usb/devices/` or
//! `/proc/usb/devices`. Includes a built-in vendor/device ID database
//! for common hardware.
//!
//! # Usage
//!
//! ```text
//! lsusb                           List all USB devices (summary)
//! lsusb -v                        Verbose (class, speed, power, endpoints)
//! lsusb -t                        Tree view (bus topology with hubs)
//! lsusb -s <bus>:<dev>            Filter by bus:device number
//! lsusb -d <vendor>:<product>     Filter by vendor:product ID (hex)
//! lsusb -D <path>                 Show info for specific sysfs device path
//! lsusb --class <class>           Filter by USB class name or number
//! lsusb --json                    JSON output
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// USB device structure
// ============================================================================

#[derive(Clone)]
struct UsbDevice {
    /// Bus number.
    bus: u16,
    /// Device number on this bus.
    devnum: u16,
    /// USB vendor ID (e.g. 0x046d for Logitech).
    vendor_id: u16,
    /// USB product ID.
    product_id: u16,
    /// Manufacturer string from the device descriptor.
    manufacturer: String,
    /// Product name string from the device descriptor.
    product: String,
    /// Serial number string.
    serial: String,
    /// USB device class code (from bDeviceClass).
    device_class: u8,
    /// USB device subclass code.
    device_subclass: u8,
    /// USB device protocol code.
    device_protocol: u8,
    /// USB specification version (e.g. "2.00").
    usb_version: String,
    /// Device speed in Mbps (as string, e.g. "480", "5000").
    speed: String,
    /// Number of configurations.
    num_configurations: u8,
    /// Number of interfaces.
    num_interfaces: u8,
    /// Maximum power draw in mA.
    max_power: String,
    /// The sysfs directory name (e.g. "1-2", "2-1.3").
    sysfs_name: String,
    /// Parent sysfs name for building topology tree.
    parent_name: String,
}

// ============================================================================
// USB class database
// ============================================================================

/// Return a human-readable name for a USB device class code.
fn usb_class_name(class: u8) -> &'static str {
    match class {
        0x00 => "Device",
        0x01 => "Audio",
        0x02 => "Communications (CDC)",
        0x03 => "HID (Human Interface Device)",
        0x05 => "Physical",
        0x06 => "Image",
        0x07 => "Printer",
        0x08 => "Mass Storage",
        0x09 => "Hub",
        0x0A => "CDC-Data",
        0x0B => "Smart Card",
        0x0D => "Content Security",
        0x0E => "Video",
        0x0F => "Personal Healthcare",
        0x10 => "Audio/Video",
        0x11 => "Billboard",
        0xDC => "Diagnostic",
        0xE0 => "Wireless Controller",
        0xEF => "Miscellaneous",
        0xFE => "Application Specific",
        0xFF => "Vendor Specific",
        _ => "Unknown",
    }
}

/// Try to parse a USB class name string back to a numeric class code.
/// Accepts both hex numbers ("09", "0x09") and partial name matches ("hub",
/// "hid", "mass storage", etc.).
fn parse_class_filter(s: &str) -> Option<u8> {
    // Try as hex number first.
    let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
    if let Ok(val) = u8::from_str_radix(trimmed, 16) {
        return Some(val);
    }

    // Try case-insensitive name match.
    let lower = s.to_lowercase();
    let pairs: &[(&str, u8)] = &[
        ("device", 0x00),
        ("audio/video", 0x10),
        ("audio", 0x01),
        ("comm", 0x02),
        ("cdc", 0x02),
        ("hid", 0x03),
        ("human", 0x03),
        ("physical", 0x05),
        ("image", 0x06),
        ("printer", 0x07),
        ("mass", 0x08),
        ("storage", 0x08),
        ("hub", 0x09),
        ("cdc-data", 0x0A),
        ("smart", 0x0B),
        ("content", 0x0D),
        ("video", 0x0E),
        ("health", 0x0F),
        ("billboard", 0x11),
        ("diag", 0xDC),
        ("wireless", 0xE0),
        ("misc", 0xEF),
        ("app", 0xFE),
        ("vendor", 0xFF),
    ];

    for &(name, code) in pairs {
        if lower.contains(name) {
            return Some(code);
        }
    }
    None
}

// ============================================================================
// USB vendor/device ID database
// ============================================================================

/// Look up a vendor name from the built-in database.
fn vendor_name(id: u16) -> &'static str {
    match id {
        0x1D6B => "Linux Foundation",
        0x8086 => "Intel Corp.",
        0x0403 => "Future Technology Devices International (FTDI)",
        0x046D => "Logitech, Inc.",
        0x045E => "Microsoft Corp.",
        0x04F2 => "Chicony Electronics Co., Ltd.",
        0x0BDA => "Realtek Semiconductor Corp.",
        0x05AC => "Apple, Inc.",
        0x1B1C => "Corsair",
        0x054C => "Sony Corp.",
        0x04E8 => "Samsung Electronics Co., Ltd.",
        0x2109 => "VIA Labs, Inc.",
        0x0781 => "SanDisk Corp.",
        0x0951 => "Kingston Technology",
        0x1058 => "Western Digital Technologies",
        0x090C => "Silicon Motion, Inc.",
        0x0CF3 => "Qualcomm Atheros Communications",
        0x0B05 => "ASUSTek Computer, Inc.",
        0x1532 => "Razer USA, Ltd.",
        0x1038 => "SteelSeries ApS",
        0x2357 => "TP-Link",
        0x148F => "Ralink Technology Corp.",
        0x0E8D => "MediaTek Inc.",
        0x1A40 => "Terminus Technology Inc.",
        0x058F => "Alcor Micro Corp.",
        0x1366 => "SEGGER",
        0x2C7C => "Quectel Wireless Solutions",
        0x067B => "Prolific Technology, Inc.",
        0x10C4 => "Silicon Labs",
        0x1004 => "LG Electronics",
        0x18D1 => "Google Inc.",
        0x22B8 => "Motorola PCS",
        _ => "",
    }
}

/// Look up a device name from the built-in database. Keyed on (vendor, product).
fn device_name(vendor: u16, product: u16) -> &'static str {
    match (vendor, product) {
        // Linux Foundation
        (0x1D6B, 0x0001) => "1.1 Root Hub",
        (0x1D6B, 0x0002) => "2.0 Root Hub",
        (0x1D6B, 0x0003) => "3.0 Root Hub",
        (0x1D6B, 0x0004) => "3.1 Root Hub",
        // Intel
        (0x8086, 0x0001) => "XHCI Host Controller",
        // FTDI
        (0x0403, 0x6001) => "FT232 Serial (UART)",
        (0x0403, 0x6010) => "FT2232 Dual UART/FIFO",
        (0x0403, 0x6011) => "FT4232 Quad UART",
        (0x0403, 0x6014) => "FT232H Single HS USB-UART/FIFO",
        (0x0403, 0x6015) => "FT-X Series",
        // Logitech
        (0x046D, 0xC077) => "M105 Optical Mouse",
        (0x046D, 0xC534) => "Unifying Receiver",
        (0x046D, 0xC52B) => "Unifying Receiver",
        (0x046D, 0x0825) => "Webcam C270",
        (0x046D, 0x085C) => "C922 Pro Stream Webcam",
        (0x046D, 0xC33A) => "G413 Gaming Keyboard",
        // Microsoft
        (0x045E, 0x0745) => "Nano Transceiver",
        (0x045E, 0x07A5) => "Wireless Receiver",
        (0x045E, 0x028E) => "Xbox360 Controller",
        (0x045E, 0x0B12) => "Xbox Wireless Controller",
        // Chicony
        (0x04F2, 0xB604) => "Integrated Camera",
        (0x04F2, 0xB6D9) => "HP TrueVision HD Camera",
        // Realtek
        (0x0BDA, 0x8179) => "RTL8188EUS 802.11n Wireless",
        (0x0BDA, 0x8812) => "RTL8812AU 802.11ac Wireless",
        (0x0BDA, 0x0129) => "RTS5129 Card Reader",
        (0x0BDA, 0x8153) => "RTL8153 Gigabit Ethernet",
        // Apple
        (0x05AC, 0x8233) => "Bluetooth Host Controller",
        (0x05AC, 0x0265) => "Magic Trackpad 2",
        (0x05AC, 0x024F) => "Aluminium Keyboard (ISO)",
        // Corsair
        (0x1B1C, 0x1B13) => "K70 RGB Keyboard",
        (0x1B1C, 0x1B2E) => "HS80 RGB Wireless Headset",
        // Samsung
        (0x04E8, 0x6860) => "Galaxy A/J/S Smartphone (MTP)",
        // VIA Labs
        (0x2109, 0x3431) => "USB 3.0 Hub",
        (0x2109, 0x2817) => "USB 2.0 Hub",
        // SanDisk
        (0x0781, 0x5567) => "Cruzer Blade",
        (0x0781, 0x5583) => "Ultra Fit",
        (0x0781, 0x5591) => "Ultra Flair",
        // Kingston
        (0x0951, 0x1666) => "DataTraveler 100 G3",
        // Prolific
        (0x067B, 0x2303) => "PL2303 Serial Port",
        // Silicon Labs
        (0x10C4, 0xEA60) => "CP2102 USB to UART Bridge",
        // Google
        (0x18D1, 0x4EE1) => "Nexus/Pixel (MTP+ADB)",
        _ => "",
    }
}

// ============================================================================
// sysfs scanner
// ============================================================================

/// Read a sysfs attribute file, returning None if it does not exist or cannot
/// be read. Trims trailing whitespace/newlines.
fn read_attr(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a sysfs attribute file and parse as hex u16, returning 0 on failure.
fn read_hex_u16(path: &str) -> u16 {
    read_attr(path)
        .and_then(|s| {
            let s = s.trim_start_matches("0x");
            u16::from_str_radix(s, 16).ok()
        })
        .unwrap_or(0)
}

/// Read a sysfs attribute file and parse as hex u8, returning 0 on failure.
fn read_hex_u8(path: &str) -> u8 {
    read_attr(path)
        .and_then(|s| {
            let s = s.trim_start_matches("0x");
            u8::from_str_radix(s, 16).ok()
        })
        .unwrap_or(0)
}

/// Determine the parent sysfs name for a USB device path.
///
/// USB sysfs names encode topology: `<bus>-<port>.<port>...`
/// The parent of "1-2.3" is "1-2"; the parent of "1-2" is "usb1" (the root
/// hub); root hubs have no parent.
fn parent_sysfs_name(name: &str) -> String {
    // Root hubs are named "usbN" -- no parent.
    if name.starts_with("usb") {
        return String::new();
    }

    // Strip last ".port" component if present (e.g. "1-2.3" -> "1-2").
    if let Some(pos) = name.rfind('.') {
        return name[..pos].to_string();
    }

    // Top-level port (e.g. "1-2") -- parent is the root hub "usbN".
    if let Some(pos) = name.find('-') {
        let bus_part = &name[..pos];
        return format!("usb{bus_part}");
    }

    String::new()
}

/// Scan `/sys/bus/usb/devices/` for USB devices.
fn scan_sysfs() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let usb_path = "/sys/bus/usb/devices";

    let entries = match fs::read_dir(usb_path) {
        Ok(e) => e,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let dev_path = format!("{usb_path}/{name}");

        // Every USB device directory must have an idVendor file. Interface
        // directories (e.g. "1-2:1.0") do not, so this test also filters them
        // out.
        let vendor_path = format!("{dev_path}/idVendor");
        if fs::metadata(&vendor_path).is_err() {
            continue;
        }

        let vendor_id = read_hex_u16(&vendor_path);
        let product_id = read_hex_u16(&format!("{dev_path}/idProduct"));
        let manufacturer = read_attr(&format!("{dev_path}/manufacturer")).unwrap_or_default();
        let product = read_attr(&format!("{dev_path}/product")).unwrap_or_default();
        let serial = read_attr(&format!("{dev_path}/serial")).unwrap_or_default();
        let device_class = read_hex_u8(&format!("{dev_path}/bDeviceClass"));
        let device_subclass = read_hex_u8(&format!("{dev_path}/bDeviceSubClass"));
        let device_protocol = read_hex_u8(&format!("{dev_path}/bDeviceProtocol"));
        let usb_version = read_attr(&format!("{dev_path}/bcdUSB")).unwrap_or_default();
        let speed = read_attr(&format!("{dev_path}/speed")).unwrap_or_default();
        let num_configurations = read_attr(&format!("{dev_path}/bNumConfigurations"))
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(0);
        let num_interfaces = read_attr(&format!("{dev_path}/bNumInterfaces"))
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(0);
        let max_power = read_attr(&format!("{dev_path}/bMaxPower")).unwrap_or_default();

        let bus = read_attr(&format!("{dev_path}/busnum"))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let devnum = read_attr(&format!("{dev_path}/devnum"))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        let parent_name = parent_sysfs_name(&name);

        devices.push(UsbDevice {
            bus,
            devnum,
            vendor_id,
            product_id,
            manufacturer,
            product,
            serial,
            device_class,
            device_subclass,
            device_protocol,
            usb_version,
            speed,
            num_configurations,
            num_interfaces,
            max_power,
            sysfs_name: name,
            parent_name,
        });
    }

    // Sort by bus, then device number.
    devices.sort_by_key(|d| (d.bus, d.devnum));
    devices
}

/// Scan a single sysfs device path (for `-D <path>`).
fn scan_single_sysfs(path: &str) -> Vec<UsbDevice> {
    let mut devices = Vec::new();

    let vendor_path = format!("{path}/idVendor");
    if fs::metadata(&vendor_path).is_err() {
        return devices;
    }

    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let vendor_id = read_hex_u16(&vendor_path);
    let product_id = read_hex_u16(&format!("{path}/idProduct"));
    let manufacturer = read_attr(&format!("{path}/manufacturer")).unwrap_or_default();
    let product = read_attr(&format!("{path}/product")).unwrap_or_default();
    let serial = read_attr(&format!("{path}/serial")).unwrap_or_default();
    let device_class = read_hex_u8(&format!("{path}/bDeviceClass"));
    let device_subclass = read_hex_u8(&format!("{path}/bDeviceSubClass"));
    let device_protocol = read_hex_u8(&format!("{path}/bDeviceProtocol"));
    let usb_version = read_attr(&format!("{path}/bcdUSB")).unwrap_or_default();
    let speed = read_attr(&format!("{path}/speed")).unwrap_or_default();
    let num_configurations = read_attr(&format!("{path}/bNumConfigurations"))
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let num_interfaces = read_attr(&format!("{path}/bNumInterfaces"))
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);
    let max_power = read_attr(&format!("{path}/bMaxPower")).unwrap_or_default();
    let bus = read_attr(&format!("{path}/busnum"))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let devnum = read_attr(&format!("{path}/devnum"))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let parent_name = parent_sysfs_name(&name);

    devices.push(UsbDevice {
        bus,
        devnum,
        vendor_id,
        product_id,
        manufacturer,
        product,
        serial,
        device_class,
        device_subclass,
        device_protocol,
        usb_version,
        speed,
        num_configurations,
        num_interfaces,
        max_power,
        sysfs_name: name,
        parent_name,
    });

    devices
}

// ============================================================================
// /proc/usb/devices fallback scanner
// ============================================================================

/// Parse the consolidated `/proc/usb/devices` text format.
///
/// Expected format (one block per device, separated by blank lines):
/// ```text
/// T:  Bus=01 Lev=00 Prnt=00 Port=00 Cnt=00 Dev#=  1 Spd=480 MxCh= 6
/// D:  Ver= 2.00 Cls=09(hub  ) Sub=00 Prot=01 MxPS=64 #Cfgs=  1
/// P:  Vendor=1d6b ProdID=0002 Rev= 5.15
/// S:  Manufacturer=Linux Foundation
/// S:  Product=USB 2.0 Root Hub
/// S:  SerialNumber=0000:00:14.0
/// C:  ...
/// ```
fn scan_proc_usb() -> Vec<UsbDevice> {
    let mut devices = Vec::new();

    let content = match read_attr("/proc/usb/devices") {
        Some(c) => c,
        None => return devices,
    };

    let mut current: Option<UsbDevice> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            if let Some(dev) = current.take() {
                devices.push(dev);
            }
            continue;
        }

        // Each line starts with a letter and colon.
        let (tag, rest) = match line.split_once(':') {
            Some(pair) => pair,
            None => continue,
        };
        let rest = rest.trim();

        match tag.trim() {
            "T" => {
                // Topology line: Bus=NN ... Dev#=NN Spd=NNN ...
                let mut dev = UsbDevice {
                    bus: 0,
                    devnum: 0,
                    vendor_id: 0,
                    product_id: 0,
                    manufacturer: String::new(),
                    product: String::new(),
                    serial: String::new(),
                    device_class: 0,
                    device_subclass: 0,
                    device_protocol: 0,
                    usb_version: String::new(),
                    speed: String::new(),
                    num_configurations: 0,
                    num_interfaces: 0,
                    max_power: String::new(),
                    sysfs_name: String::new(),
                    parent_name: String::new(),
                };
                for part in rest.split_whitespace() {
                    if let Some((k, v)) = part.split_once('=') {
                        match k {
                            "Bus" => dev.bus = v.trim().parse().unwrap_or(0),
                            "Dev#" => dev.devnum = v.trim().parse().unwrap_or(0),
                            "Spd" => dev.speed = v.trim().to_string(),
                            _ => {}
                        }
                    }
                }
                current = Some(dev);
            }
            "D" => {
                // Device descriptor line: Ver=X.XX Cls=XX(...) Sub=XX Prot=XX
                if let Some(ref mut dev) = current {
                    for part in rest.split_whitespace() {
                        if let Some((k, v)) = part.split_once('=') {
                            match k {
                                "Ver" => dev.usb_version = v.trim().to_string(),
                                "Cls" => {
                                    // Format: "09(hub  )" -- take the hex part.
                                    let hex_part = v.split('(').next().unwrap_or(v);
                                    dev.device_class =
                                        u8::from_str_radix(hex_part.trim(), 16).unwrap_or(0);
                                }
                                "Sub" => {
                                    dev.device_subclass =
                                        u8::from_str_radix(v.trim(), 16).unwrap_or(0);
                                }
                                "Prot" => {
                                    dev.device_protocol =
                                        u8::from_str_radix(v.trim(), 16).unwrap_or(0);
                                }
                                "#Cfgs" => {
                                    dev.num_configurations = v.trim().parse().unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "P" => {
                // Product ID line: Vendor=XXXX ProdID=XXXX
                if let Some(ref mut dev) = current {
                    for part in rest.split_whitespace() {
                        if let Some((k, v)) = part.split_once('=') {
                            match k {
                                "Vendor" => {
                                    dev.vendor_id =
                                        u16::from_str_radix(v.trim(), 16).unwrap_or(0);
                                }
                                "ProdID" => {
                                    dev.product_id =
                                        u16::from_str_radix(v.trim(), 16).unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "S" => {
                // String descriptor line.
                if let Some(ref mut dev) = current
                    && let Some(val) = rest.split_once('=').map(|(_, v)| v.trim().to_string()) {
                        if rest.starts_with("Manufacturer") {
                            dev.manufacturer = val;
                        } else if rest.starts_with("Product") {
                            dev.product = val;
                        } else if rest.starts_with("SerialNumber") {
                            dev.serial = val;
                        }
                    }
            }
            _ => {
                // Ignore C:, I:, E: lines for the summary scan.
            }
        }
    }

    // Flush final device.
    if let Some(dev) = current.take() {
        devices.push(dev);
    }

    devices.sort_by_key(|d| (d.bus, d.devnum));
    devices
}

// ============================================================================
// Display helpers
// ============================================================================

/// Build a display-friendly device description from the built-in database
/// and the sysfs manufacturer/product strings.
fn device_description(dev: &UsbDevice) -> (String, String) {
    let mfr = if !dev.manufacturer.is_empty() {
        dev.manufacturer.clone()
    } else {
        let db_name = vendor_name(dev.vendor_id);
        if db_name.is_empty() {
            String::new()
        } else {
            db_name.to_string()
        }
    };

    let prod = if !dev.product.is_empty() {
        dev.product.clone()
    } else {
        let db_name = device_name(dev.vendor_id, dev.product_id);
        if db_name.is_empty() {
            String::new()
        } else {
            db_name.to_string()
        }
    };

    (mfr, prod)
}

/// Format a USB speed string into a human-friendly label.
fn speed_label(speed: &str) -> &str {
    match speed {
        "1.5" => "1.5Mbps (Low Speed)",
        "12" => "12Mbps (Full Speed)",
        "480" => "480Mbps (High Speed)",
        "5000" => "5000Mbps (Super Speed)",
        "10000" => "10000Mbps (Super Speed+)",
        "20000" => "20000Mbps (Super Speed+ 2x2)",
        _ => speed,
    }
}

// ============================================================================
// Output: summary
// ============================================================================

fn display_summary(dev: &UsbDevice) {
    let (mfr, prod) = device_description(dev);
    let desc = match (mfr.is_empty(), prod.is_empty()) {
        (false, false) => format!("{mfr} {prod}"),
        (false, true) => mfr,
        (true, false) => prod,
        (true, true) => String::new(),
    };
    println!(
        "Bus {:03} Device {:03}: ID {:04x}:{:04x} {desc}",
        dev.bus, dev.devnum, dev.vendor_id, dev.product_id,
    );
}

// ============================================================================
// Output: verbose
// ============================================================================

fn display_verbose(dev: &UsbDevice) {
    display_summary(dev);

    let class_str = usb_class_name(dev.device_class);
    println!(
        "  Device Class:    0x{:02x} ({class_str})",
        dev.device_class,
    );
    println!("  Device Subclass: 0x{:02x}", dev.device_subclass);
    println!("  Device Protocol: 0x{:02x}", dev.device_protocol);

    if !dev.usb_version.is_empty() {
        println!("  USB Version:     {}", dev.usb_version);
    }

    if !dev.speed.is_empty() {
        println!("  Speed:           {}", speed_label(&dev.speed));
    }

    if !dev.max_power.is_empty() {
        println!("  Max Power:       {}", dev.max_power);
    }

    if dev.num_configurations > 0 {
        println!("  Configurations:  {}", dev.num_configurations);
    }

    if dev.num_interfaces > 0 {
        println!("  Interfaces:      {}", dev.num_interfaces);
    }

    if !dev.manufacturer.is_empty() {
        println!("  Manufacturer:    {}", dev.manufacturer);
    }

    if !dev.product.is_empty() {
        println!("  Product:         {}", dev.product);
    }

    if !dev.serial.is_empty() {
        println!("  Serial Number:   {}", dev.serial);
    }

    println!("  Sysfs path:      {}", dev.sysfs_name);
    println!();
}

// ============================================================================
// Output: tree
// ============================================================================

fn display_tree(devices: &[UsbDevice]) {
    // Build a map from sysfs_name -> device for lookup.
    let name_map: std::collections::HashMap<&str, &UsbDevice> = devices
        .iter()
        .map(|d| (d.sysfs_name.as_str(), d))
        .collect();

    // Identify root hubs (names starting with "usb").
    let mut roots: Vec<&UsbDevice> = devices
        .iter()
        .filter(|d| d.sysfs_name.starts_with("usb"))
        .collect();
    roots.sort_by_key(|d| d.bus);

    for root in &roots {
        let (mfr, prod) = device_description(root);
        let desc = if !prod.is_empty() {
            prod
        } else if !mfr.is_empty() {
            mfr
        } else {
            format!("{:04x}:{:04x}", root.vendor_id, root.product_id)
        };

        let speed_str = if root.speed.is_empty() {
            String::new()
        } else {
            format!(", {}", speed_label(&root.speed))
        };

        println!(
            "/:  Bus {:03} Dev {:03}{speed_str}",
            root.bus, root.devnum,
        );
        println!("    ID {:04x}:{:04x} {desc}", root.vendor_id, root.product_id);

        // Gather children of this root.
        let children = gather_children(&root.sysfs_name, devices);
        print_tree_children(&children, devices, &name_map, "    ", 1);
    }
}

/// Gather direct children of a given sysfs parent name.
fn gather_children<'a>(parent: &str, devices: &'a [UsbDevice]) -> Vec<&'a UsbDevice> {
    let mut kids: Vec<&UsbDevice> = devices
        .iter()
        .filter(|d| d.parent_name == parent)
        .collect();
    kids.sort_by_key(|d| d.devnum);
    kids
}

/// Recursively print tree children with box-drawing indentation.
fn print_tree_children(
    children: &[&UsbDevice],
    all_devices: &[UsbDevice],
    _name_map: &std::collections::HashMap<&str, &UsbDevice>,
    indent: &str,
    _depth: usize,
) {
    for (i, child) in children.iter().enumerate() {
        let is_last = i == children.len() - 1;
        let connector = if is_last { "\\__" } else { "|__" };
        let child_indent = if is_last {
            format!("{indent}    ")
        } else {
            format!("{indent}|   ")
        };

        let (mfr, prod) = device_description(child);
        let desc = match (mfr.is_empty(), prod.is_empty()) {
            (false, false) => format!("{mfr} {prod}"),
            (false, true) => mfr,
            (true, false) => prod,
            (true, true) => format!("{:04x}:{:04x}", child.vendor_id, child.product_id),
        };

        let class_str = usb_class_name(child.device_class);
        let speed_str = if child.speed.is_empty() {
            String::new()
        } else {
            format!(", {}", &child.speed)
        };

        println!(
            "{indent}{connector} Dev {:03}, {class_str}{speed_str}",
            child.devnum,
        );
        println!(
            "{child_indent}ID {:04x}:{:04x} {desc}",
            child.vendor_id, child.product_id,
        );

        // Recurse into this child's children.
        let grandchildren = gather_children(&child.sysfs_name, all_devices);
        if !grandchildren.is_empty() {
            print_tree_children(
                &grandchildren,
                all_devices,
                _name_map,
                &child_indent,
                _depth + 1,
            );
        }
    }
}

// ============================================================================
// Output: JSON
// ============================================================================

/// Escape a string for safe embedding in JSON.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Control characters as \u00XX.
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("\\u{:04x}", c as u32),
                );
            }
            c => out.push(c),
        }
    }
    out
}

fn display_json(devices: &[UsbDevice]) {
    println!("[");
    for (i, dev) in devices.iter().enumerate() {
        let comma = if i < devices.len() - 1 { "," } else { "" };
        let (mfr, prod) = device_description(dev);
        let class_str = usb_class_name(dev.device_class);
        println!(
            "  {{\
             \"bus\":{},\
             \"device\":{},\
             \"vendor_id\":\"{:04x}\",\
             \"product_id\":\"{:04x}\",\
             \"manufacturer\":\"{}\",\
             \"product\":\"{}\",\
             \"serial\":\"{}\",\
             \"class\":\"0x{:02x}\",\
             \"class_name\":\"{class_str}\",\
             \"subclass\":\"0x{:02x}\",\
             \"protocol\":\"0x{:02x}\",\
             \"usb_version\":\"{}\",\
             \"speed\":\"{}\",\
             \"max_power\":\"{}\",\
             \"num_configurations\":{},\
             \"num_interfaces\":{}\
             }}{comma}",
            dev.bus,
            dev.devnum,
            dev.vendor_id,
            dev.product_id,
            json_escape(&mfr),
            json_escape(&prod),
            json_escape(&dev.serial),
            dev.device_class,
            dev.device_subclass,
            dev.device_protocol,
            json_escape(&dev.usb_version),
            json_escape(&dev.speed),
            json_escape(&dev.max_power),
            dev.num_configurations,
            dev.num_interfaces,
        );
    }
    println!("]");
}

// ============================================================================
// CLI argument parsing
// ============================================================================

struct Config {
    /// Show verbose per-device details.
    verbose: bool,
    /// Show bus topology tree.
    tree: bool,
    /// Output as JSON.
    json: bool,
    /// Filter: only show device on this bus:devnum.
    filter_bus_dev: Option<(u16, u16)>,
    /// Filter: only show devices matching vendor:product.
    filter_vendor_product: Option<(Option<u16>, Option<u16>)>,
    /// Show info for a specific sysfs device path.
    device_path: Option<String>,
    /// Filter by USB class code.
    filter_class: Option<u8>,
}

fn print_usage() {
    println!("Slate OS USB Device Lister v0.1.0");
    println!();
    println!("List USB devices and their properties.");
    println!();
    println!("USAGE:");
    println!("  lsusb [options]");
    println!();
    println!("OPTIONS:");
    println!("  -v, --verbose         Show detailed device info");
    println!("  -t, --tree            Tree view (bus topology)");
    println!("  -s <bus>:<dev>        Show only specific device");
    println!("  -d <vendor>:<product> Filter by vendor:product ID (hex)");
    println!("  -D <path>             Show info for specific sysfs device path");
    println!("  --class <class>       Filter by USB class (name or hex number)");
    println!("  --json                JSON output");
    println!("  --help, -h            Show this help");
    println!();
    println!("USB CLASSES:");
    println!("  00 Device          01 Audio         02 CDC");
    println!("  03 HID             05 Physical      06 Image");
    println!("  07 Printer         08 Mass Storage   09 Hub");
    println!("  0a CDC-Data        0e Video          e0 Wireless");
    println!("  ef Miscellaneous   ff Vendor Specific");
}

/// Parse a `<bus>:<dev>` string where either part may be omitted.
fn parse_bus_dev(s: &str) -> Result<(u16, u16), String> {
    let (bus_str, dev_str) = s.split_once(':')
        .ok_or_else(|| format!("expected <bus>:<dev>, got '{s}'"))?;

    let bus = bus_str.trim().parse::<u16>()
        .map_err(|_| format!("invalid bus number: '{bus_str}'"))?;
    let dev = dev_str.trim().parse::<u16>()
        .map_err(|_| format!("invalid device number: '{dev_str}'"))?;

    Ok((bus, dev))
}

/// Parse a `<vendor>:<product>` hex filter string. Either side may be empty
/// to act as a wildcard.
fn parse_vendor_product(s: &str) -> Result<(Option<u16>, Option<u16>), String> {
    let (v_str, p_str) = s.split_once(':')
        .ok_or_else(|| format!("expected <vendor>:<product>, got '{s}'"))?;

    let vendor = if v_str.trim().is_empty() {
        None
    } else {
        Some(
            u16::from_str_radix(v_str.trim(), 16)
                .map_err(|_| format!("invalid vendor ID: '{v_str}'"))?,
        )
    };

    let product = if p_str.trim().is_empty() {
        None
    } else {
        Some(
            u16::from_str_radix(p_str.trim(), 16)
                .map_err(|_| format!("invalid product ID: '{p_str}'"))?,
        )
    };

    Ok((vendor, product))
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        verbose: false,
        tree: false,
        json: false,
        filter_bus_dev: None,
        filter_vendor_product: None,
        device_path: None,
        filter_class: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => {
                config.verbose = true;
                i += 1;
            }
            "-t" | "--tree" => {
                config.tree = true;
                i += 1;
            }
            "--json" => {
                config.json = true;
                i += 1;
            }
            "-s" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -s requires <bus>:<dev>");
                    process::exit(1);
                }
                match parse_bus_dev(&args[i + 1]) {
                    Ok(bd) => config.filter_bus_dev = Some(bd),
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -d requires <vendor>:<product>");
                    process::exit(1);
                }
                match parse_vendor_product(&args[i + 1]) {
                    Ok(vp) => config.filter_vendor_product = Some(vp),
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "-D" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -D requires a device path");
                    process::exit(1);
                }
                config.device_path = Some(args[i + 1].clone());
                i += 2;
            }
            "--class" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --class requires a class name or number");
                    process::exit(1);
                }
                match parse_class_filter(&args[i + 1]) {
                    Some(cls) => config.filter_class = Some(cls),
                    None => {
                        eprintln!("error: unknown USB class: '{}'", args[i + 1]);
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("Try 'lsusb --help' for usage.");
                process::exit(1);
            }
        }
    }

    // Scan for devices.
    let mut devices = if let Some(ref path) = config.device_path {
        scan_single_sysfs(path)
    } else {
        let devs = scan_sysfs();
        if devs.is_empty() {
            scan_proc_usb()
        } else {
            devs
        }
    };

    if devices.is_empty() {
        eprintln!(
            "No USB devices found (is /sys/bus/usb or /proc/usb/devices available?)"
        );
        process::exit(1);
    }

    // Apply filters.
    if let Some((bus, dev)) = config.filter_bus_dev {
        devices.retain(|d| d.bus == bus && d.devnum == dev);
    }
    if let Some((ref vendor_opt, ref product_opt)) = config.filter_vendor_product {
        devices.retain(|d| {
            let v_match = vendor_opt.is_none_or(|v| d.vendor_id == v);
            let p_match = product_opt.is_none_or(|p| d.product_id == p);
            v_match && p_match
        });
    }
    if let Some(cls) = config.filter_class {
        devices.retain(|d| d.device_class == cls);
    }

    // Display.
    if config.json {
        display_json(&devices);
    } else if config.tree {
        display_tree(&devices);
    } else if config.verbose {
        for dev in &devices {
            display_verbose(dev);
        }
    } else {
        for dev in &devices {
            display_summary(dev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- usb_class_name ----------------------------------------------------

    #[test]
    fn usb_class_name_known_codes() {
        assert_eq!(usb_class_name(0x03), "HID (Human Interface Device)");
        assert_eq!(usb_class_name(0x08), "Mass Storage");
        assert_eq!(usb_class_name(0x09), "Hub");
        assert_eq!(usb_class_name(0xFF), "Vendor Specific");
    }

    #[test]
    fn usb_class_name_unknown_codes_return_unknown() {
        // 0x99 is not in the class table.
        assert_eq!(usb_class_name(0x99), "Unknown");
    }

    // ---- parse_class_filter ------------------------------------------------

    #[test]
    fn parse_class_filter_accepts_plain_hex() {
        assert_eq!(parse_class_filter("09"), Some(0x09));
        assert_eq!(parse_class_filter("ff"), Some(0xFF));
    }

    #[test]
    fn parse_class_filter_accepts_0x_prefix() {
        assert_eq!(parse_class_filter("0x09"), Some(0x09));
        assert_eq!(parse_class_filter("0X0E"), Some(0x0E));
    }

    #[test]
    fn parse_class_filter_accepts_partial_names() {
        assert_eq!(parse_class_filter("hub"), Some(0x09));
        assert_eq!(parse_class_filter("HID"), Some(0x03));
        assert_eq!(parse_class_filter("mass storage"), Some(0x08));
    }

    #[test]
    fn parse_class_filter_prefers_av_over_audio_substring() {
        // "audio/video" must match before bare "audio" because of ordering
        // in the table — the substring search short-circuits on first hit.
        assert_eq!(parse_class_filter("audio/video"), Some(0x10));
    }

    #[test]
    fn parse_class_filter_returns_none_for_unknown_name() {
        assert_eq!(parse_class_filter("definitely-not-a-class"), None);
    }

    // ---- vendor_name / device_name -----------------------------------------

    #[test]
    fn vendor_name_known_vendors() {
        assert_eq!(vendor_name(0x046D), "Logitech, Inc.");
        assert_eq!(vendor_name(0x8086), "Intel Corp.");
        assert_eq!(vendor_name(0x1D6B), "Linux Foundation");
    }

    #[test]
    fn vendor_name_unknown_returns_empty() {
        assert_eq!(vendor_name(0xDEAD), "");
    }

    #[test]
    fn device_name_known_combo() {
        assert_eq!(device_name(0x1D6B, 0x0002), "2.0 Root Hub");
        assert_eq!(device_name(0x046D, 0xC52B), "Unifying Receiver");
    }

    #[test]
    fn device_name_unknown_combo_returns_empty() {
        // Known vendor, unknown product.
        assert_eq!(device_name(0x046D, 0xBEEF), "");
        // Totally unknown.
        assert_eq!(device_name(0xDEAD, 0xBEEF), "");
    }

    // ---- parent_sysfs_name -------------------------------------------------

    #[test]
    fn parent_sysfs_name_root_hubs_have_no_parent() {
        assert_eq!(parent_sysfs_name("usb1"), "");
        assert_eq!(parent_sysfs_name("usb2"), "");
    }

    #[test]
    fn parent_sysfs_name_strips_last_dotted_port() {
        assert_eq!(parent_sysfs_name("1-2.3"), "1-2");
        assert_eq!(parent_sysfs_name("1-2.3.4"), "1-2.3");
    }

    #[test]
    fn parent_sysfs_name_top_level_port_maps_to_root_hub() {
        assert_eq!(parent_sysfs_name("1-2"), "usb1");
        assert_eq!(parent_sysfs_name("2-1"), "usb2");
    }

    #[test]
    fn parent_sysfs_name_unrecognised_string_returns_empty() {
        assert_eq!(parent_sysfs_name("garbage"), "");
    }

    // ---- speed_label -------------------------------------------------------

    #[test]
    fn speed_label_known_speeds() {
        assert_eq!(speed_label("480"), "480Mbps (High Speed)");
        assert_eq!(speed_label("5000"), "5000Mbps (Super Speed)");
        assert_eq!(speed_label("1.5"), "1.5Mbps (Low Speed)");
    }

    #[test]
    fn speed_label_unknown_passes_through() {
        // Unknown speeds get returned unchanged so the user still sees them.
        assert_eq!(speed_label("123456"), "123456");
        assert_eq!(speed_label(""), "");
    }

    // ---- json_escape -------------------------------------------------------

    #[test]
    fn json_escape_passes_plain_ascii() {
        assert_eq!(json_escape("hello world"), "hello world");
    }

    #[test]
    fn json_escape_escapes_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"he said "hi""#), r#"he said \"hi\""#);
        assert_eq!(json_escape(r"C:\path"), r"C:\\path");
    }

    #[test]
    fn json_escape_escapes_control_chars() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("a\rb"), "a\\rb");
    }

    #[test]
    fn json_escape_unicode_control_uses_u_escape() {
        // \x01 is below \x20 and not one of the explicit escapes, so it falls
        // through to the \u00XX path.
        assert_eq!(json_escape("\x01"), "\\u0001");
    }

    #[test]
    fn json_escape_unicode_passthrough_for_printable() {
        // Non-ASCII printable characters pass through untouched (consumer
        // can re-encode if it wants \u-form, but lsusb's UTF-8 output is
        // also valid JSON).
        assert_eq!(json_escape("Realtek™"), "Realtek™");
    }

    // ---- parse_bus_dev -----------------------------------------------------

    #[test]
    fn parse_bus_dev_normal_case() {
        assert_eq!(parse_bus_dev("1:2"), Ok((1, 2)));
        assert_eq!(parse_bus_dev("003:045"), Ok((3, 45)));
    }

    #[test]
    fn parse_bus_dev_missing_colon_is_error() {
        assert!(parse_bus_dev("12").is_err());
    }

    #[test]
    fn parse_bus_dev_non_numeric_is_error() {
        assert!(parse_bus_dev("a:b").is_err());
        assert!(parse_bus_dev("1:x").is_err());
        assert!(parse_bus_dev("x:1").is_err());
    }

    // ---- parse_vendor_product ----------------------------------------------

    #[test]
    fn parse_vendor_product_both_sides_set() {
        assert_eq!(
            parse_vendor_product("046d:c52b"),
            Ok((Some(0x046D), Some(0xC52B))),
        );
    }

    #[test]
    fn parse_vendor_product_vendor_wildcard() {
        assert_eq!(
            parse_vendor_product(":c52b"),
            Ok((None, Some(0xC52B))),
        );
    }

    #[test]
    fn parse_vendor_product_product_wildcard() {
        assert_eq!(
            parse_vendor_product("046d:"),
            Ok((Some(0x046D), None)),
        );
    }

    #[test]
    fn parse_vendor_product_both_wildcards() {
        assert_eq!(parse_vendor_product(":"), Ok((None, None)));
    }

    #[test]
    fn parse_vendor_product_invalid_hex_is_error() {
        assert!(parse_vendor_product("zz:c52b").is_err());
        assert!(parse_vendor_product("046d:zz").is_err());
    }

    #[test]
    fn parse_vendor_product_missing_colon_is_error() {
        assert!(parse_vendor_product("046dc52b").is_err());
    }

    // ---- device_description ------------------------------------------------

    fn make_dev(vendor: u16, product: u16, mfr: &str, prod: &str) -> UsbDevice {
        UsbDevice {
            bus: 1,
            devnum: 1,
            vendor_id: vendor,
            product_id: product,
            manufacturer: mfr.to_string(),
            product: prod.to_string(),
            serial: String::new(),
            device_class: 0,
            device_subclass: 0,
            device_protocol: 0,
            usb_version: String::new(),
            speed: String::new(),
            num_configurations: 0,
            num_interfaces: 0,
            max_power: String::new(),
            sysfs_name: String::new(),
            parent_name: String::new(),
        }
    }

    #[test]
    fn device_description_prefers_sysfs_strings_over_db() {
        // Sysfs says "Acme Co." and "Widget" — db has different names, but
        // the device's own strings win.
        let dev = make_dev(0x046D, 0xC52B, "Acme Co.", "Widget");
        let (mfr, prod) = device_description(&dev);
        assert_eq!(mfr, "Acme Co.");
        assert_eq!(prod, "Widget");
    }

    #[test]
    fn device_description_falls_back_to_db_when_sysfs_empty() {
        // Known vendor/product, but sysfs strings are empty.
        let dev = make_dev(0x046D, 0xC52B, "", "");
        let (mfr, prod) = device_description(&dev);
        assert_eq!(mfr, "Logitech, Inc.");
        assert_eq!(prod, "Unifying Receiver");
    }

    #[test]
    fn device_description_empty_when_unknown_and_no_sysfs() {
        // Unknown vendor/product and no sysfs strings -> both empty.
        let dev = make_dev(0xDEAD, 0xBEEF, "", "");
        let (mfr, prod) = device_description(&dev);
        assert_eq!(mfr, "");
        assert_eq!(prod, "");
    }
}
