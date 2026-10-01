//! The machine's devices, as the kernel publishes them, turned into the rows
//! of the device tree.
//!
//! Everything comes through `hwquery` -- the reader System Information uses,
//! so the two programs cannot disagree about the machine:
//!
//! | Rows | From |
//! |---|---|
//! | every PCI function, sorted into a category by its class | `/sys/devices/pci` |
//! | each disk the kernel has registered | `/sys/devices/block` |
//! | each network interface but loopback | `/proc/net` (Linux: `/proc/net/dev`) |
//! | each display output | `/proc/monitors` |
//! | the processor | `/sys/devices/system/cpu`, CPUID |
//!
//! **What is not here is not invented.** The kernel does not say which
//! driver runs a PCI function, nor whether it is working, so a PCI row's
//! driver is empty and its status "Unknown", with the reason in its details.
//! A disk, an interface or an output the kernel has registered is one a
//! driver is running, and is "Working" -- the one thing its being listed
//! shows -- except an interface whose link the kernel says is down, which is
//! a warning, as an unplugged cable is. No IRQ, memory range or DMA channel
//! is published for any device, so the resource view has nothing, rather
//! than something plausible.

use crate::{DeviceCategory, DeviceInfo, DeviceStatus};
use hwquery::{
    Address, CpuInfo, DiskInfo, DisplayInfo, HardwareProvider, NetworkAdapterInfo, PciDeviceInfo,
};

/// What one look at the machine found.
pub struct Inventory {
    pub devices: Vec<DeviceInfo>,
    /// The sources that could not be read, with why, for the window to say.
    pub unreadable: Vec<String>,
}

/// What each source gave, or why it gave nothing.
pub struct Parts {
    pub pci: Result<Vec<PciDeviceInfo>, String>,
    pub disks: Result<Vec<DiskInfo>, String>,
    pub network: Result<Vec<NetworkAdapterInfo>, String>,
    pub display: Result<DisplayInfo, String>,
    pub cpu: Result<CpuInfo, String>,
}

/// Ask `provider` for each source.
#[must_use]
pub fn gather(provider: &dyn HardwareProvider) -> Parts {
    let why = |e: hwquery::HwQueryError| e.to_string();
    Parts {
        pci: provider.query_pci().map_err(why),
        disks: provider.query_storage().map_err(why),
        network: provider.query_network().map_err(why),
        display: provider.query_display().map_err(why),
        cpu: provider.query_cpu().map_err(why),
    }
}

/// Look at the machine through `provider`.
#[must_use]
pub fn read(provider: &dyn HardwareProvider) -> Inventory {
    from_parts(gather(provider))
}

/// The device rows the sources describe, numbered from 1 in the order
/// listed above.
#[must_use]
pub fn from_parts(parts: Parts) -> Inventory {
    let mut devices = Vec::new();
    let mut unreadable = Vec::new();
    let mut next_id = 0_u32;
    let mut add = |mut device: DeviceInfo, devices: &mut Vec<DeviceInfo>| {
        next_id = next_id.saturating_add(1);
        device.id = next_id;
        devices.push(device);
    };

    match parts.pci {
        Ok(functions) => {
            for f in &functions {
                add(pci_row(f), &mut devices);
            }
        }
        Err(why) => unreadable.push(format!("PCI devices ({why})")),
    }
    match parts.disks {
        Ok(disks) => {
            for d in &disks {
                add(disk_row(d), &mut devices);
            }
        }
        Err(why) => unreadable.push(format!("disks ({why})")),
    }
    match parts.network {
        Ok(interfaces) => {
            // Loopback is the kernel talking to itself: not a device.
            for n in interfaces.iter().filter(|n| n.name != "lo") {
                add(network_row(n), &mut devices);
            }
        }
        Err(why) => unreadable.push(format!("network interfaces ({why})")),
    }
    match parts.display {
        Ok(display) => {
            for (name, enabled) in &display.outputs {
                add(output_row(name, *enabled, &display), &mut devices);
            }
        }
        Err(why) => unreadable.push(format!("display outputs ({why})")),
    }
    match parts.cpu {
        Ok(cpu) => add(cpu_row(&cpu), &mut devices),
        Err(why) => unreadable.push(format!("the processor ({why})")),
    }
    Inventory {
        devices,
        unreadable,
    }
}

/// A row with nothing filled in but its kind and status.
fn blank(category: DeviceCategory, status: DeviceStatus) -> DeviceInfo {
    DeviceInfo {
        id: 0,
        name: String::new(),
        category,
        status,
        vendor: String::new(),
        device_type: String::new(),
        hw_id: None,
        irq: None,
        mmio_range: None,
        dma_channel: None,
        driver: None,
        enabled: true,
        location: String::new(),
        status_detail: String::new(),
    }
}

