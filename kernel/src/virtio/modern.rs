//! Modern (virtio 1.0+) PCI transport.
//!
//! The legacy transport in [`super`] talks to the device through I/O ports in
//! BAR0.  That interface is deprecated, and a growing number of devices never
//! implemented it at all: **virtio-gpu and virtio-sound are modern-only**, so
//! their BAR0 is either absent or an MMIO BAR, and a legacy driver simply
//! reports "BAR0 is not I/O space" and gives up.  That is exactly what
//! `virtio/sound.rs` did on every boot until this module existed.
//!
//! ## What "modern" means concretely
//!
//! Instead of a fixed register block at a known port offset, the device
//! publishes a set of **vendor-specific PCI capabilities** (`cap_vndr` =
//! 0x09), each of which says "config region of type T lives at `offset`
//! within BAR `n`, for `length` bytes".  Four types matter:
//!
//! | `cfg_type` | Region | Used for |
//! |---|---|---|
//! | 1 | common config | status, features, queue setup |
//! | 2 | notify | per-queue doorbell |
//! | 3 | ISR | interrupt-reason read/ack |
//! | 4 | device config | device-specific fields (e.g. `jacks`/`streams`) |
//!
//! Those regions are MMIO, not RAM, so they are **not** covered by the HHDM
//! and have to be mapped explicitly with `NO_CACHE`.
//!
//! ## Why this is a module and not a copy
//!
//! It was a copy: the whole transport lived privately inside `gpu.rs`, so
//! `sound.rs` had no way to reach it and was written against the legacy
//! transport instead — against a device that has never supported it.  A
//! transport shared by two drivers also gets the `VIRTIO_F_VERSION_1`
//! handling right in one place; see [`ModernTransport::negotiate`].
//!
//! ## References
//!
//! - Virtio 1.1 spec §4.1 "Virtio Over PCI Bus"
//! - Virtio 1.1 spec §3.1 "Device Initialization"
//! - QEMU `hw/virtio/virtio-pci.c`

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;
use crate::mm::frame::PhysFrame;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::virtio::queue::Virtqueue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Virtio vendor ID (Red Hat / QEMU).
pub const VIRTIO_VENDOR: u16 = 0x1AF4;

/// PCI capability ID for vendor-specific capabilities.
const PCI_CAP_ID_VNDR: u8 = 0x09;

/// Common configuration region.
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/// Notification region.
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
/// ISR status region.
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
/// Device-specific configuration region.
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

/// Guest has found the device and recognised it.
pub const STATUS_ACKNOWLEDGE: u8 = 1;
/// Guest knows how to drive the device.
pub const STATUS_DRIVER: u8 = 2;
/// Guest driver is ready.
pub const STATUS_DRIVER_OK: u8 = 4;
/// Feature negotiation succeeded.
pub const STATUS_FEATURES_OK: u8 = 8;
/// Something went wrong — the device is unusable.
pub const STATUS_FAILED: u8 = 128;

/// `VIRTIO_F_VERSION_1` — feature bit 32, i.e. bit 0 of feature page 1.
///
/// This is the bit that *means* "modern".  A device that offers it and a
/// driver that does not accept it fall back to legacy semantics, which for a
/// modern-only device means the device refuses `FEATURES_OK`.  See
/// [`ModernTransport::negotiate`].
const VIRTIO_F_VERSION_1_PAGE1: u32 = 1 << 0;

// Common configuration structure layout (virtio 1.1 §4.1.4.3).
const COMMON_DFSELECT: usize = 0x00; // u32 - device feature select
const COMMON_DF: usize = 0x04; // u32 - device feature (read)
const COMMON_GFSELECT: usize = 0x08; // u32 - guest feature select
const COMMON_GF: usize = 0x0C; // u32 - guest feature (write)
const COMMON_NUMQ: usize = 0x12; // u16 - number of queues
const COMMON_STATUS: usize = 0x14; // u8  - device status
const COMMON_QSELECT: usize = 0x16; // u16 - queue select
const COMMON_QSIZE: usize = 0x18; // u16 - queue size
const COMMON_QENABLE: usize = 0x1C; // u16 - queue enable
const COMMON_QNOFF: usize = 0x1E; // u16 - queue notify offset
const COMMON_QDESC_LO: usize = 0x20; // u32 - queue desc low
const COMMON_QDESC_HI: usize = 0x24; // u32 - queue desc high
const COMMON_QDRIVER_LO: usize = 0x28; // u32 - queue driver (avail) low
const COMMON_QDRIVER_HI: usize = 0x2C; // u32 - queue driver (avail) high
const COMMON_QDEVICE_LO: usize = 0x30; // u32 - queue device (used) low
const COMMON_QDEVICE_HI: usize = 0x34; // u32 - queue device (used) high

