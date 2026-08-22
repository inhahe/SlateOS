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
/// INTx assertion **disable** (PCI 2.3+).  Set = the function may not drive
/// its legacy interrupt pin.  Note the inverted sense: the bit *disables*.
const CMD_INTX_DISABLE: u16 = 1 << 10;

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
    | u32::from(offset & 0xFC) // Dword-aligned
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
        let header_type = config_read8(0, device, 0, CFG_HEADER_TYPE);
        if header_type & HEADER_TYPE_MULTIFUNCTION != 0 {
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

/// Call `f` once for each PCI function present on bus 0, without allocating.
///
/// [`scan_bus0`] is the convenient form, but it builds a `Vec`, and a caller
/// running in interrupt or softirq context during a fault — the IRQ storm
/// diagnostic is exactly that — must not depend on the heap being healthy.  An
/// allocation failure inside a kernel is a panic, so a diagnostic that
/// allocates can turn a recoverable storm into a dead machine at precisely the
/// moment it was supposed to explain one.
///
/// The traversal rule is the same as `scan_bus0`'s: probe function 0, and only
/// look at functions 1–7 when the header type says the device is
/// multi-function.
fn for_each_function(mut f: impl FnMut(PciAddress)) {
    for device in 0..32u8 {
        if config_read16(0, device, 0, CFG_VENDOR_ID) == 0xFFFF {
            continue; // No device in this slot.
        }

        f(PciAddress {
            bus: 0,
            device,
            function: 0,
        });

        // Multi-function device (header type bit 7)?
        if config_read8(0, device, 0, CFG_HEADER_TYPE) & HEADER_TYPE_MULTIFUNCTION != 0 {
            for function in 1..8u8 {
                if config_read16(0, device, function, CFG_VENDOR_ID) != 0xFFFF {
                    f(PciAddress {
                        bus: 0,
                        device,
                        function,
                    });
                }
            }
        }
    }
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
    let devices = scan_bus0();
    devices
        .into_iter()
        .find(|d| d.vendor_id == vendor && d.device_id == device)
}

/// Find all PCI devices matching a vendor/device ID pair.
///
/// Returns every matching device on bus 0.  Useful for discovering
/// multiple instances of the same device type (e.g., multiple
/// virtio-blk controllers).
#[allow(dead_code)] // API for drivers zone; unused until multi-device support.
pub fn find_all_devices(vendor: u16, device: u16) -> Vec<PciDevice> {
    let devices = scan_bus0();
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
    let devices = scan_bus0();
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

/// Enable bus mastering (DMA) for a PCI device.
///
/// Also enables I/O space and memory space access.
pub fn enable_bus_master(addr: PciAddress) {
    let cmd = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
    write_command(addr, cmd | CMD_IO_SPACE | CMD_MEMORY_SPACE | CMD_BUS_MASTER);
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

/// Pack a [`PciAddress`] into the 16-bit key stored in [`INTX_CLAIMS`].
///
/// Bus occupies bits 15:8, device bits 7:3, function bits 2:0 — so the key is
/// always below `1 << 16` and can never be mistaken for [`CLAIM_EMPTY`].
fn pack_addr(addr: PciAddress) -> u32 {
    (u32::from(addr.bus) << 8)
        | ((u32::from(addr.device) & 0x1F) << 3)
        | (u32::from(addr.function) & 0x07)
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
    let cmd = config_read16(addr.bus, addr.device, addr.function, CFG_COMMAND);
    let new_cmd = if enabled {
        cmd & !CMD_INTX_DISABLE
    } else {
        cmd | CMD_INTX_DISABLE
    };
    if new_cmd != cmd {
        write_command(addr, new_cmd);
    }
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
    for dev in scan_bus0() {
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

    self_test_intx(&devices)?;

    crate::serial_println!("[pci] Self-test PASSED");
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
