//! Minimal PCI bus enumeration via Configuration Space Mechanism #1.
//!
//! Scans PCI bus 0 by probing all 32 device slots (8 functions each)
//! through ports 0xCF8 (address) and 0xCFC (data).  This is sufficient
//! for discovering virtio devices in QEMU's q35 machine.
//!
//! ## PCI Configuration Space
//!
//! The 256-byte configuration space for each function is accessed by
//! writing a 32-bit address to port 0xCF8:
//!
//! ```text
//! Bits 31   : Enable bit (1)
//! Bits 23:16: Bus number
//! Bits 15:11: Device number (0-31)
//! Bits 10:8 : Function number (0-7)
//! Bits  7:2 : Register offset (dword-aligned)
//! Bits  1:0 : 0
//! ```
//!
//! Then read/write 32 bits from port 0xCFC.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::vec::Vec;
use crate::port;

// ---------------------------------------------------------------------------
// PCI I/O ports
// ---------------------------------------------------------------------------

/// PCI Configuration Address port.
const PCI_CONFIG_ADDR: u16 = 0xCF8;
/// PCI Configuration Data port.
const PCI_CONFIG_DATA: u16 = 0xCFC;

// ---------------------------------------------------------------------------
// PCI header offsets (common header type 0)
// ---------------------------------------------------------------------------

/// Vendor ID (16-bit, offset 0x00 low half).
const CFG_VENDOR_ID: u8 = 0x00;
/// Device ID (16-bit, offset 0x00 high half).
const CFG_DEVICE_ID: u8 = 0x02;
/// Command register (16-bit, offset 0x04 low half).
const CFG_COMMAND: u8 = 0x04;
/// Class code (8-bit, offset 0x0B).
const _CFG_CLASS: u8 = 0x0B;
/// Subclass (8-bit, offset 0x0A).
const _CFG_SUBCLASS: u8 = 0x0A;
/// Header type (8-bit, offset 0x0E).
const _CFG_HEADER_TYPE: u8 = 0x0E;
/// BAR0 (32-bit, offset 0x10).
const CFG_BAR0: u8 = 0x10;
/// Interrupt line (8-bit, offset 0x3C low byte).
const CFG_INTERRUPT_LINE: u8 = 0x3C;

// Command register bits
/// I/O space access enable.
const CMD_IO_SPACE: u16 = 1 << 0;
/// Memory space access enable.
const CMD_MEMORY_SPACE: u16 = 1 << 1;
/// Bus master enable (required for DMA).
const CMD_BUS_MASTER: u16 = 1 << 2;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// PCI bus/device/function address.
#[derive(Debug, Clone, Copy)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// Information about a discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    /// Class code (8-bit).
    pub class: u8,
    /// Subclass (8-bit).
    pub subclass: u8,
    /// IRQ line assigned by firmware.
    pub irq_line: u8,
    /// Base Address Registers (raw 32-bit values).
    pub bars: [u32; 6],
}

impl PciDevice {
    /// Return BAR0 as an I/O port base address (if BAR0 is I/O space).
    ///
    /// Returns `None` if BAR0 is memory-mapped (bit 0 = 0).
    pub fn bar0_io_port(&self) -> Option<u16> {
        let bar = self.bars[0];
        if bar & 1 != 0 {
            // I/O space BAR: bits [31:2] are the port base.
            #[allow(clippy::cast_possible_truncation)]
            Some((bar & 0xFFFF_FFFC) as u16)
        } else {
            None
        }
    }

