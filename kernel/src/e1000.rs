//! Intel e1000 Gigabit Ethernet driver.
//!
//! Implements the Intel 82540EM (e1000) network adapter commonly emulated
//! by QEMU, VirtualBox, and VMware.  This provides native NIC support for
//! virtual machines without requiring virtio drivers.
//!
//! ## Hardware Overview
//!
//! The e1000 is a PCI device with MMIO register access via BAR0:
//! - Vendor: 0x8086 (Intel)
//! - Device: 0x100E (82540EM, QEMU default), 0x100F (82545EM), etc.
//! - BAR0: Memory-mapped I/O registers (128 KiB region)
//!
//! ## DMA Descriptor Rings
//!
//! TX and RX use circular descriptor rings in host memory:
//! - Each descriptor is 16 bytes (address + length + status + command)
//! - Head pointer (read by hardware) advances as device processes descriptors
//! - Tail pointer (written by software) advances as buffers are submitted
//! - Ring wraps around: hardware processes [head..tail) mod ring_size
//!
//! ## Initialization Sequence
//!
//! 1. Reset device (CTRL.RST)
//! 2. Disable interrupts (IMC = all)
//! 3. Read MAC from RAL0/RAH0 or EEPROM
//! 4. Initialize flow control (zero FCT/FCAL/FCAH/FCTTV)
//! 5. Initialize multicast table (zero MTA[0..127])
//! 6. Set up RX: allocate ring + buffers, configure RDBAL/RDLEN/RDH/RDT/RCTL
//! 7. Set up TX: allocate ring, configure TDBAL/TDLEN/TDH/TDT/TCTL/TIPG
//! 8. Enable RX/TX
//!
//! ## References
//!
//! - Intel 82540EM PCI/PCI-X Family Software Developer's Manual (SDM)
//! - OSDev Wiki: Intel Ethernet i217
//! - Linux drivers/net/ethernet/intel/e1000/

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::virtio::net::MacAddress;

// ---------------------------------------------------------------------------
// PCI identification
// ---------------------------------------------------------------------------

/// Intel vendor ID.
const INTEL_VENDOR: u16 = 0x8086;

/// Known e1000 device IDs (QEMU, VirtualBox, VMware).
const E1000_DEVICE_IDS: &[u16] = &[
    0x100E, // 82540EM (QEMU default -netdev e1000)
    0x100F, // 82545EM (VMware)
    0x10D3, // 82574L (common real hardware)
    0x153A, // I217-LM
    0x1539, // I211-AT
];

// ---------------------------------------------------------------------------
// Register offsets (from Intel SDM)
// ---------------------------------------------------------------------------

/// Device Control Register.
const REG_CTRL: u32 = 0x0000;
/// Device Status Register.
const REG_STATUS: u32 = 0x0008;
/// EEPROM Read Register.
const REG_EERD: u32 = 0x0014;
/// Flow Control Address Low.
const REG_FCAL: u32 = 0x0028;
/// Flow Control Address High.
const REG_FCAH: u32 = 0x002C;
/// Flow Control Type.
const REG_FCT: u32 = 0x0030;
/// Flow Control Transmit Timer Value.
const REG_FCTTV: u32 = 0x0170;

/// Interrupt Mask Clear Register.
const REG_IMC: u32 = 0x00D8;

/// Receive Control Register.
const REG_RCTL: u32 = 0x0100;
/// Receive Descriptor Base Address Low.
const REG_RDBAL: u32 = 0x2800;
/// Receive Descriptor Base Address High.
const REG_RDBAH: u32 = 0x2804;
/// Receive Descriptor Length (bytes).
const REG_RDLEN: u32 = 0x2808;
/// Receive Descriptor Head.
const REG_RDH: u32 = 0x2810;
/// Receive Descriptor Tail.
const REG_RDT: u32 = 0x2818;

/// Transmit Control Register.
const REG_TCTL: u32 = 0x0400;
/// Transmit Inter-Packet Gap.
const REG_TIPG: u32 = 0x0410;
/// Transmit Descriptor Base Address Low.
const REG_TDBAL: u32 = 0x3800;
/// Transmit Descriptor Base Address High.
const REG_TDBAH: u32 = 0x3804;
/// Transmit Descriptor Length (bytes).
const REG_TDLEN: u32 = 0x3808;
/// Transmit Descriptor Head.
const REG_TDH: u32 = 0x3810;
/// Transmit Descriptor Tail.
const REG_TDT: u32 = 0x3818;

