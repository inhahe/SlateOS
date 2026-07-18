//! Block device abstraction layer.
//!
//! Provides a uniform interface for block-level storage devices
//! (virtio-blk, NVMe, AHCI/SATA).  The VFS and filesystem drivers
//! interact with storage exclusively through the [`BlockDevice`] trait,
//! never through driver-specific APIs.
//!
//! ## Architecture
//!
//! ```text
//! VFS / filesystem
//!       ↓
//!   BlockDevice trait  ← this module
//!       ↓
//!   driver (virtio-blk, NVMe, …)
//! ```
//!
//! ## Device registry
//!
//! Devices are registered with a short name (e.g., `"vda"`, `"sda"`)
//! and can be looked up by name.  The registry stores trait objects
//! behind a mutex — fine for the current single-CPU design.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Block device trait
// ---------------------------------------------------------------------------

/// Sector size in bytes (512 for all current devices).
pub const SECTOR_SIZE: usize = 512;

/// Information about a block device.
#[derive(Debug, Clone)]
pub struct BlockDeviceInfo {
    /// Human-readable device name (e.g., `"vda"`, `"sda"`).
    pub name: String,
    /// Total capacity in sectors.
    pub sector_count: u64,
    /// Bytes per sector (always 512 for now).
    pub sector_size: u32,
    /// Whether the device is read-only.
    pub read_only: bool,
}

/// Trait for block-level storage devices.
///
/// All methods take `&mut self` because device I/O inherently mutates
/// internal state (DMA buffers, queue indices, etc.).
///
/// # Sector addressing
///
/// Sectors are numbered from 0 to `info().sector_count - 1`.
/// Each sector is `info().sector_size` bytes (typically 512).
pub trait BlockDevice: Send {
    /// Return metadata about this device.
    fn info(&self) -> BlockDeviceInfo;

    /// Read a single sector into `buf`.
    ///
    /// `buf` must be exactly [`SECTOR_SIZE`] bytes.
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()>;

    /// Write a single sector from `buf`.
    ///
    /// `buf` must be exactly [`SECTOR_SIZE`] bytes.
    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()>;

    /// Read multiple contiguous sectors into `buf`.
    ///
    /// `buf` must be at least `count * SECTOR_SIZE` bytes.
    /// Default implementation calls [`read_sector`](BlockDevice::read_sector)
    /// in a loop; drivers may override for efficiency.
    // Multi-sector arithmetic uses checked ops on small values.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_sectors(&mut self, start_lba: u64, count: u32, buf: &mut [u8]) -> KernelResult<()> {
        let needed = (count as usize).checked_mul(SECTOR_SIZE)
            .ok_or(KernelError::InvalidArgument)?;
        if buf.len() < needed {
            return Err(KernelError::InvalidArgument);
        }

        let mut sector_buf = [0u8; SECTOR_SIZE];
        for i in 0..count {
            let lba = start_lba.checked_add(u64::from(i))
                .ok_or(KernelError::InvalidArgument)?;
            self.read_sector(lba, &mut sector_buf)?;

            let offset = (i as usize) * SECTOR_SIZE;
            if let Some(dest) = buf.get_mut(offset..offset + SECTOR_SIZE) {
                dest.copy_from_slice(&sector_buf);
            } else {
                return Err(KernelError::InvalidArgument);
            }
        }
        Ok(())
    }

    /// Write multiple contiguous sectors from `buf`.
    ///
    /// `buf` must be at least `count * SECTOR_SIZE` bytes.
    /// Default implementation calls [`write_sector`](BlockDevice::write_sector)
    /// in a loop; drivers may override for efficiency.
    #[allow(clippy::arithmetic_side_effects)]
    fn write_sectors(&mut self, start_lba: u64, count: u32, buf: &[u8]) -> KernelResult<()> {
        let needed = (count as usize).checked_mul(SECTOR_SIZE)
            .ok_or(KernelError::InvalidArgument)?;
        if buf.len() < needed {
            return Err(KernelError::InvalidArgument);
        }

        let mut sector_buf = [0u8; SECTOR_SIZE];
        for i in 0..count {
            let lba = start_lba.checked_add(u64::from(i))
                .ok_or(KernelError::InvalidArgument)?;

            let offset = (i as usize) * SECTOR_SIZE;
            if let Some(src) = buf.get(offset..offset + SECTOR_SIZE) {
                sector_buf.copy_from_slice(src);
            } else {
                return Err(KernelError::InvalidArgument);
            }
            self.write_sector(lba, &sector_buf)?;
        }
        Ok(())
    }

    /// Whether this device supports discard (TRIM/UNMAP).
    ///
    /// Filesystems and the `fstrim` path query this before attempting
    /// [`discard`](BlockDevice::discard); a `false` return lets fstrim treat
    /// the device as "nothing to do" (a successful no-op) rather than an
    /// error.  Default: not supported.
    fn supports_discard(&self) -> bool {
        false
    }

    /// Discard (TRIM/UNMAP) a run of `count` contiguous sectors starting at
    /// `start_lba`, hinting to the device that they no longer hold useful
    /// data.
    ///
    /// Discard is advisory: a conforming implementation may ignore the hint,
    /// zero the range, or actually release backing flash.  After a successful
    /// discard the contents of the range are unspecified (commonly read back
    /// as zero, but callers must not rely on any particular value).
    ///
    /// The default implementation reports [`KernelError::NotSupported`];
    /// drivers that can issue TRIM override both this and
    /// [`supports_discard`](BlockDevice::supports_discard).
    fn discard(&mut self, _start_lba: u64, _count: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }
}

