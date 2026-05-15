//! Realtek RTL8139 10/100 Mbps Ethernet driver.
//!
//! The RTL8139 is one of the most common and well-documented NICs, found in
//! many older PCs and emulated by QEMU (`-netdev ... -device rtl8139`).
//!
//! ## Architecture
//!
//! The RTL8139 uses I/O port-based register access (not MMIO).  It has:
//! - A continuous 8 KiB + 16 + 1500 byte RX ring buffer (hardware wraps)
//! - 4 TX descriptors, each with its own buffer address register
//! - Simple MAC address read from registers 0x00-0x05
//!
//! ## Integration
//!
//! This driver plugs into the unified network abstraction in `net/mod.rs`.
//! The send/recv functions are exposed via the `with_device()` closure pattern.

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::pci;
use crate::port;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// PCI identification
// ---------------------------------------------------------------------------

/// Realtek vendor ID.
const REALTEK_VENDOR: u16 = 0x10EC;

/// Known RTL8139 device IDs.
const RTL8139_DEVICE_IDS: &[u16] = &[
    0x8139, // RTL8139
    0x8138, // RTL8139 (alternate)
    0x8136, // RTL8101E / RTL8102E
];

// ---------------------------------------------------------------------------
// Register offsets (I/O port relative)
// ---------------------------------------------------------------------------

/// MAC address registers (bytes 0-5).
const REG_MAC: u16 = 0x00;

/// Multicast registers (8 bytes).
#[allow(dead_code)]
const REG_MAR: u16 = 0x08;

/// TX status registers (4 descriptors × 4 bytes each).
const REG_TX_STATUS0: u16 = 0x10;

/// TX start address registers (4 descriptors × 4 bytes each).
const REG_TX_ADDR0: u16 = 0x20;

/// RX buffer start address (physical).
const REG_RX_BUF: u16 = 0x30;

/// Command register.
const REG_CMD: u16 = 0x37;

/// Current Address of Packet Read (CAPR) — software read pointer.
const REG_CAPR: u16 = 0x38;

/// Current Buffer Address (CBA) — hardware write pointer.
#[allow(dead_code)]
const REG_CBA: u16 = 0x3A;

/// Interrupt Mask Register.
const REG_IMR: u16 = 0x3C;

/// Interrupt Status Register.
const REG_ISR: u16 = 0x3E;

/// TX Configuration register.
const REG_TX_CONFIG: u16 = 0x40;

/// RX Configuration register.
const REG_RX_CONFIG: u16 = 0x44;

/// Configuration 1 register.
const REG_CONFIG1: u16 = 0x52;

// ---------------------------------------------------------------------------
// Command register bits
// ---------------------------------------------------------------------------

/// Reset the chip.
const CMD_RESET: u8 = 0x10;
/// Enable receiver.
const CMD_RX_ENABLE: u8 = 0x08;
/// Enable transmitter.
const CMD_TX_ENABLE: u8 = 0x04;

// ---------------------------------------------------------------------------
// Interrupt bits
// ---------------------------------------------------------------------------

/// Receive OK.
const INT_ROK: u16 = 0x0001;
/// Transmit OK.
#[allow(dead_code)]
const INT_TOK: u16 = 0x0004;
/// RX buffer overflow.
#[allow(dead_code)]
const INT_RX_OVERFLOW: u16 = 0x0010;

// ---------------------------------------------------------------------------
// RX configuration bits
// ---------------------------------------------------------------------------

/// Accept all packets (promiscuous mode).
#[allow(dead_code)]
const RX_CFG_AAP: u32 = 1 << 0;
/// Accept physical match (our MAC).
const RX_CFG_APM: u32 = 1 << 1;
/// Accept multicast.
const RX_CFG_AM: u32 = 1 << 2;
/// Accept broadcast.
const RX_CFG_AB: u32 = 1 << 3;
/// Wrap bit: when RX buffer overflows, wrap to beginning.
const RX_CFG_WRAP: u32 = 1 << 7;
/// RX buffer size: 8K + 16.
const RX_CFG_8K: u32 = 0b00 << 11;

// ---------------------------------------------------------------------------
// TX status bits
// ---------------------------------------------------------------------------