/// Receive Address Low (first entry, index 0).
const REG_RAL0: u32 = 0x5400;
/// Receive Address High (first entry, index 0).
const REG_RAH0: u32 = 0x5404;

/// Multicast Table Array (128 entries × 4 bytes).
const REG_MTA_BASE: u32 = 0x5200;

// ---------------------------------------------------------------------------
// Register bits
// ---------------------------------------------------------------------------

// CTRL register bits
/// Full-Duplex.
const CTRL_FD: u32 = 1 << 0;
/// Set Link Up.
const CTRL_SLU: u32 = 1 << 6;
/// Device Reset.
const CTRL_RST: u32 = 1 << 26;

// RCTL register bits
/// Receiver Enable.
const RCTL_EN: u32 = 1 << 1;
/// Broadcast Accept Mode.
const RCTL_BAM: u32 = 1 << 15;
/// Strip Ethernet CRC.
const RCTL_SECRC: u32 = 1 << 26;

// TCTL register bits
/// Transmit Enable.
const TCTL_EN: u32 = 1 << 1;
/// Pad Short Packets.
const TCTL_PSP: u32 = 1 << 3;
/// Collision Threshold (shift position).
const TCTL_CT_SHIFT: u32 = 4;
/// Collision Distance (shift position).
const TCTL_COLD_SHIFT: u32 = 12;

// TIPG (Transmit Inter-Packet Gap) — standard values for 802.3.
const TIPG_IPGT: u32 = 10;
const TIPG_IPGR1: u32 = 8;
const TIPG_IPGR2: u32 = 6;

// Status register bits
/// Link Up.
const STATUS_LU: u32 = 1 << 1;

// EERD (EEPROM Read) register bits
/// Start Read.
const EERD_START: u32 = 1 << 0;
/// Read Done.
const EERD_DONE: u32 = 1 << 4;

// RX descriptor status bits
/// Descriptor Done (hardware has written data).
const RXD_STAT_DD: u8 = 1 << 0;

// TX descriptor command bits
/// End of Packet.
const TXD_CMD_EOP: u8 = 1 << 0;
/// Insert FCS/CRC.
const TXD_CMD_IFCS: u8 = 1 << 1;
/// Report Status (set DD in status when done).
const TXD_CMD_RS: u8 = 1 << 3;

// TX descriptor status bits
/// Descriptor Done.
const TXD_STAT_DD: u8 = 1 << 0;

// ---------------------------------------------------------------------------
// Descriptor ring configuration
// ---------------------------------------------------------------------------

/// Number of RX descriptors (16 = 2 frames × 8 buffers per frame).
/// Must be multiple of 8.
const RX_DESC_COUNT: usize = 16;

/// Number of TX descriptors. Must be multiple of 8.
const TX_DESC_COUNT: usize = 32;

/// Size of each RX buffer (must match RCTL_BSIZE setting = 2048 default).
const RX_BUF_SIZE: usize = 2048;

/// Maximum Ethernet frame we can transmit.
const MAX_TX_SIZE: usize = 1514;

// ---------------------------------------------------------------------------
// Descriptor structures (in host memory, DMA-accessible)
// ---------------------------------------------------------------------------

/// Receive descriptor (legacy format, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct RxDesc {
    /// Physical address of the receive buffer.
    addr: u64,
    /// Length of received data (written by hardware).
    length: u16,
    /// Packet checksum.
    checksum: u16,
    /// Descriptor status (DD, EOP, etc.).
    status: u8,
    /// Errors.
    errors: u8,
    /// Special (VLAN tag).
    special: u16,
}

/// Transmit descriptor (legacy format, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct TxDesc {
    /// Physical address of the transmit data.
    addr: u64,
    /// Length of data to transmit.
    length: u16,
    /// Checksum offset.
    cso: u8,
    /// Command bits (EOP, IFCS, RS, etc.).
    cmd: u8,
    /// Descriptor status (DD when complete).
    status: u8,
    /// Checksum start.
    css: u8,
    /// Special (VLAN tag).
    special: u16,
}

// ---------------------------------------------------------------------------
// E1000 device
// ---------------------------------------------------------------------------

