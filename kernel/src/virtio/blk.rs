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
//! acknowledges the interrupt by reading the ISR status register of
//! every virtio-blk function routed to that line -- each device records
//! its own line and port in `IRQ_ROUTES` -- then wakes the CPU from HLT.
//! The driver then checks the used ring for the completion.  Falls back
//! to polling if interrupts are not yet configured (early boot).
//!
//! ## DMA buffers
//!
//! The header, data, and status are laid out in a single 16 KiB frame
//! at known offsets.  Physical addresses are passed to the device for
//! DMA; virtual addresses (via HHDM) are used by the driver to write
//! headers and read status.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::pci::{self, PciAddress, PciDevice};
use crate::virtio::queue::{VRING_DESC_F_WRITE, Virtqueue};
use crate::virtio::{
    REG_ISR_STATUS, STATUS_ACKNOWLEDGE, STATUS_DRIVER, STATUS_DRIVER_OK, VirtioLegacyPci,
};
use spin::Mutex;

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
const DMA_HEADER_OFFSET: usize = 0; // 16 bytes
const DMA_DATA_OFFSET: usize = 512; // Up to 4096 bytes
const DMA_STATUS_OFFSET: usize = 512 + 4096; // 1 byte

// ---------------------------------------------------------------------------
// IRQ support — lock-free state for ISR context
// ---------------------------------------------------------------------------

/// Most virtio-blk functions whose interrupts the ISR can acknowledge.
///
/// The boot configuration attaches two (the swap disk and the glibc rootfs);
/// eight is headroom. Slots fill from index 0 and are never freed, so the ISR
/// stops at the first empty one and an unused slot costs nothing.
const MAX_IRQ_ROUTES: usize = 8;

/// Set in a used [`IRQ_ROUTES`] slot, so that an all-zero slot is empty.
const ROUTE_USED: u64 = 1 << 24;

/// The PCI Interrupt Line value firmware leaves on a function it did not
/// route to any interrupt input (logged as `irq=255` by the PCI scan).
const NO_IRQ_LINE: u8 = 0xFF;

/// Each initialised virtio-blk function's interrupt route, readable from the
/// ISR without a lock.
///
/// One `AtomicU64` per function: bits 0-15 hold its legacy I/O port base, bits
/// 16-23 its PCI IRQ line, bit 24 is [`ROUTE_USED`], and bits 32-47 its PCI
/// address packed as `bus << 8 | device << 3 | function`. One word per device
/// is the point. The ISR can never pair one device's line with another
/// device's port, and pairing them wrong is the bug this replaced.
///
/// That was two single-slot globals: the port was written by every device's
/// `init` (last writer wins), and the line by `probe_all` for the first device
/// only (first writer wins). With the two disks the boot attaches, IRQ 10 --
/// the swap disk's line -- was acknowledged by reading the ISR register of the
/// rootfs disk on IRQ 11. The swap disk's own interrupt was never
/// acknowledged, its level-triggered pin stayed asserted, and IRQ 10 stormed
/// at about half a million interrupts a second until the storm detector masked
/// it, three or four times a boot. The rootfs disk's line was never unmasked
/// at all, so its requests completed on whichever interrupt next woke the
/// CPU. See known-issues `A-VIRTIO-BLK-ACKED-THE-WRONG-DISK`.
static IRQ_ROUTES: [AtomicU64; MAX_IRQ_ROUTES] = [const { AtomicU64::new(0) }; MAX_IRQ_ROUTES];

/// Whether interrupt-driven I/O is active.  When false, the driver
/// falls back to polling (used during early boot before IOAPIC is up).
static IRQ_ENABLED: AtomicBool = AtomicBool::new(false);

/// One decoded [`IRQ_ROUTES`] entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IrqRoute {
    /// The function's PCI address, packed by [`pci_key`].
    pci: u16,
    /// The IRQ line the function asserts.
    irq: u8,
    /// The legacy I/O port base whose ISR status register acknowledges it.
    io_base: u16,
}

