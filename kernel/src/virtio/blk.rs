//! Virtio block device driver.
//!
//! Provides synchronous sector-level read/write to a virtio-blk disk.
//! Uses the legacy PCI transport (I/O port BAR0) and a single
//! virtqueue with polling completion.
//!
//! ## Protocol
//!
//! Each request consists of a 3-descriptor chain:
//! 1. Header (device-readable): type, reserved, sector number
//! 2. Data buffer (device-readable for write, device-writable for read)
//! 3. Status byte (device-writable): 0=OK, 1=IOERR, 2=UNSUPP
//!
//! ## DMA buffers
//!
//! The header, data, and status are laid out in a single 16 KiB frame
//! at known offsets.  Physical addresses are passed to the device for
//! DMA; virtual addresses (via HHDM) are used by the driver to write
//! headers and read status.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::pci::{self, PciDevice};
use spin::Mutex;
use crate::virtio::queue::{Virtqueue, VRING_DESC_F_WRITE};
use crate::virtio::{
    VirtioLegacyPci, STATUS_ACKNOWLEDGE, STATUS_DRIVER, STATUS_DRIVER_OK,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Virtio vendor ID (Red Hat).
const VIRTIO_VENDOR: u16 = 0x1AF4;
/// Legacy virtio-blk device ID.
const VIRTIO_BLK_DEVICE: u16 = 0x1001;

/// Read operation.
const VIRTIO_BLK_T_IN: u32 = 0;
/// Write operation.
const VIRTIO_BLK_T_OUT: u32 = 1;

/// Sector size in bytes.
pub const SECTOR_SIZE: usize = 512;

/// Status: success.
const VIRTIO_BLK_S_OK: u8 = 0;

// DMA buffer offsets within the request frame.
const DMA_HEADER_OFFSET: usize = 0;           // 16 bytes
const DMA_DATA_OFFSET: usize = 512;           // Up to 4096 bytes
const DMA_STATUS_OFFSET: usize = 512 + 4096;  // 1 byte

// ---------------------------------------------------------------------------
// Request header
// ---------------------------------------------------------------------------

/// Virtio block request header (16 bytes, device-readable).
#[repr(C)]
struct VirtioBlkReqHeader {
    type_: u32,
    reserved: u32,
    sector: u64,
}

// ---------------------------------------------------------------------------
// Block device
// ---------------------------------------------------------------------------

/// A virtio block device instance.
pub struct VirtioBlkDevice {
    /// Legacy PCI transport.
    transport: VirtioLegacyPci,
    /// The request virtqueue (queue 0).
    queue: Virtqueue,
    /// Disk capacity in 512-byte sectors.
    capacity: u64,
    /// HHDM offset for physical ↔ virtual translation.
    hhdm_offset: u64,
    /// The DMA request frame.
    dma_frame: PhysFrame,
    /// Virtual address of the DMA request frame.
    dma_virt: *mut u8,
}

// SAFETY: The device is accessed from a single thread (the shell).
// The DMA buffer is pinned and not shared.
unsafe impl Send for VirtioBlkDevice {}

impl VirtioBlkDevice {
    /// Initialize a virtio-blk device from a PCI device descriptor.
    ///
    /// Performs the full legacy virtio initialization sequence:
    /// reset → acknowledge → driver → features → queue setup → driver_ok.
    // Arithmetic on device config values, PFN computation, etc.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn init(pci_dev: &PciDevice, hhdm_offset: u64) -> KernelResult<Self> {
        // Get BAR0 as an I/O port.
        let io_base = pci_dev.bar0_io_port().ok_or(KernelError::NoSuchDevice)?;
        crate::serial_println!("[virtio-blk] BAR0 I/O port base: {:#x}", io_base);

        // Enable bus mastering for DMA.
        pci::enable_bus_master(pci_dev.address);

        let transport = VirtioLegacyPci::new(io_base);

        // 1. Reset device.
        transport.reset();

        // 2. Set ACKNOWLEDGE.
        transport.set_status(STATUS_ACKNOWLEDGE);

        // 3. Set DRIVER.
        transport.set_status(STATUS_DRIVER);

        // 4. Feature negotiation.
        let features = transport.device_features();
        crate::serial_println!("[virtio-blk] Device features: {:#010x}", features);
        // Accept no optional features for the MVP.
        transport.set_guest_features(0);

        // 5. Set up virtqueue 0 (the request queue).
        transport.select_queue(0);
        let queue_size = transport.queue_size();
        crate::serial_println!("[virtio-blk] Queue 0 size: {}", queue_size);

        if queue_size == 0 {
            transport.set_status(crate::virtio::STATUS_FAILED);
            return Err(KernelError::NoSuchDevice);
        }

        let (queue, pfn) = Virtqueue::new(queue_size, hhdm_offset)?;
        transport.set_queue_pfn(pfn);
        crate::serial_println!("[virtio-blk] Queue PFN: {:#x} (phys {:#x})", pfn, u64::from(pfn) << 12);

        // 6. Set DRIVER_OK — device is live.
        transport.set_status(STATUS_DRIVER_OK);

        // Read device config: capacity (8 bytes at device config offset 0).
        let capacity = transport.read_device_config64(0);
        crate::serial_println!("[virtio-blk] Disk capacity: {} sectors ({} KiB)",
            capacity, capacity * 512 / 1024);

        // Allocate a DMA frame for request headers/data/status.
        let dma_frame = frame::alloc_frame()?;
        let dma_virt = (dma_frame.addr() + hhdm_offset) as *mut u8;
        // Zero the DMA frame.
        // SAFETY: We just allocated this frame; HHDM maps it writable.
        unsafe { core::ptr::write_bytes(dma_virt, 0, frame::FRAME_SIZE); }

        Ok(Self {
            transport,
            queue,
            capacity,
            hhdm_offset,
            dma_frame,
            dma_virt,
        })
    }

    /// Return the disk capacity in 512-byte sectors.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Read a single 512-byte sector.
    ///
    /// `buf` must be exactly 512 bytes.  The read is synchronous — this
    /// function blocks (polling) until the device completes the request.
    // DMA offset arithmetic uses known small constants.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        if sector >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }

        // Write the request header into the DMA frame.
        let header_ptr = self.dma_virt as *mut VirtioBlkReqHeader;
        unsafe {
            core::ptr::write_volatile(header_ptr, VirtioBlkReqHeader {
                type_: VIRTIO_BLK_T_IN,
                reserved: 0,
                sector,
            });
        }

        // Clear the status byte.
        unsafe {
            core::ptr::write_volatile(self.dma_virt.add(DMA_STATUS_OFFSET), 0xFF);
        }

        // Build the 3-descriptor chain.
        let dma_phys = self.dma_frame.addr();
        let header_phys = dma_phys + DMA_HEADER_OFFSET as u64;
        let data_phys = dma_phys + DMA_DATA_OFFSET as u64;
        let status_phys = dma_phys + DMA_STATUS_OFFSET as u64;

        let chain = [
            (header_phys, 16, 0u16),                           // Header: device-readable
            (data_phys, SECTOR_SIZE as u32, VRING_DESC_F_WRITE), // Data: device-writable
            (status_phys, 1, VRING_DESC_F_WRITE),               // Status: device-writable
        ];

        let head = self.queue.submit(&chain)?;

        // Notify the device.
        self.transport.notify_queue(0);

        // Poll for completion.
        let mut spins = 0u32;
        loop {
            if let Some((completed_head, _len)) = self.queue.poll_used() {
                // Free the descriptor chain.
                self.queue.free_chain(completed_head);

                // Check status.
                let status = unsafe {
                    core::ptr::read_volatile(self.dma_virt.add(DMA_STATUS_OFFSET))
                };

                if status != VIRTIO_BLK_S_OK {
                    crate::serial_println!(
                        "[virtio-blk] Read sector {} failed: status={}",
                        sector, status
                    );
                    return Err(KernelError::IoError);
                }

                // Copy data from DMA buffer to caller's buffer.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        self.dma_virt.add(DMA_DATA_OFFSET),
                        buf.as_mut_ptr(),
                        SECTOR_SIZE,
                    );
                }

                return Ok(());
            }

            // Prevent infinite spinning.
            spins = spins.wrapping_add(1);
            if spins > 1_000_000 {
                crate::serial_println!("[virtio-blk] Read sector {} timed out", sector);
                // Try to free the descriptor anyway.
                self.queue.free_chain(head);
                return Err(KernelError::TimedOut);
            }

            // Brief pause to avoid bus flooding.
            core::hint::spin_loop();
        }
    }

    /// Write a single 512-byte sector.
    // Same DMA arithmetic as read_sector.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn write_sector(&mut self, sector: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        if sector >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }

        // Write the request header.
        let header_ptr = self.dma_virt as *mut VirtioBlkReqHeader;
        unsafe {
            core::ptr::write_volatile(header_ptr, VirtioBlkReqHeader {
                type_: VIRTIO_BLK_T_OUT,
                reserved: 0,
                sector,
            });
        }

        // Copy data to the DMA buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf.as_ptr(),
                self.dma_virt.add(DMA_DATA_OFFSET),
                SECTOR_SIZE,
            );
        }

        // Clear status.
        unsafe {
            core::ptr::write_volatile(self.dma_virt.add(DMA_STATUS_OFFSET), 0xFF);
        }

        // Build the chain (data buffer is device-READABLE for writes).
        let dma_phys = self.dma_frame.addr();
        let header_phys = dma_phys + DMA_HEADER_OFFSET as u64;
        let data_phys = dma_phys + DMA_DATA_OFFSET as u64;
        let status_phys = dma_phys + DMA_STATUS_OFFSET as u64;

        let chain = [
            (header_phys, 16, 0u16),                           // Header: device-readable
            (data_phys, SECTOR_SIZE as u32, 0u16),              // Data: device-readable
            (status_phys, 1, VRING_DESC_F_WRITE),               // Status: device-writable
        ];

        let head = self.queue.submit(&chain)?;
        self.transport.notify_queue(0);

        // Poll for completion.
        let mut spins = 0u32;
        loop {
            if let Some((completed_head, _len)) = self.queue.poll_used() {
                self.queue.free_chain(completed_head);

                let status = unsafe {
                    core::ptr::read_volatile(self.dma_virt.add(DMA_STATUS_OFFSET))
                };

                if status != VIRTIO_BLK_S_OK {
                    crate::serial_println!(
                        "[virtio-blk] Write sector {} failed: status={}",
                        sector, status
                    );
                    return Err(KernelError::IoError);
                }

                return Ok(());
            }

            spins = spins.wrapping_add(1);
            if spins > 1_000_000 {
                crate::serial_println!("[virtio-blk] Write sector {} timed out", sector);
                self.queue.free_chain(head);
                return Err(KernelError::TimedOut);
            }

            core::hint::spin_loop();
        }
    }
}

