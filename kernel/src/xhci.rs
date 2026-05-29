//! xHCI (eXtensible Host Controller Interface) USB 3.x driver.
//!
//! Implements USB device enumeration and basic transfer operations
//! via the xHCI host controller specification (Intel revision 1.2).
//!
//! ## Architecture
//!
//! The xHCI controller communicates via memory-mapped registers and
//! three ring buffer types:
//!
//! - **Command Ring**: driver submits device management commands
//! - **Event Ring**: controller posts completion events
//! - **Transfer Rings**: per-endpoint data transfer requests
//!
//! All ring entries are Transfer Request Blocks (TRBs), 16 bytes each.
//!
//! ## PCI Detection
//!
//! xHCI controllers appear as PCI class 0x0C (Serial Bus), subclass
//! 0x03 (USB), prog-if 0x30 (xHCI).  BAR0 is always a 64-bit MMIO
//! region containing the register space.
//!
//! ## References
//!
//! Based on the Intel xHCI specification revision 1.2 and the Linux
//! kernel's `drivers/usb/host/xhci*.c` implementation.


use alloc::vec::Vec;
use core::sync::atomic::{fence, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci;

// ---------------------------------------------------------------------------
// PCI identification
// ---------------------------------------------------------------------------

/// PCI class for serial bus controllers.
const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
/// PCI subclass for USB controllers.
const PCI_SUBCLASS_USB: u8 = 0x03;
/// PCI programming interface for xHCI.
const PCI_PROGIF_XHCI: u8 = 0x30;

// ---------------------------------------------------------------------------
// xHCI capability register offsets (from BAR0)
// ---------------------------------------------------------------------------

/// CAPLENGTH — length of capability registers (1 byte).
const CAP_CAPLENGTH: usize = 0x00;
/// HCIVERSION — interface version (2 bytes).
const CAP_HCIVERSION: usize = 0x02;
/// HCSPARAMS1 — structural parameters 1.
const CAP_HCSPARAMS1: usize = 0x04;
/// HCSPARAMS2 — structural parameters 2.
const CAP_HCSPARAMS2: usize = 0x08;
/// HCSPARAMS3 — structural parameters 3.
const CAP_HCSPARAMS3: usize = 0x0C;
/// HCCPARAMS1 — capability parameters 1.
const CAP_HCCPARAMS1: usize = 0x10;
/// DBOFF — doorbell array offset.
const CAP_DBOFF: usize = 0x14;
/// RTSOFF — runtime registers offset.
const CAP_RTSOFF: usize = 0x18;

// ---------------------------------------------------------------------------
// xHCI operational register offsets (from BAR0 + CAPLENGTH)
// ---------------------------------------------------------------------------

/// USBCMD — USB command register.
const OP_USBCMD: usize = 0x00;
/// USBSTS — USB status register.
const OP_USBSTS: usize = 0x04;
/// PAGESIZE — page size register.
const OP_PAGESIZE: usize = 0x08;
/// DNCTRL — device notification control.
const OP_DNCTRL: usize = 0x14;
/// CRCR — command ring control register (64-bit).
const OP_CRCR: usize = 0x18;
/// DCBAAP — device context base address array pointer (64-bit).
const OP_DCBAAP: usize = 0x30;
/// CONFIG — configuration register.
const OP_CONFIG: usize = 0x38;
/// Port register set base offset (from operational base).
const OP_PORT_BASE: usize = 0x400;

// USBCMD bits
/// Run/Stop — 1 to run, 0 to stop.
const USBCMD_RUN: u32 = 1 << 0;
/// Host Controller Reset.
const USBCMD_HCRST: u32 = 1 << 1;
/// Interrupter Enable.
const USBCMD_INTE: u32 = 1 << 2;

// USBSTS bits
/// Host Controller Halted.
const USBSTS_HCH: u32 = 1 << 0;
/// Controller Not Ready (set during reset).
const USBSTS_CNR: u32 = 1 << 11;

// ---------------------------------------------------------------------------
// xHCI runtime register offsets (from BAR0 + RTSOFF)
// ---------------------------------------------------------------------------

/// Interrupter register set size (32 bytes per interrupter).
const INTERRUPTER_SIZE: usize = 32;
/// IMAN — Interrupter Management.
const IR_IMAN: usize = 0x00;
/// IMOD — Interrupter Moderation.
const IR_IMOD: usize = 0x04;
/// ERSTSZ — Event Ring Segment Table Size.
const IR_ERSTSZ: usize = 0x08;
/// ERSTBA — Event Ring Segment Table Base Address (64-bit).
const IR_ERSTBA: usize = 0x10;
/// ERDP — Event Ring Dequeue Pointer (64-bit).
const IR_ERDP: usize = 0x18;

// ---------------------------------------------------------------------------
// Transfer Request Block (TRB) types
// ---------------------------------------------------------------------------

/// Normal TRB (data transfer).
const TRB_TYPE_NORMAL: u32 = 1;
/// Setup Stage TRB.
const TRB_TYPE_SETUP: u32 = 2;
/// Data Stage TRB.
const TRB_TYPE_DATA: u32 = 3;
/// Status Stage TRB.
const TRB_TYPE_STATUS: u32 = 4;
/// Link TRB (for ring wraparound).
const TRB_TYPE_LINK: u32 = 6;
/// No-Op Command TRB.
const TRB_TYPE_NOOP_CMD: u32 = 8;
/// Enable Slot Command.
const TRB_TYPE_ENABLE_SLOT: u32 = 9;
/// Disable Slot Command.
const TRB_TYPE_DISABLE_SLOT: u32 = 10;
/// Address Device Command.
const TRB_TYPE_ADDRESS_DEVICE: u32 = 11;
/// Configure Endpoint Command.
const TRB_TYPE_CONFIGURE_EP: u32 = 12;
/// Evaluate Context Command.
const TRB_TYPE_EVALUATE_CTX: u32 = 13;
/// Reset Endpoint Command.
const TRB_TYPE_RESET_EP: u32 = 14;
/// Transfer Event TRB (posted by controller).
const TRB_TYPE_TRANSFER_EVENT: u32 = 32;
/// Command Completion Event TRB.
const TRB_TYPE_CMD_COMPLETION: u32 = 33;
/// Port Status Change Event TRB.
const TRB_TYPE_PORT_STATUS: u32 = 34;

// TRB completion codes
/// Success.
const TRB_CC_SUCCESS: u8 = 1;
/// Short Packet (less data than expected, not always an error).
const TRB_CC_SHORT_PACKET: u8 = 13;

// TRB flags (in control dword)
/// Cycle bit (toggles on ring wrap).
const TRB_CYCLE: u32 = 1 << 0;
/// Toggle Cycle bit (for Link TRBs).
const TRB_TOGGLE_CYCLE: u32 = 1 << 1;
/// Interrupt On Completion.
const TRB_IOC: u32 = 1 << 5;
/// Immediate Data flag.
const TRB_IDT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Port register layout
// ---------------------------------------------------------------------------

/// Port register set size (16 bytes per port).
const PORT_REG_SIZE: usize = 16;
/// PORTSC — Port Status and Control.
const PORT_PORTSC: usize = 0x00;

// PORTSC bits
/// Current Connect Status.
const PORTSC_CCS: u32 = 1 << 0;
/// Port Enabled/Disabled.
const PORTSC_PED: u32 = 1 << 1;
/// Port Reset.
const PORTSC_PR: u32 = 1 << 4;
/// Port Link State (bits 8:5).
const PORTSC_PLS_MASK: u32 = 0xF << 5;
/// Port Power.
const PORTSC_PP: u32 = 1 << 9;
/// Port Speed (bits 13:10).
const PORTSC_SPEED_MASK: u32 = 0xF << 10;
/// Connect Status Change.
const PORTSC_CSC: u32 = 1 << 17;
/// Port Reset Change.
const PORTSC_PRC: u32 = 1 << 21;
/// Bits that are cleared by writing 1 (write-1-to-clear).
const PORTSC_W1C_MASK: u32 = PORTSC_CSC | PORTSC_PRC | (1 << 18) | (1 << 19)
    | (1 << 20) | (1 << 22) | (1 << 23);

// USB speed constants
/// Full Speed (12 Mbps, USB 1.1).
const USB_SPEED_FULL: u8 = 1;
/// Low Speed (1.5 Mbps, USB 1.0).
const USB_SPEED_LOW: u8 = 2;
/// High Speed (480 Mbps, USB 2.0).
const USB_SPEED_HIGH: u8 = 3;
/// Super Speed (5 Gbps, USB 3.0).
const USB_SPEED_SUPER: u8 = 4;

// ---------------------------------------------------------------------------
// USB descriptor types
// ---------------------------------------------------------------------------

/// Device Descriptor type.
const USB_DESC_DEVICE: u8 = 1;
/// Configuration Descriptor type.
const USB_DESC_CONFIGURATION: u8 = 2;
/// Interface Descriptor type.
const USB_DESC_INTERFACE: u8 = 4;
/// Endpoint Descriptor type.
const USB_DESC_ENDPOINT: u8 = 5;

// USB request types
/// Standard Device Request (GET).
const USB_REQ_GET_DESCRIPTOR: u8 = 6;
/// Standard Device Request (SET).
const USB_REQ_SET_ADDRESS: u8 = 5;
/// Set Configuration.
const USB_REQ_SET_CONFIGURATION: u8 = 9;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A Transfer Request Block — the fundamental unit of xHCI communication.
///
/// 16 bytes: parameter (8), status (4), control (4).
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct Trb {
    /// Parameter field (varies by TRB type — pointer, immediate data, etc.).
    pub parameter: u64,
    /// Status field (transfer length, completion code, etc.).
    pub status: u32,
    /// Control field (TRB type, cycle bit, flags).
    pub control: u32,
}

impl Trb {
    /// Create a zeroed TRB.
    pub const fn zeroed() -> Self {
        Self { parameter: 0, status: 0, control: 0 }
    }

    /// Extract the TRB type from the control field (bits 15:10).
    #[allow(clippy::arithmetic_side_effects)]
    pub fn trb_type(&self) -> u32 {
        (self.control >> 10) & 0x3F
    }

    /// Extract the completion code from the status field (bits 31:24).
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    /// Extract the slot ID from the control field (bits 31:24).
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }
}

/// Event Ring Segment Table Entry (16 bytes).
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
struct ErstEntry {
    /// Physical address of the ring segment (64-byte aligned).
    ring_segment_base: u64,
    /// Number of TRBs in this segment.
    ring_segment_size: u16,
    /// Reserved.
    _reserved: u16,
    /// Reserved.
    _reserved2: u32,
}

/// USB Device Descriptor (18 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbDeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub i_manufacturer: u8,
    pub i_product: u8,
    pub i_serial_number: u8,
    pub b_num_configurations: u8,
}

/// Information about a detected USB port.
#[derive(Debug, Clone)]
pub struct UsbPort {
    /// 1-based port number.
    pub number: u8,
    /// Whether a device is connected.
    pub connected: bool,
    /// Whether the port is enabled.
    pub enabled: bool,
    /// USB speed (1=Full, 2=Low, 3=High, 4=Super).
    pub speed: u8,
}