// ---------------------------------------------------------------------------
// BlockDevice implementation for VirtioBlkDevice
// ---------------------------------------------------------------------------

impl BlockDevice for crate::virtio::blk::VirtioBlkDevice {
    fn info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            name: String::from("vda"),
            sector_count: self.capacity(),
            sector_size: SECTOR_SIZE as u32,
            read_only: false,
        }
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        self.read_sector(lba, buf)
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        self.write_sector(lba, buf)
    }
}

// ---------------------------------------------------------------------------
// I/O activity tracking (for disk-idle heuristic)
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU64, Ordering};

/// Total read operations across all block devices.
static TOTAL_READS: AtomicU64 = AtomicU64::new(0);
/// Total write operations across all block devices.
static TOTAL_WRITES: AtomicU64 = AtomicU64::new(0);
/// Timestamp (APIC ticks) of the most recent I/O operation.
static LAST_IO_TICK: AtomicU64 = AtomicU64::new(0);

/// Record that a block I/O operation occurred.
///
/// Called from `with_device()` and directly by the cache layer.
/// Updates the activity counters and last-I/O timestamp.
#[inline]
pub fn record_io(is_write: bool) {
    if is_write {
        TOTAL_WRITES.fetch_add(1, Ordering::Relaxed);
    } else {
        TOTAL_READS.fetch_add(1, Ordering::Relaxed);
    }
    LAST_IO_TICK.store(crate::apic::tick_count(), Ordering::Release);
}

/// Check whether all block devices have been idle for at least `ticks`.
///
/// Used by the service manager to detect when an application has finished
/// loading from disk (disk goes quiet after initial read burst).
///
/// A reasonable threshold is 200–300 ticks at 100 Hz timer = 2–3 seconds
/// of disk silence.
#[must_use]
#[allow(dead_code)]
pub fn is_idle_for(ticks: u64) -> bool {
    let last = LAST_IO_TICK.load(Ordering::Acquire);
    if last == 0 {
        // No I/O ever recorded — trivially idle.
        return true;
    }
    let now = crate::apic::tick_count();
    now.saturating_sub(last) >= ticks
}

/// Get the tick count of the most recent I/O operation.
#[must_use]
#[allow(dead_code)]
pub fn last_io_tick() -> u64 {
    LAST_IO_TICK.load(Ordering::Relaxed)
}

/// Block I/O statistics.
#[derive(Debug, Clone, Copy)]
pub struct IoStats {
    pub total_reads: u64,
    pub total_writes: u64,
    pub last_io_tick: u64,
}

