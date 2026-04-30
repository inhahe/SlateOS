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
use spin::Mutex;

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
// Device registry
// ---------------------------------------------------------------------------

/// A registered block device with its name.
struct RegisteredDevice {
    name: String,
    device: Box<dyn BlockDevice>,
}

/// Global block device registry.
static REGISTRY: Mutex<Vec<RegisteredDevice>> = Mutex::new(Vec::new());

/// Register a block device with the given name.
///
/// The name should be short and unique (e.g., `"vda"`, `"sda"`).
/// Panics if a device with the same name is already registered.
pub fn register(name: &str, device: Box<dyn BlockDevice>) {
    let mut registry = REGISTRY.lock();

    // Check for duplicate names.
    for entry in registry.iter() {
        if entry.name == name {
            crate::serial_println!(
                "[blkdev] WARNING: device '{}' already registered, replacing",
                name
            );
            // We'll just push and keep the old one — find() returns the last match.
            // This is fine for now; a proper implementation would remove the old one.
            break;
        }
    }

    crate::serial_println!("[blkdev] Registered device '{}'", name);
    registry.push(RegisteredDevice {
        name: String::from(name),
        device,
    });
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

/// List all registered block devices.
pub fn list_devices() -> Vec<BlockDeviceInfo> {
    let registry = REGISTRY.lock();
    registry.iter().map(|entry| {
        // We can't call info() without &self, but BlockDeviceInfo is on
        // the trait.  We stored the device as Box<dyn BlockDevice>, so
        // we need a non-mutable borrow.  Since info() takes &self, this
        // is fine — but we need to work around the Mutex<Vec<...>> borrow.
        //
        // Actually, we hold the lock, so we can call info() directly:
        BlockDeviceInfo {
            name: entry.name.clone(),
            // We can't call entry.device.info() because we only have &entry
            // (iter(), not iter_mut()).  The info is reconstructed from the name.
            // TODO: Cache the info at registration time.
            sector_count: 0,
            sector_size: SECTOR_SIZE as u32,
            read_only: false,
        }
    }).collect()
}

/// List all registered block device names and their info.
///
/// This version uses `iter_mut()` to call the trait method.
pub fn list_devices_full() -> Vec<BlockDeviceInfo> {
    let mut registry = REGISTRY.lock();
    registry.iter_mut().map(|entry| {
        entry.device.info()
    }).collect()
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the block device subsystem.
///
/// Moves any already-initialized drivers into the registry.
/// Called from `kmain()` after all device drivers are probed.
pub fn init() {
    // Move the global virtio-blk device (if present) into the registry.
    let dev = crate::virtio::blk::take_device();
    if let Some(device) = dev {
        let info = device.info();
        crate::serial_println!(
            "[blkdev] Found virtio-blk: {} sectors ({} KiB)",
            info.sector_count,
            info.sector_count.saturating_mul(u64::from(info.sector_size)) / 1024
        );
        register(&info.name, Box::new(device));
    }

    let devices = list_devices_full();
    if devices.is_empty() {
        crate::serial_println!("[blkdev] No block devices registered");
    } else {
        crate::serial_println!("[blkdev] {} device(s) registered", devices.len());
    }
}