/// Information about an enumerated USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// xHCI slot ID (1-based).
    pub slot_id: u8,
    /// Port number the device is on.
    pub port: u8,
    /// USB speed.
    pub speed: u8,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Device class.
    pub device_class: u8,
    /// Device subclass.
    pub device_subclass: u8,
    /// Max packet size for endpoint 0.
    pub max_packet_size0: u8,
}

// ---------------------------------------------------------------------------
// Ring buffer management
// ---------------------------------------------------------------------------

/// Number of TRBs in the command ring (must be power of 2 for alignment).
const CMD_RING_SIZE: usize = 64;
/// Number of TRBs in the event ring segment.
const EVENT_RING_SIZE: usize = 64;
/// Number of TRBs in a transfer ring.
const TRANSFER_RING_SIZE: usize = 64;
/// Maximum device slots we support.
const MAX_SLOTS: usize = 32;

/// A ring buffer of TRBs (used for command and transfer rings).
struct TrbRing {
    /// Physical frame backing the ring memory.
    frame: PhysFrame,
    /// Virtual base address of the TRB array.
    trbs: *mut Trb,
    /// Number of usable TRBs (last one is a Link TRB).
    capacity: usize,
    /// Current enqueue index.
    enqueue_idx: usize,
    /// Producer Cycle State (toggled on wrap).
    cycle: bool,
}

impl TrbRing {
    /// Allocate a new TRB ring with the given capacity.
    ///
    /// The ring is backed by a physically contiguous frame.  The last
    /// entry is reserved for a Link TRB that wraps back to the start.
    #[allow(clippy::arithmetic_side_effects)]
    fn new(capacity: usize, hhdm_offset: u64) -> KernelResult<Self> {
        let total_bytes = capacity * 16; // 16 bytes per TRB
        if total_bytes > frame::FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let phys = frame::alloc_frame()?;
        let phys_addr = phys.addr();
        let virt = phys_addr.wrapping_add(hhdm_offset);
        let trbs = virt as *mut Trb;

        // Zero the entire ring.
        // SAFETY: We just allocated this frame and HHDM maps it.
        unsafe {
            core::ptr::write_bytes(trbs as *mut u8, 0, frame::FRAME_SIZE);
        }

        // Set up the Link TRB at the last position to wrap around.
        // SAFETY: index is within the allocated frame.
        unsafe {
            let link = &mut *trbs.add(capacity.wrapping_sub(1));
            link.parameter = phys_addr; // Points back to start.
            // Link TRB type = 6, with Toggle Cycle bit.
            link.control = (TRB_TYPE_LINK << 10) | TRB_TOGGLE_CYCLE;
            // Note: cycle bit NOT set initially — the Link TRB will be
            // activated when the producer reaches it.
        }

        Ok(Self {
            frame: phys,
            trbs,
            capacity: capacity.wrapping_sub(1), // Usable slots (excluding Link).
            enqueue_idx: 0,
            cycle: true, // Start with cycle bit = 1.
        })
    }

    /// Return the physical base address of the ring.
    fn phys_addr(&self) -> u64 {
        self.frame.addr()
    }

    /// Enqueue a TRB to the ring, returning its physical address.
    ///
    /// Sets the cycle bit appropriately and advances the enqueue pointer.
    /// When wrapping, the Link TRB's cycle bit is updated and PCS toggles.
    #[allow(clippy::arithmetic_side_effects)]
    fn enqueue(&mut self, mut trb: Trb) -> u64 {
        // Set or clear the cycle bit.
        if self.cycle {
            trb.control |= TRB_CYCLE;
        } else {
            trb.control &= !TRB_CYCLE;
        }

        let phys = self.frame.addr().wrapping_add((self.enqueue_idx * 16) as u64);

        // Write the TRB.
        // SAFETY: enqueue_idx is within [0, capacity).
        unsafe {
            core::ptr::write_volatile(self.trbs.add(self.enqueue_idx), trb);
        }

        // Advance.
        self.enqueue_idx = self.enqueue_idx.wrapping_add(1);

        // Check if we need to wrap (activate the Link TRB).
        if self.enqueue_idx >= self.capacity {
            // Update the Link TRB's cycle bit.
            // SAFETY: The Link TRB is at index `capacity` (the original
            // allocation was capacity+1 entries).
            unsafe {
                let link = &mut *self.trbs.add(self.capacity);
                if self.cycle {
                    link.control |= TRB_CYCLE;
                } else {
                    link.control &= !TRB_CYCLE;
                }
            }
            // Toggle PCS and reset to start.
            self.cycle = !self.cycle;
            self.enqueue_idx = 0;
        }

        phys
    }
}

