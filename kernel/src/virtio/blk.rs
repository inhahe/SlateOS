//! Virtio block device driver.
//!
//! Provides synchronous sector-level read/write to a virtio-blk disk.
//! Uses the legacy PCI transport (I/O port BAR0) and a single
//! virtqueue with interrupt-driven completion.
//!
//! ## Protocol
//!
//! Each request consists of a 3-descriptor chain:
//! 1. Header (device-readable): type, reserved, sector number
//! 2. Data buffer (device-readable for write, device-writable for read)
//! 3. Status byte (device-writable): 0=OK, 1=IOERR, 2=UNSUPP
//!
//! ## Completion
//!
//! After submitting a request, the driver yields the CPU via `HLT`
//! and waits for the device to fire an IRQ.  The IOAPIC handler
//! acknowledges the device interrupt by reading the ISR status
//! register, then wakes the CPU from HLT.  The driver then checks
//! the used ring for the completion.  Falls back to polling if
//! interrupts are not yet configured (early boot).
//!
//! ## DMA buffers
//!
//! The header, data, and status are laid out in a single 16 KiB frame
//! at known offsets.  Physical addresses are passed to the device for
//! DMA; virtual addresses (via HHDM) are used by the driver to write
//! headers and read status.

use core::sync::atomic::{AtomicU8, AtomicU16, AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::pci::{self, PciDevice};
use spin::Mutex;
use crate::virtio::queue::{Virtqueue, VRING_DESC_F_WRITE};
use crate::virtio::{
    VirtioLegacyPci, REG_ISR_STATUS, STATUS_ACKNOWLEDGE, STATUS_DRIVER, STATUS_DRIVER_OK,
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

/// Polling-mode completion timeout, in spin iterations.
///
/// Only used during early boot before the IOAPIC is configured, where
/// the driver busy-waits for the device to complete a request.  Sized
/// generously so that a real device under heavy host load never spuriously
/// times out (the previous 1M budget — a few milliseconds — was far too
/// short under soak-test host contention and was the trigger for the
/// virtqueue-desync cascade documented in known-issues as
/// B-VIRTIO-BLK-WRITE-TIMEOUT), yet bounded so a genuinely dead device
/// does not hang the boot forever.
const POLL_TIMEOUT_SPINS: u32 = 100_000_000;

// DMA buffer offsets within the request frame.
const DMA_HEADER_OFFSET: usize = 0;           // 16 bytes
const DMA_DATA_OFFSET: usize = 512;           // Up to 4096 bytes
const DMA_STATUS_OFFSET: usize = 512 + 4096;  // 1 byte

// ---------------------------------------------------------------------------
// IRQ support — lock-free state for ISR context
// ---------------------------------------------------------------------------

/// I/O port base for the virtio-blk device, used by the ISR to
/// acknowledge interrupts by reading the ISR status register.
/// Set to 0 when no device is initialized.
static BLK_IO_BASE: AtomicU16 = AtomicU16::new(0);

/// PCI IRQ line for the virtio-blk device (from PCI config space).
/// 0xFF means no device or IRQ not assigned.
static BLK_IRQ_LINE: AtomicU8 = AtomicU8::new(0xFF);

/// Whether interrupt-driven I/O is active.  When false, the driver
/// falls back to polling (used during early boot before IOAPIC is up).
static IRQ_ENABLED: AtomicBool = AtomicBool::new(false);

/// Called from the IOAPIC device IRQ handler for every external
/// device interrupt.  Checks whether this IRQ matches the virtio-blk
/// device's PCI IRQ line, then reads the ISR status register to
/// acknowledge the interrupt at the device level (required for
/// level-triggered PCI interrupts to de-assert the IRQ line).
///
/// For non-matching IRQs, this function performs two atomic loads
/// (~1 ns) and returns immediately.  The actual I/O port read only
/// happens when the IRQ matches.
///
/// This function runs in ISR context — no locks, no allocations.
///
/// Returns `true` if this device actually had a pending interrupt.
pub fn handle_irq(irq: u32) -> bool {
    let expected = BLK_IRQ_LINE.load(Ordering::Relaxed);
    if expected == 0xFF || irq != u32::from(expected) {
        return false;
    }
    let io_base = BLK_IO_BASE.load(Ordering::Acquire);
    if io_base == 0 {
        return false;
    }
    // Read ISR status: acknowledges the interrupt at the device.
    // Bit 0 = used buffer notification, bit 1 = config change.
    // SAFETY: io_base is a valid virtio device I/O port, set during init.
    let isr = unsafe { crate::port::inb(io_base.wrapping_add(REG_ISR_STATUS)) };
    isr != 0
}

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
    #[allow(dead_code)]
    hhdm_offset: u64,
    /// The DMA request frame.
    dma_frame: PhysFrame,
    /// Virtual address of the DMA request frame.
    dma_virt: *mut u8,
    /// PCI IRQ line (0xFF if unknown/not assigned).
    #[allow(dead_code)]
    irq_line: u8,
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

        // Store the I/O base globally so the ISR can acknowledge
        // interrupts without holding a lock.
        BLK_IO_BASE.store(io_base, Ordering::Release);

        Ok(Self {
            transport,
            queue,
            capacity,
            hhdm_offset,
            dma_frame,
            dma_virt,
            irq_line: 0xFF, // Set later by enable_irq().
        })
    }

    /// Return the disk capacity in 512-byte sectors.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Enable interrupt-driven I/O on this specific device instance.
    ///
    /// Must be called after the IOAPIC is initialized and `cpu::sti()`.
    /// Before this, the driver falls back to busy-wait polling.
    ///
    /// Prefer [`enable_interrupts()`] (module-level) when the device
    /// has already been moved into the block device registry.
    #[allow(dead_code)]
    pub fn enable_irq(&mut self, irq_line: u8) {
        self.irq_line = irq_line;
        // SAFETY: The IRQ line is valid (from PCI config space) and the
        // IOAPIC is initialized by this point.
        // PCI interrupts are level-triggered, active-low.
        unsafe { crate::ioapic::set_level_triggered(irq_line); }
        unsafe { crate::ioapic::unmask_irq(irq_line); }
        IRQ_ENABLED.store(true, Ordering::Release);
        crate::serial_println!(
            "[virtio-blk] IRQ {} unmasked — interrupt-driven I/O enabled",
            irq_line,
        );
    }

    /// Wait for the device to complete a request.
    ///
    /// If interrupts are enabled, yields the CPU via `HLT` and waits for
    /// the device IRQ to fire.  Otherwise falls back to busy-wait polling
    /// (used during early boot before the IOAPIC is configured).
    ///
    /// Returns the completed descriptor head index, or an error on timeout.
    fn wait_completion(&mut self, head: u16, op: &str, sector: u64) -> KernelResult<u16> {
        if IRQ_ENABLED.load(Ordering::Acquire) {
            // Interrupt-driven: HLT until the device fires an IRQ.
            // The APIC timer also fires at 100 Hz, so we won't sleep
            // forever even if the device IRQ is lost.
            let mut attempts = 0u32;
            loop {
                if let Some(completed_head) = self.poll_matching(head, op, sector) {
                    return Ok(completed_head);
                }

                attempts = attempts.wrapping_add(1);
                // At 100 Hz timer, 500 HLTs ≈ 5 seconds — generous timeout.
                if attempts > 500 {
                    crate::serial_println!(
                        "[virtio-blk] {} sector {} timed out (IRQ mode)",
                        op, sector,
                    );
                    self.recover_after_timeout();
                    return Err(KernelError::TimedOut);
                }

                // Yield CPU until next interrupt (device IRQ or timer tick).
                crate::cpu::hlt();
            }
        } else {
            // Polling fallback for early boot (before IOAPIC init).
            let mut spins = 0u32;
            loop {
                if let Some(completed_head) = self.poll_matching(head, op, sector) {
                    return Ok(completed_head);
                }

                spins = spins.wrapping_add(1);
                if spins > POLL_TIMEOUT_SPINS {
                    crate::serial_println!(
                        "[virtio-blk] {} sector {} timed out (polling)",
                        op, sector,
                    );
                    self.recover_after_timeout();
                    return Err(KernelError::TimedOut);
                }

                core::hint::spin_loop();
            }
        }
    }

    /// Poll the used ring for the completion of *our* request (`head`).
    ///
    /// Because this is a single-outstanding driver sharing one DMA frame,
    /// only one request is ever in flight, so the head of any completion
    /// should equal `head`.  If a completion arrives for a *different*
    /// head, it is a stale completion from a previously-timed-out request
    /// that the device has finally returned (this should not happen after
    /// [`recover_after_timeout`] resets the device, but is handled
    /// defensively): its descriptors are reclaimed and polling continues.
    ///
    /// Returns `Some(head)` only when our own request has completed.
    fn poll_matching(&mut self, head: u16, op: &str, sector: u64) -> Option<u16> {
        while let Some((completed_head, _len)) = self.queue.poll_used() {
            if completed_head == head {
                return Some(completed_head);
            }
            // Stale completion for a request we already abandoned; reclaim
            // its descriptors and keep looking for ours.
            crate::serial_println!(
                "[virtio-blk] {} sector {}: draining stale completion head={} (expected {})",
                op, sector, completed_head, head,
            );
            self.queue.free_chain(completed_head);
        }
        None
    }

    /// Recover the device after a request timed out.
    ///
    /// A timed-out request leaves its descriptors *and* the shared DMA
    /// buffer owned by the device.  Blindly freeing the descriptor chain
    /// (the previous behaviour) and reusing the shared buffer corrupts the
    /// virtqueue free list and desyncs the used ring — see known-issues
    /// B-VIRTIO-BLK-WRITE-TIMEOUT, which manifested as an unrecoverable
    /// cascade of write timeouts.  A full device + queue reset forces the
    /// device to relinquish every outstanding buffer, so the next request
    /// starts from a clean, consistent state.
    fn recover_after_timeout(&mut self) {
        match self.recover() {
            Ok(()) => crate::serial_println!(
                "[virtio-blk] device reset to recover from timeout"
            ),
            Err(e) => crate::serial_println!(
                "[virtio-blk] device recovery after timeout FAILED: {:?}", e
            ),
        }
    }

    /// Re-run the legacy virtio init handshake to reclaim device ownership
    /// of all outstanding buffers, then reset the virtqueue and re-publish
    /// it to the device.  Reuses the existing queue backing frame and DMA
    /// frame (the reset drops the device's references to them).
    // PFN computation truncates a page-aligned address; feature/config
    // arithmetic uses small device-provided values.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn recover(&mut self) -> KernelResult<()> {
        // Reset → acknowledge → driver → features (mirrors init()).
        self.transport.reset();
        self.transport.set_status(STATUS_ACKNOWLEDGE);
        self.transport.set_status(STATUS_DRIVER);
        self.transport.set_guest_features(0);

        // Re-select queue 0 and verify the size is unchanged.
        self.transport.select_queue(0);
        let queue_size = self.transport.queue_size();
        if queue_size == 0 || queue_size != self.queue.queue_size() {
            self.transport.set_status(crate::virtio::STATUS_FAILED);
            return Err(KernelError::NoSuchDevice);
        }

        // Reset the virtqueue rings/free list (reuses the same frame) and
        // re-publish its physical PFN to the device.
        self.queue.reset();
        let pfn = (self.queue.phys_addr() >> 12) as u32;
        self.transport.set_queue_pfn(pfn);

        // Device is live again.
        self.transport.set_status(STATUS_DRIVER_OK);
        Ok(())
    }

    /// Check the DMA status byte after a completed request.
    fn check_status(&self, op: &str, sector: u64) -> KernelResult<()> {
        // SAFETY: dma_virt points to an exclusively-owned 16 KiB frame.
        // DMA_STATUS_OFFSET (4608) is well within 16384.  Volatile read
        // because the device writes this byte asynchronously via DMA.
        let status = unsafe {
            core::ptr::read_volatile(self.dma_virt.add(DMA_STATUS_OFFSET))
        };
        if status != VIRTIO_BLK_S_OK {
            crate::serial_println!(
                "[virtio-blk] {} sector {} failed: status={}",
                op, sector, status,
            );
            return Err(KernelError::IoError);
        }
        Ok(())
    }

    /// Read a single 512-byte sector.
    ///
    /// `buf` must be exactly 512 bytes.  The read is synchronous — this
    /// function blocks until the device completes the request, yielding
    /// the CPU via HLT when interrupt-driven I/O is active.
    // DMA offset arithmetic uses known small constants.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        if sector >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }

        // Write the request header into the DMA frame.
        // SAFETY: dma_virt is the start of an exclusively-owned 16 KiB frame.
        // VirtioBlkReqHeader is 16 bytes at offset 0, well within bounds.
        // Volatile because the device reads this via DMA.
        let header_ptr = self.dma_virt as *mut VirtioBlkReqHeader;
        unsafe {
            core::ptr::write_volatile(header_ptr, VirtioBlkReqHeader {
                type_: VIRTIO_BLK_T_IN,
                reserved: 0,
                sector,
            });
        }

        // SAFETY: DMA_STATUS_OFFSET (4608) < 16384.  Writing 0xFF as a
        // sentinel so we can distinguish "device hasn't written yet" from
        // a real status value (0 = OK, 1 = error, 2 = unsupported).
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

        // Wait for completion (interrupt-driven or polling fallback).
        let completed_head = self.wait_completion(head, "Read", sector)?;
        self.queue.free_chain(completed_head);

        self.check_status("Read", sector)?;

        // Copy data from DMA buffer to caller's buffer.
        // SAFETY: DMA_DATA_OFFSET (512) + SECTOR_SIZE (512) = 1024 < 16384.
        // The device has written exactly SECTOR_SIZE bytes at this offset
        // (verified by check_status above).  buf is a valid &mut [u8; 512].
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.dma_virt.add(DMA_DATA_OFFSET),
                buf.as_mut_ptr(),
                SECTOR_SIZE,
            );
        }

        Ok(())
    }

    /// Write a single 512-byte sector.
    // Same DMA arithmetic as read_sector.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn write_sector(&mut self, sector: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        if sector >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }

        // SAFETY: Same DMA frame layout as read_sector — dma_virt is the
        // start of an exclusively-owned 16 KiB frame.  Header at offset 0
        // (16 bytes), data at offset 512 (512 bytes), status at offset 4608
        // (1 byte) — all well within the 16384-byte frame.
        let header_ptr = self.dma_virt as *mut VirtioBlkReqHeader;
        unsafe {
            core::ptr::write_volatile(header_ptr, VirtioBlkReqHeader {
                type_: VIRTIO_BLK_T_OUT,
                reserved: 0,
                sector,
            });
        }

        // Copy caller's data into the DMA buffer for the device to read.
        // SAFETY: dma_virt + DMA_DATA_OFFSET is within our allocated DMA frame;
        // buf.len() >= SECTOR_SIZE (checked at call site).
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf.as_ptr(),
                self.dma_virt.add(DMA_DATA_OFFSET),
                SECTOR_SIZE,
            );
        }

        // Sentinel status byte (device will overwrite with 0 on success).
        // SAFETY: dma_virt + DMA_STATUS_OFFSET is within our allocated DMA frame.
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

        // Wait for completion (interrupt-driven or polling fallback).
        let completed_head = self.wait_completion(head, "Write", sector)?;
        self.queue.free_chain(completed_head);

        self.check_status("Write", sector)
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
#[allow(dead_code)]
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
            // Store the PCI IRQ line for enable_interrupts() later.
            BLK_IRQ_LINE.store(pci_dev.irq_line, Ordering::Release);
            crate::serial_println!("[virtio-blk] Device initialized successfully");
            Some(dev)
        }
        Err(e) => {
            crate::serial_println!("[virtio-blk] Init failed: {:?}", e);
            None
        }
    }
}