    /// Return BAR0 as a memory-mapped base address (if BAR0 is MMIO).
    ///
    /// Returns `None` if BAR0 is I/O space (bit 0 = 1).
    #[allow(dead_code)] // Public API for MMIO-based PCI device drivers.
    pub fn bar0_mmio_addr(&self) -> Option<u64> {
        let bar = self.bars[0];
        if bar & 1 == 0 {
            Some(u64::from(bar & 0xFFFF_FFF0))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration space access
// ---------------------------------------------------------------------------

/// Build the 32-bit PCI configuration address for a register read/write.
// Bus/device/function/offset are small values; shifts never overflow u32.
#[allow(clippy::arithmetic_side_effects)]
fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    (1u32 << 31)                          // Enable bit
    | (u32::from(bus) << 16)
    | (u32::from(device & 0x1F) << 11)
    | (u32::from(function & 0x07) << 8)
    | u32::from(offset & 0xFC)            // Dword-aligned
}

/// Read a 32-bit value from PCI configuration space.
pub fn config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: Ports 0xCF8/0xCFC are the PCI config mechanism #1 ports,
    // always present on PC-compatible hardware.
    unsafe {
        port::outl(PCI_CONFIG_ADDR, addr);
        port::inl(PCI_CONFIG_DATA)
    }
}

/// Write a 32-bit value to PCI configuration space.
pub fn config_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: Same as config_read32.
    unsafe {
        port::outl(PCI_CONFIG_ADDR, addr);
        port::outl(PCI_CONFIG_DATA, value);
    }
}

/// Read a 16-bit value from PCI configuration space.
// The shift/mask arithmetic operates on small values within u32.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn config_read16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let dword = config_read32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 2) * 8) as u32;
    ((dword >> shift) & 0xFFFF) as u16
}

/// Write a 16-bit value to PCI configuration space.
///
/// Performs a read-modify-write of the containing 32-bit dword to
/// preserve the adjacent 16-bit half.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn config_write16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let dword = config_read32(bus, device, function, aligned);
    let shift = ((offset & 2) * 8) as u32;
    let mask = !(0xFFFF_u32 << shift);
    let new_dword = (dword & mask) | (u32::from(value) << shift);
    config_write32(bus, device, function, aligned, new_dword);
}

/// Read an 8-bit value from PCI configuration space.
// Same as config_read16 but for single byte.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn config_read8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let dword = config_read32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 3) * 8) as u32;
    ((dword >> shift) & 0xFF) as u8
}

/// Write an 8-bit value to PCI configuration space using a genuine byte
/// access to the correct byte lane of the data port.
///
/// This differs from [`config_write16`]/[`config_write32`], which always
/// emit a 32-bit `outl`. Some devices decode the *access width* on the
/// data port and only act on writes of a specific width. The QEMU
/// i6300esb watchdog is one such device: its LOCK register (config offset
/// 0x68) is only handled when written with a 1-byte access. A 32-bit
/// read-modify-write silently falls through to default config storage and
/// never triggers the device's timer-enable side effect. Byte lane is
/// selected by adding `offset & 3` to the data port base.
// PCI config mechanism #1: byte lane = data port + (offset & 3).
#[allow(clippy::arithmetic_side_effects)]
pub fn config_write8(bus: u8, device: u8, function: u8, offset: u8, value: u8) {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: 0xCF8/0xCFC are the PCI config mechanism #1 ports. The
    // aligned dword address is written first, then a byte access selects
    // the target lane within the dword via (offset & 3).
    unsafe {
        port::outl(PCI_CONFIG_ADDR, addr);
        port::outb(PCI_CONFIG_DATA + u16::from(offset & 3), value);
    }
}

/// Write a 16-bit value to PCI configuration space using a genuine 16-bit
/// access to the correct word lane of the data port.
///
/// Unlike [`config_write16`] (which read-modify-writes a full dword via a
/// 32-bit `outl`), this emits a real `outw`. Width-sensitive devices such
/// as the QEMU i6300esb watchdog only handle their CONFIG register
/// (offset 0x60) on a 2-byte access; a 4-byte write is ignored by the
/// device model. Word lane is selected by adding `offset & 2` to the data
/// port base.
// PCI config mechanism #1: word lane = data port + (offset & 2).
#[allow(clippy::arithmetic_side_effects)]
pub fn config_write16_native(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: 0xCF8/0xCFC are the PCI config mechanism #1 ports. Writing
    // the aligned dword address then a word access selects the target
    // 16-bit lane within the dword via (offset & 2).
    unsafe {
        port::outl(PCI_CONFIG_ADDR, addr);
        port::outw(PCI_CONFIG_DATA + u16::from(offset & 2), value);
    }
}

// ---------------------------------------------------------------------------
// Bus scanning
// ---------------------------------------------------------------------------

