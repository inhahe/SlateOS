//! Minimal PCI bus enumeration via Configuration Space Mechanism #1.
//!
//! Scans bus 0 and every bus behind a PCI-to-PCI bridge, probing all 32
//! device slots (8 functions each) through ports 0xCF8 (address) and 0xCFC
//! (data).  The pair is taken under a lock ([`config_access`]).
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

use crate::port;
use alloc::vec::Vec;

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
const CFG_HEADER_TYPE: u8 = 0x0E;

/// Header-type bit 7: this device has functions beyond function 0.
///
/// Clear means function 0 is the only one, and probing 1..8 would read
/// floating config space rather than absent devices.
const HEADER_TYPE_MULTIFUNCTION: u8 = 0x80;
/// Header type (bits 6:0) of a PCI-to-PCI bridge.
const HEADER_TYPE_BRIDGE: u8 = 0x01;
/// A bridge's secondary bus number: the bus behind it.
const CFG_SECONDARY_BUS: u8 = 0x19;
/// BAR0 (32-bit, offset 0x10).
const CFG_BAR0: u8 = 0x10;
/// Interrupt line (8-bit, offset 0x3C low byte).
const CFG_INTERRUPT_LINE: u8 = 0x3C;
/// Interrupt pin (8-bit, offset 0x3D): 0 for none, 1-4 for INTA-INTD.
const CFG_INTERRUPT_PIN: u8 = 0x3D;

// Command register bits
/// I/O space access enable.
const CMD_IO_SPACE: u16 = 1 << 0;
/// Memory space access enable.
const CMD_MEMORY_SPACE: u16 = 1 << 1;
/// Bus master enable (required for DMA).
const CMD_BUS_MASTER: u16 = 1 << 2;
/// INTx assertion **disable** (PCI 2.3+).  Set = the function may not drive
/// its legacy interrupt pin.  Note the inverted sense: the bit *disables*.
pub(crate) const CMD_INTX_DISABLE: u16 = 1 << 10;

// Status register bits
/// Interrupt Status (PCI 2.3+): set while this function is asserting INTx.
///
/// Read-only, and unaffected by [`CMD_INTX_DISABLE`] — the bit reports what
/// the function *wants*, so it still names a culprit after it has been
/// silenced.  That is what makes it usable as a diagnostic.
const STATUS_INTERRUPT: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// PCI bus/device/function address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    /// The requester id a request from this function carries, and what an
    /// IOMMU names it by: bus << 8 | device << 3 | function.
    #[must_use]
    pub const fn requester_id(self) -> u16 {
        ((self.bus as u16) << 8)
            | (((self.device & 0x1F) as u16) << 3)
            | ((self.function & 0x7) as u16)
    }
}

/// What an interrupt remapping entry can check of the requester id a
/// function's MSIs carry when they reach the IOMMU ([`msi_source`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiSource {
    /// Exactly this requester id: the function's own, or the one bridge
    /// above it that sends its requests under its own name.
    Exact(u16),
    /// Only a bus from `start` to `end`: behind a PCIe-to-PCI bridge, which
    /// may send a function's request under the bus behind it (VT-d's SVT
    /// 10b).
    Buses { start: u8, end: u8 },
}

/// The PCI Express capability, and its device/port type (bits 7:4 of the
/// capabilities register, offset 2; PCI Express Base §7.5.3.2).
const CAP_PCIE: u8 = 0x10;
/// Port types that pass a requester id through unchanged.
const PCIE_ROOT_PORT: u8 = 0x4;
const PCIE_UPSTREAM_PORT: u8 = 0x5;
const PCIE_DOWNSTREAM_PORT: u8 = 0x6;
/// A PCI Express to PCI/PCI-X bridge, which sends a request from behind it
/// under the bus behind it, device and function 0.
const PCIE_TO_PCI_BRIDGE: u8 = 0x7;
/// A PCI/PCI-X to PCI Express bridge, which sends one under its own id.
const PCI_TO_PCIE_BRIDGE: u8 = 0x8;

/// A bridge on the way up from a function: where it is, its PCI Express
/// port type (`None` for a conventional PCI bridge), and the bus behind it.
#[derive(Debug, Clone, Copy)]
struct Upstream {
    address: PciAddress,
    pcie_type: Option<u8>,
    secondary: u8,
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
    | u32::from(offset & 0xFC) // Dword-aligned
}

/// The config ports, an address port and a data port: one access at a time
/// (`sync::WindowLock`).
static CONFIG_PORTS: crate::sync::WindowLock = crate::sync::WindowLock::new();

/// Select `addr` in the config address port and run `data` against the data
/// port, as one access, holding [`CONFIG_PORTS`].
///
/// An NMI cannot be held off, and the watchdog's re-arm (`hardlockup`) does
/// config access from one. One that interrupted this CPU's own access does
/// its own at once, and puts back the address the interrupted access
/// selected (the address port reads back), as firmware's SMM handlers do
/// around the same pair of ports.
fn config_access<R>(addr: u32, data: impl FnOnce() -> R) -> R {
    CONFIG_PORTS.with(|nested| {
        // SAFETY: 0xCF8 is the config mechanism #1 address port, always
        // present on PC-compatible hardware; reading it gives the address the
        // interrupted access selected, written back before returning.
        let interrupted = nested.then(|| unsafe { port::inl(PCI_CONFIG_ADDR) });
        // SAFETY: as above.
        unsafe { port::outl(PCI_CONFIG_ADDR, addr) };
        let result = data();
        if let Some(address) = interrupted {
            // SAFETY: as above.
            unsafe { port::outl(PCI_CONFIG_ADDR, address) };
        }
        result
    })
}

/// Read a 32-bit value from PCI configuration space.
pub fn config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: 0xCFC is the config mechanism #1 data port, always present on
    // PC-compatible hardware; `config_access` has selected the register.
    config_access(addr, || unsafe { port::inl(PCI_CONFIG_DATA) })
}

/// Write a 32-bit value to PCI configuration space.
pub fn config_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let addr = config_address(bus, device, function, offset);
    // SAFETY: Same as config_read32.
    config_access(addr, || unsafe { port::outl(PCI_CONFIG_DATA, value) });
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
    // SAFETY: 0xCFC-0xCFF are the PCI config mechanism #1 data port's
    // lanes; `config_access` has selected the aligned dword, and a byte
    // access selects the target lane within it via (offset & 3).
    config_access(addr, || unsafe {
        port::outb(PCI_CONFIG_DATA + u16::from(offset & 3), value)
    });
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
    // SAFETY: as config_write8's, with a word access selecting the target
    // 16-bit lane within the dword via (offset & 2).
    config_access(addr, || unsafe {
        port::outw(PCI_CONFIG_DATA + u16::from(offset & 2), value)
    });
}

// ---------------------------------------------------------------------------
// Bus scanning
// ---------------------------------------------------------------------------

/// Every PCI function on every bus reachable from bus 0, with what a driver
/// needs of each ([`for_each_function`]'s order).
///
/// Scanned once and kept: a scan is thousands of slow port reads on a PC
/// with many bridges, and nothing here changes what it finds (no hotplug,
/// and firmware placed the BARs). The scan itself runs outside the lock,
/// which disables preemption; two first callers both scan, and the first to
/// finish is kept.
pub fn scan_all() -> Vec<PciDevice> {
    if let Some(found) = SCANNED.lock().as_ref() {
        return found.clone();
    }
    let found = scan_now();
    SCANNED.lock().get_or_insert(found).clone()
}

/// What [`scan_all`] last found; `None` until it first scans.
static SCANNED: crate::sync::PreemptSpinMutex<Option<Vec<PciDevice>>> =
    crate::sync::PreemptSpinMutex::new(None);

fn scan_now() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for_each_function(|addr| scan_function(addr.bus, addr.device, addr.function, &mut devices));
    devices
}