/// Own: set by software to start TX, cleared by hardware on completion.
const TX_STATUS_OWN: u32 = 1 << 13;
/// TX OK: set by hardware on successful transmission.
#[allow(dead_code)]
const TX_STATUS_TOK: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Buffer sizes
// ---------------------------------------------------------------------------

/// RX buffer size: 8K + 16 bytes header + 1500 bytes for packet wrapping.
const RX_BUF_SIZE: usize = 8192 + 16 + 1500;

/// TX buffer size per descriptor (must hold a full Ethernet frame).
const TX_BUF_SIZE: usize = 1536;

/// Number of TX descriptors.
const NUM_TX_DESC: usize = 4;

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

/// The RTL8139 device state.
pub struct Rtl8139Device {
    /// I/O port base address.
    io_base: u16,
    /// MAC address.
    mac: [u8; 6],
    /// Physical address of the RX buffer.
    rx_buf_phys: u64,
    /// Virtual address of the RX buffer.
    rx_buf_virt: u64,
    /// Current read offset into the RX buffer.
    rx_offset: usize,
    /// Physical address of TX buffers (4 × 1536 bytes).
    tx_buf_phys: [u64; NUM_TX_DESC],
    /// Virtual address of TX buffers.
    tx_buf_virt: [u64; NUM_TX_DESC],
    /// Which TX descriptor to use next (0-3).
    tx_cur: usize,
    /// Physical frame backing the RX buffer.
    _rx_frame: Option<PhysFrame>,
    /// Physical frame backing the TX buffers.
    _tx_frame: Option<PhysFrame>,
}

/// Global device instance.
static DEVICE: Mutex<Option<Rtl8139Device>> = Mutex::new(None);