/// Intel e1000 network device instance.
pub struct E1000Device {
    /// Base virtual address for MMIO registers.
    mmio_base: *mut u8,
    /// MAC address.
    mac: MacAddress,
    /// HHDM offset for physical→virtual translation.
    hhdm_offset: u64,

    // RX ring state
    /// Physical frame holding the RX descriptor ring.
    rx_ring_frame: Option<PhysFrame>,
    /// Virtual address of RX descriptor ring.
    rx_descs: *mut RxDesc,
    /// Physical frames holding RX packet buffers (2 × 16 KiB = 16 × 2048 buffers).
    rx_buf_frames: [Option<PhysFrame>; 2],
    /// Current RX tail index (next to check for received packets).
    rx_tail: u16,

    // TX ring state
    /// Physical frame holding the TX descriptor ring.
    tx_ring_frame: Option<PhysFrame>,
    /// Virtual address of TX descriptor ring.
    tx_descs: *mut TxDesc,
    /// Physical frame for TX data buffer.
    tx_buf_frame: Option<PhysFrame>,
    /// Virtual address of TX buffer.
    tx_buf: *mut u8,
    /// Current TX tail index (next available descriptor).
    tx_tail: u16,
}

// SAFETY: E1000Device contains raw pointers to DMA memory that we
// solely own. Access is synchronized via the global Mutex<Option<E1000Device>>.
unsafe impl Send for E1000Device {}

impl E1000Device {
    // -----------------------------------------------------------------------
    // Register access
    // -----------------------------------------------------------------------

    /// Read a 32-bit MMIO register.
    #[inline]
    fn read_reg(&self, offset: u32) -> u32 {
        // SAFETY: offset is within the 128 KiB MMIO region, and we have
        // exclusive access via the global mutex.
        unsafe {
            let ptr = self.mmio_base.add(offset as usize) as *const u32;
            core::ptr::read_volatile(ptr)
        }
    }

    /// Write a 32-bit MMIO register.
    #[inline]
    fn write_reg(&self, offset: u32, value: u32) {
        // SAFETY: Same as read_reg.
        unsafe {
            let ptr = self.mmio_base.add(offset as usize) as *mut u32;
            core::ptr::write_volatile(ptr, value);
        }
    }

    // -----------------------------------------------------------------------
    // EEPROM access
    // -----------------------------------------------------------------------

    /// Read a 16-bit word from the EEPROM.
    fn eeprom_read(&self, addr: u8) -> Option<u16> {
        // Start EEPROM read: write address (bits 15:8) + start bit.
        let eerd_val = (u32::from(addr) << 8) | EERD_START;
        self.write_reg(REG_EERD, eerd_val);

        // Poll for completion (DONE bit).
        for _ in 0..10000 {
            let val = self.read_reg(REG_EERD);
            if val & EERD_DONE != 0 {
                // Data is in bits 31:16.
                #[allow(clippy::cast_possible_truncation)]
                return Some((val >> 16) as u16);
            }
            core::hint::spin_loop();
        }

        None
    }

    // -----------------------------------------------------------------------
    // MAC address
    // -----------------------------------------------------------------------

    /// Read the MAC address from RAL0/RAH0 registers.
    fn read_mac_from_regs(&self) -> Option<MacAddress> {
        let ral = self.read_reg(REG_RAL0);
        let rah = self.read_reg(REG_RAH0);

        // RAH bit 31 (AV = Address Valid) should be set.
        if rah & (1 << 31) == 0 {
            return None;
        }

        #[allow(clippy::cast_possible_truncation)]
        let mac = MacAddress([
            (ral & 0xFF) as u8,
            ((ral >> 8) & 0xFF) as u8,
            ((ral >> 16) & 0xFF) as u8,
            ((ral >> 24) & 0xFF) as u8,
            (rah & 0xFF) as u8,
            ((rah >> 8) & 0xFF) as u8,
        ]);

        // Reject all-zeros or all-ones (invalid).
        if mac.0 == [0; 6] || mac.0 == [0xFF; 6] {
            return None;
        }

        Some(mac)
    }