/// Upper bound on the status-reads spent waiting for a reset to be observed.
///
/// The spec requires reading status back until it returns 0.  A device that
/// never does is broken, and spinning forever in `init` would hang the boot.
const RESET_POLL_LIMIT: u32 = 100_000;

// ---------------------------------------------------------------------------
// Capability parsing
// ---------------------------------------------------------------------------

/// One parsed virtio PCI capability.
#[derive(Debug, Clone, Copy)]
struct VirtioPciCap {
    /// BAR index the region lives in.
    bar: u8,
    /// Offset within that BAR.
    offset: u32,
    /// Length of the region in bytes.
    length: u32,
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// A mapped modern virtio PCI transport.
///
/// All four config regions are mapped MMIO pointers into the HHDM range.  The
/// struct is `!Send`/`!Sync` by virtue of holding raw pointers; every current
/// user keeps it inside its driver's device mutex, which is what serialises
/// access to the device's registers.
pub struct ModernTransport {
    /// Log prefix of the owning driver, e.g. `"[virtio-snd]"`.
    ///
    /// Carried rather than passed per call so that a failure deep in queue
    /// setup still names the driver that is failing; with two drivers sharing
    /// this code, "Queue 0 size is 0" alone is not actionable.
    tag: &'static str,
    /// Virtual address of the common config region.
    common_cfg: *mut u8,
    /// Virtual address of the notify region.
    notify_cfg: *mut u8,
    /// Notify offset multiplier (from the notify capability).
    notify_off_multiplier: u32,
    /// Virtual address of the ISR config region.
    ///
    /// Nothing reads this yet: both drivers built on this transport poll their
    /// used rings rather than taking an interrupt.  It is still parsed and
    /// mapped rather than skipped, because a device that does not expose an
    /// ISR capability is malformed and we would rather say so at probe time
    /// than discover it the first time someone wires up an IRQ.
    #[allow(dead_code)]
    isr_cfg: *mut u8,
    /// Virtual address of the device-specific config region.
    device_cfg: *mut u8,
}

impl ModernTransport {
    /// Parse the device's virtio PCI capabilities and map its config regions.
    ///
    /// `tag` is the caller's serial-log prefix, including brackets.
    ///
    /// # Errors
    ///
    /// [`KernelError::NoSuchDevice`] if the device exposes no vendor-specific
    /// capabilities, is missing one of the four required config regions, or
    /// names a BAR that is not a mappable MMIO BAR.  [`KernelError::IoError`]
    /// if a config region cannot be mapped into the address space.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn probe(tag: &'static str, dev: &PciDevice, hhdm_offset: u64) -> KernelResult<Self> {
        let caps = pci::find_capabilities(dev.address, PCI_CAP_ID_VNDR);
        if caps.is_empty() {
            serial_println!(
                "{} No vendor-specific PCI capabilities: this is not a modern virtio device",
                tag
            );
            return Err(KernelError::NoSuchDevice);
        }

        let mut common_cap: Option<VirtioPciCap> = None;
        let mut notify_cap: Option<VirtioPciCap> = None;
        let mut isr_cap: Option<VirtioPciCap> = None;
        let mut device_cap: Option<VirtioPciCap> = None;
        let mut notify_off_multiplier: u32 = 0;

