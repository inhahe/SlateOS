//! Virtio network device driver.
//!
//! Provides raw Ethernet frame send/receive via a virtio-net device.
//! Uses the legacy PCI transport (I/O port BAR0) with separate
//! RX and TX virtqueues.
//!
//! ## Protocol
//!
//! Each packet consists of a 2-descriptor chain:
//! 1. Virtio-net header (10 bytes) — flags, offload info (all zero for MVP)
//! 2. Ethernet frame data (up to 1514 bytes for standard frames)
//!
//! ## RX flow
//!
//! Pre-populate the RX queue with empty buffers.  The device fills
//! them when packets arrive.  Poll the used ring to retrieve received
//! frames.  Recycle descriptors back to the available ring.
//!
//! ## TX flow
//!
//! Build a header+frame descriptor chain, submit to the TX queue,
//! notify, and poll for completion.

use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::pci::{self, PciDevice};
use crate::virtio::queue::{Virtqueue, VRING_DESC_F_WRITE};
use crate::virtio::{
    VirtioLegacyPci, STATUS_ACKNOWLEDGE, STATUS_DRIVER, STATUS_DRIVER_OK,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Virtio vendor ID (Red Hat).
const VIRTIO_VENDOR: u16 = 0x1AF4;
/// Legacy virtio-net device ID.
const VIRTIO_NET_DEVICE: u16 = 0x1000;

/// Size of the virtio-net header (without mergeable rx buffers).
const NET_HDR_SIZE: usize = 10;

/// Maximum Ethernet frame size (without VLAN tag).
const MAX_FRAME_SIZE: usize = 1514;

/// Size of each RX buffer (header + max frame + padding).
const RX_BUF_SIZE: usize = NET_HDR_SIZE + MAX_FRAME_SIZE + 2; // 1526, round up in DMA

/// Number of pre-allocated RX buffers.
const NUM_RX_BUFS: usize = 16;

// DMA layout:
// TX: header at offset 0, data at offset 16 within dma_frame.
// RX: buffers laid out sequentially from offset 0 within rx_frame.
const DMA_TX_HEADER_OFFSET: usize = 0;          // 10 bytes
const DMA_TX_DATA_OFFSET: usize = 16;           // Up to 1514 bytes

// ---------------------------------------------------------------------------
// Virtio-net header
// ---------------------------------------------------------------------------

/// Virtio-net packet header (10 bytes, per virtio 0.9.5 spec).
#[repr(C)]
struct VirtioNetHeader {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
}

// ---------------------------------------------------------------------------
// Network device
// ---------------------------------------------------------------------------

/// MAC address (6 bytes).
#[derive(Debug, Clone, Copy)]
pub struct MacAddress(pub [u8; 6]);

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

/// A virtio network device instance.
pub struct VirtioNetDevice {
    /// Legacy PCI transport.
    transport: VirtioLegacyPci,
    /// Receive virtqueue (queue 0).
    rx_queue: Virtqueue,
    /// Transmit virtqueue (queue 1).
    tx_queue: Virtqueue,
    /// Device MAC address.
    mac: MacAddress,
    /// HHDM offset for physical ↔ virtual translation.
    hhdm_offset: u64,
    /// Physical frame for DMA buffers (TX header/data).
    dma_frame: PhysFrame,
    /// Virtual address of the DMA frame.
    dma_virt: *mut u8,
    /// Physical frame for RX buffers.
    rx_frame: PhysFrame,
    /// Virtual address of the RX frame.
    rx_virt: *mut u8,
    /// Number of outstanding RX descriptors.
    rx_pending: u16,
}

// SAFETY: The device is accessed from a single thread.
// DMA buffers are pinned and not shared.
unsafe impl Send for VirtioNetDevice {}

impl VirtioNetDevice {
    /// Initialize a virtio-net device from a PCI device descriptor.
    // Complex initialization with many register accesses.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn init(pci_dev: &PciDevice, hhdm_offset: u64) -> KernelResult<Self> {
        let io_base = pci_dev.bar0_io_port().ok_or(KernelError::NoSuchDevice)?;
        crate::serial_println!("[virtio-net] BAR0 I/O port base: {:#x}", io_base);

        // Enable bus mastering for DMA.
        pci::enable_bus_master(pci_dev.address);

        let transport = VirtioLegacyPci::new(io_base);

        // 1. Reset.
        transport.reset();

        // 2. ACKNOWLEDGE.
        transport.set_status(STATUS_ACKNOWLEDGE);

        // 3. DRIVER.
        transport.set_status(STATUS_DRIVER);

        // 4. Feature negotiation — accept MAC feature only.
        let features = transport.device_features();
        crate::serial_println!("[virtio-net] Device features: {:#010x}", features);
        // Accept VIRTIO_NET_F_MAC (bit 5) if offered.
        let guest_features = features & (1 << 5);
        transport.set_guest_features(guest_features);

        // 5. Set up RX queue (queue 0).
        transport.select_queue(0);
        let rx_queue_size = transport.queue_size();
        crate::serial_println!("[virtio-net] RX queue size: {}", rx_queue_size);
        if rx_queue_size == 0 {
            transport.set_status(crate::virtio::STATUS_FAILED);
            return Err(KernelError::NoSuchDevice);
        }
        let (rx_queue, rx_pfn) = Virtqueue::new(rx_queue_size, hhdm_offset)?;
        transport.set_queue_pfn(rx_pfn);

        // 6. Set up TX queue (queue 1).
        transport.select_queue(1);
        let tx_queue_size = transport.queue_size();
        crate::serial_println!("[virtio-net] TX queue size: {}", tx_queue_size);
        if tx_queue_size == 0 {
            transport.set_status(crate::virtio::STATUS_FAILED);
            return Err(KernelError::NoSuchDevice);
        }
        let (tx_queue, tx_pfn) = Virtqueue::new(tx_queue_size, hhdm_offset)?;
        transport.set_queue_pfn(tx_pfn);

        // 7. DRIVER_OK.
        transport.set_status(STATUS_DRIVER_OK);

        // 8. Read MAC address from device config (6 bytes at offset 0).
        let mut mac_bytes = [0u8; 6];
        for (i, byte) in mac_bytes.iter_mut().enumerate() {
            *byte = transport.read_device_config8(i as u16);
        }
        let mac = MacAddress(mac_bytes);
        crate::serial_println!("[virtio-net] MAC address: {}", mac);

        // 9. Allocate DMA frames.
        let dma_frame = frame::alloc_frame()?;
        let dma_virt = (dma_frame.addr() + hhdm_offset) as *mut u8;
        // SAFETY: We own this frame; HHDM maps it writable.
        unsafe { core::ptr::write_bytes(dma_virt, 0, frame::FRAME_SIZE); }

        let rx_frame = frame::alloc_frame()?;
        let rx_virt = (rx_frame.addr() + hhdm_offset) as *mut u8;
        unsafe { core::ptr::write_bytes(rx_virt, 0, frame::FRAME_SIZE); }

        let mut dev = Self {
            transport,
            rx_queue,
            tx_queue,
            mac,
            hhdm_offset,
            dma_frame,
            dma_virt,
            rx_frame,
            rx_virt,
            rx_pending: 0,
        };

        // 10. Pre-populate the RX queue with empty buffers.
        dev.refill_rx();

        Ok(dev)
    }

    /// Return the device's MAC address.
    pub fn mac(&self) -> MacAddress {
        self.mac
    }

    /// Pre-populate the RX queue with empty buffers.
    ///
    /// Each RX descriptor is a 2-descriptor chain:
    /// 1. Header buffer (NET_HDR_SIZE bytes, device-writable)
    /// 2. Frame data buffer (MAX_FRAME_SIZE bytes, device-writable)
    #[allow(clippy::arithmetic_side_effects)]
    fn refill_rx(&mut self) {
        let rx_phys_base = self.rx_frame.addr();
        let max_bufs = NUM_RX_BUFS.min(frame::FRAME_SIZE / RX_BUF_SIZE);

        for i in 0..max_bufs {
            if self.rx_pending >= self.rx_queue.queue_size() {
                break; // Queue is full.
            }

            let buf_offset = i * RX_BUF_SIZE;
            let header_phys = rx_phys_base + buf_offset as u64;
            let data_phys = header_phys + NET_HDR_SIZE as u64;

            let chain = [
                // Header: device-writable, receives the virtio-net header.
                (header_phys, NET_HDR_SIZE as u32, VRING_DESC_F_WRITE),
                // Data: device-writable, receives the Ethernet frame.
                (data_phys, MAX_FRAME_SIZE as u32, VRING_DESC_F_WRITE),
            ];

            match self.rx_queue.submit(&chain) {
                Ok(_head) => {
                    self.rx_pending = self.rx_pending.wrapping_add(1);
                }
                Err(_) => break,
            }
        }

        // Notify the device that RX buffers are available.
        if self.rx_pending > 0 {
            self.transport.notify_queue(0);
        }

        crate::serial_println!(
            "[virtio-net] RX queue populated with {} buffers",
            self.rx_pending
        );
    }

    /// Poll for received packets.
    ///
    /// Returns `Some((frame_data, length))` if a packet was received,
    /// `None` if no packets are pending.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn recv(&mut self) -> Option<Vec<u8>> {
        let used = self.rx_queue.poll_used()?;
        let (head_idx, total_len) = used;

        // Free the descriptor chain.
        self.rx_queue.free_chain(head_idx);
        self.rx_pending = self.rx_pending.wrapping_sub(1);

        // The total_len includes the virtio-net header.
        let frame_len = (total_len as usize).saturating_sub(NET_HDR_SIZE);
        if frame_len == 0 {
            // Refill and try again.
            self.refill_rx_one();
            return None;
        }

        // Find which RX buffer this was.
        // For simplicity, since we pre-populated sequentially, the
        // head_idx maps to a buffer slot.  However, the device may
        // return them in any order.  We need to read from the right
        // buffer based on the descriptor's address.
        //
        // Since we used sequential descriptors, head_idx / 2 gives
        // the buffer index (each chain is 2 descriptors).
        let buf_idx = (head_idx as usize) / 2;
        let buf_offset = buf_idx * RX_BUF_SIZE + NET_HDR_SIZE;

        let mut frame = Vec::with_capacity(frame_len);
        // SAFETY: rx_virt points to our allocated RX frame; buf_offset
        // and frame_len are bounded by the frame size.
        unsafe {
            let src = self.rx_virt.add(buf_offset);
            for i in 0..frame_len {
                frame.push(core::ptr::read_volatile(src.add(i)));
            }
        }

        // Refill the slot.
        self.refill_rx_one();

        Some(frame)
    }

    /// Refill one RX buffer slot.
    #[allow(clippy::arithmetic_side_effects)]
    fn refill_rx_one(&mut self) {
        let rx_phys_base = self.rx_frame.addr();
        let max_bufs = NUM_RX_BUFS.min(frame::FRAME_SIZE / RX_BUF_SIZE);

        // Find a free slot (just use a simple sequential approach).
        if self.rx_pending >= max_bufs as u16 {
            return;
        }

        let i = self.rx_pending as usize;
        let buf_offset = i * RX_BUF_SIZE;
        let header_phys = rx_phys_base + buf_offset as u64;
        let data_phys = header_phys + NET_HDR_SIZE as u64;

        let chain = [
            (header_phys, NET_HDR_SIZE as u32, VRING_DESC_F_WRITE),
            (data_phys, MAX_FRAME_SIZE as u32, VRING_DESC_F_WRITE),
        ];

        if self.rx_queue.submit(&chain).is_ok() {
            self.rx_pending = self.rx_pending.wrapping_add(1);
            self.transport.notify_queue(0);
        }
    }

    /// Send an Ethernet frame.
    ///
    /// `frame` should be a complete Ethernet frame (dest MAC, src MAC,
    /// ethertype, payload).  The virtio-net header is prepended
    /// automatically (all zeros for the MVP — no offloads).
    #[allow(clippy::arithmetic_side_effects)]
    pub fn send(&mut self, frame: &[u8]) -> KernelResult<()> {
        if frame.len() > MAX_FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let dma_phys = self.dma_frame.addr();

        // Write the virtio-net header (all zeros).
        let header_ptr = self.dma_virt as *mut VirtioNetHeader;
        // SAFETY: dma_virt points to our DMA frame, header fits.
        unsafe {
            core::ptr::write_volatile(header_ptr, VirtioNetHeader {
                flags: 0,
                gso_type: 0,
                hdr_len: 0,
                gso_size: 0,
                csum_start: 0,
                csum_offset: 0,
            });
        }

        // Copy the frame data to the DMA buffer.
        // SAFETY: DMA_TX_DATA_OFFSET + frame.len() is within the frame.
        unsafe {
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.dma_virt.add(DMA_TX_DATA_OFFSET),
                frame.len(),
            );
        }

        // Build the descriptor chain.
        let header_phys = dma_phys + DMA_TX_HEADER_OFFSET as u64;
        let data_phys = dma_phys + DMA_TX_DATA_OFFSET as u64;

        let chain = [
            // Header: device-readable.
            (header_phys, NET_HDR_SIZE as u32, 0u16),
            // Frame data: device-readable.
            (data_phys, frame.len() as u32, 0u16),
        ];

        let head = self.tx_queue.submit(&chain)?;

        // Notify the device.
        self.transport.notify_queue(1);

        // Poll for completion.
        let mut spins = 0u32;
        loop {
            if let Some((completed_head, _len)) = self.tx_queue.poll_used() {
                self.tx_queue.free_chain(completed_head);
                return Ok(());
            }

            spins = spins.wrapping_add(1);
            if spins > 1_000_000 {
                crate::serial_println!("[virtio-net] TX timed out");
                self.tx_queue.free_chain(head);
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }
    }

    /// Return the number of packets waiting to be received.
    pub fn rx_pending(&self) -> u16 {
        self.rx_pending
    }
}