    /// Read the MAC address from the EEPROM.
    fn read_mac_from_eeprom(&self) -> Option<MacAddress> {
        let word0 = self.eeprom_read(0)?;
        let word1 = self.eeprom_read(1)?;
        let word2 = self.eeprom_read(2)?;

        #[allow(clippy::cast_possible_truncation)]
        let mac = MacAddress([
            (word0 & 0xFF) as u8,
            ((word0 >> 8) & 0xFF) as u8,
            (word1 & 0xFF) as u8,
            ((word1 >> 8) & 0xFF) as u8,
            (word2 & 0xFF) as u8,
            ((word2 >> 8) & 0xFF) as u8,
        ]);

        // Reject invalid.
        if mac.0 == [0; 6] || mac.0 == [0xFF; 6] {
            return None;
        }

        Some(mac)
    }

    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize an e1000 device from a PCI device.
    ///
    /// Performs full hardware init: reset, MAC read, descriptor ring setup,
    /// and RX/TX enable.
    pub fn init(pci_dev: &PciDevice, hhdm_offset: u64) -> KernelResult<Self> {
        // Get BAR0 MMIO address.
        let bar0_phys = pci_dev.bar0_mmio_addr()
            .ok_or(KernelError::InvalidArgument)?;

        // For 64-bit BAR: combine BAR0 (low) + BAR1 (high).
        let bar0_type = (pci_dev.bars[0] >> 1) & 0x3;
        let mmio_phys = if bar0_type == 2 {
            // 64-bit BAR: high 32 bits in BAR1.
            bar0_phys | (u64::from(pci_dev.bars[1]) << 32)
        } else {
            bar0_phys
        };

        // Map MMIO region into kernel virtual address space.
        // PCI BAR addresses may be above physical RAM, so the HHDM
        // bootloader mapping doesn't cover them. We must explicitly
        // map the MMIO pages with NO_CACHE attribute.
        let mmio_virt = mmio_phys.wrapping_add(hhdm_offset);
        let mmio_base = mmio_virt as *mut u8;

        // Map 128 KiB of MMIO space (8 × 16 KiB frames).
        let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
        let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;
        // Map each 16 KiB frame covering the 128 KiB register space.
        for i in 0..8u64 {
            let frame_phys = mmio_phys.wrapping_add(i.wrapping_mul(16384));
            if let Some(frame) = PhysFrame::from_addr(frame_phys) {
                let virt = VirtAddr::new(mmio_virt.wrapping_add(i.wrapping_mul(16384)));
                // SAFETY: frame_phys is the PCI BAR MMIO region.
                // Mapping device registers into kernel VA space.
                if let Err(_e) = unsafe {
                    page_table::map_frame(pml4_phys, virt, frame, mmio_flags)
                } {
                    // May already be mapped (e.g., within HHDM range on large-RAM systems).
                    // Continue — the mapping might work via existing HHDM.
                }
            }
        }
        // Flush TLB for the mapped region.
        for i in 0..8u64 {
            let addr = mmio_virt.wrapping_add(i.wrapping_mul(16384));
            // SAFETY: Standard invlpg to flush stale TLB entries.
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
            }
        }

        // Enable bus mastering for DMA.
        pci::enable_bus_master(pci_dev.address);

        // Create a device for register access during init.
        let mut dev = Self {
            mmio_base,
            mac: MacAddress([0; 6]),
            hhdm_offset,
            rx_ring_frame: None,
            rx_descs: core::ptr::null_mut(),
            rx_buf_frames: [None, None],
            rx_tail: 0,
            tx_ring_frame: None,
            tx_descs: core::ptr::null_mut(),
            tx_buf_frame: None,
            tx_buf: core::ptr::null_mut(),
            tx_tail: 0,
        };

        // Step 1: Reset the device.
        dev.reset();

        // Step 2: Disable all interrupts (we use polling).
        dev.write_reg(REG_IMC, 0xFFFF_FFFF);

        // Step 3: Read MAC address (try registers first, then EEPROM).
        dev.mac = dev.read_mac_from_regs()
            .or_else(|| dev.read_mac_from_eeprom())
            .ok_or(KernelError::InternalError)?;

        serial_println!("[e1000] MAC: {}", dev.mac);

        // Step 4: Clear flow control registers.
        dev.write_reg(REG_FCAL, 0);
        dev.write_reg(REG_FCAH, 0);
        dev.write_reg(REG_FCT, 0);
        dev.write_reg(REG_FCTTV, 0);