/// Which branch of the tree a PCI function belongs in, by its class and
/// subclass codes.
#[must_use]
pub fn pci_category(class: Option<u8>, subclass: Option<u8>) -> DeviceCategory {
    match (class, subclass) {
        (Some(0x01), _) => DeviceCategory::Storage,
        (Some(0x02 | 0x0D), _) => DeviceCategory::Network,
        (Some(0x03), _) => DeviceCategory::Display,
        (Some(0x04), Some(0x01 | 0x03)) => DeviceCategory::Audio,
        (Some(0x09), _) => DeviceCategory::Input,
        (Some(0x0C), Some(0x03)) => DeviceCategory::Usb,
        (Some(0x05 | 0x06 | 0x08 | 0x0B | 0x0C | 0x40), _) => DeviceCategory::System,
        _ => DeviceCategory::Other,
    }
}

/// A PCI function's row.
fn pci_row(f: &PciDeviceInfo) -> DeviceInfo {
    let mut row = blank(
        pci_category(f.class_code, f.subclass_code),
        DeviceStatus::Unknown,
    );
    let what = if f.description.is_empty() {
        f.class.clone()
    } else {
        f.description.clone()
    };
    row.name = match (f.vendor_name.is_empty(), what.is_empty()) {
        (false, false) => format!("{} {what}", f.vendor_name),
        (true, false) => what,
        _ => format!("PCI device {:04x}:{:04x}", f.vendor_id, f.device_id),
    };
    row.vendor = if f.vendor_name.is_empty() {
        format!("{:04x}", f.vendor_id)
    } else {
        f.vendor_name.clone()
    };
    row.device_type.clone_from(&f.class);
    row.hw_id = Some(format!("{:04x}:{:04x}", f.vendor_id, f.device_id));
    row.location = format!("PCI {:02x}:{:02x}.{}", f.bus, f.device, f.function);
    row.status_detail =
        String::from("The kernel does not say which driver, if any, runs this device");
    row
}

/// A registered disk's row.
fn disk_row(d: &DiskInfo) -> DeviceInfo {
    let mut row = blank(DeviceCategory::Storage, DeviceStatus::Working);
    row.name = format!("Disk {} ({})", d.model, guitk::bytes::iec(d.capacity_bytes));
    row.device_type = String::from("Disk");
    row.location = format!("Block device {}", d.model);
    row.status_detail = if d.smart_status.is_empty() {
        String::from("Registered with the kernel as a block device")
    } else {
        format!(
            "Registered with the kernel as a block device ({})",
            d.smart_status
        )
    };
    row
}

/// A network interface's row.
fn network_row(n: &NetworkAdapterInfo) -> DeviceInfo {
    // A link the kernel says is down is a warning, as a device manager shows
    // an unplugged cable; one it says nothing about is simply listed.
    let status = if n.up == Some(false) {
        DeviceStatus::Warning
    } else {
        DeviceStatus::Working
    };
    let mut row = blank(DeviceCategory::Network, status);
    row.name = format!("Network interface {}", n.name);
    row.device_type = String::from("Network interface");
    row.location = n.name.clone();
    let mut detail = String::from("Listed by the kernel among the network interfaces");
    match n.up {
        Some(true) => detail.push_str("; link up"),
        Some(false) => detail.push_str("; link down"),
        None => {}
    }
    match &n.ipv4 {
        Address::Is(ip) => detail.push_str(&format!("; address {ip}")),
        // The kernel's own word that there is none: the card has not been
        // configured, which is what a reader chasing "no network" needs.
        Address::Unassigned => detail.push_str("; no address assigned"),
        Address::NotReported => {}
    }
    if !n.mac_address.is_empty() {
        detail.push_str(&format!("; MAC {}", n.mac_address));
    }
    row.status_detail = detail;
    row
}

/// A display output's row.
fn output_row(name: &str, enabled: bool, display: &DisplayInfo) -> DeviceInfo {
    let mut row = blank(
        DeviceCategory::Display,
        if enabled {
            DeviceStatus::Working
        } else {
            DeviceStatus::Disabled
        },
    );
    row.name = format!("Display output {name}");
    row.device_type = String::from("Display output");
    row.location = name.to_owned();
    row.enabled = enabled;
    row.status_detail = if enabled {
        if display.resolution.is_empty() {
            String::from("An output the kernel is driving")
        } else {
            format!("An output the kernel is driving ({})", display.resolution)
        }
    } else {
        String::from("An output the kernel lists as disabled")
    };
    row
}

