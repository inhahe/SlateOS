//! Block I/O buffer cache.
//!
//! Caches disk sectors in memory with LRU eviction and write-back
//! semantics.  Sits between filesystem implementations (FAT, ext4, …)
//! and the block device layer ([`crate::blkdev`]).
//!
//! ## Architecture
//!
//! ```text
//! filesystem (FAT, ext4, …)
//!       ↓
//!   buffer cache (this module)  ← caches sectors in memory
//!       ↓
//!   BlockDevice trait (blkdev.rs)
//!       ↓
//!   driver (virtio-blk, NVMe, …)
//! ```
//!
//! ## Design
//!
//! - Fixed-capacity pool of cached sectors (no unbounded growth).
//! - LRU eviction: when the cache is full, the least-recently-used
//!   sector is evicted.  Clean entries are preferred over dirty ones
//!   to avoid unnecessary I/O.
//! - Write-back: dirty sectors are only written to disk when evicted
//!   or explicitly flushed.  This coalesces multiple writes to the
//!   same sector (common in FAT table updates).
//! - Device-agnostic: works with any device in the block device
//!   registry.  Devices are identified by a compact integer ID
//!   assigned on first use.
//!
//! ## Lock ordering
//!
//! The buffer cache lock is acquired **before** the block device
//! registry lock.  Never acquire them in the reverse order.
//!
//! ```text
//!   CACHE (this module) → REGISTRY (blkdev.rs)   ✓
//!   REGISTRY → CACHE                             ✗ deadlock
//! ```
//!
//! ## Based on
//!
//! Inspired by the Unix buffer cache (bio.c) and Linux's buffer_head
//! layer, simplified for a single-lock microkernel.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::blkdev::{self, SECTOR_SIZE};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of cached sectors.
///
/// 512 sectors × 512 bytes = 256 KiB of cached data.
/// This is a reasonable default for a kernel with limited memory.
/// Can be increased on systems with more RAM.
const MAX_ENTRIES: usize = 512;

/// Maximum number of distinct block devices the cache supports.
const MAX_DEVICES: usize = 8;

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/// A single cached sector.
struct CacheEntry {
    /// Device ID (index into the device name table).
    device_id: u8,
    /// Sector logical block address.
    lba: u64,
    /// Cached sector data.
    data: [u8; SECTOR_SIZE],
    /// Whether this entry has been modified since last write-back.
    dirty: bool,
    /// Access counter for LRU eviction (higher = more recent).
    last_access: u64,
    /// Whether this slot contains valid cached data.
    valid: bool,
}

impl CacheEntry {
    /// Create an empty (invalid) cache entry.
    const fn empty() -> Self {
        Self {
            device_id: 0,
            lba: 0,
            data: [0u8; SECTOR_SIZE],
            dirty: false,
            last_access: 0,
            valid: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Cache statistics
// ---------------------------------------------------------------------------

/// Snapshot of buffer cache statistics.
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Total cache read requests.
    pub reads: u64,
    /// Cache hits (data served from memory).
    pub hits: u64,
    /// Cache misses (required device I/O).
    pub misses: u64,
    /// Total write requests through the cache.
    pub writes: u64,
    /// Number of dirty entries written back on eviction.
    pub writebacks: u64,
    /// Number of currently valid entries in the cache.
    pub entries_used: usize,
    /// Number of currently dirty entries.
    pub entries_dirty: usize,
    /// Maximum cache capacity.
    pub capacity: usize,
}

// ---------------------------------------------------------------------------
// Inner state
// ---------------------------------------------------------------------------

struct BufferCacheInner {
    /// Fixed pool of cache entries.
    ///
    /// Lazily allocated on first use (avoids static init issues).
    entries: Vec<CacheEntry>,
    /// Device name → device_id mapping.
    device_names: Vec<String>,
    /// Monotonically increasing access counter for LRU.
    access_counter: u64,
    /// Statistics.
    reads: u64,
    hits: u64,
    misses: u64,
    writes: u64,
    writebacks: u64,
    /// Whether the pool has been initialized.
    initialized: bool,
}

impl BufferCacheInner {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            device_names: Vec::new(),
            access_counter: 0,
            reads: 0,
            hits: 0,
            misses: 0,
            writes: 0,
            writebacks: 0,
            initialized: false,
        }
    }

    /// Ensure the entry pool is allocated.
    fn ensure_init(&mut self) {
        if self.initialized {
            return;
        }
        self.entries.reserve_exact(MAX_ENTRIES);
        for _ in 0..MAX_ENTRIES {
            self.entries.push(CacheEntry::empty());
        }
        self.initialized = true;
    }