/// Find and initialize ALL virtio-blk devices on the PCI bus.
///
/// Returns a Vec of successfully initialized devices.  QEMU can
/// present multiple virtio-blk devices (e.g., disk.img=vda,
/// ext4_test.img=vdb, swap.img=vdc).
pub fn probe_all(hhdm_offset: u64) -> alloc::vec::Vec<VirtioBlkDevice> {
    let pci_devs = pci::find_all_devices(VIRTIO_VENDOR, VIRTIO_BLK_DEVICE);
    let mut devices = alloc::vec::Vec::new();

    for pci_dev in &pci_devs {
        crate::serial_println!(
            "[virtio-blk] Found device at {:02x}:{:02x}.{} (irq={})",
            pci_dev.address.bus,
            pci_dev.address.device,
            pci_dev.address.function,
            pci_dev.irq_line,
        );

        match VirtioBlkDevice::init(pci_dev, hhdm_offset) {
            Ok(dev) => {
                // Store the first device's IRQ line for the shared
                // interrupt handler.  All virtio-blk devices share
                // the same IRQ handler via level-triggered IOAPIC.
                if devices.is_empty() {
                    BLK_IRQ_LINE.store(pci_dev.irq_line, Ordering::Release);
                }
                crate::serial_println!(
                    "[virtio-blk] Device {} initialized ({} sectors)",
                    devices.len(),
                    dev.capacity()
                );
                devices.push(dev);
            }
            Err(e) => {
                crate::serial_println!(
                    "[virtio-blk] Init failed at {:02x}:{:02x}.{}: {:?}",
                    pci_dev.address.bus,
                    pci_dev.address.device,
                    pci_dev.address.function,
                    e
                );
            }
        }
    }

    if devices.is_empty() {
        crate::serial_println!("[virtio-blk] No devices found");
    } else {
        crate::serial_println!(
            "[virtio-blk] {} device(s) discovered",
            devices.len()
        );
    }

    devices
}