        // Step 5: Clear Multicast Table Array (128 entries).
        for i in 0..128u32 {
            #[allow(clippy::arithmetic_side_effects)]
            dev.write_reg(REG_MTA_BASE.wrapping_add(i.wrapping_mul(4)), 0);
        }

        // Step 6: Set up RX.
        dev.init_rx()?;

        // Step 7: Set up TX.
        dev.init_tx()?;

        // Step 8: Set link up.
        let ctrl = dev.read_reg(REG_CTRL);
        dev.write_reg(REG_CTRL, ctrl | CTRL_SLU | CTRL_FD);

        // Wait for link up.
        let mut link_up = false;
        for _ in 0..1000 {
            let status = dev.read_reg(REG_STATUS);
            if status & STATUS_LU != 0 {
                link_up = true;
                break;
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }

        if link_up {
            serial_println!("[e1000] Link is UP");
        } else {
            serial_println!("[e1000] Link not detected (may come up later)");
        }

        Ok(dev)
    }

    /// Reset the device.
    fn reset(&self) {
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | CTRL_RST);

        // Wait for reset to complete (RST bit self-clears).
        for _ in 0..10000 {
            if self.read_reg(REG_CTRL) & CTRL_RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // Disable interrupts again after reset.
        self.write_reg(REG_IMC, 0xFFFF_FFFF);
    }

    /// Initialize the receive descriptor ring and buffers.
    ///
    /// Allocates 16 RX descriptors backed by 2 DMA frames (8 buffers each).
    /// Each buffer is 2048 bytes, matching the RCTL buffer size setting.
    fn init_rx(&mut self) -> KernelResult<()> {
        // Allocate a frame for the RX descriptor ring.
        // 16 descriptors × 16 bytes = 256 bytes (fits in one 16 KiB frame).
        let ring_frame = frame::alloc_frame()?;
        let ring_phys = ring_frame.addr();
        let ring_virt = ring_frame.to_virt(self.hhdm_offset) as *mut RxDesc;

        // Allocate 2 frames for RX packet buffers.
        // Each 16 KiB frame holds 8 × 2048-byte buffers → 16 buffers total.
        let buf_frame0 = frame::alloc_frame()?;
        let buf_frame1 = frame::alloc_frame()?;

        // Zero out the descriptor ring.
        // SAFETY: We own the frame and have exclusive access.
        unsafe {
            core::ptr::write_bytes(ring_virt as *mut u8, 0, 16384);
        }

        // Initialize RX descriptors with buffer physical addresses.
        // Descriptors 0..7 use buf_frame0, descriptors 8..15 use buf_frame1.
        for i in 0..RX_DESC_COUNT {
            let buf_phys = if i < 8 {
                buf_frame0.addr().wrapping_add((i as u64).wrapping_mul(RX_BUF_SIZE as u64))
            } else {
                buf_frame1.addr().wrapping_add(((i - 8) as u64).wrapping_mul(RX_BUF_SIZE as u64))
            };

            // SAFETY: ring_virt points to zeroed, owned memory within bounds.
            unsafe {
                let desc = &mut *ring_virt.add(i);
                desc.addr = buf_phys;
                desc.status = 0;
            }
        }

        // Configure the hardware RX registers.
        #[allow(clippy::cast_possible_truncation)]
        {
            self.write_reg(REG_RDBAL, ring_phys as u32);
            self.write_reg(REG_RDBAH, (ring_phys >> 32) as u32);
        }
        // RDLEN is in bytes (number of descriptors × 16).
        #[allow(clippy::cast_possible_truncation)]
        self.write_reg(REG_RDLEN, (RX_DESC_COUNT * 16) as u32);
        // Head = 0 (hardware starts reading from here).
        self.write_reg(REG_RDH, 0);
        // Tail = last valid descriptor (tells hardware all buffers are available).
        #[allow(clippy::cast_possible_truncation)]
        self.write_reg(REG_RDT, (RX_DESC_COUNT - 1) as u32);

        // Enable receiver: broadcast accept + strip CRC + enable.
        self.write_reg(REG_RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);

        // Store state.
        self.rx_ring_frame = Some(ring_frame);
        self.rx_descs = ring_virt;
        self.rx_buf_frames = [Some(buf_frame0), Some(buf_frame1)];
        self.rx_tail = 0;

        Ok(())
    }