impl Drop for TrbRing {
    fn drop(&mut self) {
        // SAFETY: We own this frame.
        if let Err(e) = unsafe { frame::free_frame(self.frame) } {
            crate::serial_println!("[xhci] WARNING: failed to free ring frame: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// xHCI Controller state
// ---------------------------------------------------------------------------

/// The xHCI host controller driver state.
struct XhciController {
    /// MMIO base virtual address.
    mmio_base: *mut u8,
    /// Operational registers virtual base (mmio_base + cap_length).
    op_base: *mut u8,
    /// Runtime registers virtual base (mmio_base + rts_off).
    rt_base: *mut u8,
    /// Doorbell array virtual base (mmio_base + db_off).
    db_base: *mut u8,
    /// Maximum number of device slots supported.
    max_slots: u8,
    /// Maximum number of ports.
    max_ports: u8,
    /// Whether 64-byte device contexts are used (vs 32-byte).
    context_size_64: bool,
    /// HHDM offset for physical-to-virtual translation.
    hhdm_offset: u64,
    /// Command Ring.
    cmd_ring: TrbRing,
    /// Event Ring segment (physical frame).
    event_ring_frame: PhysFrame,
    /// Event Ring TRBs virtual address.
    event_ring_trbs: *mut Trb,
    /// Event Ring Segment Table frame.
    erst_frame: PhysFrame,
    /// Current event ring dequeue index.
    event_dequeue_idx: usize,
    /// Event ring Consumer Cycle State.
    event_ccs: bool,
    /// Device Context Base Address Array frame.
    dcbaa_frame: PhysFrame,
    /// DCBAA virtual pointer.
    dcbaa: *mut u64,
    /// Frames allocated for device contexts (one per slot).
    slot_frames: [Option<PhysFrame>; MAX_SLOTS],
    /// Transfer rings for slot endpoint 0 (one per slot).
    slot_ep0_rings: [Option<TrbRing>; MAX_SLOTS],
    /// Interrupt IN transfer rings (one per slot, for HID devices).
    slot_int_rings: [Option<TrbRing>; MAX_SLOTS],
    /// Interrupt receive buffer frames (one per slot).
    slot_int_bufs: [Option<PhysFrame>; MAX_SLOTS],
    /// Enumerated devices.
    devices: Vec<UsbDevice>,
    /// Port status cache.
    ports: Vec<UsbPort>,
    /// Configured HID interfaces.
    hid_interfaces: Vec<UsbHidInterface>,
}

// SAFETY: The controller is only accessed from the BSP during init.
// No concurrent access occurs.
unsafe impl Send for XhciController {}
unsafe impl Sync for XhciController {}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

use spin::Mutex;

/// Global xHCI controller instance.
static XHCI: Mutex<Option<XhciController>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// MMIO register access helpers
// ---------------------------------------------------------------------------

/// Read a 32-bit MMIO register.
///
/// # Safety
/// `base` must point to a valid MMIO-mapped region.
#[inline]
unsafe fn mmio_read32(base: *const u8, offset: usize) -> u32 {
    // SAFETY: Caller guarantees base is valid MMIO.
    unsafe {
        let ptr = base.add(offset) as *const u32;
        core::ptr::read_volatile(ptr)
    }
}

/// Write a 32-bit MMIO register.
///
/// # Safety
/// `base` must point to a valid MMIO-mapped region.
#[inline]
unsafe fn mmio_write32(base: *mut u8, offset: usize, value: u32) {
    // SAFETY: Caller guarantees base is valid MMIO.
    unsafe {
        let ptr = base.add(offset) as *mut u32;
        core::ptr::write_volatile(ptr, value);
    }
}

/// Read a 64-bit MMIO register.
///
/// # Safety
/// `base` must point to a valid MMIO-mapped region.
#[inline]
unsafe fn mmio_read64(base: *const u8, offset: usize) -> u64 {
    // SAFETY: Caller guarantees base is valid MMIO.
    unsafe {
        let ptr = base.add(offset) as *const u64;
        core::ptr::read_volatile(ptr)
    }
}

/// Write a 64-bit MMIO register.
///
/// # Safety
/// `base` must point to a valid MMIO-mapped region.
#[inline]
unsafe fn mmio_write64(base: *mut u8, offset: usize, value: u64) {
    // SAFETY: Caller guarantees base is valid MMIO.
    unsafe {
        let ptr = base.add(offset) as *mut u64;
        core::ptr::write_volatile(ptr, value);
    }
}

// ---------------------------------------------------------------------------
// Controller initialization
// ---------------------------------------------------------------------------

impl XhciController {
    /// Detect and initialize the xHCI controller.
    ///
    /// Returns None if no xHCI controller is found on the PCI bus.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn init(hhdm_offset: u64) -> KernelResult<Self> {
        // Find xHCI controllers via PCI class/subclass.
        let controllers = pci::find_devices_by_class(PCI_CLASS_SERIAL_BUS, PCI_SUBCLASS_USB);
        let pci_dev = controllers.iter()
            .find(|d| {
                // Check prog-if = 0x30 (xHCI).
                let prog_if = pci::config_read8(
                    d.address.bus, d.address.device, d.address.function, 0x09
                );
                prog_if == PCI_PROGIF_XHCI
            })
            .ok_or(KernelError::NotFound)?;

        crate::serial_println!(
            "[xhci] Found xHCI controller: {:04X}:{:04X} at {:?}",
            pci_dev.vendor_id, pci_dev.device_id, pci_dev.address
        );

        // Get BAR0 as 64-bit MMIO address.
        let mmio_phys = pci::bar_mmio_addr64(pci_dev, 0)
            .ok_or(KernelError::InvalidArgument)?;

        if mmio_phys == 0 {
            return Err(KernelError::InvalidArgument);
        }

        crate::serial_println!("[xhci] BAR0 MMIO at physical {:#X}", mmio_phys);

        // Map MMIO region into kernel virtual address space.
        // xHCI register space is typically 64 KiB (4 × 16 KiB frames).
        let mmio_virt = mmio_phys.wrapping_add(hhdm_offset);
        let mmio_base = mmio_virt as *mut u8;

        let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
        let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

        // Map 4 frames (64 KiB) covering the xHCI register space.
        for i in 0..4u64 {
            let frame_phys = mmio_phys.wrapping_add(i.wrapping_mul(16384));
            if let Some(frame) = PhysFrame::from_addr(frame_phys) {
                let virt = VirtAddr::new(mmio_virt.wrapping_add(i.wrapping_mul(16384)));
                // SAFETY: frame_phys is PCI BAR MMIO. Mapping device
                // registers into kernel VA space with NO_CACHE.
                if let Err(_e) = unsafe {
                    page_table::map_frame(pml4_phys, virt, frame, mmio_flags)
                } {
                    // May already be mapped if within HHDM on large-RAM systems.
                }
            }
        }
        // Flush TLB for mapped region.
        for i in 0..4u64 {
            let addr = mmio_virt.wrapping_add(i.wrapping_mul(16384));
            // SAFETY: Standard TLB invalidation.
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
            }
        }

        // Enable bus mastering and memory space.
        pci::enable_bus_master(pci_dev.address);

        // Read capability registers.
        // SAFETY: mmio_base is now mapped and valid for the entire xHCI register
        // space.  All reads below target offsets within the capability register
        // block whose layout is defined by the xHCI specification.
        let cap_length = unsafe { mmio_read32(mmio_base, CAP_CAPLENGTH) } as u8;
        let hci_version = unsafe { mmio_read32(mmio_base, CAP_CAPLENGTH) >> 16 } as u16;
        let hcsparams1 = unsafe { mmio_read32(mmio_base, CAP_HCSPARAMS1) };
        let _hcsparams2 = unsafe { mmio_read32(mmio_base, CAP_HCSPARAMS2) };
        let hccparams1 = unsafe { mmio_read32(mmio_base, CAP_HCCPARAMS1) };
        let db_off = unsafe { mmio_read32(mmio_base, CAP_DBOFF) } & !0x3;
        let rts_off = unsafe { mmio_read32(mmio_base, CAP_RTSOFF) } & !0x1F;

        // Parse HCSPARAMS1.
        let max_slots = (hcsparams1 & 0xFF) as u8;
        let max_intrs = ((hcsparams1 >> 8) & 0x7FF) as u16;
        let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

        // Parse HCCPARAMS1.
        let ac64 = (hccparams1 & 1) != 0; // 64-bit addressing capable
        let csz = (hccparams1 & (1 << 2)) != 0; // Context Size (1 = 64-byte)

        crate::serial_println!(
            "[xhci] Version {}.{:02X}, slots={}, ports={}, intrs={}, 64-bit={}, ctx64={}",
            hci_version >> 8, hci_version & 0xFF,
            max_slots, max_ports, max_intrs, ac64, csz
        );

        if !ac64 {
            crate::serial_println!("[xhci] WARNING: Controller does not support 64-bit addressing");
        }

        // Calculate register base addresses.
        // SAFETY: cap_length, rts_off, db_off come from the controller's own
        // capability registers.  The resulting pointers stay within the MMIO
        // region we mapped above (BAR0 always covers the full register space).
        let op_base = unsafe { mmio_base.add(cap_length as usize) };
        let rt_base = unsafe { mmio_base.add(rts_off as usize) };
        let db_base = unsafe { mmio_base.add(db_off as usize) };

        // ---- Controller Reset ----

        // Stop the controller first.
        // SAFETY: op_base is valid MMIO.
        unsafe {
            let cmd = mmio_read32(op_base, OP_USBCMD);
            mmio_write32(op_base, OP_USBCMD, cmd & !USBCMD_RUN);
        }

        // Wait for halt (HCH = 1).
        let mut timeout = 100_000u32;
        loop {
            // SAFETY: op_base is valid MMIO (verified above).
            let sts = unsafe { mmio_read32(op_base, OP_USBSTS) };
            if sts & USBSTS_HCH != 0 {
                break;
            }
            timeout = timeout.wrapping_sub(1);
            if timeout == 0 {
                crate::serial_println!("[xhci] ERROR: Controller failed to halt");
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }

        // Issue reset (set HCRST).
        // SAFETY: op_base is valid MMIO.
        unsafe {
            mmio_write32(op_base, OP_USBCMD, USBCMD_HCRST);
        }

        // Wait for reset complete (HCRST clears and CNR clears).
        timeout = 1_000_000;
        loop {
            // SAFETY: op_base is valid MMIO.
            let cmd = unsafe { mmio_read32(op_base, OP_USBCMD) };
            let sts = unsafe { mmio_read32(op_base, OP_USBSTS) };
            if cmd & USBCMD_HCRST == 0 && sts & USBSTS_CNR == 0 {
                break;
            }
            timeout = timeout.wrapping_sub(1);
            if timeout == 0 {
                crate::serial_println!("[xhci] ERROR: Controller failed to reset");
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }

        crate::serial_println!("[xhci] Controller reset complete");

        // ---- Configure Max Device Slots ----

        let slots_en = max_slots.min(MAX_SLOTS as u8);
        // SAFETY: op_base is valid MMIO.
        unsafe {
            mmio_write32(op_base, OP_CONFIG, u32::from(slots_en));
        }

        // ---- Allocate Device Context Base Address Array (DCBAA) ----

        let dcbaa_phys_frame = frame::alloc_frame()?;
        let dcbaa_phys = dcbaa_phys_frame.addr();
        let dcbaa_virt = dcbaa_phys.wrapping_add(hhdm_offset) as *mut u64;

        // Zero the DCBAA (MAX_SLOTS+1 entries of 8 bytes each).
        // SAFETY: Just allocated, HHDM maps it.
        unsafe {
            core::ptr::write_bytes(dcbaa_virt as *mut u8, 0, frame::FRAME_SIZE);
        }

        // Write DCBAAP.
        // SAFETY: op_base is valid MMIO; dcbaa_phys is the physical address
        // of a freshly allocated and zeroed frame.
        unsafe {
            mmio_write64(op_base, OP_DCBAAP, dcbaa_phys);
        }

        // ---- Allocate Command Ring ----

        let cmd_ring = TrbRing::new(CMD_RING_SIZE, hhdm_offset)?;
        let cmd_ring_phys = cmd_ring.phys_addr();

        // Write CRCR (command ring pointer with cycle bit = 1).
        // SAFETY: op_base is valid MMIO; cmd_ring_phys is the physical base
        // of a properly initialised TRB ring.
        unsafe {
            mmio_write64(op_base, OP_CRCR, cmd_ring_phys | 1); // RCS = 1
        }

        // ---- Allocate Event Ring ----

        // Event Ring Segment.
        let event_ring_frame = frame::alloc_frame()?;
        let event_ring_phys = event_ring_frame.addr();
        let event_ring_virt = event_ring_phys.wrapping_add(hhdm_offset) as *mut Trb;

        // Zero the event ring.
        // SAFETY: event_ring_frame was just allocated; HHDM maps it at
        // event_ring_virt for the full FRAME_SIZE.
        unsafe {
            core::ptr::write_bytes(event_ring_virt as *mut u8, 0, frame::FRAME_SIZE);
        }

        // Event Ring Segment Table (ERST) — one entry pointing to our segment.
        let erst_frame = frame::alloc_frame()?;
        let erst_phys = erst_frame.addr();
        let erst_virt = erst_phys.wrapping_add(hhdm_offset) as *mut ErstEntry;

        // Zero and fill the ERST.
        // SAFETY: erst_frame was just allocated and HHDM-mapped.  erst_virt
        // points to the start, and we write only the first ErstEntry.
        unsafe {
            core::ptr::write_bytes(erst_virt as *mut u8, 0, frame::FRAME_SIZE);
            let entry = &mut *erst_virt;
            entry.ring_segment_base = event_ring_phys;
            entry.ring_segment_size = EVENT_RING_SIZE as u16;
        }

        // Configure Interrupter 0.
        // Interrupter registers are at runtime_base + 0x20 + (interrupter * 32).
        // SAFETY: rt_base is valid MMIO; 0x20 is the start of the interrupter
        // register set per the xHCI spec.
        let ir0_base = unsafe { rt_base.add(0x20) };

        // SAFETY: ir0_base points to Interrupter 0's register set in valid
        // MMIO space.  The following writes configure the event ring segment
        // table size, dequeue pointer, base address, and interrupt enable.
        // ERSTSZ = 1 (one segment).
        unsafe {
            mmio_write32(ir0_base, IR_ERSTSZ, 1);
        }

        // ERDP = event ring physical base.
        unsafe {
            mmio_write64(ir0_base, IR_ERDP, event_ring_phys);
        }

        // ERSTBA = ERST physical base (writing this enables the event ring).
        unsafe {
            mmio_write64(ir0_base, IR_ERSTBA, erst_phys);
        }

        // Enable Interrupter 0 (set IE bit in IMAN).
        unsafe {
            let iman = mmio_read32(ir0_base, IR_IMAN);
            mmio_write32(ir0_base, IR_IMAN, iman | 0x2); // IE = bit 1
        }

        // ---- Start the Controller ----

        // Enable interrupts and run.
        // SAFETY: op_base is valid MMIO.
        unsafe {
            let cmd = mmio_read32(op_base, OP_USBCMD);
            mmio_write32(op_base, OP_USBCMD, cmd | USBCMD_RUN | USBCMD_INTE);
        }

        // Wait for controller to start (HCH clears).
        timeout = 100_000;
        loop {
            // SAFETY: op_base is valid MMIO.
            let sts = unsafe { mmio_read32(op_base, OP_USBSTS) };
            if sts & USBSTS_HCH == 0 {
                break;
            }
            timeout = timeout.wrapping_sub(1);
            if timeout == 0 {
                crate::serial_println!("[xhci] ERROR: Controller failed to start");
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }

        crate::serial_println!("[xhci] Controller running");

        // Build the controller struct.
        // Initialize slot arrays (all None).
        const NONE_FRAME: Option<PhysFrame> = None;
        const NONE_RING: Option<TrbRing> = None;

        let mut ctrl = Self {
            mmio_base,
            op_base,
            rt_base,
            db_base,
            max_slots: slots_en,
            max_ports,
            context_size_64: csz,
            hhdm_offset,
            cmd_ring,
            event_ring_frame,
            event_ring_trbs: event_ring_virt,
            erst_frame,
            event_dequeue_idx: 0,
            event_ccs: true,
            dcbaa_frame: dcbaa_phys_frame,
            dcbaa: dcbaa_virt,
            slot_frames: [NONE_FRAME; MAX_SLOTS],
            slot_ep0_rings: [NONE_RING; MAX_SLOTS],
            slot_int_rings: [NONE_RING; MAX_SLOTS],
            slot_int_bufs: [NONE_FRAME; MAX_SLOTS],
            devices: Vec::new(),
            ports: Vec::new(),
            hid_interfaces: Vec::new(),
        };

        // Scan ports for connected devices.
        ctrl.scan_ports();

        Ok(ctrl)
    }

    // -----------------------------------------------------------------------
    // Port management
    // -----------------------------------------------------------------------

    /// Scan all ports and detect connected devices.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn scan_ports(&mut self) {
        self.ports.clear();

        for port_num in 1..=self.max_ports {
            let offset = OP_PORT_BASE + (port_num as usize - 1) * PORT_REG_SIZE;
            // SAFETY: op_base is valid, offset within mapped region.
            let portsc = unsafe { mmio_read32(self.op_base, offset + PORT_PORTSC) };

            let connected = portsc & PORTSC_CCS != 0;
            let enabled = portsc & PORTSC_PED != 0;
            let speed = ((portsc & PORTSC_SPEED_MASK) >> 10) as u8;

            self.ports.push(UsbPort {
                number: port_num,
                connected,
                enabled,
                speed,
            });

            if connected {
                crate::serial_println!(
                    "[xhci] Port {}: connected, speed={}, enabled={}",
                    port_num, speed_name(speed), enabled
                );
            }
        }
    }

    /// Reset a port and wait for it to become enabled.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn reset_port(&self, port_num: u8) -> KernelResult<u8> {
        let offset = OP_PORT_BASE + (port_num as usize - 1) * PORT_REG_SIZE;

        // Read current PORTSC — preserve PP, clear W1C bits, set PR.
        // SAFETY: op_base is valid MMIO.
        let portsc = unsafe { mmio_read32(self.op_base, offset + PORT_PORTSC) };
        let write_val = (portsc & !PORTSC_W1C_MASK) | PORTSC_PR;
        // SAFETY: op_base is valid MMIO (from init); offset targets this port's register set.
        unsafe {
            mmio_write32(self.op_base, offset + PORT_PORTSC, write_val);
        }

        // Wait for reset to complete (PRC = 1 means reset complete).
        let mut timeout = 500_000u32;
        loop {
            // SAFETY: op_base is valid MMIO; offset targets this port's PORTSC.
            let portsc = unsafe { mmio_read32(self.op_base, offset + PORT_PORTSC) };
            if portsc & PORTSC_PRC != 0 {
                // Clear PRC by writing 1 to it.
                let clear = (portsc & !PORTSC_W1C_MASK) | PORTSC_PRC;
                unsafe {
                    mmio_write32(self.op_base, offset + PORT_PORTSC, clear);
                }
                break;
            }
            timeout = timeout.wrapping_sub(1);
            if timeout == 0 {
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }

        // Read the port speed after reset.
        // SAFETY: op_base is valid MMIO; offset targets this port's PORTSC.
        let portsc = unsafe { mmio_read32(self.op_base, offset + PORT_PORTSC) };
        let speed = ((portsc & PORTSC_SPEED_MASK) >> 10) as u8;

        if portsc & PORTSC_PED == 0 {
            return Err(KernelError::IoError);
        }

        Ok(speed)
    }

    // -----------------------------------------------------------------------
    // Command ring operations
    // -----------------------------------------------------------------------

    /// Ring the host controller doorbell (doorbell 0 = command ring).
    fn ring_doorbell(&self, slot: u8, target: u32) {
        let offset = (slot as usize) * 4;
        // SAFETY: db_base is valid MMIO.
        unsafe {
            mmio_write32(self.db_base, offset, target);
        }
    }

    /// Ring doorbell 0 to notify the controller of new command ring entries.
    fn ring_cmd_doorbell(&self) {
        self.ring_doorbell(0, 0);
    }

    /// Submit a command TRB and wait for the completion event.
    ///
    /// Returns the completion TRB.
    fn submit_command(&mut self, trb: Trb) -> KernelResult<Trb> {
        // Enqueue and ring doorbell.
        fence(Ordering::SeqCst);
        let _phys = self.cmd_ring.enqueue(trb);
        fence(Ordering::SeqCst);
        self.ring_cmd_doorbell();

        // Poll event ring for completion.
        self.wait_for_event(TRB_TYPE_CMD_COMPLETION, 1_000_000)
    }

    /// Poll the event ring for a specific event type.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn wait_for_event(&mut self, expected_type: u32, max_polls: u32) -> KernelResult<Trb> {
        let mut polls = 0u32;
        loop {
            if let Some(trb) = self.poll_event() {
                if trb.trb_type() == expected_type {
                    return Ok(trb);
                }
                // Handle other event types (port status changes, etc.)
                self.handle_event(&trb);
            }

            polls = polls.wrapping_add(1);
            if polls >= max_polls {
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }
    }

    /// Poll the event ring for one event.
    ///
    /// Returns the event TRB if one is ready, None otherwise.
    #[allow(clippy::arithmetic_side_effects)]
    fn poll_event(&mut self) -> Option<Trb> {
        // SAFETY: event_ring_trbs is valid memory.
        let trb = unsafe {
            core::ptr::read_volatile(self.event_ring_trbs.add(self.event_dequeue_idx))
        };

        // Check cycle bit — matches our Consumer Cycle State?
        let trb_cycle = (trb.control & TRB_CYCLE) != 0;
        if trb_cycle != self.event_ccs {
            return None; // No new event.
        }

        // Advance dequeue pointer.
        self.event_dequeue_idx = self.event_dequeue_idx.wrapping_add(1);
        if self.event_dequeue_idx >= EVENT_RING_SIZE {
            self.event_dequeue_idx = 0;
            self.event_ccs = !self.event_ccs;
        }

        // Update ERDP to tell the controller we've consumed this event.
        let erdp_phys = self.event_ring_frame.addr()
            .wrapping_add((self.event_dequeue_idx * 16) as u64);
        // SAFETY: rt_base is valid MMIO; 0x20 is the Interrupter 0 register offset.
        let ir0_base = unsafe { self.rt_base.add(0x20) };
        // Set EHB (Event Handler Busy) clear bit (bit 3).
        // SAFETY: ir0_base points to Interrupter 0's registers.
        unsafe {
            mmio_write64(ir0_base, IR_ERDP, erdp_phys | (1 << 3));
        }

        Some(trb)
    }

    /// Handle a non-command event (port status change, transfer event, etc.).
    fn handle_event(&mut self, trb: &Trb) {
        match trb.trb_type() {
            TRB_TYPE_PORT_STATUS => {
                // Port status change — re-scan ports later.
                let _port_id = (trb.parameter >> 24) as u8;
            }
            TRB_TYPE_TRANSFER_EVENT => {
                // Transfer completion — handled by caller.
            }
            _ => {
                // Unknown event type — log it.
                crate::serial_println!(
                    "[xhci] Unhandled event: type={}, cc={}",
                    trb.trb_type(), trb.completion_code()
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Device enumeration
    // -----------------------------------------------------------------------

    /// Enable a device slot via the Enable Slot command.
    fn enable_slot(&mut self) -> KernelResult<u8> {
        let trb = Trb {
            parameter: 0,
            status: 0,
            control: TRB_TYPE_ENABLE_SLOT << 10,
        };

        let completion = self.submit_command(trb)?;
        let cc = completion.completion_code();
        if cc != TRB_CC_SUCCESS {
            crate::serial_println!("[xhci] Enable Slot failed: cc={}", cc);
            return Err(KernelError::IoError);
        }

        let slot_id = completion.slot_id();
        crate::serial_println!("[xhci] Enabled slot {}", slot_id);
        Ok(slot_id)
    }

    /// Context size in bytes (32 or 64 depending on controller capability).
    fn context_entry_size(&self) -> usize {
        if self.context_size_64 { 64 } else { 32 }
    }

    /// Allocate and set up a device context for a slot.
    ///
    /// Creates the Input Context for Address Device command.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn setup_device_context(
        &mut self,
        slot_id: u8,
        port_num: u8,
        speed: u8,
    ) -> KernelResult<()> {
        let ctx_size = self.context_entry_size();
        let slot_idx = (slot_id as usize).wrapping_sub(1);

        if slot_idx >= MAX_SLOTS {
            return Err(KernelError::InvalidArgument);
        }

        // Allocate an Output Device Context (the controller writes here).
        let out_ctx_frame = frame::alloc_frame()?;
        let out_ctx_phys = out_ctx_frame.addr();
        let out_ctx_virt = out_ctx_phys.wrapping_add(self.hhdm_offset) as *mut u8;
        // SAFETY: Just allocated.
        unsafe { core::ptr::write_bytes(out_ctx_virt, 0, frame::FRAME_SIZE); }

        // Store in DCBAA.
        // SAFETY: dcbaa is valid, slot_id is within bounds.
        unsafe {
            *self.dcbaa.add(slot_id as usize) = out_ctx_phys;
        }
        self.slot_frames[slot_idx] = Some(out_ctx_frame);

        // Allocate Transfer Ring for endpoint 0.
        let ep0_ring = TrbRing::new(TRANSFER_RING_SIZE, self.hhdm_offset)?;
        let ep0_ring_phys = ep0_ring.phys_addr();
        self.slot_ep0_rings[slot_idx] = Some(ep0_ring);

        // Allocate an Input Context (for the Address Device command).
        let in_ctx_frame = frame::alloc_frame()?;
        let in_ctx_phys = in_ctx_frame.addr();
        let in_ctx_virt = in_ctx_phys.wrapping_add(self.hhdm_offset) as *mut u8;
        // SAFETY: Just allocated.
        unsafe { core::ptr::write_bytes(in_ctx_virt, 0, frame::FRAME_SIZE); }

        // Fill the Input Control Context (first context entry).
        // Add flags: bits 0 (Slot) and 1 (EP0) set.
        // SAFETY: in_ctx_virt is valid, within allocated frame.
        unsafe {
            // Input Control Context is at offset 0 (size ctx_size).
            // Add Context Flags at offset 0x04 of the ICC.
            let icc_add_flags = in_ctx_virt.add(0x04) as *mut u32;
            core::ptr::write_volatile(icc_add_flags, 0x3); // Slot + EP0
        }

        // Fill Slot Context (second context entry, at offset ctx_size).
        // SAFETY: in_ctx_virt points to a freshly allocated 16 KiB frame;
        // ctx_size (32 or 64 bytes) puts slot_ctx well within bounds.
        let slot_ctx = unsafe { in_ctx_virt.add(ctx_size) };
        // SAFETY: slot_ctx within allocated frame.
        unsafe {
            // Dword 0: Route String (0 for root hub ports), Speed, Context Entries (1 = EP0 only)
            let dword0 = (1u32 << 27) // Context Entries = 1 (only EP0 configured)
                | (u32::from(speed) << 20); // Speed
            core::ptr::write_volatile(slot_ctx as *mut u32, dword0);

            // Dword 1: Root Hub Port Number (bits 23:16)
            let dword1 = u32::from(port_num) << 16;
            core::ptr::write_volatile(slot_ctx.add(4) as *mut u32, dword1);
        }

        // Fill Endpoint 0 Context (third entry, at offset 2 * ctx_size).
        // SAFETY: 2 * ctx_size is at most 128 bytes, well within the 16 KiB frame.
        let ep0_ctx = unsafe { in_ctx_virt.add(2 * ctx_size) };
        // Max packet size based on speed.
        let max_packet = match speed {
            USB_SPEED_LOW => 8u16,
            USB_SPEED_FULL => 64,
            USB_SPEED_HIGH => 64,
            USB_SPEED_SUPER => 512,
            _ => 8,
        };
        // SAFETY: ep0_ctx within allocated frame.
        unsafe {
            // Dword 1: EP Type (4 = Control Bidirectional), CErr = 3, Max Burst = 0
            let dword1 = (3u32 << 1) // CErr = 3
                | (4u32 << 3) // EP Type = Control Bidirectional
                | (u32::from(max_packet) << 16); // Max Packet Size
            core::ptr::write_volatile(ep0_ctx.add(4) as *mut u32, dword1);

            // Dword 2-3: TR Dequeue Pointer (64-bit physical address of EP0 ring + DCS=1)
            let tr_dequeue = ep0_ring_phys | 1; // DCS (Dequeue Cycle State) = 1
            core::ptr::write_volatile(ep0_ctx.add(8) as *mut u64, tr_dequeue);

            // Dword 4: Average TRB Length = 8 (for control transfers)
            core::ptr::write_volatile(ep0_ctx.add(16) as *mut u32, 8);
        }

        // Submit Address Device command.
        let addr_trb = Trb {
            parameter: in_ctx_phys,
            status: 0,
            control: (TRB_TYPE_ADDRESS_DEVICE << 10) | (u32::from(slot_id) << 24),
        };

        let completion = self.submit_command(addr_trb)?;
        let cc = completion.completion_code();

        // Free the input context frame (no longer needed after command).
        // SAFETY: We own this frame and have finished using it.
        let _ = unsafe { frame::free_frame(in_ctx_frame) };

        if cc != TRB_CC_SUCCESS {
            crate::serial_println!("[xhci] Address Device failed: cc={}", cc);
            return Err(KernelError::IoError);
        }

        crate::serial_println!("[xhci] Slot {} addressed (port {}, {})",
            slot_id, port_num, speed_name(speed));

        Ok(())
    }

    /// Perform a control transfer on endpoint 0 (GET_DESCRIPTOR etc.).
    ///
    /// Returns the number of bytes actually transferred.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn control_transfer(
        &mut self,
        slot_id: u8,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_buf_phys: u64,
        length: u16,
        direction_in: bool,
    ) -> KernelResult<usize> {
        let slot_idx = (slot_id as usize).wrapping_sub(1);
        if slot_idx >= MAX_SLOTS {
            return Err(KernelError::InvalidArgument);
        }

        let ring = self.slot_ep0_rings[slot_idx].as_mut()
            .ok_or(KernelError::NoSuchDevice)?;

        // Setup Stage TRB.
        // Parameter: bRequestType, bRequest, wValue, wIndex, wLength packed.
        let setup_data: u64 = u64::from(request_type)
            | (u64::from(request) << 8)
            | (u64::from(value) << 16)
            | (u64::from(index) << 32)
            | (u64::from(length) << 48);

        // TRT (Transfer Type): 0=No Data, 2=OUT, 3=IN
        let trt = if length == 0 {
            0u32
        } else if direction_in {
            3u32
        } else {
            2u32
        };

        let setup_trb = Trb {
            parameter: setup_data,
            status: 8, // TRB Transfer Length = 8 (setup packet is 8 bytes)
            control: (TRB_TYPE_SETUP << 10) | TRB_IDT | (trt << 16),
        };
        ring.enqueue(setup_trb);

        // Data Stage TRB (if any data).
        if length > 0 {
            let dir_bit = if direction_in { 1u32 << 16 } else { 0 };
            let data_trb = Trb {
                parameter: data_buf_phys,
                status: u32::from(length),
                control: (TRB_TYPE_DATA << 10) | dir_bit,
            };
            ring.enqueue(data_trb);
        }

        // Status Stage TRB.
        // Direction is opposite of data stage (IN data → OUT status).
        let status_dir = if length == 0 || direction_in { 0u32 } else { 1u32 << 16 };
        let status_trb = Trb {
            parameter: 0,
            status: 0,
            control: (TRB_TYPE_STATUS << 10) | TRB_IOC | status_dir,
        };
        ring.enqueue(status_trb);

        fence(Ordering::SeqCst);

        // Ring the doorbell for this slot, target = 1 (EP0 = DCI 1).
        self.ring_doorbell(slot_id, 1);

        // Wait for transfer event.
        let event = self.wait_for_event(TRB_TYPE_TRANSFER_EVENT, 2_000_000)?;
        let cc = event.completion_code();
        if cc != TRB_CC_SUCCESS && cc != TRB_CC_SHORT_PACKET {
            crate::serial_println!(
                "[xhci] Control transfer failed: cc={}", cc
            );
            return Err(KernelError::IoError);
        }

        // Bytes transferred = length - residual.
        let residual = event.status & 0x00FF_FFFF;
        let transferred = u32::from(length).wrapping_sub(residual) as usize;

        Ok(transferred)
    }

    /// Read the Device Descriptor from a device.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn get_device_descriptor(&mut self, slot_id: u8) -> KernelResult<UsbDeviceDescriptor> {
        // Allocate a buffer for the descriptor (18 bytes, but use a full frame).
        let buf_frame = frame::alloc_frame()?;
        let buf_phys = buf_frame.addr();
        let buf_virt = buf_phys.wrapping_add(self.hhdm_offset) as *mut u8;

        // Zero it.
        // SAFETY: Just allocated.
        unsafe { core::ptr::write_bytes(buf_virt, 0, 64); }

        // GET_DESCRIPTOR request: device descriptor = type 1, index 0.
        let transferred = self.control_transfer(
            slot_id,
            0x80,                    // bmRequestType: Device-to-Host, Standard, Device
            USB_REQ_GET_DESCRIPTOR,  // bRequest
            u16::from(USB_DESC_DEVICE) << 8, // wValue: type << 8 | index
            0,                       // wIndex
            buf_phys,                // data buffer physical address
            18,                      // wLength (device descriptor is 18 bytes)
            true,                    // IN transfer
        )?;

        if transferred < 18 {
            // SAFETY: We own this frame.
            let _ = unsafe { frame::free_frame(buf_frame) };
            return Err(KernelError::IoError);
        }

        // Copy out the descriptor.
        // SAFETY: We verified at least 18 bytes were transferred.
        let desc = unsafe {
            core::ptr::read_unaligned(buf_virt as *const UsbDeviceDescriptor)
        };

        // Free the buffer.
        // SAFETY: We own this frame.
        let _ = unsafe { frame::free_frame(buf_frame) };

        Ok(desc)
    }

    /// Enumerate all connected devices.
    ///
    /// For each connected port: reset, enable slot, address, read descriptor.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn enumerate_devices(&mut self) {
        // Collect connected ports first (avoid borrow issues).
        let connected_ports: Vec<u8> = self.ports.iter()
            .filter(|p| p.connected)
            .map(|p| p.number)
            .collect();

        for port_num in connected_ports {
            crate::serial_println!("[xhci] Enumerating device on port {}...", port_num);

            // Reset the port.
            let speed = match self.reset_port(port_num) {
                Ok(s) => s,
                Err(e) => {
                    crate::serial_println!("[xhci] Port {} reset failed: {:?}", port_num, e);
                    continue;
                }
            };

            // Enable a device slot.
            let slot_id = match self.enable_slot() {
                Ok(id) => id,
                Err(e) => {
                    crate::serial_println!("[xhci] Enable slot failed: {:?}", e);
                    continue;
                }
            };

            // Set up the device context and address the device.
            if let Err(e) = self.setup_device_context(slot_id, port_num, speed) {
                crate::serial_println!("[xhci] Address device failed: {:?}", e);
                continue;
            }

            // Read the device descriptor.
            match self.get_device_descriptor(slot_id) {
                Ok(desc) => {
                    // Copy fields from packed struct to avoid unaligned references.
                    let vid = { desc.id_vendor };
                    let pid = { desc.id_product };
                    let cls = desc.b_device_class;
                    let sub = desc.b_device_sub_class;
                    let mps = desc.b_max_packet_size0;

                    crate::serial_println!(
                        "[xhci] Device on port {}: VID={:04X} PID={:04X} class={:02X}/{:02X} maxpkt={}",
                        port_num, vid, pid, cls, sub, mps
                    );

                    self.devices.push(UsbDevice {
                        slot_id,
                        port: port_num,
                        speed,
                        vendor_id: vid,
                        product_id: pid,
                        device_class: cls,
                        device_subclass: sub,
                        max_packet_size0: mps,
                    });
                }
                Err(e) => {
                    crate::serial_println!(
                        "[xhci] Failed to read device descriptor on port {}: {:?}",
                        port_num, e
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// USB HID class support
// ---------------------------------------------------------------------------

/// USB HID interface class code.
const USB_CLASS_HID: u8 = 0x03;
/// HID subclass: boot interface.
const USB_HID_SUBCLASS_BOOT: u8 = 0x01;
/// HID protocol: keyboard.
const USB_HID_PROTOCOL_KEYBOARD: u8 = 0x01;
/// HID protocol: mouse.
const USB_HID_PROTOCOL_MOUSE: u8 = 0x02;

/// SET_CONFIGURATION request.
const USB_REQ_TYPE_HOST_TO_DEVICE: u8 = 0x00;
/// Class-specific interface request (host to device).
const USB_REQ_TYPE_CLASS_IFACE_OUT: u8 = 0x21;
/// HID SET_IDLE request code.
const USB_HID_SET_IDLE: u8 = 0x0A;
/// HID SET_PROTOCOL request code.
const USB_HID_SET_PROTOCOL: u8 = 0x0B;

/// Describes a HID interface found on a USB device.
#[derive(Debug, Clone)]
pub struct UsbHidInterface {
    /// Slot ID of the device.
    pub slot_id: u8,
    /// Interface number.
    pub interface_num: u8,
    /// HID subclass (0 = none, 1 = boot interface).
    pub subclass: u8,
    /// HID protocol (0 = none, 1 = keyboard, 2 = mouse).
    pub protocol: u8,
    /// Interrupt IN endpoint number (1-based).
    pub interrupt_ep: u8,
    /// Max packet size of the interrupt endpoint.
    pub interrupt_max_packet: u16,
    /// Interrupt endpoint interval (polling rate in frames).
    pub interval: u8,
}

/// USB Configuration Descriptor header (9 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct UsbConfigDescriptor {
    b_length: u8,
    b_descriptor_type: u8,
    w_total_length: u16,
    b_num_interfaces: u8,
    b_configuration_value: u8,
    i_configuration: u8,
    bm_attributes: u8,
    b_max_power: u8,
}

/// USB Interface Descriptor (9 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct UsbInterfaceDescriptor {
    b_length: u8,
    b_descriptor_type: u8,
    b_interface_number: u8,
    b_alternate_setting: u8,
    b_num_endpoints: u8,
    b_interface_class: u8,
    b_interface_sub_class: u8,
    b_interface_protocol: u8,
    i_interface: u8,
}

/// USB Endpoint Descriptor (7 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct UsbEndpointDescriptor {
    b_length: u8,
    b_descriptor_type: u8,
    b_endpoint_address: u8,
    bm_attributes: u8,
    w_max_packet_size: u16,
    b_interval: u8,
}

/// Boot protocol keyboard input report (8 bytes).
#[derive(Debug, Clone, Copy, Default)]
pub struct HidKeyboardReport {
    /// Modifier keys (Ctrl, Shift, Alt, GUI).
    pub modifiers: u8,
    /// Reserved byte.
    pub reserved: u8,
    /// Up to 6 simultaneous key codes.
    pub keycodes: [u8; 6],
}

/// Boot protocol mouse input report (3-4 bytes).
#[derive(Debug, Clone, Copy, Default)]
pub struct HidMouseReport {
    /// Button bits (bit 0 = left, bit 1 = right, bit 2 = middle).
    pub buttons: u8,
    /// X movement (signed 8-bit).
    pub x: i8,
    /// Y movement (signed 8-bit).
    pub y: i8,
    /// Scroll wheel (signed 8-bit, optional).
    pub wheel: i8,
}

impl XhciController {
    /// Read the Configuration Descriptor and find HID interfaces.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn get_config_descriptor(&mut self, slot_id: u8) -> KernelResult<(u8, Vec<UsbHidInterface>)> {
        // First, read just the config descriptor header to get total length.
        let buf_frame = frame::alloc_frame()?;
        let buf_phys = buf_frame.addr();
        let buf_virt = buf_phys.wrapping_add(self.hhdm_offset) as *mut u8;
        // SAFETY: Just allocated.
        unsafe { core::ptr::write_bytes(buf_virt, 0, 256); }

        // GET_DESCRIPTOR for Configuration (type 2, index 0).
        let transferred = self.control_transfer(
            slot_id,
            0x80, // Device-to-Host, Standard, Device
            USB_REQ_GET_DESCRIPTOR,
            u16::from(USB_DESC_CONFIGURATION) << 8,
            0,
            buf_phys,
            255, // Read up to 255 bytes
            true,
        )?;

        if transferred < 9 {
            // SAFETY: We own buf_frame exclusively.
            let _ = unsafe { frame::free_frame(buf_frame) };
            return Err(KernelError::IoError);
        }

        // Parse config descriptor header.
        // SAFETY: buf_virt is valid for at least `transferred` (≥9) bytes; the
        // config descriptor header is 9 bytes and may be unaligned in the buffer.
        let config_desc = unsafe {
            core::ptr::read_unaligned(buf_virt as *const UsbConfigDescriptor)
        };
        let config_value = config_desc.b_configuration_value;
        let total_len = { config_desc.w_total_length } as usize;
        let actual_len = transferred.min(total_len).min(255);

        // Walk the descriptor list looking for HID interfaces.
        let mut hid_interfaces = Vec::new();
        let mut offset = 0usize;
        let mut current_iface: Option<(u8, u8, u8)> = None; // (iface_num, subclass, protocol)

        while offset.wrapping_add(2) <= actual_len {
            // SAFETY: offset+1 < actual_len ≤ 255 ≤ FRAME_SIZE, so both reads
            // are within the buf_frame we allocated and the controller wrote into.
            let desc_len = unsafe { *buf_virt.add(offset) } as usize;
            let desc_type = unsafe { *buf_virt.add(offset.wrapping_add(1)) };

            if desc_len < 2 || offset.wrapping_add(desc_len) > actual_len {
                break;
            }

            match desc_type {
                USB_DESC_INTERFACE => {
                    if desc_len >= 9 {
                        // SAFETY: desc_len ≥ 9 = sizeof(UsbInterfaceDescriptor)
                        // and offset + desc_len ≤ actual_len, all within buf_frame.
                        let iface = unsafe {
                            core::ptr::read_unaligned(buf_virt.add(offset) as *const UsbInterfaceDescriptor)
                        };
                        if iface.b_interface_class == USB_CLASS_HID {
                            current_iface = Some((
                                iface.b_interface_number,
                                iface.b_interface_sub_class,
                                iface.b_interface_protocol,
                            ));
                        } else {
                            current_iface = None;
                        }
                    }
                }
                USB_DESC_ENDPOINT => {
                    if desc_len >= 7 {
                        if let Some((iface_num, subclass, protocol)) = current_iface {
                            // SAFETY: desc_len ≥ 7 = sizeof(UsbEndpointDescriptor)
                            // and offset + desc_len ≤ actual_len, all within buf_frame.
                            let ep = unsafe {
                                core::ptr::read_unaligned(buf_virt.add(offset) as *const UsbEndpointDescriptor)
                            };
                            let ep_addr = ep.b_endpoint_address;
                            let ep_attrs = ep.bm_attributes;
                            let max_pkt = { ep.w_max_packet_size };
                            let interval = ep.b_interval;

                            // Check if this is an Interrupt IN endpoint.
                            let is_in = (ep_addr & 0x80) != 0;
                            let is_interrupt = (ep_attrs & 0x03) == 0x03;

                            if is_in && is_interrupt {
                                hid_interfaces.push(UsbHidInterface {
                                    slot_id,
                                    interface_num: iface_num,
                                    subclass,
                                    protocol,
                                    interrupt_ep: ep_addr & 0x0F,
                                    interrupt_max_packet: max_pkt,
                                    interval,
                                });
                                current_iface = None; // Done with this interface.
                            }
                        }
                    }
                }
                _ => {}
            }

            offset = offset.wrapping_add(desc_len);
        }

        // Free buffer.
        // SAFETY: We own buf_frame exclusively and have finished reading it.
        let _ = unsafe { frame::free_frame(buf_frame) };

        Ok((config_value, hid_interfaces))
    }

    /// Set the device configuration (SET_CONFIGURATION).
    fn set_configuration(&mut self, slot_id: u8, config_value: u8) -> KernelResult<()> {
        self.control_transfer(
            slot_id,
            USB_REQ_TYPE_HOST_TO_DEVICE, // Host-to-Device, Standard, Device
            USB_REQ_SET_CONFIGURATION,
            u16::from(config_value),
            0,
            0,
            0,
            false,
        )?;
        Ok(())
    }

    /// Set HID boot protocol on an interface.
    fn hid_set_boot_protocol(&mut self, slot_id: u8, interface: u8) -> KernelResult<()> {
        // SET_PROTOCOL: wValue = 0 (boot protocol), wIndex = interface
        self.control_transfer(
            slot_id,
            USB_REQ_TYPE_CLASS_IFACE_OUT,
            USB_HID_SET_PROTOCOL,
            0, // 0 = boot protocol
            u16::from(interface),
            0,
            0,
            false,
        )?;
        Ok(())
    }

    /// Set HID idle rate (0 = report only on change).
    fn hid_set_idle(&mut self, slot_id: u8, interface: u8) -> KernelResult<()> {
        // SET_IDLE: wValue = idle_rate << 8 | report_id, wIndex = interface
        self.control_transfer(
            slot_id,
            USB_REQ_TYPE_CLASS_IFACE_OUT,
            USB_HID_SET_IDLE,
            0, // 0 = infinite (only report on change)
            u16::from(interface),
            0,
            0,
            false,
        )?;
        Ok(())
    }

    /// Configure all detected HID devices for boot protocol.
    ///
    /// This sets the configuration, switches to boot protocol, and
    /// configures idle reporting.  After this, devices are ready for
    /// interrupt transfers.
    #[allow(clippy::arithmetic_side_effects)]
    fn configure_hid_devices(&mut self) -> Vec<UsbHidInterface> {
        let mut configured = Vec::new();

        // Collect slot IDs to avoid borrow issues.
        let slot_ids: Vec<u8> = self.devices.iter().map(|d| d.slot_id).collect();

        for slot_id in slot_ids {
            // Read configuration descriptor.
            let (config_value, hid_interfaces) = match self.get_config_descriptor(slot_id) {
                Ok(v) => v,
                Err(e) => {
                    crate::serial_println!(
                        "[xhci] Failed to read config desc for slot {}: {:?}", slot_id, e
                    );
                    continue;
                }
            };

            if hid_interfaces.is_empty() {
                continue;
            }

            // Set configuration.
            if let Err(e) = self.set_configuration(slot_id, config_value) {
                crate::serial_println!(
                    "[xhci] SET_CONFIGURATION failed for slot {}: {:?}", slot_id, e
                );
                continue;
            }

            // Configure each HID interface.
            for hid in &hid_interfaces {
                let protocol_name = match hid.protocol {
                    USB_HID_PROTOCOL_KEYBOARD => "keyboard",
                    USB_HID_PROTOCOL_MOUSE => "mouse",
                    _ => "unknown HID",
                };

                // Set boot protocol (simpler fixed-format reports).
                if hid.subclass == USB_HID_SUBCLASS_BOOT {
                    if let Err(e) = self.hid_set_boot_protocol(slot_id, hid.interface_num) {
                        crate::serial_println!(
                            "[xhci] SET_PROTOCOL failed for slot {} {}: {:?}",
                            slot_id, protocol_name, e
                        );
                    }
                }

                // Set idle rate to 0 (report only on change).
                let _ = self.hid_set_idle(slot_id, hid.interface_num);

                // Configure the interrupt endpoint for receiving reports.
                let dev_speed = self.devices.iter()
                    .find(|d| d.slot_id == slot_id)
                    .map(|d| d.speed)
                    .unwrap_or(USB_SPEED_HIGH);
                if let Err(e) = self.setup_interrupt_endpoint(
                    slot_id, hid.interrupt_ep, hid.interrupt_max_packet,
                    hid.interval, dev_speed,
                ) {
                    crate::serial_println!(
                        "[xhci] Setup interrupt EP failed for slot {} {}: {:?}",
                        slot_id, protocol_name, e
                    );
                } else {
                    // Post initial receive buffer.
                    let _ = self.post_interrupt_receive(
                        slot_id, hid.interrupt_ep, hid.interrupt_max_packet,
                    );
                }

                crate::serial_println!(
                    "[xhci] Configured {} on slot {} (EP{} IN, {} bytes, interval={})",
                    protocol_name, slot_id, hid.interrupt_ep,
                    hid.interrupt_max_packet, hid.interval
                );

                configured.push(hid.clone());
            }
        }

        configured
    }

    /// Set up an interrupt IN endpoint for a HID device.
    ///
    /// This issues a Configure Endpoint command to activate the interrupt
    /// endpoint so it can receive data transfers.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn setup_interrupt_endpoint(
        &mut self,
        slot_id: u8,
        ep_num: u8,
        max_packet: u16,
        interval: u8,
        speed: u8,
    ) -> KernelResult<()> {
        let slot_idx = (slot_id as usize).wrapping_sub(1);
        if slot_idx >= MAX_SLOTS {
            return Err(KernelError::InvalidArgument);
        }

        let ctx_size = self.context_entry_size();

        // Allocate a Transfer Ring for this interrupt endpoint.
        let int_ring = TrbRing::new(TRANSFER_RING_SIZE, self.hhdm_offset)?;
        let int_ring_phys = int_ring.phys_addr();

        // DCI (Device Context Index) for an IN endpoint N = 2*N + 1
        let dci = (ep_num as usize) * 2 + 1;

        // Build an Input Context for Configure Endpoint.
        let in_ctx_frame = frame::alloc_frame()?;
        let in_ctx_phys = in_ctx_frame.addr();
        let in_ctx_virt = in_ctx_phys.wrapping_add(self.hhdm_offset) as *mut u8;
        // SAFETY: Just allocated.
        unsafe { core::ptr::write_bytes(in_ctx_virt, 0, frame::FRAME_SIZE); }

        // Input Control Context: Add Context Flags — set bit for the endpoint DCI
        // and the Slot Context (bit 0 always set for Configure Endpoint).
        // SAFETY: in_ctx_virt is valid.
        unsafe {
            let add_flags = in_ctx_virt.add(0x04) as *mut u32;
            let flag = (1u32 << dci) | 1; // Add endpoint DCI + Slot
            core::ptr::write_volatile(add_flags, flag);
        }

        // Update Slot Context (at offset ctx_size) — set Context Entries
        // to include this endpoint.
        // SAFETY: in_ctx_virt is a freshly allocated 16 KiB frame; ctx_size
        // (32 or 64 bytes) is well within bounds.
        let slot_ctx = unsafe { in_ctx_virt.add(ctx_size) };
        unsafe {
            let dword0 = ((dci as u32) << 27) // Context Entries = max DCI
                | (u32::from(speed) << 20); // Speed
            core::ptr::write_volatile(slot_ctx as *mut u32, dword0);
        }

        // Fill the Endpoint Context (at offset (dci + 1) * ctx_size).
        // SAFETY: dci ≤ 31, ctx_size ≤ 64, so (dci+1)*ctx_size ≤ 2048,
        // well within the 16 KiB input context frame.
        let ep_ctx = unsafe { in_ctx_virt.add((dci + 1) * ctx_size) };

        // Convert interval to xHCI format (power-of-2 exponent).
        // For HS/SS: interval = 2^(bInterval-1) microframes.
        // For FS/LS: interval in frames.
        let xhci_interval = match speed {
            USB_SPEED_HIGH | USB_SPEED_SUPER => {
                // USB 2.0/3.0: bInterval is the exponent directly.
                interval.max(1)
            }
            _ => {
                // USB 1.x: convert ms interval to closest power of 2.
                // interval in frames → log2(interval) + 3 for 125us microframes.
                let val = interval.max(1);
                let mut exp = 0u8;
                let mut v = val;
                while v > 1 { v >>= 1; exp = exp.wrapping_add(1); }
                exp.wrapping_add(3).min(15)
            }
        };

        // SAFETY: ep_ctx within allocated frame.
        unsafe {
            // Dword 0: Interval, Mult=0, MaxPStreams=0, LSA=0, MaxESITPayload
            let dword0 = u32::from(xhci_interval) << 16;
            core::ptr::write_volatile(ep_ctx as *mut u32, dword0);

            // Dword 1: CErr=3, EP Type=7 (Interrupt IN), Max Packet Size
            // EP Type encoding: 7 = Interrupt IN
            let dword1 = (3u32 << 1)  // CErr = 3
                | (7u32 << 3)          // EP Type = Interrupt IN
                | (u32::from(max_packet) << 16); // Max Packet Size
            core::ptr::write_volatile(ep_ctx.add(4) as *mut u32, dword1);

            // Dword 2-3: TR Dequeue Pointer + DCS=1
            let tr_dequeue = int_ring_phys | 1; // DCS = 1
            core::ptr::write_volatile(ep_ctx.add(8) as *mut u64, tr_dequeue);

            // Dword 4: Average TRB Length
            core::ptr::write_volatile(ep_ctx.add(16) as *mut u32, u32::from(max_packet));
        }

        // Submit Configure Endpoint command.
        let cfg_trb = Trb {
            parameter: in_ctx_phys,
            status: 0,
            control: (TRB_TYPE_CONFIGURE_EP << 10) | (u32::from(slot_id) << 24),
        };

        let completion = self.submit_command(cfg_trb)?;
        let cc = completion.completion_code();

        // Free input context.
        // SAFETY: We own in_ctx_frame exclusively; the Configure Endpoint
        // command has completed and the controller no longer references it.
        let _ = unsafe { frame::free_frame(in_ctx_frame) };

        if cc != TRB_CC_SUCCESS {
            crate::serial_println!(
                "[xhci] Configure Endpoint failed for slot {} EP{}: cc={}",
                slot_id, ep_num, cc
            );
            return Err(KernelError::IoError);
        }

        // Store the interrupt ring for this slot.
        self.slot_int_rings[slot_idx] = Some(int_ring);

        crate::serial_println!(
            "[xhci] Interrupt EP{} IN configured for slot {} (max_pkt={}, interval={})",
            ep_num, slot_id, max_packet, xhci_interval
        );

        Ok(())
    }

    /// Post a Normal TRB on the interrupt endpoint's transfer ring for
    /// receiving a HID input report.
    ///
    /// Allocates a receive buffer if not already allocated, enqueues a
    /// Normal TRB pointing to it, and rings the doorbell.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn post_interrupt_receive(
        &mut self,
        slot_id: u8,
        ep_num: u8,
        max_packet: u16,
    ) -> KernelResult<()> {
        let slot_idx = (slot_id as usize).wrapping_sub(1);
        if slot_idx >= MAX_SLOTS {
            return Err(KernelError::InvalidArgument);
        }

        // Ensure we have a receive buffer for this slot.
        if self.slot_int_bufs[slot_idx].is_none() {
            let buf_frame = frame::alloc_frame()?;
            let buf_virt = buf_frame.addr().wrapping_add(self.hhdm_offset) as *mut u8;
            // SAFETY: Just allocated.
            unsafe { core::ptr::write_bytes(buf_virt, 0, 64); }
            self.slot_int_bufs[slot_idx] = Some(buf_frame);
        }

        let buf_phys = self.slot_int_bufs[slot_idx]
            .as_ref()
            .ok_or(KernelError::InternalError)?
            .addr();

        // Get the interrupt ring.
        let ring = self.slot_int_rings[slot_idx].as_mut()
            .ok_or(KernelError::NoSuchDevice)?;

        // Enqueue a Normal TRB (device writes data to our buffer).
        let normal_trb = Trb {
            parameter: buf_phys,
            status: u32::from(max_packet), // Transfer length
            control: (TRB_TYPE_NORMAL << 10) | TRB_IOC, // IOC = Interrupt On Completion
        };
        ring.enqueue(normal_trb);

        fence(Ordering::SeqCst);

        // DCI for IN endpoint = 2*ep_num + 1
        let dci = (ep_num as u32) * 2 + 1;
        self.ring_doorbell(slot_id, dci);

        Ok(())
    }

    /// Poll for a completed HID input report on a device.
    ///
    /// Returns the raw report bytes if one is available.  The buffer
    /// is only valid until the next call.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn poll_hid_report(&mut self, slot_id: u8) -> Option<&[u8]> {
        let slot_idx = (slot_id as usize).wrapping_sub(1);
        if slot_idx >= MAX_SLOTS {
            return None;
        }

        // Check event ring for a Transfer Event targeting this slot.
        if let Some(trb) = self.poll_event() {
            if trb.trb_type() == TRB_TYPE_TRANSFER_EVENT {
                let cc = trb.completion_code();
                if cc == TRB_CC_SUCCESS || cc == TRB_CC_SHORT_PACKET {
                    let transfer_len = trb.status & 0x00FF_FFFF;
                    let _event_slot = trb.slot_id();

                    // Get the buffer virtual address.
                    if let Some(buf_frame) = &self.slot_int_bufs[slot_idx] {
                        let buf_virt = buf_frame.addr().wrapping_add(self.hhdm_offset) as *const u8;
                        let len = transfer_len.min(64) as usize;
                        // SAFETY: buf_virt is valid, len is bounded.
                        let data = unsafe {
                            core::slice::from_raw_parts(buf_virt, len)
                        };
                        return Some(data);
                    }
                }
            } else {
                // Handle other events.
                self.handle_event(&trb);
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Return a human-readable name for a USB speed value.
fn speed_name(speed: u8) -> &'static str {
    match speed {
        USB_SPEED_FULL => "Full-Speed (12 Mbps)",
        USB_SPEED_LOW => "Low-Speed (1.5 Mbps)",
        USB_SPEED_HIGH => "High-Speed (480 Mbps)",
        USB_SPEED_SUPER => "Super-Speed (5 Gbps)",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the xHCI USB host controller.
///
/// Detects the controller on the PCI bus, resets it, sets up ring
/// buffers, and enumerates any connected USB devices.
///
/// This is a best-effort operation — if no xHCI controller is found
/// or initialization fails, a warning is printed but boot continues.
pub fn init(hhdm_offset: u64) {
    match XhciController::init(hhdm_offset) {
        Ok(mut ctrl) => {
            // Enumerate connected devices.
            ctrl.enumerate_devices();

            // Configure HID devices (keyboards, mice) for boot protocol.
            let hid = ctrl.configure_hid_devices();
            let n_hid = hid.len();
            ctrl.hid_interfaces = hid;

            let n_devices = ctrl.devices.len();
            let n_ports = ctrl.ports.len();
            crate::serial_println!(
                "[xhci] Initialization complete: {} ports, {} devices, {} HID interfaces",
                n_ports, n_devices, n_hid
            );

            *XHCI.lock() = Some(ctrl);
        }
        Err(KernelError::NotFound) => {
            crate::serial_println!("[xhci] No xHCI controller found (USB not available)");
        }
        Err(e) => {
            crate::serial_println!("[xhci] ERROR: Initialization failed: {:?}", e);
        }
    }
}

/// Check if the xHCI controller is initialized and operational.
pub fn is_available() -> bool {
    XHCI.lock().is_some()
}

/// Return a list of detected USB ports and their status.
pub fn port_status() -> Vec<UsbPort> {
    match XHCI.lock().as_ref() {
        Some(ctrl) => ctrl.ports.clone(),
        None => Vec::new(),
    }
}

/// Return a list of enumerated USB devices.
pub fn devices() -> Vec<UsbDevice> {
    match XHCI.lock().as_ref() {
        Some(ctrl) => ctrl.devices.clone(),
        None => Vec::new(),
    }
}

/// Return a list of configured HID interfaces (keyboards, mice).
pub fn hid_interfaces() -> Vec<UsbHidInterface> {
    match XHCI.lock().as_ref() {
        Some(ctrl) => ctrl.hid_interfaces.clone(),
        None => Vec::new(),
    }
}

/// Check if a USB keyboard is available.
pub fn has_keyboard() -> bool {
    match XHCI.lock().as_ref() {
        Some(ctrl) => ctrl.hid_interfaces.iter().any(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD),
        None => false,
    }
}

/// Check if a USB mouse is available.
pub fn has_mouse() -> bool {
    match XHCI.lock().as_ref() {
        Some(ctrl) => ctrl.hid_interfaces.iter().any(|h| h.protocol == USB_HID_PROTOCOL_MOUSE),
        None => false,
    }
}

/// Re-scan ports for newly connected/disconnected devices.
pub fn rescan() {
    if let Some(ctrl) = XHCI.lock().as_mut() {
        ctrl.scan_ports();
        ctrl.enumerate_devices();
        let hid = ctrl.configure_hid_devices();
        ctrl.hid_interfaces = hid;
    }
}

/// Poll for USB keyboard input.
///
/// Returns `Some(HidKeyboardReport)` if a key event is available.
/// This is non-blocking — returns None immediately if no data.
pub fn poll_keyboard() -> Option<HidKeyboardReport> {
    let mut ctrl = XHCI.lock();
    let ctrl = ctrl.as_mut()?;

    // Find the keyboard slot.
    let kb_iface = ctrl.hid_interfaces.iter()
        .find(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD)?;
    let slot_id = kb_iface.slot_id;
    let slot_idx = (slot_id as usize).wrapping_sub(1);

    // Poll for a report.
    if let Some(data) = ctrl.poll_hid_report(slot_id) {
        if data.len() >= 8 {
            let report = HidKeyboardReport {
                modifiers: data[0],
                reserved: data[1],
                keycodes: [data[2], data[3], data[4], data[5], data[6], data[7]],
            };
            // Re-post receive buffer for next report.
            let max_pkt = ctrl.hid_interfaces.iter()
                .find(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD)
                .map(|h| h.interrupt_max_packet)
                .unwrap_or(8);
            let ep_num = ctrl.hid_interfaces.iter()
                .find(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD)
                .map(|h| h.interrupt_ep)
                .unwrap_or(1);
            let _ = ctrl.post_interrupt_receive(slot_id, ep_num, max_pkt);
            return Some(report);
        }
    }

    // Re-post if the buffer was consumed but no valid report.
    if ctrl.slot_int_rings[slot_idx].is_some() {
        let max_pkt = ctrl.hid_interfaces.iter()
            .find(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD)
            .map(|h| h.interrupt_max_packet)
            .unwrap_or(8);
        let ep_num = ctrl.hid_interfaces.iter()
            .find(|h| h.protocol == USB_HID_PROTOCOL_KEYBOARD)
            .map(|h| h.interrupt_ep)
            .unwrap_or(1);
        // Only post if the ring has space.
        let _ = ctrl.post_interrupt_receive(slot_id, ep_num, max_pkt);
    }

    None
}

/// Poll for USB mouse input.
///
/// Returns `Some(HidMouseReport)` if a mouse event is available.
/// This is non-blocking — returns None immediately if no data.
pub fn poll_mouse() -> Option<HidMouseReport> {
    let mut ctrl = XHCI.lock();
    let ctrl = ctrl.as_mut()?;

    // Find the mouse slot.
    let mouse_iface = ctrl.hid_interfaces.iter()
        .find(|h| h.protocol == USB_HID_PROTOCOL_MOUSE)?;
    let slot_id = mouse_iface.slot_id;
    let slot_idx = (slot_id as usize).wrapping_sub(1);

    // Poll for a report.
    if let Some(data) = ctrl.poll_hid_report(slot_id) {
        if data.len() >= 3 {
            let report = HidMouseReport {
                buttons: data[0],
                x: data[1] as i8,
                y: data[2] as i8,
                wheel: if data.len() >= 4 { data[3] as i8 } else { 0 },
            };
            // Re-post receive buffer.
            let max_pkt = ctrl.hid_interfaces.iter()
                .find(|h| h.protocol == USB_HID_PROTOCOL_MOUSE)
                .map(|h| h.interrupt_max_packet)
                .unwrap_or(4);
            let ep_num = ctrl.hid_interfaces.iter()
                .find(|h| h.protocol == USB_HID_PROTOCOL_MOUSE)
                .map(|h| h.interrupt_ep)
                .unwrap_or(1);
            let _ = ctrl.post_interrupt_receive(slot_id, ep_num, max_pkt);
            return Some(report);
        }
    }

    // Re-post if needed.
    if ctrl.slot_int_rings[slot_idx].is_some() {
        let max_pkt = ctrl.hid_interfaces.iter()
            .find(|h| h.protocol == USB_HID_PROTOCOL_MOUSE)
            .map(|h| h.interrupt_max_packet)
            .unwrap_or(4);
        let ep_num = ctrl.hid_interfaces.iter()
            .find(|h| h.protocol == USB_HID_PROTOCOL_MOUSE)
            .map(|h| h.interrupt_ep)
            .unwrap_or(1);
        let _ = ctrl.post_interrupt_receive(slot_id, ep_num, max_pkt);
    }

    None
}

/// USB HID keycode to PS/2 scan code conversion table.
///
/// Maps USB HID keyboard usage codes (0x04-0x65) to AT/PS2 make scan
/// codes.  This allows USB keyboard reports to feed into the existing
/// PS/2 keyboard infrastructure.
///
/// Index = HID usage code.  Value = PS/2 scan code (0 = no mapping).
static HID_TO_SCANCODE: [u8; 104] = [
    0x00, 0x00, 0x00, 0x00, // 0x00-0x03: reserved
    0x1E, 0x30, 0x2E, 0x20, // 0x04-0x07: A, B, C, D
    0x12, 0x21, 0x22, 0x23, // 0x08-0x0B: E, F, G, H
    0x17, 0x24, 0x25, 0x26, // 0x0C-0x0F: I, J, K, L
    0x32, 0x31, 0x18, 0x19, // 0x10-0x13: M, N, O, P
    0x10, 0x13, 0x1F, 0x14, // 0x14-0x17: Q, R, S, T
    0x16, 0x2F, 0x11, 0x2D, // 0x18-0x1B: U, V, W, X
    0x15, 0x2C, 0x02, 0x03, // 0x1C-0x1F: Y, Z, 1, 2
    0x04, 0x05, 0x06, 0x07, // 0x20-0x23: 3, 4, 5, 6
    0x08, 0x09, 0x0A, 0x0B, // 0x24-0x27: 7, 8, 9, 0
    0x1C, 0x01, 0x0E, 0x0F, // 0x28-0x2B: Enter, Escape, Backspace, Tab
    0x39, 0x0C, 0x0D, 0x1A, // 0x2C-0x2F: Space, -, =, [
    0x1B, 0x2B, 0x2B, 0x27, // 0x30-0x33: ], \, #, ;
    0x28, 0x29, 0x33, 0x34, // 0x34-0x37: ', `, ,, .
    0x35, 0x3A, 0x3B, 0x3C, // 0x38-0x3B: /, CapsLock, F1, F2
    0x3D, 0x3E, 0x3F, 0x40, // 0x3C-0x3F: F3, F4, F5, F6
    0x41, 0x42, 0x43, 0x44, // 0x40-0x43: F7, F8, F9, F10
    0x57, 0x58, 0x00, 0x46, // 0x44-0x47: F11, F12, PrintScreen, ScrollLock
    0x00, 0x52, 0x47, 0x49, // 0x48-0x4B: Pause, Insert, Home, PageUp
    0x53, 0x4F, 0x51, 0x4D, // 0x4C-0x4F: Delete, End, PageDown, Right
    0x4B, 0x50, 0x48, 0x45, // 0x50-0x53: Left, Down, Up, NumLock
    0x00, 0x37, 0x4A, 0x4E, // 0x54-0x57: KP/, KP*, KP-, KP+
    0x00, 0x4F, 0x50, 0x51, // 0x58-0x5B: KPEnter, KP1, KP2, KP3
    0x4B, 0x4C, 0x4D, 0x47, // 0x5C-0x5F: KP4, KP5, KP6, KP7
    0x48, 0x49, 0x52, 0x53, // 0x60-0x63: KP8, KP9, KP0, KP.
    0x56, 0x00, 0x00, 0x00, // 0x64-0x67: NonUS\, Application, Power, KP=
];

/// Convert a USB HID keyboard report to the first valid scan code.
///
/// Returns the PS/2-equivalent scan code for the first non-zero
/// keycode in the report, or None if no key is pressed.
pub fn hid_report_to_scancode(report: &HidKeyboardReport) -> Option<u8> {
    for &keycode in &report.keycodes {
        if keycode != 0 && (keycode as usize) < HID_TO_SCANCODE.len() {
            let scancode = HID_TO_SCANCODE[keycode as usize];
            if scancode != 0 {
                return Some(scancode);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the xHCI self-test.
///
/// Verifies that PCI detection works and the controller can be
/// initialized.  If no xHCI hardware is present, the test passes
/// (non-fatal).
pub fn self_test() {
    crate::serial_println!("[xhci] Self-test...");

    // Test 1: PCI detection API.
    let controllers = pci::find_devices_by_class(PCI_CLASS_SERIAL_BUS, PCI_SUBCLASS_USB);
    let xhci_count = controllers.iter().filter(|d| {
        let prog_if = pci::config_read8(
            d.address.bus, d.address.device, d.address.function, 0x09
        );
        prog_if == PCI_PROGIF_XHCI
    }).count();

    crate::serial_println!("[xhci]   PCI scan: {} USB controller(s), {} xHCI", controllers.len(), xhci_count);

    // Test 2: Check global state consistency.
    let available = is_available();
    let ports = port_status();
    let devs = devices();
    crate::serial_println!(
        "[xhci]   State: available={}, ports={}, devices={}",
        available, ports.len(), devs.len()
    );

    // Test 3: Verify TRB structure layout.
    assert_eq!(core::mem::size_of::<Trb>(), 16, "TRB must be 16 bytes");
    assert_eq!(core::mem::size_of::<ErstEntry>(), 16, "ERST entry must be 16 bytes");
    assert_eq!(core::mem::size_of::<UsbDeviceDescriptor>(), 18, "Device descriptor must be 18 bytes");

    // Test 4: TRB field extraction.
    let test_trb = Trb {
        parameter: 0,
        status: 0x01_000000, // completion code = 1 (success)
        control: (TRB_TYPE_CMD_COMPLETION << 10) | (5u32 << 24), // slot_id = 5
    };
    assert_eq!(test_trb.trb_type(), TRB_TYPE_CMD_COMPLETION);
    assert_eq!(test_trb.completion_code(), TRB_CC_SUCCESS);
    assert_eq!(test_trb.slot_id(), 5);

    // Test 5: HID keycode mapping.
    let kb_report = HidKeyboardReport {
        modifiers: 0,
        reserved: 0,
        keycodes: [0x04, 0, 0, 0, 0, 0], // 'A' key
    };
    assert_eq!(hid_report_to_scancode(&kb_report), Some(0x1E)); // PS/2 'A' = 0x1E

    let kb_report2 = HidKeyboardReport {
        modifiers: 0,
        reserved: 0,
        keycodes: [0x28, 0, 0, 0, 0, 0], // Enter key
    };
    assert_eq!(hid_report_to_scancode(&kb_report2), Some(0x1C)); // PS/2 Enter = 0x1C

    let kb_empty = HidKeyboardReport::default();
    assert_eq!(hid_report_to_scancode(&kb_empty), None); // No key pressed

    // Test 6: HID report structures.
    assert_eq!(core::mem::size_of::<HidKeyboardReport>(), 8);
    assert_eq!(core::mem::size_of::<HidMouseReport>(), 4);

    crate::serial_println!("[xhci] Self-test PASSED");
}