/// IRQ line used by this device (set during init).
static IRQ_LINE: AtomicU8 = AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the RTL8139 driver if the hardware is present.
///
/// Scans PCI for a Realtek RTL8139 NIC.  If found, resets it,
/// configures RX/TX buffers, reads the MAC address, and enables
/// the receiver and transmitter.
pub fn init(hhdm_offset: u64) {
    let pci_dev = match find_rtl8139() {
        Some(d) => d,
        None => {
            crate::serial_println!("[rtl8139] No Realtek RTL8139 NIC found");
            return;
        }
    };

    crate::serial_println!(
        "[rtl8139] Found RTL8139 at {:?} (IRQ {})",
        pci_dev.address,
        pci_dev.irq_line
    );

    let io_base = match pci_dev.bar0_io_port() {
        Some(port) => port,
        None => {
            crate::serial_println!("[rtl8139] ERROR: BAR0 is not I/O space");
            return;
        }
    };

    // Enable bus mastering for DMA.
    pci::enable_bus_master(pci_dev.address);

    // Power on the device (write 0x00 to Config1).
    // SAFETY: Standard RTL8139 register write.
    unsafe {
        port::outb(io_base + REG_CONFIG1, 0x00);
    }

    // Software reset.
    // SAFETY: Standard reset command.
    unsafe {
        port::outb(io_base + REG_CMD, CMD_RESET);
    }
    // Wait for reset to complete (bit 4 clears when done).
    for _ in 0..10_000u32 {
        // SAFETY: Reading command register is safe.
        let cmd = unsafe { port::inb(io_base + REG_CMD) };
        if cmd & CMD_RESET == 0 {
            break;
        }
    }

    // Read MAC address from registers 0x00-0x05.
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        // SAFETY: Reading MAC registers.
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            *byte = port::inb(io_base + REG_MAC + i as u16);
        }
    }

    crate::serial_println!(
        "[rtl8139] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );

    // Allocate RX buffer (one 16 KiB frame gives us enough for the 8K+16+1500 buffer).
    let rx_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(_) => {
            crate::serial_println!("[rtl8139] ERROR: cannot allocate RX buffer frame");
            return;
        }
    };
    let rx_buf_phys = rx_frame.addr();
    let rx_buf_virt = rx_buf_phys + hhdm_offset;

    // Zero the RX buffer.
    // SAFETY: We just allocated this frame and have exclusive access.
    unsafe {
        core::ptr::write_bytes(rx_buf_virt as *mut u8, 0, FRAME_SIZE);
    }

    // Allocate TX buffers (one frame is 16 KiB = enough for 4 × 1536 = 6144 bytes).
    let tx_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(_) => {
            // SAFETY: We just allocated rx_frame and have not shared it.
            unsafe { let _ = frame::free_frame(rx_frame); }
            crate::serial_println!("[rtl8139] ERROR: cannot allocate TX buffer frame");
            return;
        }
    };
    let tx_base_phys = tx_frame.addr();
    let tx_base_virt = tx_base_phys + hhdm_offset;

    // Zero TX buffers.
    // SAFETY: Just allocated, exclusive access.
    unsafe {
        core::ptr::write_bytes(tx_base_virt as *mut u8, 0, FRAME_SIZE);
    }

    // Compute per-descriptor TX buffer addresses.
    let mut tx_buf_phys = [0u64; NUM_TX_DESC];
    let mut tx_buf_virt = [0u64; NUM_TX_DESC];
    for i in 0..NUM_TX_DESC {
        tx_buf_phys[i] = tx_base_phys + (i * TX_BUF_SIZE) as u64;
        tx_buf_virt[i] = tx_base_virt + (i * TX_BUF_SIZE) as u64;
    }

    // Tell hardware the RX buffer physical address.
    // SAFETY: Standard RTL8139 register write.
    #[allow(clippy::cast_possible_truncation)]
    unsafe {
        port::outl(io_base + REG_RX_BUF, rx_buf_phys as u32);
    }

    // Configure interrupt mask: only RX OK for now.
    // SAFETY: Standard register write.
    unsafe {
        port::outw(io_base + REG_IMR, INT_ROK);
    }

    // Configure RX: accept broadcast + multicast + physical match,
    // 8K buffer, wrap mode.
    let rx_config = RX_CFG_APM | RX_CFG_AM | RX_CFG_AB | RX_CFG_WRAP | RX_CFG_8K;
    // SAFETY: Standard register write.
    unsafe {
        port::outl(io_base + REG_RX_CONFIG, rx_config);
    }

    // Configure TX: default (IFG = 960ns, DMA burst = 1024 bytes).
    // SAFETY: Standard register write.
    unsafe {
        port::outl(io_base + REG_TX_CONFIG, 0x0300_0000);
    }

    // Enable receiver and transmitter.
    // SAFETY: Standard command.
    unsafe {
        port::outb(io_base + REG_CMD, CMD_RX_ENABLE | CMD_TX_ENABLE);
    }

    // Store IRQ line for later use.
    IRQ_LINE.store(pci_dev.irq_line, Ordering::Release);

    // Store device state.
    let device = Rtl8139Device {
        io_base,
        mac,
        rx_buf_phys,
        rx_buf_virt,
        rx_offset: 0,
        tx_buf_phys,
        tx_buf_virt,
        tx_cur: 0,
        _rx_frame: Some(rx_frame),
        _tx_frame: Some(tx_frame),
    };

    *DEVICE.lock() = Some(device);

    crate::serial_println!("[rtl8139] RTL8139 initialized (io_base={:#06x}, IRQ {})",
        io_base, pci_dev.irq_line);
}

/// Access the device through a closure (same pattern as e1000/virtio-net).
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Rtl8139Device) -> R,
{
    let mut guard = DEVICE.lock();
    guard.as_mut().map(f)
}

/// Send a raw Ethernet frame.
pub fn send(frame: &[u8]) -> KernelResult<()> {
    with_device(|dev| dev.send(frame))
        .unwrap_or(Err(KernelError::NotFound))
}

/// Receive a raw Ethernet frame (returns None if no packet available).
pub fn recv() -> Option<Vec<u8>> {
    with_device(|dev| dev.recv()).flatten()
}

// ---------------------------------------------------------------------------
// Device methods
// ---------------------------------------------------------------------------