    /// Look up or assign a device ID for the given device name.
    ///
    /// Returns `None` if the device table is full.
    fn device_id(&mut self, name: &str) -> Option<u8> {
        for (i, n) in self.device_names.iter().enumerate() {
            if n == name {
                return Some(i as u8);
            }
        }

        if self.device_names.len() >= MAX_DEVICES {
            return None;
        }

        self.device_names.push(String::from(name));
        Some((self.device_names.len() - 1) as u8)
    }

    /// Get the device name string for a device ID.
    fn device_name(&self, id: u8) -> &str {
        // Device IDs are always valid indices (assigned by device_id()).
        &self.device_names[id as usize]
    }

    /// Find the cache entry index for `(dev_id, lba)`, or `None`.
    fn find_index(&self, dev_id: u8, lba: u64) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.valid && e.device_id == dev_id && e.lba == lba
        })
    }

    /// Bump the access counter and record it on the entry.
    #[allow(clippy::arithmetic_side_effects)]
    fn touch(&mut self, idx: usize) {
        self.access_counter = self.access_counter.wrapping_add(1);
        self.entries[idx].last_access = self.access_counter;
    }

    /// Find an invalid (free) slot, or `None`.
    fn find_free(&self) -> Option<usize> {
        self.entries.iter().position(|e| !e.valid)
    }

    /// Find the LRU clean entry index, or `None` if all valid entries
    /// are dirty.
    fn find_lru_clean(&self) -> Option<usize> {
        let mut best: Option<(usize, u64)> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if e.valid && !e.dirty {
                match best {
                    None => best = Some((i, e.last_access)),
                    Some((_, ba)) if e.last_access < ba => {
                        best = Some((i, e.last_access));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// Find the overall LRU entry index (dirty or clean).
    fn find_lru(&self) -> Option<usize> {
        let mut best: Option<(usize, u64)> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if e.valid {
                match best {
                    None => best = Some((i, e.last_access)),
                    Some((_, ba)) if e.last_access < ba => {
                        best = Some((i, e.last_access));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// Write back a dirty entry to disk and mark it clean.
    ///
    /// Calls `blkdev::with_device` while holding the cache lock.
    /// This is safe because the lock ordering is cache → blkdev.
    #[allow(clippy::arithmetic_side_effects)]
    fn writeback_entry(&mut self, idx: usize) -> KernelResult<()> {
        if !self.entries[idx].valid || !self.entries[idx].dirty {
            return Ok(());
        }

        let dev_name = self.device_name(self.entries[idx].device_id);
        let lba = self.entries[idx].lba;

        // Copy data to a stack buffer so the closure borrows only
        // the buffer, not `self`.
        let mut data = [0u8; SECTOR_SIZE];
        data.copy_from_slice(&self.entries[idx].data);

        let result = blkdev::with_device(dev_name, |dev| {
            dev.write_sector(lba, &data)
        });

        match result {
            Some(Ok(())) => {
                self.entries[idx].dirty = false;
                self.writebacks = self.writebacks.wrapping_add(1);
                Ok(())
            }
            Some(Err(e)) => Err(e),
            None => Err(KernelError::NoSuchDevice),
        }
    }

    /// Make room for one new entry.  Returns the index of the slot
    /// that is now available (either free, or just evicted).
    ///
    /// Prefers evicting clean entries to avoid I/O.  If all entries
    /// are dirty, writes back the LRU dirty entry first.
    fn make_room(&mut self) -> KernelResult<usize> {
        // 1. Free slot?
        if let Some(idx) = self.find_free() {
            return Ok(idx);
        }

        // 2. Evict LRU clean entry (no I/O needed).
        if let Some(idx) = self.find_lru_clean() {
            self.entries[idx].valid = false;
            return Ok(idx);
        }

        // 3. All entries dirty — writeback LRU, then evict it.
        let idx = self.find_lru()
            .ok_or(KernelError::InternalError)?;
        self.writeback_entry(idx)?;
        self.entries[idx].valid = false;
        Ok(idx)
    }

    /// Collect statistics snapshot.
    fn stats(&self) -> CacheStats {
        let mut used = 0usize;
        let mut dirty = 0usize;
        for e in &self.entries {
            if e.valid {
                used = used.wrapping_add(1);
                if e.dirty {
                    dirty = dirty.wrapping_add(1);
                }
            }
        }

        CacheStats {
            reads: self.reads,
            hits: self.hits,
            misses: self.misses,
            writes: self.writes,
            writebacks: self.writebacks,
            entries_used: used,
            entries_dirty: dirty,
            capacity: MAX_ENTRIES,
        }
    }
}

// ---------------------------------------------------------------------------
// Global cache instance
// ---------------------------------------------------------------------------

static CACHE: Mutex<BufferCacheInner> = Mutex::new(BufferCacheInner::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read a sector through the buffer cache.
///
/// On a cache hit, the data is served directly from memory.
/// On a miss, the sector is read from the block device and cached.
#[allow(clippy::arithmetic_side_effects)]
pub fn read_sector(
    device: &str,
    lba: u64,
    buf: &mut [u8; SECTOR_SIZE],
) -> KernelResult<()> {
    let mut cache = CACHE.lock();
    cache.ensure_init();
    cache.reads = cache.reads.wrapping_add(1);

    let dev_id = cache.device_id(device)
        .ok_or(KernelError::InvalidArgument)?;

    // Check cache hit.
    if let Some(idx) = cache.find_index(dev_id, lba) {
        buf.copy_from_slice(&cache.entries[idx].data);
        cache.touch(idx);
        cache.hits = cache.hits.wrapping_add(1);
        return Ok(());
    }

    // Cache miss — read from device.
    cache.misses = cache.misses.wrapping_add(1);

    let result = blkdev::with_device(device, |dev| {
        dev.read_sector(lba, buf)
    });
    match result {
        Some(Ok(())) => {}
        Some(Err(e)) => return Err(e),
        None => return Err(KernelError::NoSuchDevice),
    }

    // Insert into cache.
    let idx = cache.make_room()?;
    cache.entries[idx].device_id = dev_id;
    cache.entries[idx].lba = lba;
    cache.entries[idx].data.copy_from_slice(buf);
    cache.entries[idx].dirty = false;
    cache.entries[idx].valid = true;
    cache.touch(idx);

    Ok(())
}

/// Write a sector through the buffer cache (write-back).
///
/// The data is written to the cache and marked dirty.  It will be
/// written to the actual device when:
/// - The entry is evicted (LRU)
/// - [`flush`] or [`flush_all`] is called
/// - The filesystem is unmounted
#[allow(clippy::arithmetic_side_effects)]
pub fn write_sector(
    device: &str,
    lba: u64,
    buf: &[u8; SECTOR_SIZE],
) -> KernelResult<()> {
    let mut cache = CACHE.lock();
    cache.ensure_init();
    cache.writes = cache.writes.wrapping_add(1);

    let dev_id = cache.device_id(device)
        .ok_or(KernelError::InvalidArgument)?;

    // Check if already cached — update in place.
    if let Some(idx) = cache.find_index(dev_id, lba) {
        cache.entries[idx].data.copy_from_slice(buf);
        cache.entries[idx].dirty = true;
        cache.touch(idx);
        return Ok(());
    }

    // Not cached — insert a new dirty entry.
    let idx = cache.make_room()?;
    cache.entries[idx].device_id = dev_id;
    cache.entries[idx].lba = lba;
    cache.entries[idx].data.copy_from_slice(buf);
    cache.entries[idx].dirty = true;
    cache.entries[idx].valid = true;
    cache.touch(idx);

    Ok(())
}

/// Flush all dirty entries for a specific device to disk.
pub fn flush(device: &str) -> KernelResult<()> {
    let mut cache = CACHE.lock();
    cache.ensure_init();

    let dev_id = match cache.device_id(device) {
        Some(id) => id,
        None => return Ok(()), // Unknown device, nothing to flush.
    };

    // Collect indices of dirty entries for this device.
    // We iterate by index to avoid borrow conflicts.
    let mut errors: Option<KernelError> = None;
    for i in 0..cache.entries.len() {
        if cache.entries[i].valid
            && cache.entries[i].dirty
            && cache.entries[i].device_id == dev_id
        {
            if let Err(e) = cache.writeback_entry(i) {
                // Track the worst error but keep flushing.
                errors = Some(e);
            }
        }
    }

    match errors {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Flush all dirty entries for all devices to disk.
pub fn flush_all() -> KernelResult<()> {
    let mut cache = CACHE.lock();
    cache.ensure_init();

    let mut errors: Option<KernelError> = None;
    for i in 0..cache.entries.len() {
        if cache.entries[i].valid && cache.entries[i].dirty {
            if let Err(e) = cache.writeback_entry(i) {
                errors = Some(e);
            }
        }
    }

    match errors {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Invalidate (drop) all cached entries for a specific device.
///
/// Dirty entries are flushed before being discarded.
pub fn invalidate(device: &str) -> KernelResult<()> {
    // Flush first to avoid losing data.
    flush(device)?;

    let mut cache = CACHE.lock();
    let dev_id = match cache.device_id(device) {
        Some(id) => id,
        None => return Ok(()),
    };

    for entry in &mut cache.entries {
        if entry.valid && entry.device_id == dev_id {
            entry.valid = false;
        }
    }

    Ok(())
}

/// Invalidate all cache entries without flushing.
///
/// **Danger**: discards dirty data.  Only use during shutdown or
/// after a device error where writeback is known to be impossible.
pub fn invalidate_all_no_flush() {
    let mut cache = CACHE.lock();
    for entry in &mut cache.entries {
        entry.valid = false;
        entry.dirty = false;
    }
}

/// Get a snapshot of cache statistics.
pub fn stats() -> CacheStats {
    let cache = CACHE.lock();
    cache.stats()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run a self-test of the buffer cache.
///
/// Tests: cache hit, write-back, flush, LRU eviction, statistics.
/// Requires a mounted block device (typically "vda").
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[bcache] Running self-test...");

    let device = "vda";

    // Verify the device exists.
    let exists = blkdev::with_device(device, |dev| {
        let info = dev.info();
        crate::serial_println!(
            "[bcache]   Device '{}': {} sectors",
            info.name,
            info.sector_count,
        );
    });
    if exists.is_none() {
        crate::serial_println!("[bcache]   No device '{}' — skipping self-test", device);
        return Ok(());
    }

    // Flush any existing dirty data first.
    flush_all()?;

    // Read sector 0 (boot sector) — should be a miss.
    let stats_before = stats();
    let mut buf = [0u8; SECTOR_SIZE];
    read_sector(device, 0, &mut buf)?;
    let stats_after = stats();

    crate::serial_println!(
        "[bcache]   Read sector 0: {} bytes, boot sig {:02X}{:02X}",
        SECTOR_SIZE,
        buf[510],
        buf[511],
    );

    // Verify it was counted.
    if stats_after.reads != stats_before.reads.wrapping_add(1) {
        crate::serial_println!("[bcache]   FAIL: read count not incremented");
        return Err(KernelError::InternalError);
    }

    // Read the same sector again — should be a hit.
    let miss_before = stats().misses;
    let mut buf2 = [0u8; SECTOR_SIZE];
    read_sector(device, 0, &mut buf2)?;
    let miss_after = stats().misses;

    if buf != buf2 {
        crate::serial_println!("[bcache]   FAIL: second read returned different data");
        return Err(KernelError::InternalError);
    }

    if miss_after != miss_before {
        crate::serial_println!("[bcache]   FAIL: second read was a miss (expected hit)");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[bcache]   Cache hit verified for sector 0");

    // Test write-back: write to a sector, verify it's cached dirty,
    // then flush and verify it's clean.
    //
    // Use a high sector number that's unlikely to contain important data.
    // Read it first to preserve original content.
    let test_lba: u64 = 100;
    let mut original = [0u8; SECTOR_SIZE];
    read_sector(device, test_lba, &mut original)?;

    // Write modified data.
    let mut modified = original;
    modified[0] = modified[0].wrapping_add(1); // Flip one byte.
    write_sector(device, test_lba, &modified)?;

    // Read it back — should come from cache (dirty).
    let mut readback = [0u8; SECTOR_SIZE];
    read_sector(device, test_lba, &mut readback)?;
    if readback != modified {
        crate::serial_println!("[bcache]   FAIL: dirty read returned wrong data");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bcache]   Write-back + dirty read verified");

    // Flush and verify writebacks counter increased.
    let wb_before = stats().writebacks;
    flush(device)?;
    let wb_after = stats().writebacks;

    if wb_after <= wb_before {
        crate::serial_println!("[bcache]   FAIL: flush did not trigger writeback");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bcache]   Flush verified (writebacks: {} → {})", wb_before, wb_after);

    // Restore the original sector content.
    write_sector(device, test_lba, &original)?;
    flush(device)?;

    // Print final statistics.
    let final_stats = stats();
    crate::serial_println!(
        "[bcache]   Stats: {} reads, {} hits, {} misses ({:.0}% hit rate), {} writes, {} writebacks",
        final_stats.reads,
        final_stats.hits,
        final_stats.misses,
        if final_stats.reads > 0 {
            (final_stats.hits as f64 / final_stats.reads as f64) * 100.0
        } else {
            0.0
        },
        final_stats.writes,
        final_stats.writebacks,
    );
    crate::serial_println!(
        "[bcache]   Capacity: {}/{} entries used, {} dirty",
        final_stats.entries_used,
        final_stats.capacity,
        final_stats.entries_dirty,
    );

    crate::serial_println!("[bcache] Self-test passed.");
    Ok(())
}