/// Get block I/O statistics.
#[must_use]
pub fn io_stats() -> IoStats {
    IoStats {
        total_reads: TOTAL_READS.load(Ordering::Relaxed),
        total_writes: TOTAL_WRITES.load(Ordering::Relaxed),
        last_io_tick: LAST_IO_TICK.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Device registry
// ---------------------------------------------------------------------------

/// A registered block device with its name.
struct RegisteredDevice {
    name: String,
    device: Box<dyn BlockDevice>,
    /// Device metadata, snapshotted at registration time.
    ///
    /// Captured once via [`BlockDevice::info`] (which needs `&self`) so
    /// that read-only listing paths holding only a shared borrow of the
    /// registry — [`list_devices`] — can return the *real* capacity /
    /// read-only flag instead of fabricated zeros.  The `name` field is
    /// overridden with the registry-assigned name (authoritative — a
    /// driver may hard-code its own `info().name`, e.g. every virtio-blk
    /// device reports `"vda"`, while the registry assigns vda/vdb/vdc).
    /// Block-device geometry is fixed after init for all current drivers
    /// (virtio-blk, AHCI, NVMe), so the snapshot never goes stale.
    info: BlockDeviceInfo,
}

/// Global block device registry.
static REGISTRY: Mutex<Vec<RegisteredDevice>> = Mutex::new(Vec::new());

/// Register a block device with the given name.
///
/// The name should be short and unique (e.g., `"vda"`, `"sda"`).
///
/// If a device with the same name is already registered, the old entry is
/// removed and replaced by the new one.  This matters because lookups
/// ([`with_device`]) return the *first* matching entry: leaving a stale
/// duplicate behind would shadow the new device, so re-registration must
/// drop the old entry rather than append alongside it.
pub fn register(name: &str, device: Box<dyn BlockDevice>) {
    let mut registry = REGISTRY.lock();

    // Replace on duplicate name: drop any existing entry so the freshly
    // registered device becomes the one lookups resolve to.
    if registry.iter().any(|entry| entry.name == name) {
        crate::serial_println!(
            "[blkdev] WARNING: device '{}' already registered, replacing",
            name
        );
        registry.retain(|entry| entry.name != name);
    }

    // Snapshot the device metadata now, while we still have direct access
    // to the trait object, and stamp it with the registry-assigned name
    // (authoritative over any name the driver hard-codes in its own info()).
    let mut info = device.info();
    info.name = String::from(name);

    crate::serial_println!("[blkdev] Registered device '{}'", name);
    registry.push(RegisteredDevice {
        name: String::from(name),
        device,
        info,
    });
}

/// Remove a registered block device by name.
///
/// Returns `true` if a matching device was found and removed.  Intended for
/// self-tests that register a temporary RAM-backed device and need to clean
/// it up afterwards; production storage devices live for the lifetime of the
/// system and are never unregistered.
pub fn unregister(name: &str) -> bool {
    let mut registry = REGISTRY.lock();
    let before = registry.len();
    registry.retain(|entry| entry.name != name);
    let removed = registry.len() != before;
    if removed {
        crate::serial_println!("[blkdev] Unregistered device '{}'", name);
    }
    removed
}

/// Execute a closure with a named block device.
///
/// Returns `None` if no device with that name is registered.
pub fn with_device<F, R>(name: &str, f: F) -> Option<R>
where
    F: FnOnce(&mut dyn BlockDevice) -> R,
{
    let mut registry = REGISTRY.lock();
    for entry in registry.iter_mut() {
        if entry.name == name {
            return Some(f(entry.device.as_mut()));
        }
    }
    None
}

/// Query whether the named device supports discard (TRIM/UNMAP).
///
/// Returns `None` if no device with that name is registered, otherwise
/// `Some(true|false)` per [`BlockDevice::supports_discard`].
pub fn supports_discard(name: &str) -> Option<bool> {
    let registry = REGISTRY.lock();
    registry
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.device.supports_discard())
}

/// Issue a discard (TRIM/UNMAP) for `count` sectors starting at `start_lba`
/// on the named device.
///
/// Returns `None` if no device with that name is registered; otherwise the
/// inner `KernelResult` reports whether the discard succeeded (devices that
/// do not support discard return [`KernelError::NotSupported`]).
pub fn discard(name: &str, start_lba: u64, count: u64) -> Option<KernelResult<()>> {
    with_device(name, |dev| dev.discard(start_lba, count))
}

/// List all registered block devices.
///
/// Returns the metadata snapshotted at registration time (real capacity,
/// sector size, and read-only flag — see [`RegisteredDevice::info`]).
pub fn list_devices() -> Vec<BlockDeviceInfo> {
    let registry = REGISTRY.lock();
    registry.iter().map(|entry| entry.info.clone()).collect()
}

/// List all registered block device names and their info.
///
/// Equivalent to [`list_devices`]; both now return the registration-time
/// snapshot.  Retained as a separate name for existing callers.
pub fn list_devices_full() -> Vec<BlockDeviceInfo> {
    list_devices()
}

// ---------------------------------------------------------------------------
// In-memory block device (for self-tests)
// ---------------------------------------------------------------------------

/// A RAM-backed block device whose contents live in a `Vec<u8>`.
///
/// This exists so disk-administration code paths (mkfs, fsck, partitioning)
/// can be exercised in boot self-tests without a real, data-bearing device —
/// formatting a real disk in a self-test would destroy user data.  A test
/// constructs one, [`register`]s it under a scratch name, runs the operation
/// under test, then [`unregister`]s it.
pub struct RamBlockDevice {
    /// Backing storage, exactly `sector_count * SECTOR_SIZE` bytes.
    data: Vec<u8>,
    /// Total capacity in sectors.
    sector_count: u64,
    /// Whether writes are rejected.
    read_only: bool,
}

impl RamBlockDevice {
    /// Create a zeroed RAM disk of `sector_count` 512-byte sectors.
    ///
    /// Returns `OutOfMemory`-style failure implicitly via allocation; the
    /// caller picks a small size (a few MiB) for tests.
    #[must_use]
    pub fn new(sector_count: u64) -> Self {
        let bytes = usize::try_from(sector_count)
            .ok()
            .and_then(|s| s.checked_mul(SECTOR_SIZE))
            .unwrap_or(0);
        Self {
            data: alloc::vec![0u8; bytes],
            sector_count,
            read_only: false,
        }
    }

    /// Byte offset of sector `lba`, or `None` if it lies outside the device.
    fn sector_range(&self, lba: u64) -> Option<(usize, usize)> {
        let start = usize::try_from(lba)
            .ok()?
            .checked_mul(SECTOR_SIZE)?;
        let end = start.checked_add(SECTOR_SIZE)?;
        if end > self.data.len() {
            return None;
        }
        Some((start, end))
    }
}

impl BlockDevice for RamBlockDevice {
    fn info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            name: String::new(), // overridden by the registry on register().
            sector_count: self.sector_count,
            sector_size: SECTOR_SIZE as u32,
            read_only: self.read_only,
        }
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        let (start, end) = self
            .sector_range(lba)
            .ok_or(KernelError::InvalidArgument)?;
        let src = self
            .data
            .get(start..end)
            .ok_or(KernelError::InvalidArgument)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        if self.read_only {
            return Err(KernelError::ReadOnlyFilesystem);
        }
        let (start, end) = self
            .sector_range(lba)
            .ok_or(KernelError::InvalidArgument)?;
        let dst = self
            .data
            .get_mut(start..end)
            .ok_or(KernelError::InvalidArgument)?;
        dst.copy_from_slice(buf);
        Ok(())
    }

    fn supports_discard(&self) -> bool {
        // The RAM disk models discard by zeroing the range, so it always
        // "supports" it (and reads back as zero afterwards — within the
        // unspecified-contents allowance of the discard contract).
        !self.read_only
    }

    fn discard(&mut self, start_lba: u64, count: u64) -> KernelResult<()> {
        if self.read_only {
            return Err(KernelError::ReadOnlyFilesystem);
        }
        if count == 0 {
            return Ok(());
        }
        // Compute the byte range, rejecting any overflow or out-of-bounds
        // request rather than silently truncating.
        let end_lba = start_lba
            .checked_add(count)
            .ok_or(KernelError::InvalidArgument)?;
        if end_lba > self.sector_count {
            return Err(KernelError::InvalidArgument);
        }
        let (start, _) = self
            .sector_range(start_lba)
            .ok_or(KernelError::InvalidArgument)?;
        let byte_len = usize::try_from(count)
            .ok()
            .and_then(|c| c.checked_mul(SECTOR_SIZE))
            .ok_or(KernelError::InvalidArgument)?;
        let end = start
            .checked_add(byte_len)
            .ok_or(KernelError::InvalidArgument)?;
        let dst = self
            .data
            .get_mut(start..end)
            .ok_or(KernelError::InvalidArgument)?;
        dst.fill(0);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Discard self-test
// ---------------------------------------------------------------------------

/// Boot self-test for the block-layer discard primitive.
///
/// Registers a small scratch [`RamBlockDevice`], fills a few sectors with a
/// non-zero pattern, discards a sub-range through the registry helper, and
/// verifies that (a) the discarded sectors read back as zero, (b) the
/// surrounding sectors are untouched, and (c) out-of-bounds / overflow
/// requests are rejected.  Runs on the bare-metal target where `#[cfg(test)]`
/// does not, so the discard path is exercised on every boot.
pub fn self_test_discard() -> KernelResult<()> {
    crate::serial_println!("[blkdev] Running discard self-test...");

    let dev = "disctest0";
    // Clean up any leftovers from a previous run.
    let _ = unregister(dev);

    // 64 sectors * 512 B = 32 KiB scratch RAM disk.
    register(dev, Box::new(RamBlockDevice::new(64)));

    let result = (|| -> KernelResult<()> {
        // Discard must advertise as supported on a writable RAM disk.
        if supports_discard(dev) != Some(true) {
            crate::serial_println!("[blkdev]   FAIL: RAM disk reports discard unsupported");
            return Err(KernelError::InternalError);
        }

        // Fill sectors 0..8 with 0xAB so we can tell discarded from untouched.
        let pattern = [0xABu8; SECTOR_SIZE];
        for lba in 0..8u64 {
            with_device(dev, |d| d.write_sector(lba, &pattern))
                .ok_or(KernelError::NotFound)??;
        }

        // Discard the middle run: sectors 2..6 (count = 4).
        discard(dev, 2, 4).ok_or(KernelError::NotFound)??;

        // Sectors 2..6 must now read back zero; 0..2 and 6..8 stay 0xAB.
        for lba in 0..8u64 {
            let mut buf = [0u8; SECTOR_SIZE];
            with_device(dev, |d| d.read_sector(lba, &mut buf))
                .ok_or(KernelError::NotFound)??;
            let expect_zero = (2..6).contains(&lba);
            let ok = if expect_zero {
                buf.iter().all(|&b| b == 0)
            } else {
                buf.iter().all(|&b| b == 0xAB)
            };
            if !ok {
                crate::serial_println!(
                    "[blkdev]   FAIL: sector {} has wrong contents after discard",
                    lba
                );
                return Err(KernelError::InternalError);
            }
        }

        // Out-of-bounds discard (past the 64-sector device) must be rejected.
        if discard(dev, 60, 8).ok_or(KernelError::NotFound)?.is_ok() {
            crate::serial_println!("[blkdev]   FAIL: out-of-bounds discard was accepted");
            return Err(KernelError::InternalError);
        }
        // Overflow at the u64 boundary must be rejected, not wrap.
        if discard(dev, u64::MAX, 2).ok_or(KernelError::NotFound)?.is_ok() {
            crate::serial_println!("[blkdev]   FAIL: overflowing discard was accepted");
            return Err(KernelError::InternalError);
        }
        // Zero-length discard is a successful no-op.
        discard(dev, 0, 0).ok_or(KernelError::NotFound)??;

        Ok(())
    })();

    let _ = unregister(dev);

    match result {
        Ok(()) => {
            crate::serial_println!("[blkdev]   discard self-test OK");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the block device subsystem.
///
/// Moves any already-initialized drivers into the registry.
/// Called from `kmain()` after all device drivers are probed.
#[allow(dead_code)]
pub fn init() {
    // Move the global virtio-blk device (if present) into the registry.
    // This handles the single-device path used by virtio::blk::init().
    let dev = crate::virtio::blk::take_device();
    if let Some(device) = dev {
        let cap = device.capacity();
        crate::serial_println!(
            "[blkdev] Found virtio-blk: {} sectors ({} KiB)",
            cap,
            cap.saturating_mul(SECTOR_SIZE as u64) / 1024
        );
        register("vda", Box::new(device));
    }

    let devices = list_devices_full();
    if devices.is_empty() {
        crate::serial_println!("[blkdev] No block devices registered");
    } else {
        crate::serial_println!("[blkdev] {} device(s) registered", devices.len());
    }
}

/// Initialize block devices by discovering ALL virtio-blk devices.
///
/// Unlike [`init()`] which only takes the single pre-probed device,
/// this function probes the PCI bus for every virtio-blk device and
/// registers them as vda, vdb, vdc, etc.
///
/// Call this instead of `init()` when multi-device support is needed
/// (e.g., QEMU with disk.img + ext4_test.img + swap.img).
pub fn init_multi(hhdm_offset: u64) {
    let devices = crate::virtio::blk::probe_all(hhdm_offset);

    for (i, device) in devices.into_iter().enumerate() {
        // Generate name: vda, vdb, vdc, ...
        let suffix = b'a'.checked_add(i as u8).unwrap_or(b'z');
        let name = alloc::format!("vd{}", suffix as char);

        let cap = device.capacity();
        crate::serial_println!(
            "[blkdev] Registering '{}': {} sectors ({} KiB)",
            name,
            cap,
            cap.saturating_mul(SECTOR_SIZE as u64) / 1024
        );
        register(&name, Box::new(device));
    }

    let devices = list_devices_full();
    if devices.is_empty() {
        crate::serial_println!("[blkdev] No block devices registered");
    } else {
        crate::serial_println!("[blkdev] {} device(s) registered", devices.len());
    }
}