impl Rtl8139Device {
    /// Send a raw Ethernet frame via the next available TX descriptor.
    pub fn send(&mut self, frame: &[u8]) -> KernelResult<()> {
        if frame.len() > TX_BUF_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let desc = self.tx_cur;

        // Wait for the descriptor to become available (OWN bit clear
        // means hardware finished with it).
        let status_reg = REG_TX_STATUS0 + (desc as u16) * 4;
        for _ in 0..100_000u32 {
            // SAFETY: Reading TX status register.
            let status = unsafe { port::inl(self.io_base + status_reg) };
            if status & TX_STATUS_OWN == 0 {
                break;
            }
        }

        // Copy frame data to the TX buffer.
        let dst = self.tx_buf_virt[desc] as *mut u8;
        // SAFETY: We own this buffer, length is bounded.
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), dst, frame.len());
        }

        // Set the TX start address for this descriptor.
        let addr_reg = REG_TX_ADDR0 + (desc as u16) * 4;
        #[allow(clippy::cast_possible_truncation)]
        // SAFETY: Standard register writes.
        unsafe {
            port::outl(self.io_base + addr_reg, self.tx_buf_phys[desc] as u32);
        }

        // Write the TX status: size in bits [12:0], clear OWN (bit 13),
        // set threshold to 8 (bits [16:21] = 0, so early TX threshold).
        #[allow(clippy::cast_possible_truncation)]
        let size = frame.len() as u32;
        // SAFETY: Initiates transmission.
        unsafe {
            port::outl(self.io_base + status_reg, size);
        }

        // Advance to next descriptor.
        self.tx_cur = (self.tx_cur + 1) % NUM_TX_DESC;

        Ok(())
    }

    /// Advance the RX read pointer past an entry of `raw_length` bytes and
    /// update the hardware CAPR register.
    ///
    /// The RTL8139 ring layout is:  `[status:u16][length:u16][payload…]`
    /// Each entry is 4-byte aligned.  Based on Linux `8139too.c` recovery logic.
    fn rx_advance(&mut self, raw_length: u16) {
        /// Wrap boundary: the actual ring is 8 KiB + 16 bytes = 8208 bytes.
        /// The remaining 1500 bytes are a guard zone for hardware wrap-around.
        const WRAP: usize = RX_BUF_SIZE - 1500;

        // 4-byte header + payload, rounded up to 4-byte alignment.
        let next = (self.rx_offset + 4 + raw_length as usize + 3) & !3;
        self.rx_offset = next % WRAP;

        // Update CAPR (Current Address of Packet Read).
        // Hardware expects CAPR = offset − 0x10 (16-byte bias).
        #[allow(clippy::cast_possible_truncation)]
        let capr_val = (self.rx_offset as u16).wrapping_sub(0x10);
        // SAFETY: Standard register write to update read pointer.
        unsafe {
            port::outw(self.io_base + REG_CAPR, capr_val);
        }
    }

    /// Try to receive a frame from the RX ring buffer.
    ///
    /// Returns `None` if no complete packet is available.
    pub fn recv(&mut self) -> Option<Vec<u8>> {
        /// Wrap boundary: the actual ring is 8 KiB + 16 bytes = 8208 bytes.
        const WRAP: usize = RX_BUF_SIZE - 1500;

        // Check if RX buffer is empty (CMD register bit 0 = BUFE).
        // SAFETY: Reading command register.
        let cmd = unsafe { port::inb(self.io_base + REG_CMD) };
        if cmd & 0x01 != 0 {
            return None;
        }

        // Defensive: clamp rx_offset in case prior logic left it corrupt.
        if self.rx_offset >= WRAP {
            self.rx_offset = 0;
        }

        let base = self.rx_buf_virt as *const u8;

        // Read the 4-byte packet header at the current offset.
        // Header format: [status: u16le] [length: u16le] [packet data…]
        //
        // SAFETY: rx_offset < 8208 (clamped above), buffer is 9708 bytes, so
        // rx_offset + 4 ≤ 8211 < 9708.  The 1500-byte guard zone at the end
        // contains hardware-written wrap-around data, making unaligned header
        // reads across the 8208-byte boundary safe.
        let (status, length) = unsafe {
            let ptr = base.add(self.rx_offset);
            let status = u16::from_le(core::ptr::read_unaligned(ptr as *const u16));
            let length = u16::from_le(core::ptr::read_unaligned(ptr.add(2) as *const u16));
            (status, length)
        };

        // Check if packet is valid (bit 0 of status = ROK).
        if status & 0x0001 == 0 {
            // Bad packet — advance past it so we don't get stuck re-reading
            // the same bad header forever.  If the length field looks plausible,
            // use it; otherwise skip just the 4-byte header (minimum advance).
            let skip = if length > 0 && length <= 1518 { length } else { 0 };
            self.rx_advance(skip);
            return None;
        }

        // Length includes the 4-byte CRC appended by hardware.
        let pkt_len = (length as usize).saturating_sub(4);
        if pkt_len == 0 || pkt_len > 1514 {
            // Invalid length — advance past the entry to avoid getting stuck.
            self.rx_advance(length);
            return None;
        }

        // Copy packet data (starts 4 bytes past the header).
        let data_offset = self.rx_offset + 4;
        let mut packet = vec![0u8; pkt_len];

        // The buffer is circular; wrap indices at the 8208-byte boundary.
        for i in 0..pkt_len {
            let buf_idx = (data_offset + i) % WRAP;
            // SAFETY: buf_idx < 8208 < 9708 (total allocated buffer).
            unsafe {
                packet[i] = *base.add(buf_idx);
            }
        }

        // Advance past this packet and notify hardware.
        self.rx_advance(length);

        Some(packet)
    }

    /// Return the MAC address.
    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Check if the link is up by reading the Basic Mode Status Register
    /// (accessible via MII registers at offset 0x58/0x5A).
    #[allow(dead_code)]
    pub fn link_up(&self) -> bool {
        // Read BMSR via the MII access registers.
        // The RTL8139 provides direct register access for link status.
        // Bit 2 of the MSR (Media Status Register at offset 0x58) indicates link.
        // SAFETY: Reading status register.
        let msr = unsafe { port::inb(self.io_base + 0x58) };
        // Bit 2 = 0 means link is up (active low).
        (msr & 0x04) == 0
    }
}