/// Call `f` once for each PCI function present, without allocating: bus 0
/// first, then the bus behind each PCI-to-PCI bridge found, in the order
/// found. A PC's chipset puts its own devices on bus 0 and everything in a
/// slot -- an M.2 drive, a PCIe card -- behind a root port's bridge. (A
/// machine with a second root bus that no bridge leads to, a two-socket
/// server's, would need ACPI's host bridges to find it.)
///
/// [`scan_all`] is the convenient form, but it builds a `Vec`, and a caller
/// running in interrupt or softirq context during a fault — the IRQ storm
/// diagnostic is exactly that — must not depend on the heap being healthy.  An
/// allocation failure inside a kernel is a panic, so a diagnostic that
/// allocates can turn a recoverable storm into a dead machine at precisely the
/// moment it was supposed to explain one.
///
/// In each slot, function 0 is probed, and functions 1–7 only when the header
/// type says the device is multi-function.
fn for_each_function(mut f: impl FnMut(PciAddress)) {
    // Buses to scan, in the order their bridges were found; each once.
    let mut queue = [0u8; 256];
    let mut seen = [false; 256];
    let mut queued = 1usize;
    if let Some(first) = seen.first_mut() {
        *first = true;
    }
    let mut next = 0usize;
    while let Some(&bus) = queue.get(next).filter(|_| next < queued) {
        next = next.saturating_add(1);
        let mut visit = |device: u8, function: u8| {
            f(PciAddress {
                bus,
                device,
                function,
            });
            let header = config_read8(bus, device, function, CFG_HEADER_TYPE);
            if header & !HEADER_TYPE_MULTIFUNCTION == HEADER_TYPE_BRIDGE {
                let behind = config_read8(bus, device, function, CFG_SECONDARY_BUS);
                if let Some(mark) = seen.get_mut(usize::from(behind)).filter(|s| !**s) {
                    *mark = true;
                    if let Some(slot) = queue.get_mut(queued) {
                        *slot = behind;
                        queued = queued.saturating_add(1);
                    }
                }
            }
        };
        for device in 0..32u8 {
            if config_read16(bus, device, 0, CFG_VENDOR_ID) == 0xFFFF {
                continue; // No device in this slot.
            }
            visit(device, 0);
            if config_read8(bus, device, 0, CFG_HEADER_TYPE) & HEADER_TYPE_MULTIFUNCTION != 0 {
                for function in 1..8u8 {
                    if config_read16(bus, device, function, CFG_VENDOR_ID) != 0xFFFF {
                        visit(device, function);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serial ports, before the heap
// ---------------------------------------------------------------------------

/// Class, subclass and programming interface of the serial ports driven as
/// a 16550: class 07h (communication), subclass 00h (serial), interface 02h
/// (16550) to 06h (16650, 16750, 16850, 16950) -- the 16550's registers,
/// with more beside them. Linux's `serial_pci_guess_board` takes the same.
const CLASS_SERIAL_16550: core::ops::RangeInclusive<u32> = 0x07_00_02..=0x07_00_06;
/// Class code, subclass, interface and revision (32-bit, offset 0x08).
const CFG_CLASS_REVISION: u8 = 0x08;
/// Subsystem vendor id (low half) and subsystem id (high half), offset 0x2C
/// of a type-0 header.
const CFG_SUBSYSTEM: u8 = 0x2C;
/// A bridge's subordinate bus number: the highest bus behind it.
const CFG_SUBORDINATE_BUS: u8 = 0x1A;
/// A bridge's I/O window: base (bits 7:4 are address bits 15:12; bits 3:0
/// are 1 for 32-bit I/O addressing) and limit, 8-bit each.
const CFG_IO_BASE: u8 = 0x1C;
const CFG_IO_LIMIT: u8 = 0x1D;
/// The upper 16 bits of the window's base and limit, for 32-bit I/O.
const CFG_IO_BASE_UPPER: u8 = 0x30;
const CFG_IO_LIMIT_UPPER: u8 = 0x32;
/// The most bridges [`early_uart`] looks through to reach a port: a PC's
/// slots sit one or two deep.
const UART_PATH_MAX: usize = 8;
/// Vendor id (low half) and device id (high half) of the ASIX AX99100, a
/// PCIe chip whose functions are serial, parallel, SPI or I2C ports.
const ASIX_AX99100_ID: u32 = 0x9100_125B;
/// The subsystem dword of an AX99100 function set up as a serial port
/// (subsystem vendor A000h, subsystem 1000h); its other kinds of port carry
/// other subsystem ids. This is how Linux's `8250_pci` tells them apart.
const ASIX_AX99100_SERIAL: u32 = 0x1000_A000;

/// A serial port on the PCI bus that is driven as a 16550 through I/O ports.
#[derive(Debug, Clone, Copy)]
pub struct PciUart {
    pub address: PciAddress,
    /// The I/O port of its first register (its BAR0).
    pub port: u16,
    /// How many command registers had I/O decoding switched on to reach it:
    /// its own and those of the bridges leading to it, firmware leaving it
    /// off where no firmware driver used the device. They are the first
    /// `opened` of `switched`, which [`close_io_decode`] switches off again.
    pub opened: usize,
    switched: [PciAddress; UART_PATH_MAX + 1],
}

/// What [`early_uart`] found.
#[derive(Debug, Clone, Copy)]
pub enum UartSearch {
    /// The port asked for, its decoding on.
    Found(PciUart),
    /// Fewer serial ports than asked for: how many there are.
    TooFew(usize),
    /// The port asked for has no I/O-port registers to drive: its BAR0
    /// (shown raw) is memory, unassigned, or above the 64 KiB I/O space.
    NoIoPorts(PciAddress, u32),
    /// A bridge leading to the port does not pass its I/O port through: the
    /// port is outside the bridge's I/O window, which firmware never set for
    /// it. Nothing was switched on.
    BridgeClosed(PciAddress, u16),
    /// More bridges lead to the port than [`UART_PATH_MAX`]: not looked
    /// through. Nothing was switched on.
    TooDeep(PciAddress),
}

/// Whether a function is a serial port the kernel can drive as a 16550:
/// one whose class says so, or an AX99100 serial function. `id` is the
/// dword at offset 0, `class_revision` the one at 0x08, and `subsystem`
/// reads the dword at 0x2C (only asked for of an AX99100, since in a bridge
/// that offset is something else).
fn is_serial_16550(id: u32, class_revision: u32, subsystem: impl FnOnce() -> u32) -> bool {
    CLASS_SERIAL_16550.contains(&(class_revision >> 8))
        || (id == ASIX_AX99100_ID && subsystem() == ASIX_AX99100_SERIAL)
}

/// The `nth` (from 1) 16550-compatible serial port on the PCI bus, in
/// [`for_each_function`]'s order, with I/O decoding switched on in it and in
/// every bridge leading to it -- once each of those bridges is seen to pass
/// the port through (its I/O window holds it), and not otherwise: a bridge
/// switched on with a window firmware never set would claim I/O ports the
/// machine's own devices answer.
///
/// Allocates nothing and needs nothing set up but the config ports, so the
/// serial console can call it before anything else runs.
pub fn early_uart(nth: usize) -> UartSearch {
    let mut seen = 0usize;
    let mut hit = None;
    for_each_function(|addr| {
        let (bus, dev, func) = (addr.bus, addr.device, addr.function);
        if hit.is_some()
            || !is_serial_16550(
                config_read32(bus, dev, func, CFG_VENDOR_ID),
                config_read32(bus, dev, func, CFG_CLASS_REVISION),
                || config_read32(bus, dev, func, CFG_SUBSYSTEM),
            )
        {
            return;
        }
        seen = seen.saturating_add(1);
        if seen == nth {
            hit = Some(addr);
        }
    });
    let Some(addr) = hit else {
        return UartSearch::TooFew(seen);
    };
    let bar = config_read32(addr.bus, addr.device, addr.function, CFG_BAR0);
    let port = match u16::try_from(bar & 0xFFFF_FFFC) {
        Ok(port) if bar & 1 != 0 && port != 0 => port,
        _ => return UartSearch::NoIoPorts(addr, bar),
    };
    // The bridges leading to it, each checked to pass the port through.
    let mut path = [addr; UART_PATH_MAX];
    let mut depth = 0usize;
    let mut refused = None;
    for_each_function(|bridge| {
        let (bus, dev, func) = (bridge.bus, bridge.device, bridge.function);
        if refused.is_some()
            || config_read8(bus, dev, func, CFG_HEADER_TYPE) & !HEADER_TYPE_MULTIFUNCTION
                != HEADER_TYPE_BRIDGE
        {
            return;
        }
        // A bridge leads to the buses from its secondary to its subordinate;
        // one firmware never numbered (secondary 0) leads nowhere.
        let behind = config_read8(bus, dev, func, CFG_SECONDARY_BUS);
        if behind == 0
            || !(behind..=config_read8(bus, dev, func, CFG_SUBORDINATE_BUS)).contains(&addr.bus)
        {
            return;
        }
        if !io_window(bridge).contains(&u32::from(port)) {
            refused = Some(UartSearch::BridgeClosed(bridge, port));
        } else if let Some(slot) = path.get_mut(depth) {
            *slot = bridge;
            depth = depth.saturating_add(1);
        } else {
            refused = Some(UartSearch::TooDeep(addr));
        }
    });
    if let Some(refusal) = refused {
        return refusal;
    }
    let mut uart = PciUart {
        address: addr,
        port,
        opened: 0,
        switched: [addr; UART_PATH_MAX + 1],
    };
    for &function in core::iter::once(&addr).chain(path.get(..depth).unwrap_or(&[])) {
        if open_io_decode(function)
            && let Some(slot) = uart.switched.get_mut(uart.opened)
        {
            *slot = function;
            uart.opened = uart.opened.saturating_add(1);
        }
    }
    UartSearch::Found(uart)
}

/// The I/O ports a bridge passes through to the buses behind it: its I/O
/// window ([`io_window_of`]).
fn io_window(bridge: PciAddress) -> core::ops::RangeInclusive<u32> {
    let (bus, dev, func) = (bridge.bus, bridge.device, bridge.function);
    let base = config_read8(bus, dev, func, CFG_IO_BASE);
    io_window_of(
        base,
        config_read8(bus, dev, func, CFG_IO_LIMIT),
        // Read only for 32-bit addressing: a 16-bit window's are not there.
        (base & 0x0F == 0x01).then(|| {
            (
                config_read16(bus, dev, func, CFG_IO_BASE_UPPER),
                config_read16(bus, dev, func, CFG_IO_LIMIT_UPPER),
            )
        }),
    )
}

/// A bridge's I/O window from its registers: base and limit (address bits
/// 15:12 in bits 7:4; the limit's low 12 bits all ones), and for 32-bit
/// addressing the upper 16 bits of each (PCI-to-PCI Bridge Architecture
/// Specification 1.2, 3.2.5.6). Empty when the base is above the limit.
fn io_window_of(base: u8, limit: u8, upper: Option<(u16, u16)>) -> core::ops::RangeInclusive<u32> {
    let (base_upper, limit_upper) = upper.unwrap_or((0, 0));
    let low = (u32::from(base & 0xF0) << 8) | (u32::from(base_upper) << 16);
    let high = (u32::from(limit & 0xF0) << 8) | 0xFFF | (u32::from(limit_upper) << 16);
    low..=high
}

/// Switch off again the I/O decoding [`early_uart`] switched on for `uart`:
/// for a port that then did not answer.
pub fn close_io_decode(uart: &PciUart) {
    for &function in uart.switched.get(..uart.opened).unwrap_or(&[]) {
        let command = config_read16(
            function.bus,
            function.device,
            function.function,
            CFG_COMMAND,
        );
        config_write16_native(
            function.bus,
            function.device,
            function.function,
            CFG_COMMAND,
            command & !CMD_IO_SPACE,
        );
    }
}

/// Switch on I/O decoding in a function's command register; whether it was
/// off. A genuine 16-bit write, so the status register beside it -- whose
/// error bits clear when a 1 is written to them -- is left alone.
fn open_io_decode(addr: PciAddress) -> bool {
    let command = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
    if command & CMD_IO_SPACE != 0 {
        return false;
    }
    config_write16_native(
        addr.bus,
        addr.device,
        addr.function,
        CFG_COMMAND,
        command | CMD_IO_SPACE,
    );
    true
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
        address: PciAddress {
            bus,
            device,
            function,
        },
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
    let devices = scan_all();
    devices
        .into_iter()
        .find(|d| d.vendor_id == vendor && d.device_id == device)
}

/// Find all PCI devices matching a vendor/device ID pair.
///
/// Returns every matching function on every bus the scan reaches, behind
/// bridges too (`scan_all`). Useful for discovering multiple instances of
/// the same device type (e.g., multiple virtio-blk controllers).
pub fn find_all_devices(vendor: u16, device: u16) -> Vec<PciDevice> {
    let devices = scan_all();
    devices
        .into_iter()
        .filter(|d| d.vendor_id == vendor && d.device_id == device)
        .collect()
}

/// Find all PCI devices matching a class/subclass pair.
///
/// Useful for discovering all devices of a category regardless of
/// vendor (e.g., all mass-storage controllers: class=0x01).
#[allow(dead_code)] // API for drivers zone; unused until driver framework.
pub fn find_devices_by_class(class: u8, subclass: u8) -> Vec<PciDevice> {
    let devices = scan_all();
    devices
        .into_iter()
        .filter(|d| d.class == class && d.subclass == subclass)
        .collect()
}

/// Write the 16-bit Command register without disturbing the Status register
/// that shares its dword.
///
/// Command (0x04) and Status (0x06) are two halves of one dword, and the only
/// config-space write the 0xCF8/0xCFC mechanism offers is 32 bits wide — so
/// every command write necessarily writes *something* to status.  The Status
/// register's error bits (Master Data Parity Error, Signalled Target Abort,
/// Received Master Abort, …) are **write-1-to-clear**, which makes the obvious
/// read-modify-write wrong: reading status and writing it back sets a 1 into
/// every bit that was already 1, and so clears exactly the errors that had
/// been recorded.  A routine that merely enables DMA would silently destroy
/// the evidence of a bus fault.
///
/// Writing **zero** into the status half is the correct move: 0 is a no-op for
/// a write-1-to-clear bit, so every recorded error survives untouched.
fn write_command(addr: PciAddress, cmd: u16) {
    // Status half deliberately zero — see the doc comment.
    config_write32(
        addr.bus,
        addr.device,
        addr.function,
        CFG_COMMAND,
        u32::from(cmd),
    );
}

/// `addr`'s command register changed by `change`, read and written as one:
/// the config ports held throughout, with interrupts off, so that no other
/// CPU's change of it, and no interrupt handler's on this one, falls
/// between the read and the write -- each would otherwise undo the other's
/// bits. Nothing is written when `change` keeps it as it was. Returns what
/// it was.
pub fn update_command(addr: PciAddress, change: impl FnOnce(u16) -> u16) -> u16 {
    CONFIG_PORTS.with(|_| {
        let cmd = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
        let new = change(cmd);
        if new != cmd {
            write_command(addr, new);
        }
        cmd
    })
}

/// Enable bus mastering (DMA) for a PCI device.
///
/// Also enables I/O space and memory space access.
pub fn enable_bus_master(addr: PciAddress) {
    update_command(addr, |cmd| {
        cmd | CMD_IO_SPACE | CMD_MEMORY_SPACE | CMD_BUS_MASTER
    });
}

/// Clear bus mastering for a PCI device, stopping any DMA it is performing.
///
/// Used when quiescing the machine for a kexec handoff: a device left bus-
/// mastering could DMA into memory the new kernel is about to use, through the
/// old kernel's now-defunct mappings. Clearing the command word's bus-master bit
/// on every function stops that at the source, without a per-driver shutdown
/// path (each driver resets its device when the new kernel probes it). I/O and
/// memory space are left as they were.
pub fn disable_bus_master(addr: PciAddress) {
    update_command(addr, |cmd| cmd & !CMD_BUS_MASTER);
}

/// Stop every function on every bus mastering, for a kexec: DMA stops at
/// its source. Called with every other CPU stopped where it stood, so it
/// takes no lock and allocates nothing: the config ports are seized from
/// whichever stopped CPU held them ([`CONFIG_PORTS`]), and the bus is
/// walked afresh rather than read from the kept scan, whose lock a stopped
/// CPU may hold for good.
///
/// # Safety
///
/// For a restart, with the other CPUs stopped: a CPU still holding the
/// config ports after `WindowLock::seize`'s wait must be one that will
/// never run again.
pub unsafe fn disable_bus_master_everywhere() {
    // SAFETY: the caller's.
    unsafe { CONFIG_PORTS.seize() };
    for_each_function(disable_bus_master);
}

// ---------------------------------------------------------------------------
// Legacy INTx interrupt control
// ---------------------------------------------------------------------------
//
// Legacy PCI interrupt pins are **level-triggered and shared**: several
// functions are wired to one IOAPIC input, and the line stays asserted until
// *every* function driving it has been told to stop, by a device-specific
// write that only that device's driver knows how to make.
//
// That makes an unhandled asserting function uniquely destructive.  An
// unhandled *edge* interrupt is one wasted trip through the ISR; an unhandled
// *level* interrupt never deasserts, so the CPU re-enters the handler the
// instant it returns, forever.  On this tree's QEMU configuration IRQ 10 is
// shared by eight functions, of which the kernel's ISR knows how to quiesce
// three — and a storm there has been observed at ~500 kHz, starving the
// scheduler badly enough to wedge a boot (`known-issues.md`, "IRQ 10 storming
// at ~500 kHz").
//
// PCI 2.3 gives the generic remedy: Command bit 10, **Interrupt Disable**.
// Setting it forbids the function from asserting its pin at all.  It is one
// bit with identical meaning on every conforming function, which is what
// makes it the right tool — the alternative, a per-device stub handler that
// acks, needs correct knowledge of a different status register for each of
// AC'97, NVMe, xHCI and AHCI, and a stub that guesses wrong leaves the storm
// in place while looking like a fix.
//
// The policy this module implements: **a function whose interrupt nothing
// services must not be permitted to assert one.**  Drivers that do service a
// function's INTx call [`claim_intx`]; [`quiesce_unclaimed_intx`] then
// silences everything else.  Order does not matter — a claim that arrives
// after the sweep re-enables the function it names.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Sentinel for an unused slot in [`INTX_CLAIMS`].
///
/// A real packed address always has bit 31 clear (bus, device and function
/// together occupy 16 bits), so `u32::MAX` cannot collide with one.
const CLAIM_EMPTY: u32 = u32::MAX;

/// Upper bound on functions claiming INTx.
///
/// Sized for a bus-0-only enumeration (32 devices × 8 functions is the
/// theoretical ceiling, but only a handful ever take a legacy interrupt).
/// Overflow is reported rather than silently dropped — see [`claim_intx`].
const MAX_INTX_CLAIMS: usize = 32;

/// Functions whose driver services their legacy interrupt.
static INTX_CLAIMS: [AtomicU32; MAX_INTX_CLAIMS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const EMPTY: AtomicU32 = AtomicU32::new(CLAIM_EMPTY);
    [EMPTY; MAX_INTX_CLAIMS]
};

/// Set once [`quiesce_unclaimed_intx`] has run.
///
/// Read by [`claim_intx`] so that a claim registered after the sweep undoes
/// the sweep's effect on that one function, making the two order-independent.
static INTX_SWEPT: AtomicBool = AtomicBool::new(false);

/// Pack a [`PciAddress`] into the 16-bit key stored in [`INTX_CLAIMS`]
/// (and `devhandle`'s INTx table).
///
/// Bus occupies bits 15:8, device bits 7:3, function bits 2:0 — so the key is
/// always below `1 << 16` and can never be mistaken for [`CLAIM_EMPTY`].
pub(crate) fn pack_addr(addr: PciAddress) -> u32 {
    (u32::from(addr.bus) << 8)
        | ((u32::from(addr.device) & 0x1F) << 3)
        | (u32::from(addr.function) & 0x07)
}

/// [`pack_addr`] undone.
// Each field is masked to its width before it is narrowed.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn unpack_addr(key: u32) -> PciAddress {
    PciAddress {
        bus: ((key >> 8) & 0xFF) as u8,
        device: ((key >> 3) & 0x1F) as u8,
        function: (key & 0x07) as u8,
    }
}

/// Is this function currently asserting its legacy interrupt pin?
///
/// Reads Status bit 3, which PCI 2.3 defines as read-only and **independent of
/// Command bit 10** — a function that has been silenced still reports that it
/// wanted to interrupt.  That independence is the point: it lets a storm
/// diagnostic name the culprit even after the culprit has been muzzled.
pub fn intx_asserting(addr: PciAddress) -> bool {
    let status = config_read16(addr.bus, addr.device, addr.function, CFG_STATUS);
    status & STATUS_INTERRUPT != 0
}

/// May this function assert its legacy interrupt pin?
pub fn intx_is_enabled(addr: PciAddress) -> bool {
    let cmd = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
    // Inverted sense: the *set* bit is the disable.
    cmd & CMD_INTX_DISABLE == 0
}

/// Permit or forbid this function from asserting its legacy interrupt pin.
///
/// Forbidding is safe for any function whose interrupt nothing handles: the
/// only thing lost is an interrupt that would have been ignored, and on a
/// shared level-triggered line "ignored" means "re-delivered forever".
///
/// This does not touch MSI or MSI-X, which are separate mechanisms with their
/// own enables; a function using either is unaffected either way.
pub fn intx_set_enabled(addr: PciAddress, enabled: bool) {
    update_command(addr, |cmd| {
        if enabled {
            cmd & !CMD_INTX_DISABLE
        } else {
            cmd | CMD_INTX_DISABLE
        }
    });
}

/// `addr`'s INTx quenched -- its INTx-disable bit set -- if it asserts its
/// pin while it may: whether it did. Its command and status registers are
/// read as one dword, and the quench written, under one hold of the config
/// ports ([`update_command`]'s reason). For an interrupt handler.
pub fn quench_intx_if_asserting(addr: PciAddress) -> bool {
    CONFIG_PORTS.with(|_| {
        let dword = config_read32(addr.bus, addr.device, addr.function, CFG_COMMAND);
        let (cmd, status) = split_command_status(dword);
        if cmd & CMD_INTX_DISABLE != 0 || status & STATUS_INTERRUPT == 0 {
            return false;
        }
        write_command(addr, cmd | CMD_INTX_DISABLE);
        true
    })
}

/// The command register (low half) and the status register (high half) of
/// the dword at offset 4.
#[allow(clippy::cast_possible_truncation)] // Each half is 16 bits, shifted down first.
fn split_command_status(dword: u32) -> (u16, u16) {
    (dword as u16, (dword >> 16) as u16)
}

/// Whether `addr` honours its INTx-disable bit -- PCI 2.3's INTx masking,
/// which a function from before it lacks, the bit then reading 0 whatever is
/// written: the bit flipped, read back, and put back as it was, under one
/// hold of the config ports, as Linux's `pci_intx_mask_supported` does.
pub fn intx_mask_supported(addr: PciAddress) -> bool {
    CONFIG_PORTS.with(|_| {
        let (b, d, f) = (addr.bus, addr.device, addr.function);
        let cmd = config_read16(b, d, f, CFG_COMMAND);
        write_command(addr, cmd ^ CMD_INTX_DISABLE);
        let flipped = config_read16(b, d, f, CFG_COMMAND);
        write_command(addr, cmd);
        (flipped ^ cmd) & CMD_INTX_DISABLE != 0
    })
}

/// `width` bytes (1, 2 or 4, naturally aligned) of `value` written at
/// `offset` in `addr`'s configuration space -- the INTx-disable bit, where
/// the write covers it, kept as `addr` has it now: read and written under
/// one hold of the config ports, so that an interrupt handler's quench
/// cannot fall between. A userspace driver's writes (`devhandle`), the bit
/// being the kernel's.
pub fn config_write_keeping_intx_disable(addr: PciAddress, offset: u8, width: u8, value: u32) {
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    // The bit's place in configuration space, and in the write, in bits.
    let bit = u32::from(CFG_COMMAND)
        .saturating_mul(8)
        .saturating_add(CMD_INTX_DISABLE.trailing_zeros());
    let at = bit
        .checked_sub(u32::from(offset).saturating_mul(8))
        .filter(|&at| at < u32::from(width).saturating_mul(8));
    CONFIG_PORTS.with(|_| {
        let value = match at {
            Some(at) => {
                let set = config_read16(b, d, f, CFG_COMMAND) & CMD_INTX_DISABLE != 0;
                (value & !(1 << at)) | u32::from(set) << at
            }
            None => value,
        };
        #[allow(clippy::cast_possible_truncation)] // Narrowed to the width written.
        match width {
            1 => config_write8(b, d, f, offset, value as u8),
            2 => config_write16_native(b, d, f, offset, value as u16),
            _ => config_write32(b, d, f, offset, value),
        }
    });
}

/// `addr`'s interrupt pin (0 for none, 1-4 for INTA-INTD) and the line the
/// firmware routed it to (the Interrupt Line register).
pub fn interrupt_pin_and_line(addr: PciAddress) -> (u8, u8) {
    let (b, d, f) = (addr.bus, addr.device, addr.function);
    (
        config_read8(b, d, f, CFG_INTERRUPT_PIN),
        config_read8(b, d, f, CFG_INTERRUPT_LINE),
    )
}

/// Declare that this function's legacy interrupt is serviced by a driver.
///
/// Call this from any driver that installs an ISR path capable of quiescing
/// the device — for the in-kernel drivers that means the ones
/// `ioapic::handle_device_irq` dispatches to.  A function that is not claimed
/// is silenced by [`quiesce_unclaimed_intx`].
///
/// Safe to call before or after the sweep, and safe to call twice.
pub fn claim_intx(addr: PciAddress) {
    let key = pack_addr(addr);
    let mut recorded = false;
    for slot in &INTX_CLAIMS {
        // Already recorded — a second claim from the same driver is a no-op.
        if slot.load(Ordering::Acquire) == key {
            recorded = true;
            break;
        }
        if slot
            .compare_exchange(CLAIM_EMPTY, key, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            recorded = true;
            break;
        }
    }

    if !recorded {
        // Louder than a silent drop on purpose: an unrecorded claim means the
        // sweep will silence a device whose driver is expecting interrupts,
        // which presents as that device simply never responding.
        crate::serial_println!(
            "[pci] WARNING: INTx claim table full ({} entries); {:02x}:{:02x}.{} not recorded",
            MAX_INTX_CLAIMS,
            addr.bus,
            addr.device,
            addr.function
        );
        return;
    }

    // If the sweep already ran, this function was silenced by it.  Undo that
    // now, so a driver initialising late is not penalised for its ordering.
    if INTX_SWEPT.load(Ordering::Acquire) {
        intx_set_enabled(addr, true);
    }
}

/// Has this function been claimed by a driver?
fn intx_is_claimed(addr: PciAddress) -> bool {
    let key = pack_addr(addr);
    INTX_CLAIMS
        .iter()
        .any(|slot| slot.load(Ordering::Acquire) == key)
}

/// Forbid legacy interrupts on every enumerated function no driver has
/// claimed, and report what was silenced.
///
/// Call once, after the last PCI driver has initialised.  Functions claimed
/// later via [`claim_intx`] re-enable themselves, so a late driver still
/// works; the sweep is therefore safe to run early rather than needing to be
/// threaded through every driver's completion.
///
/// Returns the number of functions silenced.
pub fn quiesce_unclaimed_intx() -> usize {
    // Publish before the scan.  A claim racing this sweep then either lands
    // in the table before we read it (and is skipped), or observes the flag
    // and re-enables itself afterwards.  It cannot fall between the two.
    INTX_SWEPT.store(true, Ordering::Release);

    let mut silenced = 0usize;
    for dev in scan_all() {
        // 0xFF means "not routed to any legacy interrupt line"; such a
        // function has no pin to silence.
        if dev.irq_line == 0xFF {
            continue;
        }
        if intx_is_claimed(dev.address) {
            continue;
        }
        if !intx_is_enabled(dev.address) {
            continue;
        }

        // Sample before silencing: a function asserting *right now* with no
        // handler is not a hypothetical risk, it is an active storm source,
        // and saying so turns the next storm report into a confirmation
        // rather than an investigation.
        let asserting = intx_asserting(dev.address);
        intx_set_enabled(dev.address, false);
        silenced = silenced.saturating_add(1);

        crate::serial_println!(
            "[pci] INTx disabled on {:02x}:{:02x}.{} ({:04x}:{:04x}, irq {}){}",
            dev.address.bus,
            dev.address.device,
            dev.address.function,
            dev.vendor_id,
            dev.device_id,
            dev.irq_line,
            if asserting {
                " — WAS ASSERTING with no handler"
            } else {
                ""
            }
        );
    }

    crate::serial_println!(
        "[pci] INTx quiesce: {} unclaimed function(s) silenced",
        silenced
    );
    silenced
}

/// Log every function wired to `irq` that is asserting its interrupt pin.
///
/// Used by the IRQ storm detector to name the device responsible.  Before this
/// existed, a storm on a shared line reported only the line number, which on
/// IRQ 10 narrows the culprit to one of eight functions — which is why
/// `known-issues.md` recorded the storm as needing "its own investigation"
/// rather than simply being fixed.
///
/// Allocation-free by way of [`for_each_function`], because this runs from the
/// timer softirq while a device is melting the line: the heap is not something
/// to lean on there.
pub fn report_intx_asserting_on_irq(irq: u8) {
    for_each_function(|addr| {
        if config_read8(addr.bus, addr.device, addr.function, CFG_INTERRUPT_LINE) != irq {
            return;
        }
        if !intx_asserting(addr) {
            return;
        }
        crate::serial_println!(
            "[pci]   irq {} asserted by {:02x}:{:02x}.{} ({:04x}:{:04x}){}{}",
            irq,
            addr.bus,
            addr.device,
            addr.function,
            config_read16(addr.bus, addr.device, addr.function, CFG_VENDOR_ID),
            config_read16(addr.bus, addr.device, addr.function, CFG_DEVICE_ID),
            if intx_is_claimed(addr) {
                " [claimed]"
            } else {
                " [UNCLAIMED]"
            },
            if intx_is_enabled(addr) {
                ""
            } else {
                " [INTx already disabled — cannot be the source]"
            }
        );
    });
}

/// Report every function on `irq` whose INTx pin is disabled, and return how
/// many there were.
///
/// This exists because [`quiesce_unclaimed_intx`] and `sys_irq_register` can
/// combine into a silent hang.  Registration unmasks the IOAPIC line and
/// records the task, and both succeed — but if the sweep already silenced the
/// functions routed to that line, INTx is off *at the device*, no interrupt is
/// ever raised, and the driver blocks in `sys_irq_wait` forever with nothing
/// in the log tying the hang back to a sweep that ran during boot.  Naming the
/// functions at registration time is what makes that diagnosable.
///
/// Deliberately reports rather than re-enables, even though [`claim_intx`]
/// un-silences itself in exactly this situation.  The asymmetry is forced:
/// `claim_intx` is called by a driver that owns a specific function and passes
/// its [`PciAddress`], whereas `sys_irq_register` receives only an IRQ number.
/// On a shared line that number can name several functions, and the caller
/// owns at most one of them — so re-enabling all of them would re-arm pins the
/// caller has no claim to, including whatever storm source the sweep was
/// quieting.  The syscall lacks the information needed to do this safely; the
/// fix is an interface that names the function, not a bolder guess here.  See
/// `known-issues.md` `TD-A-IRQ-REGISTER-CANNOT-NAME-THE-PCI-FUNCTION`.
///
/// Allocation-free via [`for_each_function`]: this runs from a syscall, and
/// mirrors [`report_intx_asserting_on_irq`] next door.
pub fn report_intx_silenced_on_irq(irq: u8) -> usize {
    let mut silenced = 0usize;
    for_each_function(|addr| {
        if config_read8(addr.bus, addr.device, addr.function, CFG_INTERRUPT_LINE) != irq {
            return;
        }
        if intx_is_enabled(addr) {
            return;
        }
        silenced = silenced.saturating_add(1);
        crate::serial_println!(
            "[pci]   irq {} has INTx disabled on {:02x}:{:02x}.{} ({:04x}:{:04x}){}",
            irq,
            addr.bus,
            addr.device,
            addr.function,
            config_read16(addr.bus, addr.device, addr.function, CFG_VENDOR_ID),
            config_read16(addr.bus, addr.device, addr.function, CFG_DEVICE_ID),
            if intx_is_claimed(addr) {
                " [claimed by an in-kernel driver]"
            } else {
                " [silenced by the unclaimed-INTx sweep]"
            }
        );
    });
    silenced
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

        caps.push(PciCapability {
            id: cap_id,
            offset: ptr,
        });

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
    walk_capabilities(addr)
        .into_iter()
        .filter(|c| c.id == cap_id)
        .collect()
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
// Which driver took which function
// ---------------------------------------------------------------------------

/// The in-kernel driver that has taken each PCI function, by packed address
/// ([`pack_addr`]).
///
/// The question a device manager exists to answer -- which device has no
/// driver? -- needs this, and nothing recorded it: each driver found its
/// function and initialised it, and only the driver knew
/// (`requests/e-a-publish-each-pci-functions-irq-bars-and-driver.md`).
/// `/sys/devices/pci/<BB:DD.F>` reads it as its `driver:` line. Leaf lock.
static DRIVER_BINDINGS: crate::sync::PreemptSpinMutex<Vec<(u32, &'static str)>> =
    crate::sync::PreemptSpinMutex::new(Vec::new());

/// Record that the in-kernel driver `name` has taken function `addr` -- call
/// it once the driver has initialised the function, not when it merely finds
/// it, so that a function whose initialisation failed reads as unclaimed.
///
/// A later binding of the same function replaces the earlier one (a driver
/// re-probing after a reset). Also reported as a hot-plug event
/// ([`crate::devhotplug::driver_bound`]), as a binding is.
pub fn bind_driver(addr: PciAddress, name: &'static str) {
    record_binding(addr, name);
    crate::serial_println!(
        "[pci] {:02x}:{:02x}.{} is driven by {}",
        addr.bus,
        addr.device,
        addr.function,
        name
    );
    crate::devhotplug::driver_bound(
        crate::udriver::DeviceAddr::new(addr.bus, addr.device, addr.function),
        name,
    );
}

/// Forget that `name` drives `addr`, if it is the one recorded -- a binding
/// another driver holds is left alone, so a release cannot unbind it -- and
/// say so, as [`bind_driver`] does: a log line and a hot-plug event.
pub fn unbind_driver(addr: PciAddress, name: &'static str) {
    let key = pack_addr(addr);
    let removed = {
        let mut bindings = DRIVER_BINDINGS.lock();
        let before = bindings.len();
        bindings.retain(|&(k, n)| k != key || n != name);
        bindings.len() != before
    };
    if removed {
        crate::serial_println!(
            "[pci] {:02x}:{:02x}.{} is no longer driven by {}",
            addr.bus,
            addr.device,
            addr.function,
            name
        );
        crate::devhotplug::driver_unbound(
            crate::udriver::DeviceAddr::new(addr.bus, addr.device, addr.function),
            name,
        );
    }
}

/// [`bind_driver`]'s record alone, without its log line and hot-plug event:
/// what the self-test can make and unmake without leaving an event behind.
fn record_binding(addr: PciAddress, name: &'static str) {
    let key = pack_addr(addr);
    let mut bindings = DRIVER_BINDINGS.lock();
    match bindings.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = name,
        None => bindings.push((key, name)),
    }
}

/// The in-kernel driver that has taken function `addr`, or `None` if none
/// has ([`bind_driver`]).
#[must_use]
pub fn bound_driver(addr: PciAddress) -> Option<&'static str> {
    let key = pack_addr(addr);
    DRIVER_BINDINGS
        .lock()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|&(_, name)| name)
}

/// One implemented BAR, as `/sys/devices/pci` reports it: its index, base,
/// whether it is I/O-port space, and for memory whether it is 64-bit and
/// prefetchable. A 64-bit BAR spans two registers and is reported once, at
/// the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarInfo {
    /// The BAR's register index, 0-5.
    pub index: usize,
    /// Its base address (I/O port or physical memory).
    pub base: u64,
    /// I/O-port space rather than memory.
    pub io: bool,
    /// A 64-bit memory BAR.
    pub is_64: bool,
    /// Prefetchable memory.
    pub prefetch: bool,
}

/// The implemented BARs of `dev`, decoded from the raw values the scan read:
/// a register reading 0 has nothing assigned and is left out. No size: the
/// kernel does not size BARs (that writes all-ones to a register a running
/// driver uses).
#[must_use]
pub fn decode_bars(dev: &PciDevice) -> Vec<BarInfo> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while let Some(&raw) = dev.bars.get(index) {
        let next = index.saturating_add(1);
        if raw == 0 {
            index = next;
            continue;
        }
        if raw & 1 != 0 {
            out.push(BarInfo {
                index,
                base: u64::from(raw & 0xFFFF_FFFC),
                io: true,
                is_64: false,
                prefetch: false,
            });
            index = next;
            continue;
        }
        let is_64 = (raw >> 1) & 0x3 == 0x2;
        let base = bar_mmio_addr64(dev, index).unwrap_or(u64::from(raw & 0xFFFF_FFF0));
        out.push(BarInfo {
            index,
            base,
            io: false,
            is_64,
            prefetch: raw & 0x8 != 0,
        });
        // A 64-bit BAR's upper half is the next register, not a BAR of its own.
        index = if is_64 { next.saturating_add(1) } else { next };
    }
    out
}

// ---------------------------------------------------------------------------
// The requester id an MSI carries
// ---------------------------------------------------------------------------

/// What an interrupt remapping entry for `addr`'s MSI can check of the
/// requester id it arrives with. A function on a PCI Express path sends
/// its own; behind a bridge to conventional PCI, the bridge may send it
/// under another, which only a bus range, or the bridge's own id, matches.
/// Linux's `pci_for_each_dma_alias` walk, read as its interrupt remapping's
/// `set_msi_sid` reads it.
#[must_use]
pub fn msi_source(addr: PciAddress) -> MsiSource {
    msi_source_in(addr, parent_bridge)
}

/// The bridge whose secondary bus is `bus`, if one leads there: none for
/// bus 0, or for a bus no bridge leads to (a root complex's own).
fn parent_bridge(bus: u8) -> Option<Upstream> {
    if bus == 0 {
        return None;
    }
    let mut found = None;
    for_each_function(|f| {
        if found.is_some()
            || config_read8(f.bus, f.device, f.function, CFG_HEADER_TYPE)
                & !HEADER_TYPE_MULTIFUNCTION
                != HEADER_TYPE_BRIDGE
            || config_read8(f.bus, f.device, f.function, CFG_SECONDARY_BUS) != bus
        {
            return;
        }
        let pcie_type = find_capability(f, CAP_PCIE)
            .map(|cap| {
                (config_read16(f.bus, f.device, f.function, cap.offset.wrapping_add(2)) >> 4) & 0xF
            })
            // A nibble.
            .map(|t| t as u8);
        found = Some(Upstream {
            address: f,
            pcie_type,
            secondary: bus,
        });
    });
    found
}

/// [`msi_source`] over the topology `parent` describes, `parent(bus)`
/// naming the bridge leading to `bus`.
fn msi_source_in(addr: PciAddress, parent: impl Fn(u8) -> Option<Upstream>) -> MsiSource {
    let own = addr.requester_id();
    // The last alias the walk found, and the bus of the function that made
    // it; how many aliases, and how many of them on the bus before.
    let (mut alias, mut alias_from_bus) = (own, addr.bus);
    let (mut count, mut same_bus) = (1u32, 1u32);
    let mut bus = addr.bus;
    // A bus is passed once at most; more is a loop in the topology.
    for _ in 0..256 {
        let Some(up) = parent(bus) else {
            break;
        };
        let found = match up.pcie_type {
            Some(PCIE_ROOT_PORT | PCIE_UPSTREAM_PORT | PCIE_DOWNSTREAM_PORT) => None,
            Some(PCIE_TO_PCI_BRIDGE) => Some(u16::from(up.secondary) << 8),
            Some(PCI_TO_PCIE_BRIDGE) | None => Some(up.address.requester_id()),
            Some(_) => None,
        };
        if let Some(next) = found {
            if next >> 8 == alias >> 8 {
                same_bus = same_bus.saturating_add(1);
            }
            alias = next;
            alias_from_bus = up.address.bus;
            count = count.saturating_add(1);
        }
        bus = up.address.bus;
    }
    // The high byte: a bus number.
    #[allow(clippy::cast_possible_truncation)]
    let alias_bus = (alias >> 8) as u8;
    if alias_bus != alias_from_bus {
        MsiSource::Buses {
            start: alias_bus,
            end: addr.bus,
        }
    } else if count >= 2 && same_bus == count {
        MsiSource::Buses {
            start: addr.bus,
            end: addr.bus,
        }
    } else if alias_from_bus != addr.bus {
        MsiSource::Exact(alias)
    } else {
        MsiSource::Exact(own)
    }
}

/// [`msi_source_in`] on four made-up topologies, and [`PciAddress::
/// requester_id`] bit by bit.
fn self_test_msi_source() -> Result<(), &'static str> {
    let at = |bus, device, function| PciAddress {
        bus,
        device,
        function,
    };
    let bridge = |address, pcie_type, secondary| Upstream {
        address,
        pcie_type,
        secondary,
    };
    // 00:1c.0 a root port to bus 4; 04:00.0 a PCIe-to-PCI bridge to bus 5;
    // 05:03.0 a conventional PCI bridge to bus 6.
    let topology = |bus: u8| match bus {
        4 => Some(bridge(at(0, 0x1C, 0), Some(PCIE_ROOT_PORT), 4)),
        5 => Some(bridge(at(4, 0, 0), Some(PCIE_TO_PCI_BRIDGE), 5)),
        6 => Some(bridge(at(5, 3, 0), None, 6)),
        _ => None,
    };
    let cases = [
        // On the root bus: its own.
        (at(0, 0x1F, 3), MsiSource::Exact(0x00FB)),
        // Behind a root port only: its own.
        (at(4, 0, 1), MsiSource::Exact(0x0401)),
        // Behind the PCIe-to-PCI bridge: bus 5 only.
        (at(5, 2, 0), MsiSource::Buses { start: 5, end: 5 }),
        // Behind the conventional bridge too: buses 5 to 6.
        (at(6, 1, 0), MsiSource::Buses { start: 5, end: 6 }),
    ];
    for (addr, want) in cases {
        let got = msi_source_in(addr, topology);
        if got != want {
            crate::serial_println!(
                "[pci]   FAIL: {:x?}'s MSI source {:x?}, want {:x?}",
                addr,
                got,
                want
            );
            return Err("an MSI's requester id was read wrongly from the topology");
        }
    }
    // A PCI-to-PCIe bridge sends under its own id.
    let up_only = |bus: u8| (bus == 7).then(|| bridge(at(3, 4, 2), Some(PCI_TO_PCIE_BRIDGE), 7));
    if msi_source_in(at(7, 0, 0), up_only) != MsiSource::Exact(0x0322)
        || at(0xAB, 0x1F, 7).requester_id() != 0xABFF
        || at(1, 0x20, 8).requester_id() != 0x0100
    {
        return Err("a requester id is not laid out as PCI lays it out");
    }
    crate::serial_println!("[pci]   MSI requester ids through bridges: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Scan every bus and log all discovered PCI devices.
pub fn self_test() -> Result<(), &'static str> {
    crate::serial_println!("[pci] Scanning the PCI buses...");

    let devices = scan_all();
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
    // Kept: the next caller is given this scan, not a new one.
    if SCANNED.lock().as_ref().map(Vec::len) != Some(devices.len()) {
        crate::serial_println!("[pci]   FAIL: the scan was not kept");
        return Err("scan not kept");
    }

    self_test_intx(&devices)?;
    self_test_bindings_and_bars()?;
    self_test_serial_match()?;
    self_test_nested_access()?;
    self_test_msi_source()?;

    crate::serial_println!("[pci] Self-test PASSED");
    Ok(())
}

/// [`config_access`] entered by the CPU that holds the ports, as an NMI
/// interrupting its access would: the read must still be right, and the
/// address the interrupted access selected must be back in place.
fn self_test_nested_access() -> Result<(), &'static str> {
    let normal = config_read32(0, 0, 0, CFG_VENDOR_ID);
    let selected = config_address(0, 0, 0, CFG_CLASS_REVISION);
    // Hold the ports as an access does, and select a register; then a read
    // from inside, as an NMI's would be.
    let (nested, after) = CONFIG_PORTS.with(|_| {
        // SAFETY: 0xCF8 is the config address port; this CPU holds it.
        unsafe { port::outl(PCI_CONFIG_ADDR, selected) };
        let nested = config_read32(0, 0, 0, CFG_VENDOR_ID);
        // SAFETY: as above.
        let after = unsafe { port::inl(PCI_CONFIG_ADDR) };
        (nested, after)
    });
    if nested != normal || after != selected {
        crate::serial_println!(
            "[pci]   FAIL: a nested config read gave {:#010x} (want {:#010x}) and left {:#010x} selected (want {:#010x})",
            nested,
            normal,
            after,
            selected
        );
        return Err("nested config access");
    }
    crate::serial_println!("[pci]   nested config access: OK");
    Ok(())
}

/// [`is_serial_16550`] on made-up register values: a 16550 by class, an
/// AX99100 serial function, and the near misses -- another kind of serial
/// controller, an AX99100 parallel port, another vendor's chip carrying the
/// AX99100's subsystem ids.
fn self_test_serial_match() -> Result<(), &'static str> {
    let cases: [(u32, u32, u32, bool); 9] = [
        // QEMU's pci-serial: class 07/00/02, revision 1.
        (0x0002_1B36, 0x0700_0201, 0, true),
        (ASIX_AX99100_ID, 0x0700_0000, ASIX_AX99100_SERIAL, true),
        // An 8250 (interface 00) and a 16450 (01): not 16550s. A 16650 (03)
        // and a 16950 (06) are: the 16550's registers, and more.
        (0x0002_1B36, 0x0700_0001, 0, false),
        (0x0002_1B36, 0x0700_0101, 0, false),
        (0x0002_1B36, 0x0700_0301, 0, true),
        (0x0002_1B36, 0x0700_0601, 0, true),
        (0x0002_1B36, 0x0700_0701, 0, false),
        // The AX99100's parallel port (subsystem 2000h).
        (ASIX_AX99100_ID, 0x0701_0300, 0x2000_A000, false),
        (0x9100_1234, 0x0780_0000, ASIX_AX99100_SERIAL, false),
    ];
    for (id, class_revision, subsystem, want) in cases {
        if is_serial_16550(id, class_revision, || subsystem) != want {
            crate::serial_println!(
                "[pci]   FAIL: id {:#010x} class {:#010x} subsystem {:#010x} taken for a 16550: {}",
                id,
                class_revision,
                subsystem,
                !want
            );
            return Err("serial port matching");
        }
    }
    // A bridge's I/O window: as reset (0, 0: the first 4 KiB, which a port
    // firmware placed higher is not in), one at 0x6000, a 32-bit one, and one
    // turned off (base above limit).
    let windows = [
        io_window_of(0, 0, None),
        io_window_of(0x60, 0x70, None),
        io_window_of(0x61, 0x61, Some((1, 1))),
    ];
    let off = io_window_of(0xF0, 0x00, None);
    if windows != [0..=0xFFF, 0x6000..=0x7FFF, 0x1_6000..=0x1_6FFF]
        || windows
            .iter()
            .map(|w| w.contains(&0x6000))
            .collect::<Vec<_>>()
            != [false, true, false]
        || !off.is_empty()
    {
        crate::serial_println!("[pci]   FAIL: bridge I/O windows read as {:x?}", windows);
        return Err("bridge I/O window");
    }
    crate::serial_println!("[pci]   serial port matching, bridge I/O windows: OK");
    Ok(())
}