/// Scan PCI bus 0 and return all discovered devices.
///
/// Probes all 32 device slots × 8 functions.  Multi-function devices
/// are detected via header type bit 7.
// Loop arithmetic with small counters; no overflow possible.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn scan_bus0() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for device in 0..32u8 {
        // Check function 0 first.
        let vendor = config_read16(0, device, 0, CFG_VENDOR_ID);
        if vendor == 0xFFFF {
            continue; // No device in this slot.
        }

        scan_function(0, device, 0, &mut devices);

        // Check if this is a multi-function device (header type bit 7).
        let header_type = config_read8(0, device, 0, 0x0E);
        if header_type & 0x80 != 0 {
            for function in 1..8u8 {
                let vendor = config_read16(0, device, function, CFG_VENDOR_ID);
                if vendor != 0xFFFF {
                    scan_function(0, device, function, &mut devices);
                }
            }
        }
    }

    devices
}

/// Read all fields for one PCI function and add it to the device list.
#[allow(clippy::cast_possible_truncation)]
fn scan_function(bus: u8, device: u8, function: u8, devices: &mut Vec<PciDevice>) {
    let vendor_id = config_read16(bus, device, function, CFG_VENDOR_ID);
    let device_id = config_read16(bus, device, function, CFG_DEVICE_ID);
    let class = config_read8(bus, device, function, 0x0B);
    let subclass = config_read8(bus, device, function, 0x0A);
    let irq_line = config_read8(bus, device, function, CFG_INTERRUPT_LINE);

    let mut bars = [0u32; 6];
    for (i, bar) in bars.iter_mut().enumerate() {
        #[allow(clippy::arithmetic_side_effects)]
        let offset = CFG_BAR0 + (i as u8 * 4);
        *bar = config_read32(bus, device, function, offset);
    }

    devices.push(PciDevice {
        address: PciAddress { bus, device, function },
        vendor_id,
        device_id,
        class,
        subclass,
        irq_line,
        bars,
    });
}

// ---------------------------------------------------------------------------
// Device helpers
// ---------------------------------------------------------------------------

/// Find the first PCI device matching a vendor/device ID pair.
pub fn find_device(vendor: u16, device: u16) -> Option<PciDevice> {
    let devices = scan_bus0();
    devices.into_iter().find(|d| d.vendor_id == vendor && d.device_id == device)
}

/// Find all PCI devices matching a vendor/device ID pair.
///
/// Returns every matching device on bus 0.  Useful for discovering
/// multiple instances of the same device type (e.g., multiple
/// virtio-blk controllers).
#[allow(dead_code)] // API for drivers zone; unused until multi-device support.
pub fn find_all_devices(vendor: u16, device: u16) -> Vec<PciDevice> {
    let devices = scan_bus0();
    devices.into_iter().filter(|d| d.vendor_id == vendor && d.device_id == device).collect()
}

/// Find all PCI devices matching a class/subclass pair.
///
/// Useful for discovering all devices of a category regardless of
/// vendor (e.g., all mass-storage controllers: class=0x01).
#[allow(dead_code)] // API for drivers zone; unused until driver framework.
pub fn find_devices_by_class(class: u8, subclass: u8) -> Vec<PciDevice> {
    let devices = scan_bus0();
    devices.into_iter().filter(|d| d.class == class && d.subclass == subclass).collect()
}

/// Enable bus mastering (DMA) for a PCI device.
///
/// Also enables I/O space and memory space access.
pub fn enable_bus_master(addr: PciAddress) {
    let cmd = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
    let new_cmd = cmd | CMD_IO_SPACE | CMD_MEMORY_SPACE | CMD_BUS_MASTER;
    // Write back as 32-bit (the upper 16 bits are the status register,
    // writing back what we read is safe — status bits are write-1-to-clear).
    let status = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND + 2);
    let dword = u32::from(new_cmd) | (u32::from(status) << 16);
    config_write32(addr.bus, addr.device, addr.function, CFG_COMMAND, dword);
}

// ---------------------------------------------------------------------------
// PCI Capabilities
// ---------------------------------------------------------------------------

/// Offset of the Capabilities Pointer in PCI config space.
const CFG_CAP_PTR: u8 = 0x34;

/// Offset of the Status register in PCI config space.
const CFG_STATUS: u8 = 0x06;