        for cap in &caps {
            // virtio PCI cap structure (§4.1.4.3):
            //   +0 cap_vndr, +1 cap_next, +2 cap_len, +3 cfg_type,
            //   +4 bar, +5 padding[3], +8 offset (u32), +12 length (u32)
            // and for a NOTIFY cap only, +16 notify_off_multiplier (u32).
            let a = dev.address;
            let off = cap.offset;
            let cfg_type = pci::config_read8(a.bus, a.device, a.function, off.wrapping_add(3));
            let bar = pci::config_read8(a.bus, a.device, a.function, off.wrapping_add(4));
            let region_offset =
                pci::config_read32(a.bus, a.device, a.function, off.wrapping_add(8));
            let region_length =
                pci::config_read32(a.bus, a.device, a.function, off.wrapping_add(12));

            let vcap = VirtioPciCap {
                bar,
                offset: region_offset,
                length: region_length,
            };

            serial_println!(
                "{}   Cap type={} bar={} offset={:#x} len={:#x}",
                tag,
                cfg_type,
                bar,
                region_offset,
                region_length
            );

            match cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => common_cap = Some(vcap),
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    notify_cap = Some(vcap);
                    notify_off_multiplier =
                        pci::config_read32(a.bus, a.device, a.function, off.wrapping_add(16));
                }
                VIRTIO_PCI_CAP_ISR_CFG => isr_cap = Some(vcap),
                VIRTIO_PCI_CAP_DEVICE_CFG => device_cap = Some(vcap),
                // cfg_type 5 is PCI_CFG (config access through a window); we
                // never need it because we map the BARs directly.  Anything
                // else is a capability defined after this code was written.
                _ => {}
            }
        }

        let missing = |what: &str| -> KernelError {
            serial_println!("{} Missing {} capability", tag, what);
            KernelError::NoSuchDevice
        };
        let common = common_cap.ok_or_else(|| missing("COMMON_CFG"))?;
        let notify = notify_cap.ok_or_else(|| missing("NOTIFY_CFG"))?;
        let isr = isr_cap.ok_or_else(|| missing("ISR_CFG"))?;
        let devcfg = device_cap.ok_or_else(|| missing("DEVICE_CFG"))?;

        let common_cfg = map_bar_region(tag, dev, &common, hhdm_offset)?;
        let notify_cfg = map_bar_region(tag, dev, &notify, hhdm_offset)?;
        let isr_cfg = map_bar_region(tag, dev, &isr, hhdm_offset)?;
        let device_cfg = map_bar_region(tag, dev, &devcfg, hhdm_offset)?;

        serial_println!(
            "{} Transport: common={:p} notify={:p} isr={:p} dev={:p} mult={}",
            tag,
            common_cfg,
            notify_cfg,
            isr_cfg,
            device_cfg,
            notify_off_multiplier
        );

        Ok(Self {
            tag,
            common_cfg,
            notify_cfg,
            notify_off_multiplier,
            isr_cfg,
            device_cfg,
        })
    }

    /// Read the device status register.
    #[must_use]
    pub fn status(&self) -> u8 {
        // SAFETY: `common_cfg` points at the mapped common-config MMIO region
        // and `COMMON_STATUS` is within its spec-defined 56-byte layout.
        unsafe { core::ptr::read_volatile(self.common_cfg.add(COMMON_STATUS)) }
    }

    /// Write the device status register.
    pub fn set_status(&self, status: u8) {
        // SAFETY: as `status`.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(COMMON_STATUS), status);
        }
    }

    /// Add bits to the device status register (the spec's normal idiom: the
    /// driver never clears a bit it has set, except by a full reset).
    pub fn add_status(&self, bits: u8) {
        self.set_status(self.status() | bits);
    }

    /// Reset the device and wait for the reset to be observed.
    ///
    /// Per §4.1.4.3.1 the driver must read status back until it reads 0; the
    /// write alone does not mean the reset has completed.  Bounded so that a
    /// device that never acknowledges cannot hang the boot.
    pub fn reset(&self) {
        self.set_status(0);
        let mut attempts = 0u32;
        while self.status() != 0 && attempts < RESET_POLL_LIMIT {
            core::hint::spin_loop();
            attempts = attempts.wrapping_add(1);
        }
        if attempts >= RESET_POLL_LIMIT {
            serial_println!("{} WARNING: device did not acknowledge reset", self.tag);
        }
    }

    /// Read a 32-bit page of the device's offered feature bits.
    #[must_use]
    pub fn device_features(&self, page: u32) -> u32 {
        // SAFETY: `common_cfg` is mapped MMIO; DFSELECT/DF are within the
        // common-config layout and are naturally aligned u32 fields.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(COMMON_DFSELECT).cast::<u32>(), page);
            core::ptr::read_volatile(self.common_cfg.add(COMMON_DF).cast::<u32>())
        }
    }

    /// Write a 32-bit page of the driver's accepted feature bits.
    pub fn set_guest_features(&self, page: u32, features: u32) {
        // SAFETY: as `device_features`.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(COMMON_GFSELECT).cast::<u32>(), page);
            core::ptr::write_volatile(self.common_cfg.add(COMMON_GF).cast::<u32>(), features);
        }
    }

    /// Run steps 1–5 of the §3.1 initialisation sequence.
    ///
    /// Resets the device, announces the driver, negotiates features and
    /// confirms `FEATURES_OK`.  `wanted_page0` is the set of device-specific
    /// feature bits (0..32) the caller wants; bit 32 (`VIRTIO_F_VERSION_1`) is
    /// handled here because getting it wrong is the single most common way to
    /// have a modern device refuse to start:
    ///
    /// A modern-only device offers `VIRTIO_F_VERSION_1` and expects the driver
    /// to accept it.  A driver that writes 0 to feature page 1 is telling the
    /// device "I am a legacy driver", and a device with no legacy
    /// implementation then refuses `FEATURES_OK` — with no other diagnostic
    /// than that one bit failing to stick.  So we accept it whenever it is
    /// offered, and only fall back to not offering it if the device does not.
    ///
    /// Only feature bits the device actually offers are accepted, as §3.1
    /// requires: acking an unoffered bit is a driver bug the device is
    /// entitled to reject.
    ///
    /// # Errors
    ///
    /// [`KernelError::NotSupported`] if the device clears `FEATURES_OK`,
    /// meaning it will not run with the feature set we accepted.  The device
    /// is left reset and marked `FAILED` in that case.
    pub fn negotiate(&self, wanted_page0: u32) -> KernelResult<u32> {
        self.reset();
        self.set_status(STATUS_ACKNOWLEDGE);
        self.add_status(STATUS_DRIVER);

        let offered0 = self.device_features(0);
        let offered1 = self.device_features(1);
        let accept0 = offered0 & wanted_page0;
        let accept1 = offered1 & VIRTIO_F_VERSION_1_PAGE1;

        serial_println!(
            "{} Features offered {:#010x}:{:#010x}, accepting {:#010x}:{:#010x}{}",
            self.tag,
            offered1,
            offered0,
            accept1,
            accept0,
            if accept1 == 0 { " (no VERSION_1!)" } else { "" }
        );

        self.set_guest_features(0, accept0);
        self.set_guest_features(1, accept1);
        self.add_status(STATUS_FEATURES_OK);

        if self.status() & STATUS_FEATURES_OK == 0 {
            serial_println!(
                "{} Device rejected the feature set (status {:#x})",
                self.tag,
                self.status()
            );
            self.set_status(STATUS_FAILED);
            self.reset();
            return Err(KernelError::NotSupported);
        }

        Ok(accept0)
    }

    /// Number of virtqueues the device implements.
    #[must_use]
    pub fn num_queues(&self) -> u16 {
        // SAFETY: `common_cfg` is mapped MMIO; NUMQ is a u16 within the
        // common-config layout.
        unsafe { core::ptr::read_volatile(self.common_cfg.add(COMMON_NUMQ).cast::<u16>()) }
    }

    /// Select a queue for subsequent per-queue register accesses.
    fn select_queue(&self, index: u16) {
        // SAFETY: `common_cfg` is mapped MMIO; QSELECT is a u16 within it.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(COMMON_QSELECT).cast::<u16>(), index);
        }
    }

    /// Size of the currently selected queue (0 means "does not exist").
    fn queue_size(&self) -> u16 {
        // SAFETY: `common_cfg` is mapped MMIO; QSIZE is a u16 within it.
        unsafe { core::ptr::read_volatile(self.common_cfg.add(COMMON_QSIZE).cast::<u16>()) }
    }

    /// Write a 64-bit physical address into a lo/hi register pair.
    #[allow(clippy::cast_possible_truncation)]
    fn set_queue_addr(&self, lo: usize, hi: usize, addr: u64) {
        // SAFETY: `common_cfg` is mapped MMIO and both offsets are naturally
        // aligned u32 fields within the common-config layout.  The truncation
        // to u32 is the intended split of a 64-bit address into two halves.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(lo).cast::<u32>(), addr as u32);
            core::ptr::write_volatile(self.common_cfg.add(hi).cast::<u32>(), (addr >> 32) as u32);
        }
    }

    /// Mark the currently selected queue as ready for the device to use.
    fn enable_queue(&self) {
        // SAFETY: `common_cfg` is mapped MMIO; QENABLE is a u16 within it.
        unsafe {
            core::ptr::write_volatile(self.common_cfg.add(COMMON_QENABLE).cast::<u16>(), 1);
        }
    }

    /// Notify offset of the currently selected queue.
    fn queue_notify_off(&self) -> u16 {
        // SAFETY: `common_cfg` is mapped MMIO; QNOFF is a u16 within it.
        unsafe { core::ptr::read_volatile(self.common_cfg.add(COMMON_QNOFF).cast::<u16>()) }
    }

    /// Ring the doorbell for `queue_index`.
    ///
    /// The doorbell address is `notify_cfg + queue_notify_off * multiplier`,
    /// where `queue_notify_off` is a *per-queue* value read out of the common
    /// config — it is not the queue index, and assuming it is happens to work
    /// on QEMU only because QEMU assigns them in order.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn notify_queue(&self, queue_index: u16) {
        self.select_queue(queue_index);
        let off = self.queue_notify_off();
        // SAFETY: the device guarantees `queue_notify_off * multiplier` stays
        // within the notify region it described in its NOTIFY capability, and
        // that whole region is mapped.
        let notify_addr = unsafe {
            self.notify_cfg
                .add((u32::from(off) * self.notify_off_multiplier) as usize)
        };
        // SAFETY: `notify_addr` was just computed to be inside the mapped
        // notify region; a u16 write there is the spec-defined doorbell.
        unsafe {
            core::ptr::write_volatile(notify_addr.cast::<u16>(), queue_index);
        }
    }

    /// Read and acknowledge the ISR status byte.
    ///
    /// Reading this register clears it, which is how a virtio interrupt is
    /// acknowledged; bit 0 means "a virtqueue advanced", bit 1 means "the
    /// device configuration changed".  Unused for as long as every driver
    /// here polls — kept beside [`Self::isr_cfg`] because the pointer and the
    /// one operation defined on it belong together.
    #[allow(dead_code)]
    #[must_use]
    pub fn isr_status(&self) -> u8 {
        // SAFETY: `isr_cfg` points at the mapped ISR region, whose first byte
        // is the status register.
        unsafe { core::ptr::read_volatile(self.isr_cfg) }
    }

    /// Read a `u32` from the device-specific config region.
    #[must_use]
    pub fn read_device_config32(&self, offset: usize) -> u32 {
        // SAFETY: as `read_device_config8`; the offset is a naturally aligned
        // u32 field of the device's config layout.
        unsafe { core::ptr::read_volatile(self.device_cfg.add(offset).cast::<u32>()) }
    }

    /// Allocate, describe and enable virtqueue `queue_idx`.
    ///
    /// The modern transport wants the descriptor table, available ring and
    /// used ring as three independent 64-bit physical addresses rather than
    /// the legacy transport's single page-frame number, so the sub-ring
    /// offsets within our one-frame [`Virtqueue`] allocation are computed here.
    ///
    /// # Errors
    ///
    /// [`KernelError::NoSuchDevice`] if the device reports queue size 0 (the
    /// queue does not exist), or whatever [`Virtqueue::new`] returns.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn setup_queue(&self, queue_idx: u16, hhdm_offset: u64) -> KernelResult<Virtqueue> {
        self.select_queue(queue_idx);
        let queue_size = self.queue_size();
        if queue_size == 0 {
            serial_println!("{} Queue {} does not exist (size 0)", self.tag, queue_idx);
            return Err(KernelError::NoSuchDevice);
        }

        let (vq, _pfn) = Virtqueue::new(queue_size, hhdm_offset)?;

        // Layout inside the single frame `Virtqueue::new` allocated:
        //   desc[queue_size]   16 bytes each, at offset 0
        //   avail              flags(2) + idx(2) + ring[queue_size](2 each)
        //                      + used_event(2), immediately after desc
        //   used               4096-aligned after avail
        let phys_base = vq.phys_addr();
        let qs = u64::from(queue_size);
        let desc_addr = phys_base;
        let avail_addr = phys_base + qs * 16;
        let avail_size = 4 + qs * 2 + 2;
        let used_addr = align_up_u64(avail_addr + avail_size, 4096);

        self.set_queue_addr(COMMON_QDESC_LO, COMMON_QDESC_HI, desc_addr);
        self.set_queue_addr(COMMON_QDRIVER_LO, COMMON_QDRIVER_HI, avail_addr);
        self.set_queue_addr(COMMON_QDEVICE_LO, COMMON_QDEVICE_HI, used_addr);
        self.enable_queue();

        serial_println!(
            "{}   Queue {}: size={} desc={:#x} avail={:#x} used={:#x}",
            self.tag,
            queue_idx,
            queue_size,
            desc_addr,
            avail_addr,
            used_addr
        );

        Ok(vq)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Round `value` up to a multiple of `align` (which must be a power of two).