/// [`bind_driver`] / [`bound_driver`] on an address no function has, and
/// [`decode_bars`] on register values made up for each case: an I/O BAR, a
/// 32-bit memory BAR, a 64-bit prefetchable one spanning two registers (and
/// reported once), and an unassigned register left out.
fn self_test_bindings_and_bars() -> Result<(), &'static str> {
    // An address no function answers at, so nothing real is bound there:
    // function 7 of slot 31, on the highest bus where none does.
    let nowhere = (0..=0xFFu8)
        .rev()
        .map(|bus| PciAddress {
            bus,
            device: 31,
            function: 7,
        })
        .find(|a| config_read16(a.bus, a.device, a.function, CFG_VENDOR_ID) == 0xFFFF)
        .ok_or("every bus has a function at slot 31, function 7")?;
    if bound_driver(nowhere).is_some() {
        return Err("an address no driver took reads as bound");
    }
    record_binding(nowhere, "selftest-a");
    record_binding(nowhere, "selftest-b");
    let bound = bound_driver(nowhere);
    // Leave the table as the boot's drivers made it.
    DRIVER_BINDINGS
        .lock()
        .retain(|&(k, _)| k != pack_addr(nowhere));
    if bound != Some("selftest-b") {
        return Err("a second binding of a function did not replace the first");
    }

    let dev = PciDevice {
        address: nowhere,
        vendor_id: 0,
        device_id: 0,
        class: 0,
        subclass: 0,
        irq_line: 0xFF,
        // BAR0: I/O at 0xc040. BAR1: unassigned. BAR2+3: 64-bit prefetchable
        // memory at 0x1_8000_0000. BAR4: 32-bit memory at 0xfeb0_0000.
        bars: [0xc041, 0, 0x8000_000c, 0x1, 0xfeb0_0000, 0],
    };
    let want = [
        BarInfo {
            index: 0,
            base: 0xc040,
            io: true,
            is_64: false,
            prefetch: false,
        },
        BarInfo {
            index: 2,
            base: 0x1_8000_0000,
            io: false,
            is_64: true,
            prefetch: true,
        },
        BarInfo {
            index: 4,
            base: 0xfeb0_0000,
            io: false,
            is_64: false,
            prefetch: false,
        },
    ];
    if decode_bars(&dev) != want {
        crate::serial_println!("[pci]   decode_bars: {:?}", decode_bars(&dev));
        return Err("decode_bars misread a BAR");
    }
    crate::serial_println!("[pci]   driver bindings and BAR decoding: OK");
    Ok(())
}