// ---------------------------------------------------------------------------
// Global device instance
// ---------------------------------------------------------------------------

/// The global virtio-blk device (if present).
#[allow(dead_code)]
static DEVICE: Mutex<Option<VirtioBlkDevice>> = Mutex::new(None);

/// Initialize the virtio-blk subsystem.
///
/// Probes for a virtio-blk device on the PCI bus.  If found,
/// initializes it, runs a self-test, and stores it globally.
#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut VirtioBlkDevice) -> R,
{
    let mut guard = DEVICE.lock();
    guard.as_mut().map(f)
}

/// Take the device out of the global slot, transferring ownership
/// to the caller.  Used by the block device registry to take
/// ownership of driver instances.
///
/// Returns `None` if no device was stored (or already taken).
#[allow(dead_code)]
pub fn take_device() -> Option<VirtioBlkDevice> {
    DEVICE.lock().take()
}

/// Self-test: read sector 0 and verify no error.
#[allow(dead_code)]
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

/// Enable interrupt-driven I/O for the virtio-blk device.
///
/// Configures the PCI IRQ line as level-triggered (required for PCI
/// interrupts) and unmasks it in the IOAPIC.  After this call, the
/// driver uses `HLT` to yield the CPU while waiting for completions
/// instead of busy-wait polling.
///
/// Must be called after:
/// - IOAPIC is initialized
/// - Interrupts are enabled (`cpu::sti()`)
/// - The virtio-blk device has been probed (`init()` already called)
///
/// Safe to call even if no device was found (returns silently).
pub fn enable_interrupts() {
    let irq = BLK_IRQ_LINE.load(Ordering::Acquire);
    if irq == 0xFF {
        return; // No device initialized.
    }
    // PCI interrupts are level-triggered, active-low.
    // SAFETY: IOAPIC is initialized (caller guarantees).
    unsafe { crate::ioapic::set_level_triggered(irq); }
    // SAFETY: The IDT handler is installed and calls handle_irq().
    unsafe { crate::ioapic::unmask_irq(irq); }
    IRQ_ENABLED.store(true, Ordering::Release);
    crate::serial_println!(
        "[virtio-blk] IRQ {} enabled — interrupt-driven I/O active",
        irq,
    );
}