    /// Initialize the transmit descriptor ring.
    fn init_tx(&mut self) -> KernelResult<()> {
        // Allocate a frame for the TX descriptor ring.
        // 32 descriptors × 16 bytes = 512 bytes (fits in one 16 KiB frame).
        let ring_frame = frame::alloc_frame()?;
        let ring_phys = ring_frame.addr();
        let ring_virt = ring_frame.to_virt(self.hhdm_offset) as *mut TxDesc;

        // Allocate a frame for TX data (single buffer for sequential sends).
        let buf_frame = frame::alloc_frame()?;
        let buf_virt = buf_frame.to_virt(self.hhdm_offset) as *mut u8;

        // Zero out the descriptor ring.
        // SAFETY: We own the frame.
        unsafe {
            core::ptr::write_bytes(ring_virt as *mut u8, 0, 16384);
        }

        // Mark all TX descriptors as done (DD=1) so we know they're free.
        for i in 0..TX_DESC_COUNT {
            // SAFETY: ring_virt is valid for TX_DESC_COUNT entries.
            unsafe {
                let desc = &mut *ring_virt.add(i);
                desc.status = TXD_STAT_DD; // Available for use.
            }
        }

        // Configure the hardware TX registers.
        #[allow(clippy::cast_possible_truncation)]
        {
            self.write_reg(REG_TDBAL, ring_phys as u32);
            self.write_reg(REG_TDBAH, (ring_phys >> 32) as u32);
            self.write_reg(REG_TDLEN, (TX_DESC_COUNT * 16) as u32);
        }
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);

        // Configure TCTL: enable TX, pad short packets,
        // collision threshold = 15, collision distance = 64.
        let tctl = TCTL_EN | TCTL_PSP
            | (15u32 << TCTL_CT_SHIFT)
            | (64u32 << TCTL_COLD_SHIFT);
        self.write_reg(REG_TCTL, tctl);

        // Configure TIPG (Inter-Packet Gap) — standard IEEE 802.3 values.
        let tipg = TIPG_IPGT | (TIPG_IPGR1 << 10) | (TIPG_IPGR2 << 20);
        self.write_reg(REG_TIPG, tipg);

        // Store state.
        self.tx_ring_frame = Some(ring_frame);
        self.tx_descs = ring_virt;
        self.tx_buf_frame = Some(buf_frame);
        self.tx_buf = buf_virt;
        self.tx_tail = 0;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Get the MAC address.
    pub fn mac(&self) -> MacAddress {
        self.mac
    }