/// Exercise the legacy-INTx control path against the real devices on the bus.
///
/// Runs before any driver has claimed anything and before
/// [`quiesce_unclaimed_intx`], so it is free to toggle bits as long as it puts
/// every one of them back.
fn self_test_intx(devices: &[PciDevice]) -> Result<(), &'static str> {
    // --- 1. The claim key is injective and never collides with the sentinel.
    //
    // A collision would make one function's claim silently protect a
    // different function, and the sentinel case would make an empty slot read
    // as a claim — both fail in the direction of leaving a storm in place.
    let probe = [
        (0u8, 0u8, 0u8),
        (0, 0, 1),
        (0, 1, 0),
        (1, 0, 0),
        (0xFF, 0x1F, 7),
    ]
    .map(|(bus, device, function)| PciAddress {
        bus,
        device,
        function,
    });
    for (i, a) in probe.iter().enumerate() {
        let key = pack_addr(*a);
        if key == CLAIM_EMPTY {
            return Err("pack_addr collided with the empty-slot sentinel");
        }
        for b in probe.iter().skip(i.saturating_add(1)) {
            if key == pack_addr(*b) {
                return Err("pack_addr is not injective over bus/device/function");
            }
        }
    }
    crate::serial_println!("[pci]   INTx claim key: injective, no sentinel collision OK");

    // --- 2. Enable/disable is observable and reversible on every function,
    //        and the write does not disturb the Status register.
    //
    // Command and Status share one dword and the config mechanism only writes
    // 32 bits, so every command write touches status.  Status' error bits are
    // write-1-to-clear, which means a read-modify-write of the whole dword
    // clears exactly the errors that were recorded.  Assert the bits survive.
    let mut toggled = 0usize;
    let mut rw1c_witnessed = 0usize;
    for dev in devices {
        let addr = dev.address;
        let original = intx_is_enabled(addr);
        let status_before = config_read16(addr.bus, addr.device, addr.function, CFG_STATUS);

        // Any write-1-to-clear bit set right now makes the status check below
        // a real test rather than a vacuous one; count them so the log says
        // which it was.
        const RW1C_MASK: u16 = (1 << 8) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 14) | (1 << 15);
        if status_before & RW1C_MASK != 0 {
            rw1c_witnessed = rw1c_witnessed.saturating_add(1);
        }

        intx_set_enabled(addr, !original);
        let flipped = intx_is_enabled(addr);
        let status_after = config_read16(addr.bus, addr.device, addr.function, CFG_STATUS);

        // Put it back before judging, so a failure cannot also leave the bus
        // in a changed state.
        intx_set_enabled(addr, original);
        let restored = intx_is_enabled(addr);

        // The bit did not move, i.e. it read back as it was before the write.
        if flipped == original {
            // Not fatal on its own: a function that hardwires Interrupt
            // Disable is permitted, and QEMU's host bridge does exactly that.
            // Only count the ones that genuinely moved.
            continue;
        }
        toggled = toggled.saturating_add(1);

        if status_after & RW1C_MASK != status_before & RW1C_MASK {
            return Err("a Command-register write cleared write-1-to-clear Status bits");
        }
        if restored != original {
            return Err("INTx enable state was not restored after the toggle");
        }
    }

    if toggled == 0 {
        return Err("no PCI function accepted an INTx enable/disable toggle");
    }
    crate::serial_println!(
        "[pci]   INTx enable/disable: {} function(s) toggled and restored, Status preserved ({} carried a write-1-to-clear bit) OK",
        toggled,
        rw1c_witnessed
    );

    // --- 3. Nothing is claimed yet — the sweep has not run and no driver has
    //        initialised, so a `true` here would mean the table starts dirty.
    if devices.iter().any(|d| intx_is_claimed(d.address)) {
        return Err("INTx claim table was non-empty before any driver initialised");
    }
    crate::serial_println!("[pci]   INTx claim table: empty before driver init OK");

    Ok(())
}