impl Drop for VirtioNetDevice {
    fn drop(&mut self) {
        self.transport.reset();

        // SAFETY: We own these frames.
        if let Err(e) = unsafe { frame::free_frame(self.dma_frame) } {
            crate::serial_println!(
                "[virtio-net] WARNING: failed to free DMA frame: {:?}", e
            );
        }
        if let Err(e) = unsafe { frame::free_frame(self.rx_frame) } {
            crate::serial_println!(
                "[virtio-net] WARNING: failed to free RX frame: {:?}", e
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Global device instance
// ---------------------------------------------------------------------------

/// The global virtio-net device (if present).
static DEVICE: Mutex<Option<VirtioNetDevice>> = Mutex::new(None);

/// Execute a closure with the global network device, if present.
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut VirtioNetDevice) -> R,
{
    let mut guard = DEVICE.lock();
    guard.as_mut().map(f)
}

// ---------------------------------------------------------------------------
// Discovery and initialization
// ---------------------------------------------------------------------------

/// Find and initialize a virtio-net device on the PCI bus.
pub fn probe(hhdm_offset: u64) -> Option<VirtioNetDevice> {
    let pci_dev = pci::find_device(VIRTIO_VENDOR, VIRTIO_NET_DEVICE)?;
    crate::serial_println!(
        "[virtio-net] Found device at {:02x}:{:02x}.{} (irq={})",
        pci_dev.address.bus,
        pci_dev.address.device,
        pci_dev.address.function,
        pci_dev.irq_line,
    );

    match VirtioNetDevice::init(&pci_dev, hhdm_offset) {
        Ok(dev) => {
            crate::serial_println!("[virtio-net] Device initialized successfully");
            Some(dev)
        }
        Err(e) => {
            crate::serial_println!("[virtio-net] Init failed: {:?}", e);
            None
        }
    }
}

/// Initialize the virtio-net subsystem.
pub fn init(hhdm_offset: u64) {
    if let Some(dev) = probe(hhdm_offset) {
        crate::serial_println!(
            "[virtio-net] MAC: {}, RX buffers: {}",
            dev.mac(),
            dev.rx_pending()
        );
        *DEVICE.lock() = Some(dev);
    } else {
        crate::serial_println!("[virtio-net] No device found (non-fatal)");
    }
}

/// Self-test: verify device is initialized and can be queried.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[virtio-net] Running self-test...");

    let result = with_device(|dev| {
        crate::serial_println!("[virtio-net]   MAC: {}", dev.mac());
        crate::serial_println!("[virtio-net]   RX pending: {}", dev.rx_pending());
    });

    if result.is_none() {
        crate::serial_println!("[virtio-net] Self-test SKIPPED (no device)");
        return Ok(());
    }

    crate::serial_println!("[virtio-net] Self-test PASSED");
    Ok(())
}