    /// Send an Ethernet frame.
    ///
    /// The frame should include the Ethernet header (dst + src + ethertype)
    /// but NOT the FCS/CRC (hardware appends it).
    pub fn send(&mut self, frame: &[u8]) -> KernelResult<()> {
        if frame.len() > MAX_TX_SIZE || frame.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let buf_frame = self.tx_buf_frame.ok_or(KernelError::InternalError)?;
        let tail = self.tx_tail as usize;

        // Check that the descriptor is free (DD bit set by hardware on completion).
        // SAFETY: tx_descs is valid for TX_DESC_COUNT entries and we have exclusive access.
        let desc = unsafe { &mut *self.tx_descs.add(tail) };
        if desc.status & TXD_STAT_DD == 0 {
            // Descriptor not yet completed by hardware — TX ring is full.
            return Err(KernelError::ResourceExhausted);
        }

        // Copy frame data to the TX buffer.
        let buf_phys = buf_frame.addr();
        // SAFETY: We own the TX buffer frame and frame.len() <= MAX_TX_SIZE < 16384.
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_buf, frame.len());
        }

        // Set up the descriptor.
        desc.addr = buf_phys;
        #[allow(clippy::cast_possible_truncation)]
        { desc.length = frame.len() as u16; }
        desc.cmd = TXD_CMD_EOP | TXD_CMD_IFCS | TXD_CMD_RS;
        desc.status = 0; // Clear DD — hardware will set it on completion.

        // Advance tail (notify hardware).
        #[allow(clippy::cast_possible_truncation)]
        {
            self.tx_tail = ((tail + 1) % TX_DESC_COUNT) as u16;
        }
        self.write_reg(REG_TDT, u32::from(self.tx_tail));

        // Poll for completion (RS + DD).
        for _ in 0..100_000 {
            let status = unsafe { core::ptr::read_volatile(&(*self.tx_descs.add(tail)).status) };
            if status & TXD_STAT_DD != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }

        Err(KernelError::TimedOut)
    }

    /// Receive the next pending Ethernet frame (if any).
    ///
    /// Returns `None` if no packet is available.
    pub fn recv(&mut self) -> Option<Vec<u8>> {
        let tail = self.rx_tail as usize;

        // Check if the current descriptor has data (DD bit set by hardware).
        // SAFETY: rx_descs is valid for RX_DESC_COUNT entries, we have exclusive access.
        let desc = unsafe { &mut *self.rx_descs.add(tail) };
        if desc.status & RXD_STAT_DD == 0 {
            return None; // No packet available.
        }

        // Read the packet length.
        let length = desc.length as usize;
        if length == 0 || length > RX_BUF_SIZE {
            // Invalid length — reset descriptor and move on.
            desc.status = 0;
            #[allow(clippy::cast_possible_truncation)]
            { self.rx_tail = ((tail + 1) % RX_DESC_COUNT) as u16; }
            self.write_reg(REG_RDT, tail as u32);
            return None;
        }

        // Get the virtual address of this descriptor's buffer.
        let buf_virt = if tail < 8 {
            let frame0 = self.rx_buf_frames[0]?;
            let base = frame0.to_virt(self.hhdm_offset) as *const u8;
            // SAFETY: tail < 8, offset < 16384 (8 × 2048).
            unsafe { base.add(tail.wrapping_mul(RX_BUF_SIZE)) }
        } else {
            let frame1 = self.rx_buf_frames[1]?;
            let base = frame1.to_virt(self.hhdm_offset) as *const u8;
            // SAFETY: (tail-8) < 8, offset < 16384.
            unsafe { base.add((tail - 8).wrapping_mul(RX_BUF_SIZE)) }
        };

        // Copy frame data to a new Vec.
        let mut packet = Vec::with_capacity(length);
        // SAFETY: buf_virt is valid for `length` bytes (hardware wrote ≤ 2048).
        unsafe {
            let slice = core::slice::from_raw_parts(buf_virt, length);
            packet.extend_from_slice(slice);
        }

        // Reset the descriptor for reuse.
        desc.status = 0;

        // Advance tail — tell hardware this buffer is available again.
        let old_tail = tail;
        #[allow(clippy::cast_possible_truncation)]
        { self.rx_tail = ((tail + 1) % RX_DESC_COUNT) as u16; }
        self.write_reg(REG_RDT, old_tail as u32);

        Some(packet)
    }

    /// Check if the link is up.
    pub fn link_up(&self) -> bool {
        self.read_reg(REG_STATUS) & STATUS_LU != 0
    }
}