/// Pack a PCI address into 16 bits: bus in 15:8, device in 7:3, function in
/// 2:0, the layout `pci` uses for its own INTx claim table.
fn pci_key(addr: PciAddress) -> u16 {
    (u16::from(addr.bus) << 8)
        | ((u16::from(addr.device) & 0x1F) << 3)
        | (u16::from(addr.function) & 0x07)
}

impl IrqRoute {
    /// The slot encoding described at [`IRQ_ROUTES`].
    fn pack(self) -> u64 {
        ROUTE_USED
            | u64::from(self.io_base)
            | (u64::from(self.irq) << 16)
            | (u64::from(self.pci) << 32)
    }

    /// Decode a slot, or `None` for an empty one.
    #[allow(clippy::cast_possible_truncation)] // each field is masked to its width first
    fn unpack(word: u64) -> Option<Self> {
        if word & ROUTE_USED == 0 {
            return None;
        }
        Some(Self {
            io_base: (word & 0xFFFF) as u16,
            irq: ((word >> 16) & 0xFF) as u8,
            pci: ((word >> 32) & 0xFFFF) as u16,
        })
    }
}

/// The routes registered so far, in registration order.
fn irq_routes() -> impl Iterator<Item = IrqRoute> {
    IRQ_ROUTES
        .iter()
        .map_while(|slot| IrqRoute::unpack(slot.load(Ordering::Acquire)))
}