// ---------------------------------------------------------------------------
// IRQ handling
// ---------------------------------------------------------------------------

/// Handle an IRQ from the RTL8139.
///
/// Called from the IOAPIC handler when the RTL8139's IRQ fires.
/// Acknowledges the interrupt by reading and writing back the ISR.
pub fn handle_irq(irq: u32) {
    let expected = IRQ_LINE.load(Ordering::Acquire);
    if expected == 0 || irq != u32::from(expected) {
        return;
    }

    let guard = DEVICE.lock();
    if let Some(ref dev) = *guard {
        // Read ISR to determine interrupt source and acknowledge.
        // SAFETY: Standard register read/write.
        unsafe {
            let isr = port::inw(dev.io_base + REG_ISR);
            // Write back to clear the bits.
            port::outw(dev.io_base + REG_ISR, isr);
        }
    }
}

// ---------------------------------------------------------------------------
// PCI scanning
// ---------------------------------------------------------------------------

/// Scan PCI for a Realtek RTL8139 NIC.
fn find_rtl8139() -> Option<pci::PciDevice> {
    for &dev_id in RTL8139_DEVICE_IDS {
        if let Some(dev) = pci::find_device(REALTEK_VENDOR, dev_id) {
            return Some(dev);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify driver initialization.
pub fn self_test() {
    let guard = DEVICE.lock();
    if guard.is_none() {
        crate::serial_println!("[rtl8139] Self-test: no device (skipped)");
        return;
    }
    let dev = guard.as_ref().unwrap();

    // Verify MAC is not all-zeros or all-ones.
    let all_zero = dev.mac.iter().all(|&b| b == 0);
    let all_ones = dev.mac.iter().all(|&b| b == 0xFF);
    if all_zero || all_ones {
        crate::serial_println!("[rtl8139] Self-test FAILED: invalid MAC address");
        return;
    }

    // Verify TX/RX buffers are valid.
    if dev.rx_buf_phys == 0 || dev.tx_buf_phys[0] == 0 {
        crate::serial_println!("[rtl8139] Self-test FAILED: buffer addresses are zero");
        return;
    }

    crate::serial_println!("[rtl8139] Self-test PASSED (MAC={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
        dev.mac[0], dev.mac[1], dev.mac[2], dev.mac[3], dev.mac[4], dev.mac[5]);
}