impl Drop for E1000Device {
    fn drop(&mut self) {
        // Disable RX and TX.
        self.write_reg(REG_RCTL, 0);
        self.write_reg(REG_TCTL, 0);

        // Free DMA frames.
        // SAFETY: We own all these frames exclusively.
        unsafe {
            if let Some(f) = self.rx_ring_frame {
                let _ = frame::free_frame(f);
            }
            if let Some(f) = self.rx_buf_frames[0] {
                let _ = frame::free_frame(f);
            }
            if let Some(f) = self.rx_buf_frames[1] {
                let _ = frame::free_frame(f);
            }
            if let Some(f) = self.tx_ring_frame {
                let _ = frame::free_frame(f);
            }
            if let Some(f) = self.tx_buf_frame {
                let _ = frame::free_frame(f);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global device instance
// ---------------------------------------------------------------------------

/// The global e1000 device (if present).
static DEVICE: Mutex<Option<E1000Device>> = Mutex::new(None);

/// Whether the e1000 is the active NIC (preferred over virtio-net).
#[allow(dead_code)]
static E1000_ACTIVE: AtomicBool = AtomicBool::new(false);

/// IRQ line for interrupt support (future).
#[allow(dead_code)]
static E1000_IRQ_LINE: AtomicU8 = AtomicU8::new(0xFF);

/// Execute a closure with the global e1000 device, if present.
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut E1000Device) -> R,
{
    let mut guard = DEVICE.lock();
    guard.as_mut().map(f)
}

/// Check if the e1000 is the active network device.
#[allow(dead_code)]
pub fn is_active() -> bool {
    E1000_ACTIVE.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Discovery and initialization
// ---------------------------------------------------------------------------

/// Find an Intel e1000 on the PCI bus.
fn find_e1000() -> Option<PciDevice> {
    for &dev_id in E1000_DEVICE_IDS {
        if let Some(dev) = pci::find_device(INTEL_VENDOR, dev_id) {
            return Some(dev);
        }
    }
    None
}

/// Probe and initialize an e1000 device.
pub fn probe(hhdm_offset: u64) -> Option<E1000Device> {
    let pci_dev = find_e1000()?;
    serial_println!(
        "[e1000] Found Intel NIC at {:02x}:{:02x}.{} (device={:#06x}, irq={})",
        pci_dev.address.bus,
        pci_dev.address.device,
        pci_dev.address.function,
        pci_dev.device_id,
        pci_dev.irq_line,
    );

    match E1000Device::init(&pci_dev, hhdm_offset) {
        Ok(dev) => {
            E1000_IRQ_LINE.store(pci_dev.irq_line, Ordering::Release);
            serial_println!("[e1000] Device initialized successfully");
            Some(dev)
        }
        Err(e) => {
            serial_println!("[e1000] Init failed: {:?}", e);
            None
        }
    }
}

/// Initialize the e1000 subsystem.
///
/// Probes PCI for an Intel e1000 NIC. If found (and virtio-net is NOT
/// available), this becomes the active network device.
pub fn init(hhdm_offset: u64) {
    if let Some(dev) = probe(hhdm_offset) {
        serial_println!(
            "[e1000] MAC: {}, link: {}",
            dev.mac(),
            if dev.link_up() { "up" } else { "down" }
        );
        *DEVICE.lock() = Some(dev);

        // Mark e1000 as active if virtio-net is not present.
        let has_virtio = crate::virtio::net::with_device(|_| ()).is_some();
        if !has_virtio {
            E1000_ACTIVE.store(true, Ordering::Release);
            serial_println!("[e1000] Active NIC (no virtio-net detected)");
        } else {
            serial_println!("[e1000] Available as secondary NIC (virtio-net is primary)");
        }
    } else {
        serial_println!("[e1000] No Intel e1000 NIC found (non-fatal)");
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for e1000 driver.
pub fn self_test() {
    serial_println!("[e1000] Running self-test...");

    let has_device = DEVICE.lock().is_some();
    if !has_device {
        serial_println!("[e1000]   No device — skipping hardware tests");
        serial_println!("[e1000] Self-test PASSED (no hardware)");
        return;
    }

    // Test 1: MAC address is valid.
    let mac = with_device(|dev| dev.mac()).unwrap();
    assert!(mac.0 != [0; 6], "MAC should not be all-zeros");
    assert!(mac.0 != [0xFF; 6], "MAC should not be all-ones");
    serial_println!("[e1000]   MAC valid: {}", mac);

    // Test 2: Link status readable.
    let link = with_device(|dev| dev.link_up()).unwrap();
    serial_println!("[e1000]   Link status: {}", if link { "UP" } else { "DOWN" });

    // Test 3: Registers are accessible (read STATUS).
    let status = with_device(|dev| dev.read_reg(REG_STATUS)).unwrap();
    assert!(status != 0xFFFF_FFFF, "STATUS should not be all-ones (unmapped MMIO)");
    serial_println!("[e1000]   STATUS register: {:#010x}", status);

    // Test 4: RCTL is configured (EN bit should be set).
    let rctl = with_device(|dev| dev.read_reg(REG_RCTL)).unwrap();
    assert!(rctl & RCTL_EN != 0, "RCTL.EN should be set after init");
    serial_println!("[e1000]   RCTL: {:#010x} (RX enabled)", rctl);

    // Test 5: TCTL is configured (EN bit should be set).
    let tctl = with_device(|dev| dev.read_reg(REG_TCTL)).unwrap();
    assert!(tctl & TCTL_EN != 0, "TCTL.EN should be set after init");
    serial_println!("[e1000]   TCTL: {:#010x} (TX enabled)", tctl);

    serial_println!("[e1000] Self-test PASSED");
}