/// Record the interrupt route of a function this driver services.
///
/// A second registration for the same PCI function -- a re-probe --
/// replaces its own slot rather than taking another.
///
/// # Errors
///
/// `ResourceExhausted` when [`MAX_IRQ_ROUTES`] functions are already routed.
/// The caller must then leave the function unclaimed: an interrupt the ISR
/// cannot acknowledge is exactly what storms a shared line, and an unclaimed
/// function is silenced by `pci::quiesce_unclaimed_intx` instead.
fn register_irq_route(route: IrqRoute) -> KernelResult<()> {
    let word = route.pack();
    for slot in &IRQ_ROUTES {
        let current = slot.load(Ordering::Acquire);
        match IrqRoute::unpack(current) {
            Some(existing) if existing.pci == route.pci => {
                slot.store(word, Ordering::Release);
                return Ok(());
            }
            Some(_) => {}
            None => {
                if slot
                    .compare_exchange(0, word, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Ok(());
                }
                // Lost a race for this slot: if the winner was a re-probe of
                // the same function, replace it; otherwise try the next slot.
                if IrqRoute::unpack(slot.load(Ordering::Acquire))
                    .is_some_and(|winner| winner.pci == route.pci)
                {
                    slot.store(word, Ordering::Release);
                    return Ok(());
                }
            }
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Called from the IOAPIC device IRQ handler for every external device
/// interrupt: acknowledges every virtio-blk function routed to `irq`, by
/// reading its ISR status register, which de-asserts its level-triggered pin.
///
/// *Every* function on the line, not the first match: two disks on one shared
/// line both have to be acknowledged, and which of them interrupted is only
/// known from reading each one's register.
///
/// For an IRQ no virtio-blk function uses, this is one relaxed load per
/// registered route and no port I/O.
///
/// This function runs in ISR context — no locks, no allocations.
///
/// Returns `true` if any of them actually had a pending interrupt.
pub fn handle_irq(irq: u32) -> bool {
    let mut pending = false;
    for route in irq_routes() {
        if u32::from(route.irq) != irq {
            continue;
        }
        // Read ISR status: acknowledges the interrupt at the device.
        // Bit 0 = used buffer notification, bit 1 = config change.
        // SAFETY: `route.io_base` is the legacy I/O port base of a virtio-blk
        // function this driver initialised (it is registered only from
        // `VirtioBlkDevice::init`, from the function's own BAR0), and the ISR
        // status register is a read-only byte at a fixed offset within it.
        let isr = unsafe { crate::port::inb(route.io_base.wrapping_add(REG_ISR_STATUS)) };
        pending |= isr != 0;
    }
    pending
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
    /// The PCI function this device is, for [`Self::pci_address`].
    pci_address: PciAddress,
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

        // Route before claiming. The claim below lets this function keep
        // asserting its pin, and only a route lets the ISR acknowledge it, so
        // a function this driver cannot route must not be claimed: returning
        // here leaves it for `pci::quiesce_unclaimed_intx` to silence. A
        // function with no line assigned cannot interrupt and needs no route.
        if pci_dev.irq_line != NO_IRQ_LINE {
            register_irq_route(IrqRoute {
                pci: pci_key(pci_dev.address),
                irq: pci_dev.irq_line,
                io_base,
            })?;
        }

        // Enable bus mastering for DMA.
        pci::enable_bus_master(pci_dev.address);

        // `handle_device_irq` calls this driver's `handle_irq` on every device
        // IRQ, and that reads the ISR status register of every function routed
        // to the line, which deasserts it.  Recording the claim keeps
        // `quiesce_unclaimed_intx` from silencing a function that is in fact
        // serviced.
        pci::claim_intx(pci_dev.address);

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
        crate::serial_println!(
            "[virtio-blk] Queue PFN: {:#x} (phys {:#x})",
            pfn,
            u64::from(pfn) << 12
        );

        // 6. Set DRIVER_OK — device is live.
        transport.set_status(STATUS_DRIVER_OK);

        // Read device config: capacity (8 bytes at device config offset 0).
        let capacity = transport.read_device_config64(0);
        crate::serial_println!(
            "[virtio-blk] Disk capacity: {} sectors ({} KiB)",
            capacity,
            capacity * 512 / 1024
        );

        // Allocate a DMA frame for request headers/data/status.
        let dma_frame = frame::alloc_frame()?;
        let dma_virt = (dma_frame.addr() + hhdm_offset) as *mut u8;
        // Zero the DMA frame.
        // SAFETY: We just allocated this frame; HHDM maps it writable.
        unsafe {
            core::ptr::write_bytes(dma_virt, 0, frame::FRAME_SIZE);
        }

        // DRIVER_OK and its queue set up: the function is this driver's.
        pci::bind_driver(pci_dev.address, "virtio-blk");
        Ok(Self {
            transport,
            queue,
            capacity,
            hhdm_offset,
            dma_frame,
            dma_virt,
            pci_address: pci_dev.address,
        })
    }

    /// Return the disk capacity in 512-byte sectors.
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// The PCI function this device is.
    ///
    /// For correlating a registered block device with its interrupt pin:
    /// [`self_test_irq_routes`] reads a sector through the registry and then
    /// watches this function's INTx status.
    #[must_use]
    pub fn pci_address(&self) -> PciAddress {
        self.pci_address
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
                        op,
                        sector,
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
                        op,
                        sector,
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
                op,
                sector,
                completed_head,
                head,
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
            Ok(()) => crate::serial_println!("[virtio-blk] device reset to recover from timeout"),
            Err(e) => {
                crate::serial_println!("[virtio-blk] device recovery after timeout FAILED: {:?}", e)
            }
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
        let status = unsafe { core::ptr::read_volatile(self.dma_virt.add(DMA_STATUS_OFFSET)) };
        if status != VIRTIO_BLK_S_OK {
            crate::serial_println!(
                "[virtio-blk] {} sector {} failed: status={}",
                op,
                sector,
                status,
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
            core::ptr::write_volatile(
                header_ptr,
                VirtioBlkReqHeader {
                    type_: VIRTIO_BLK_T_IN,
                    reserved: 0,
                    sector,
                },
            );
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
            (header_phys, 16, 0u16), // Header: device-readable
            (data_phys, SECTOR_SIZE as u32, VRING_DESC_F_WRITE), // Data: device-writable
            (status_phys, 1, VRING_DESC_F_WRITE), // Status: device-writable
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
            core::ptr::write_volatile(
                header_ptr,
                VirtioBlkReqHeader {
                    type_: VIRTIO_BLK_T_OUT,
                    reserved: 0,
                    sector,
                },
            );
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
            (header_phys, 16, 0u16),               // Header: device-readable
            (data_phys, SECTOR_SIZE as u32, 0u16), // Data: device-readable
            (status_phys, 1, VRING_DESC_F_WRITE),  // Status: device-writable
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
            crate::serial_println!("[virtio-blk] WARNING: failed to free DMA frame: {:?}", e);
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
            // `init` registered the device's interrupt route itself.
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
                // `init` registered this device's own interrupt route. The
                // devices do not share one line: the boot attaches two disks
                // on different lines (10 and 11), and a single recorded line
                // is how the second disk's pin went unacknowledged.
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
        crate::serial_println!("[virtio-blk] {} device(s) discovered", devices.len());
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
                crate::serial_println!("[virtio-blk] Self-test failed, device NOT stored: {:?}", e);
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

/// Enable interrupt-driven I/O for every virtio-blk device.
///
/// Configures each IRQ line a routed device uses as level-triggered
/// (required for PCI interrupts) and unmasks it in the IOAPIC, once per line
/// however many devices share it.  After this call, the driver uses `HLT` to
/// yield the CPU while waiting for completions instead of busy-wait polling.
///
/// Every line, not the first device's: a disk whose line stays masked
/// completes its requests on whatever interrupt next wakes the CPU, and one
/// whose line is unmasked but unacknowledged storms it. Both happened, one to
/// each disk; see [`IRQ_ROUTES`].
///
/// Must be called after:
/// - IOAPIC is initialized
/// - Interrupts are enabled (`cpu::sti()`)
/// - The virtio-blk devices have been probed (`probe_all()` already called)
///
/// Safe to call even if no device was found (returns silently).
pub fn enable_interrupts() {
    let mut done = [NO_IRQ_LINE; MAX_IRQ_ROUTES];
    let mut lines = 0usize;
    for route in irq_routes() {
        if done.contains(&route.irq) {
            continue;
        }
        // PCI interrupts are level-triggered, active-low.
        // SAFETY: IOAPIC is initialized (caller guarantees), and `route.irq`
        // is the line firmware assigned to a function this driver
        // initialised, not the no-line value (`init` routes only real lines).
        unsafe {
            crate::ioapic::set_level_triggered(route.irq);
        }
        // SAFETY: the IDT handler is installed, and `handle_device_irq` calls
        // `handle_irq`, which acknowledges every function routed to the line.
        unsafe {
            crate::ioapic::unmask_irq(route.irq);
        }
        if let Some(slot) = done.get_mut(lines) {
            *slot = route.irq;
        }
        lines = lines.saturating_add(1);
        crate::serial_println!(
            "[virtio-blk] IRQ {} enabled — interrupt-driven I/O active",
            route.irq,
        );
    }
    if lines > 0 {
        IRQ_ENABLED.store(true, Ordering::Release);
    }
}

/// How long [`self_test_irq_routes`] gives a function's pin to fall after the
/// read that raised it: far longer than an ISR on another CPU takes to run,
/// far shorter than the storm detector's three-second window.
const INTX_SETTLE_NS: u64 = 200_000_000;

/// Every virtio-blk function's interrupt is acknowledged by its own ISR
/// status register.
///
/// Two checks, both of which the single-slot globals `IRQ_ROUTES` replaced
/// would have failed:
///
/// 1. **The table.** Each route pairs a function's own IRQ line with its own
///    I/O port, as its PCI configuration space states them now. The old
///    globals paired the swap disk's line (10) with the rootfs disk's port.
/// 2. **The pin.** For every registered block device that is a virtio-blk
///    function, one sector is read with interrupts on, and the function's
///    INTx status bit must then clear within [`INTX_SETTLE_NS`]. A pin still
///    asserted is an interrupt nothing acknowledged -- a storm in the making
///    on a shared line, or a request that completed only because the CPU
///    happened to wake. Before the fix both disks failed this: the swap
///    disk's pin because its line was acknowledged at the other disk's port,
///    the rootfs disk's because its line was never unmasked.
///
/// Run after [`enable_interrupts`]: with the lines masked there is no ISR to
/// clear anything, and check 2 would fail on a correct tree.
///
/// # Errors
///
/// `InternalError`, after naming the function, when a route disagrees with
/// configuration space, when a routed disk is missing from the registry, or
/// when a pin stays asserted; the read's own error when a read fails.
pub fn self_test_irq_routes() -> KernelResult<()> {
    crate::serial_println!("[virtio-blk] Running interrupt-route self-test...");
    let routes: alloc::vec::Vec<IrqRoute> = irq_routes().collect();
    if routes.is_empty() {
        crate::serial_println!("[virtio-blk]   no routed device -- nothing to check");
        return Ok(());
    }

    // 1. Each route against configuration space as it stands now.
    let functions = pci::find_all_devices(VIRTIO_VENDOR, VIRTIO_BLK_DEVICE);
    for route in &routes {
        let Some(func) = functions.iter().find(|f| pci_key(f.address) == route.pci) else {
            crate::serial_println!(
                "[virtio-blk]   FAIL: route {:?} names a PCI function that is not a virtio-blk disk",
                route
            );
            return Err(KernelError::InternalError);
        };
        if func.irq_line != route.irq || func.bar0_io_port() != Some(route.io_base) {
            crate::serial_println!(
                "[virtio-blk]   FAIL: {:02x}:{:02x}.{} is routed as irq {} at port {:#x}, \
                 but its configuration space says irq {} at port {:?}",
                func.address.bus,
                func.address.device,
                func.address.function,
                route.irq,
                route.io_base,
                func.irq_line,
                func.bar0_io_port()
            );
            return Err(KernelError::InternalError);
        }
    }

    // 2. Every registered virtio-blk disk: read, then watch its pin fall.
    let mut checked: alloc::vec::Vec<u16> = alloc::vec::Vec::new();
    for info in crate::blkdev::list_devices() {
        let mut buf = [0u8; SECTOR_SIZE];
        let Some((addr, read)) = crate::blkdev::with_device(&info.name, |dev| {
            dev.pci_address().map(|a| (a, dev.read_sector(0, &mut buf)))
        })
        .flatten() else {
            continue; // not a PCI function (a RAM disk, say), or gone
        };
        if !routes.iter().any(|r| r.pci == pci_key(addr)) {
            // Not ours: another driver's PCI disk.
            continue;
        }
        read?;
        let deadline = crate::hrtimer::now_ns().saturating_add(INTX_SETTLE_NS);
        while pci::intx_asserting(addr) {
            if crate::hrtimer::now_ns() >= deadline {
                crate::serial_println!(
                    "[virtio-blk]   FAIL: '{}' ({:02x}:{:02x}.{}) still asserts its interrupt \
                     {} ms after a read completed -- nothing acknowledged it",
                    info.name,
                    addr.bus,
                    addr.device,
                    addr.function,
                    INTX_SETTLE_NS / 1_000_000
                );
                return Err(KernelError::InternalError);
            }
            core::hint::spin_loop();
        }
        checked.push(pci_key(addr));
    }
    // A routed disk the registry does not hold was never read, and the table
    // in step 1 cannot say whether its pin works: name it rather than pass.
    if let Some(missed) = routes.iter().find(|r| !checked.contains(&r.pci)) {
        crate::serial_println!(
            "[virtio-blk]   FAIL: route {:?} belongs to no registered disk, so its pin was \
             never exercised",
            missed
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[virtio-blk]   {} disk(s): each route matches configuration space, and each pin \
         fell after a read -- every interrupt was acknowledged at its own port",
        checked.len()
    );
    crate::serial_println!("[virtio-blk] Interrupt-route self-test PASSED");
    Ok(())
}