#[must_use]
pub const fn align_up_u64(value: u64, align: u64) -> u64 {
    (value.wrapping_add(align.wrapping_sub(1))) & !(align.wrapping_sub(1))
}

/// Map one capability's config region into the HHDM range as uncached MMIO.
///
/// These regions live in a PCI BAR, which is device memory: it is not RAM, so
/// the bootloader's direct map does not cover it and it must never be cached.
/// We map it at the address it *would* have had in the direct map so that
/// physical-to-virtual arithmetic elsewhere keeps working.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn map_bar_region(
    tag: &'static str,
    dev: &PciDevice,
    cap: &VirtioPciCap,
    hhdm_offset: u64,
) -> KernelResult<*mut u8> {
    let bar_phys = pci::bar_mmio_addr64(dev, cap.bar as usize).ok_or_else(|| {
        serial_println!(
            "{} BAR{} is not a mappable MMIO BAR (modern virtio requires MMIO)",
            tag,
            cap.bar
        );
        KernelError::NoSuchDevice
    })?;
    let region_phys = bar_phys + u64::from(cap.offset);
    let region_virt = region_phys + hhdm_offset;

    // Cover the whole region, but never fewer than one frame: a capability may
    // legitimately describe a region smaller than a page (the ISR region is a
    // single byte), and mapping zero frames would leave the pointer unbacked.
    let region_len = u64::from(cap.length).max(FRAME_SIZE as u64);
    let first_frame_phys = region_phys & !(FRAME_SIZE as u64 - 1);
    // The region may straddle a frame boundary even when it is short, because
    // `region_phys` is not frame-aligned; measure from the aligned base.
    let span = (region_phys - first_frame_phys) + region_len;
    let num_frames = (span as usize).div_ceil(FRAME_SIZE);

    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    for i in 0..num_frames {
        let frame_phys = first_frame_phys + (i as u64) * (FRAME_SIZE as u64);
        let frame_virt = frame_phys + hhdm_offset;
        let Some(frame) = PhysFrame::from_addr(frame_phys) else {
            serial_println!(
                "{} BAR{} frame {:#x} is not a valid physical frame",
                tag,
                cap.bar,
                frame_phys
            );
            return Err(KernelError::IoError);
        };
        let va = VirtAddr::new(frame_virt);
        // SAFETY: we are mapping a PCI BAR — device memory the firmware has
        // already assigned to this device — into the address it would occupy
        // in the direct map, uncached and writable.  No RAM mapping is
        // displaced because BAR space is carved out of the physical address
        // space above RAM, and the same pattern is used for the local APIC.
        let mapped = unsafe { page_table::map_frame(pml4_phys, va, frame, mmio_flags) };
        if let Err(e) = mapped {
            // Distinguish "already mapped" from a real failure: two config
            // regions frequently share a BAR and therefore a frame, and the
            // second map of a frame the first already mapped is expected.
            if e != KernelError::AlreadyExists {
                serial_println!(
                    "{} Failed to map BAR{} frame {:#x}: {:?}",
                    tag,
                    cap.bar,
                    frame_phys,
                    e
                );
                return Err(e);
            }
        }
        // SAFETY: flushing the TLB entry for a page we just mapped is always
        // sound, and is required before the new mapping is used.
        unsafe {
            page_table::flush_frame(va);
        }
    }

    Ok(region_virt as *mut u8)
}