/// Status register bit: device has capabilities list.
const STATUS_CAP_LIST: u16 = 1 << 4;

/// A PCI capability entry found during capability list traversal.
#[derive(Debug, Clone, Copy)]
pub struct PciCapability {
    /// Capability ID (e.g., 0x09 = Vendor Specific).
    pub id: u8,
    /// Offset in config space where this capability starts.
    pub offset: u8,
}

/// Walk the PCI capabilities linked list for a device.
///
/// Returns all capabilities found.  The list terminates when the next
/// pointer is 0x00 or when we've traversed 48 entries (safety limit).
#[allow(clippy::arithmetic_side_effects)]
pub fn walk_capabilities(addr: PciAddress) -> Vec<PciCapability> {
    let mut caps = Vec::new();

    // Check that the device has capabilities (Status bit 4).
    let status = config_read16(addr.bus, addr.device, addr.function, CFG_STATUS);
    if status & STATUS_CAP_LIST == 0 {
        return caps;
    }

    // Read the capabilities pointer (low byte of dword at 0x34).
    let mut ptr = config_read8(addr.bus, addr.device, addr.function, CFG_CAP_PTR);
    ptr &= 0xFC; // Dword-aligned.

    let mut count = 0u8;
    while ptr != 0 && count < 48 {
        let cap_id = config_read8(addr.bus, addr.device, addr.function, ptr);
        let cap_next = config_read8(addr.bus, addr.device, addr.function, ptr.wrapping_add(1));

        caps.push(PciCapability { id: cap_id, offset: ptr });

        ptr = cap_next & 0xFC;
        count = count.wrapping_add(1);
    }

    caps
}

/// Find the first capability with a given ID for a device.
pub fn find_capability(addr: PciAddress, cap_id: u8) -> Option<PciCapability> {
    walk_capabilities(addr).into_iter().find(|c| c.id == cap_id)
}

/// Find all capabilities with a given ID for a device.
pub fn find_capabilities(addr: PciAddress, cap_id: u8) -> Vec<PciCapability> {
    walk_capabilities(addr).into_iter().filter(|c| c.id == cap_id).collect()
}

/// Decode a 64-bit BAR (for memory-mapped BARs that are 64-bit).
///
/// If `bar_index` is a 64-bit BAR, reads BARs[index] and BARs[index+1]
/// to form the full 64-bit base address.  Returns None if the BAR is
/// I/O space or if the index is out of range.
#[allow(clippy::arithmetic_side_effects)]
pub fn bar_mmio_addr64(dev: &PciDevice, bar_index: usize) -> Option<u64> {
    if bar_index >= 6 {
        return None;
    }
    let bar_lo = dev.bars[bar_index];
    // Bit 0 = 0 means memory space.
    if bar_lo & 1 != 0 {
        return None; // I/O space.
    }
    // Bits 2:1 indicate type: 00 = 32-bit, 10 = 64-bit.
    let bar_type = (bar_lo >> 1) & 0x3;
    let base_lo = u64::from(bar_lo & 0xFFFF_FFF0);

    if bar_type == 0x2 && bar_index + 1 < 6 {
        // 64-bit BAR.
        let bar_hi = dev.bars[bar_index + 1];
        Some(base_lo | (u64::from(bar_hi) << 32))
    } else {
        // 32-bit BAR.
        Some(base_lo)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Scan bus 0 and log all discovered PCI devices.
pub fn self_test() -> Result<(), &'static str> {
    crate::serial_println!("[pci] Scanning PCI bus 0...");

    let devices = scan_bus0();
    if devices.is_empty() {
        crate::serial_println!("[pci]   No devices found (unexpected!)");
        return Err("no PCI devices found");
    }

    for dev in &devices {
        crate::serial_println!(
            "[pci]   {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x}:{:02x} irq={} bar0={:#010x}",
            dev.address.bus,
            dev.address.device,
            dev.address.function,
            dev.vendor_id,
            dev.device_id,
            dev.class,
            dev.subclass,
            dev.irq_line,
            dev.bars[0]
        );
    }
    crate::serial_println!("[pci]   {} device(s) found", devices.len());

    crate::serial_println!("[pci] Self-test PASSED");
    Ok(())
}