/// The processor's row.
fn cpu_row(cpu: &CpuInfo) -> DeviceInfo {
    let mut row = blank(DeviceCategory::System, DeviceStatus::Working);
    row.name = cpu
        .brand
        .clone()
        .filter(|b| !b.trim().is_empty())
        .map_or_else(|| String::from("Processor"), |b| b.trim().to_owned());
    row.vendor = cpu.vendor.clone().unwrap_or_default();
    row.device_type = String::from("Processor");
    row.location = format!("{} logical processor(s)", cpu.logical_processors);
    row.status_detail = String::from("Running this program");
    row
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn pci(bus: u8, dev: u8, class: u8, sub: u8, vendor: &str, what: &str) -> PciDeviceInfo {
        PciDeviceInfo {
            bus,
            device: dev,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x1234,
            class: String::from("Class"),
            description: String::from(what),
            vendor_name: String::from(vendor),
            class_code: Some(class),
            subclass_code: Some(sub),
        }
    }

    fn nothing<T>() -> Result<T, String> {
        Err(String::from("not published"))
    }

    fn parts(pci: Vec<PciDeviceInfo>) -> Parts {
        Parts {
            pci: Ok(pci),
            disks: nothing(),
            network: nothing(),
            display: nothing(),
            cpu: nothing(),
        }
    }

    /// A PCI function is sorted into a branch by its class, named by what it
    /// is and who made it, and says plainly that its driver is not known.
    #[test]
    fn a_pci_function_becomes_a_row_in_its_branch() {
        let found = from_parts(parts(vec![
            pci(0, 0x1f, 0x01, 0x06, "Intel", "SATA controller"),
            pci(0, 2, 0x03, 0x00, "QEMU", "VGA compatible controller"),
            pci(0, 3, 0x04, 0x03, "", "Audio device"),
        ]));
        let rows: Vec<(u32, &str, DeviceCategory, DeviceStatus)> = found
            .devices
            .iter()
            .map(|d| (d.id, d.name.as_str(), d.category, d.status))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    1,
                    "Intel SATA controller",
                    DeviceCategory::Storage,
                    DeviceStatus::Unknown
                ),
                (
                    2,
                    "QEMU VGA compatible controller",
                    DeviceCategory::Display,
                    DeviceStatus::Unknown
                ),
                (
                    3,
                    "Audio device",
                    DeviceCategory::Audio,
                    DeviceStatus::Unknown
                ),
            ]
        );
        let sata = &found.devices[0];
        assert_eq!(sata.location, "PCI 00:1f.0");
        assert_eq!(sata.hw_id.as_deref(), Some("8086:1234"));
        assert!(sata.driver.is_none(), "a driver was invented");
        assert!(sata.status_detail.contains("does not say which driver"));
        assert!(
            (sata.irq, sata.mmio_range, sata.dma_channel) == (None, None, None),
            "a resource was invented"
        );
        assert_eq!(
            found.devices[2].vendor, "8086",
            "an unknown vendor is its id"
        );
        assert_eq!(
            found.unreadable.len(),
            4,
            "each source that failed is named: {:?}",
            found.unreadable
        );
    }

    /// A function with neither a known vendor nor a known kind is still a
    /// row, named by the ids the kernel gave -- not an empty name, which
    /// draws as a blank line in the tree.
    #[test]
    fn a_pci_function_with_no_name_is_named_by_its_ids() {
        let mut nameless = pci(0, 5, 0xFF, 0x00, "", "");
        nameless.class = String::new();
        let found = from_parts(parts(vec![nameless]));
        assert_eq!(found.devices[0].name, "PCI device 8086:1234");

        let mut classed = pci(0, 6, 0x08, 0x80, "", "");
        classed.class = String::from("Base system peripheral");
        let found = from_parts(parts(vec![classed]));
        assert_eq!(
            found.devices[0].name, "Base system peripheral",
            "with no subclass name, the class names it"
        );
    }

    /// The processor is one row, named by its brand string -- or plainly
    /// "Processor" when the brand is blank, as some firmware leaves it.
    #[test]
    fn the_processor_is_one_row_named_by_its_brand() {
        let cpu = |brand: Option<&str>| CpuInfo {
            brand: brand.map(String::from),
            vendor: Some(String::from("GenuineIntel")),
            family: 6,
            model: 158,
            stepping: 10,
            physical_cores: 4,
            logical_processors: 8,
            base_clock_mhz: None,
            max_turbo_mhz: None,
            l1_data_kb: None,
            l1_inst_kb: None,
            l2_kb: None,
            l3_kb: None,
            features: Vec::new(),
        };
        let row = |brand| {
            let mut all = parts(Vec::new());
            all.cpu = Ok(cpu(brand));
            let found = from_parts(all);
            assert_eq!(found.devices.len(), 1, "one processor row");
            found.devices.into_iter().next().expect("a row")
        };
        let named = row(Some("  Intel(R) Core(TM) i7-8700K  "));
        assert_eq!(named.name, "Intel(R) Core(TM) i7-8700K");
        assert_eq!(named.vendor, "GenuineIntel");
        assert_eq!(named.location, "8 logical processor(s)");
        assert_eq!(named.category, DeviceCategory::System);
        assert_eq!(
            row(Some("   ")).name,
            "Processor",
            "a blank brand is no name"
        );
        assert_eq!(row(None).name, "Processor");
    }

    /// The kind of each PCI class, by the specification's codes.
    #[test]
    fn a_pci_class_decides_the_branch() {
        use DeviceCategory as C;
        let at = |class, sub| pci_category(Some(class), Some(sub));
        assert_eq!(at(0x02, 0x00), C::Network);
        assert_eq!(at(0x0D, 0x11), C::Network, "wireless is networking");
        assert_eq!(at(0x04, 0x00), C::Other, "video capture is not audio");
        assert_eq!(at(0x0C, 0x03), C::Usb);
        assert_eq!(at(0x0C, 0x05), C::System, "SMBus");
        assert_eq!(at(0x06, 0x00), C::System, "a host bridge");
        assert_eq!(at(0x09, 0x00), C::Input);
        assert_eq!(at(0xFF, 0x00), C::Other);
        assert_eq!(pci_category(None, None), C::Other);
    }

    /// Disks, interfaces (not loopback), outputs and the processor each make
    /// a row; what the kernel registered is Working, a disabled output is
    /// Disabled.
    #[test]
    fn what_the_kernel_registered_is_listed_as_working() {
        let found = from_parts(Parts {
            pci: Ok(Vec::new()),
            disks: Ok(vec![DiskInfo {
                model: String::from("vda"),
                capacity_bytes: 8 * 1024 * 1024 * 1024,
                interface: String::new(),
                serial: String::new(),
                smart_status: String::from("read-only"),
                partitions: Vec::new(),
            }]),
            network: Ok(vec![adapter("lo"), adapter("eth0")]),
            display: Ok(DisplayInfo {
                gpu_name: String::new(),
                vendor: String::new(),
                vram_mb: None,
                resolution: String::from("1280x800"),
                refresh_rate_hz: Some(60),
                outputs: vec![
                    (String::from("VIRTUAL-1"), true),
                    (String::from("HDMI-1"), false),
                ],
                driver_version: String::new(),
            }),
            cpu: nothing(),
        });
        let rows: Vec<(&str, DeviceStatus)> = found
            .devices
            .iter()
            .map(|d| (d.name.as_str(), d.status))
            .collect();
        assert_eq!(
            rows,
            [
                ("Disk vda (8.0 GiB)", DeviceStatus::Working),
                ("Network interface eth0", DeviceStatus::Working),
                ("Display output VIRTUAL-1", DeviceStatus::Working),
                ("Display output HDMI-1", DeviceStatus::Disabled),
            ]
        );
        assert!(found.devices[0].status_detail.contains("read-only"));
        assert!(
            !found.devices[3].enabled,
            "an output the kernel lists as disabled was called enabled"
        );
        assert!(found.devices[2].enabled);
        assert!(found.devices[2].status_detail.contains("1280x800"));
        assert_eq!(found.unreadable, ["the processor (not published)"]);
    }

    /// An interface's row says what the kernel says of it: a link that is
    /// down is a warning, an address it has none of is said, and its address
    /// and MAC when it has them -- nothing when nothing is reported.
    #[test]
    fn an_interface_row_says_its_link_and_address() {
        let down = NetworkAdapterInfo {
            up: Some(false),
            ipv4: Address::Unassigned,
            mac_address: String::from("52:54:00:12:34:56"),
            ..adapter("eth0")
        };
        let row = network_row(&down);
        assert_eq!(
            row.status,
            DeviceStatus::Warning,
            "a link that is down was fine"
        );
        assert!(
            row.status_detail.contains("; link down"),
            "{}",
            row.status_detail
        );
        assert!(
            row.status_detail.contains("; no address assigned"),
            "{}",
            row.status_detail
        );
        assert!(row.status_detail.contains("; MAC 52:54:00:12:34:56"));

        let up = NetworkAdapterInfo {
            up: Some(true),
            ipv4: Address::Is(String::from("10.0.2.15")),
            ..adapter("eth0")
        };
        let row = network_row(&up);
        assert_eq!(row.status, DeviceStatus::Working);
        assert!(row.status_detail.contains("; link up"));
        assert!(row.status_detail.contains("; address 10.0.2.15"));

        let row = network_row(&adapter("eth1"));
        assert_eq!(
            row.status,
            DeviceStatus::Working,
            "no word on the link is not a fault"
        );
        assert_eq!(
            row.status_detail, "Listed by the kernel among the network interfaces",
            "something unreported was described"
        );
    }

    fn adapter(name: &str) -> NetworkAdapterInfo {
        NetworkAdapterInfo::named(name)
    }
}