impl Drop for VirtioBlkDevice {
    fn drop(&mut self) {
        // Reset the device.
        self.transport.reset();

        // Free the DMA frame.
        // SAFETY: We own this frame and are being dropped.
        if let Err(e) = unsafe { frame::free_frame(self.dma_frame) } {
            crate::serial_println!(
                "[virtio-blk] WARNING: failed to free DMA frame: {:?}",
                e
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery and initialization
// ---------------------------------------------------------------------------

/// Find and initialize a virtio-blk device on the PCI bus.
///
/// Returns `None` if no virtio-blk device is present.
pub fn probe(hhdm_offset: u64) -> Option<VirtioBlkDevice> {
    let pci_dev = pci::find_device(VIRTIO_VENDOR, VIRTIO_BLK_DEVICE)?;
    crate::serial_println!(
        "[virtio-blk] Found device at {:02x}:{:02x}.{} (irq={})",
        pci_dev.address.bus,
        pci_dev.address.device,
        pci_dev.address.function,
        pci_dev.irq_line,
    );

    match VirtioBlkDevice::init(&pci_dev, hhdm_offset) {
        Ok(dev) => {
            crate::serial_println!("[virtio-blk] Device initialized successfully");
            Some(dev)
        }
        Err(e) => {
            crate::serial_println!("[virtio-blk] Init failed: {:?}", e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Global device instance
// ---------------------------------------------------------------------------

/// The global virtio-blk device (if present).
static DEVICE: Mutex<Option<VirtioBlkDevice>> = Mutex::new(None);

/// Initialize the virtio-blk subsystem.
///
/// Probes for a virtio-blk device on the PCI bus.  If found,
/// initializes it, runs a self-test, and stores it globally.
pub fn init(hhdm_offset: u64) {
    if let Some(mut dev) = probe(hhdm_offset) {
        match self_test(&mut dev) {
            Ok(()) => {
                *DEVICE.lock() = Some(dev);
            }
            Err(e) => {
                crate::serial_println!(
                    "[virtio-blk] Self-test failed, device NOT stored: {:?}",
                    e
                );
            }
        }
    } else {
        crate::serial_println!("[virtio-blk] No device found (non-fatal)");
    }
}

/// Execute a closure with the global block device, if present.
///
/// Returns `None` if no device has been initialized.
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut VirtioBlkDevice) -> R,
{
    let mut guard = DEVICE.lock();
    guard.as_mut().map(f)
}

/// Self-test: read sector 0 and verify no error.
pub fn self_test(dev: &mut VirtioBlkDevice) -> KernelResult<()> {
    crate::serial_println!("[virtio-blk] Running self-test...");
    crate::serial_println!("[virtio-blk]   Capacity: {} sectors", dev.capacity());

    let mut buf = [0u8; SECTOR_SIZE];
    dev.read_sector(0, &mut buf)?;

    // Log first 16 bytes.
    crate::serial_print!("[virtio-blk]   Sector 0 (first 16 bytes):");
    for byte in &buf[..16] {
        crate::serial_print!(" {:02x}", byte);
    }
    crate::serial_println!();

    crate::serial_println!("[virtio-blk] Self-test PASSED");
    Ok(())
}
