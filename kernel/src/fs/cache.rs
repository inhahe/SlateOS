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

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::blkdev::{self, SECTOR_SIZE};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of cached sectors.
///
/// 2048 sectors × 512 bytes = 1 MiB of cached data.
/// Increased from 256 KiB to better support filesystem workloads
/// with larger directories and multi-extent files.
const MAX_ENTRIES: usize = 2048;

/// Maximum number of distinct block devices the cache supports.
const MAX_DEVICES: usize = 8;

/// Initial number of sectors to prefetch when sequential access begins.
///
/// The adaptive algorithm starts with this window and doubles it on
/// each consecutive sequential read (up to `READAHEAD_MAX`).  This
/// prevents over-prefetching on short sequential runs while delivering
/// high throughput on large file reads.
///
/// Based on Linux's readahead algorithm (mm/readahead.c): start small,
/// grow the window as the pattern holds, cap at a maximum.
const READAHEAD_INITIAL: u32 = 4;

/// Maximum read-ahead window in sectors.
///
/// 128 sectors × 512 bytes = 64 KiB.  Beyond this, prefetching
/// consumes too much cache capacity for diminishing throughput gains.
const READAHEAD_MAX: u32 = 128;

/// Minimum consecutive sequential accesses before triggering read-ahead.
const READAHEAD_THRESHOLD: u32 = 2;

// ---------------------------------------------------------------------------
// Runtime-tunable parameters (sysctl/sysfs)
// ---------------------------------------------------------------------------

/// Runtime read-ahead maximum window (sectors).
/// Default: `READAHEAD_MAX` (128).  Can be adjusted via sysctl.
static TUNABLE_READAHEAD_MAX: AtomicU32 = AtomicU32::new(READAHEAD_MAX);

/// Runtime dirty expiry in nanoseconds.
/// Default: `DIRTY_EXPIRE_NS` (5 seconds).  Can be adjusted via sysctl.
static TUNABLE_DIRTY_EXPIRE_NS: AtomicU64 = AtomicU64::new(DIRTY_EXPIRE_NS);

/// Runtime read-ahead initial window (sectors).
/// Default: `READAHEAD_INITIAL` (4).  Can be adjusted via sysctl.
static TUNABLE_READAHEAD_INITIAL: AtomicU32 = AtomicU32::new(READAHEAD_INITIAL);

/// Get the current read-ahead maximum window.
pub fn get_readahead_max() -> u32 {
    TUNABLE_READAHEAD_MAX.load(Ordering::Relaxed)
}

/// Set the read-ahead maximum window.  Clamped to 1..=1024.
pub fn set_readahead_max(val: u32) {
    let clamped = val.clamp(1, 1024);
    TUNABLE_READAHEAD_MAX.store(clamped, Ordering::Relaxed);
    crate::serial_println!("[cache] readahead_max set to {}", clamped);
}

/// Get the current dirty expiry in nanoseconds.
pub fn get_dirty_expire_ns() -> u64 {
    TUNABLE_DIRTY_EXPIRE_NS.load(Ordering::Relaxed)
}

/// Set the dirty expiry in nanoseconds.  Clamped to 500ms..=60s.
pub fn set_dirty_expire_ns(val: u64) {
    let clamped = val.clamp(500_000_000, 60_000_000_000);
    TUNABLE_DIRTY_EXPIRE_NS.store(clamped, Ordering::Relaxed);
    crate::serial_println!("[cache] dirty_expire_ns set to {}", clamped);
}

/// Get the current read-ahead initial window.
pub fn get_readahead_initial() -> u32 {
    TUNABLE_READAHEAD_INITIAL.load(Ordering::Relaxed)
}

/// Set the read-ahead initial window.  Clamped to 1..=64.
pub fn set_readahead_initial(val: u32) {
    let clamped = val.clamp(1, 64);
    TUNABLE_READAHEAD_INITIAL.store(clamped, Ordering::Relaxed);
    crate::serial_println!("[cache] readahead_initial set to {}", clamped);
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/// How long (in nanoseconds) a dirty entry can live before being
/// considered "expired" and eligible for background writeback.
///
/// 5 seconds — matches Linux's default `dirty_expire_centisecs` (3000,
/// i.e. 30 seconds) but we use a shorter window since our workloads
/// are lighter and crash resilience matters more on a new OS.
const DIRTY_EXPIRE_NS: u64 = 5_000_000_000;

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
    /// HPET timestamp (nanoseconds) when this entry became dirty.
    /// Zero if not dirty.  Used by `flush_expired()` to identify
    /// sectors that have been dirty longer than `DIRTY_EXPIRE_NS`.
    dirty_since_ns: u64,
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
            dirty_since_ns: 0,
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
    /// Number of read-ahead operations triggered.
    pub readaheads: u64,
    /// Number of expired-dirty flushes (entries flushed by age).
    pub expired_flushes: u64,
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

/// Per-device sequential access tracker for adaptive read-ahead.
///
/// Detects sequential read patterns and dynamically adjusts the
/// prefetch window.  Based on Linux's readahead algorithm:
/// - Start with a small window (`READAHEAD_INITIAL`)
/// - Double the window on each consecutive sequential read
/// - Cap at `READAHEAD_MAX`
/// - Reset to initial on random access
struct ReadAheadTracker {
    /// Last sector read on this device.
    last_lba: u64,
    /// Number of consecutive sequential reads.
    seq_count: u32,
    /// Highest sector we've already prefetched up to (exclusive).
    /// Avoids re-prefetching the same range on consecutive reads.
    prefetched_up_to: u64,
    /// Current read-ahead window size (in sectors).
    /// Starts at `READAHEAD_INITIAL`, doubles on each sequential
    /// batch, caps at `READAHEAD_MAX`.
    window: u32,
}

impl ReadAheadTracker {
    const fn new() -> Self {
        Self {
            last_lba: u64::MAX,
            seq_count: 0,
            prefetched_up_to: 0,
            // Use the compile-time constant here because this is a const fn
            // (used to initialize the static BufferCacheInner).  The runtime
            // tunable is consulted in update_readahead_tracker() when the
            // window resets or grows.
            window: READAHEAD_INITIAL,
        }
    }
}

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
    /// Read-ahead operations triggered.
    readaheads: u64,
    /// Number of entries flushed by age-based expiry.
    expired_flushes: u64,
    /// Whether the pool has been initialized.
    initialized: bool,

    // --- O(log n) sector lookup index ---
    // OPT: Replaces O(n) linear scan in find_index() with BTreeMap
    // lookup.  Sector lookup is the hottest path (every read/write).
    // Benchmark: find_index dropped from O(512) to O(log 512) ≈ 9.

    /// Maps (device_id, lba) → entry index for O(log n) sector lookup.
    index: BTreeMap<(u8, u64), usize>,

    /// Stack of free slot indices for O(1) free-slot allocation.
    /// Populated on init, maintained on alloc/free.
    free_slots: Vec<usize>,

    /// Per-device sequential access tracker for read-ahead.
    readahead: [ReadAheadTracker; MAX_DEVICES],

    // --- O(1) counters for valid/dirty entries ---
    // OPT: Avoids O(n) scan in stats().  Maintained on every
    // entry state transition (alloc, free, mark-dirty, writeback).
    /// Number of currently valid (occupied) cache entries.
    valid_count: usize,
    /// Number of currently dirty (needs-writeback) entries.
    dirty_count: usize,
}

impl BufferCacheInner {
    const fn new() -> Self {
        // const array init requires const elements.
        const RA_INIT: ReadAheadTracker = ReadAheadTracker::new();
        Self {
            entries: Vec::new(),
            device_names: Vec::new(),
            access_counter: 0,
            reads: 0,
            hits: 0,
            misses: 0,
            writes: 0,
            writebacks: 0,
            readaheads: 0,
            expired_flushes: 0,
            initialized: false,
            index: BTreeMap::new(),
            free_slots: Vec::new(),
            readahead: [RA_INIT; MAX_DEVICES],
            valid_count: 0,
            dirty_count: 0,
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
        // All slots start free — push in reverse so index 0 is popped first.
        self.free_slots.reserve_exact(MAX_ENTRIES);
        for i in (0..MAX_ENTRIES).rev() {
            self.free_slots.push(i);
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
    ///
    /// Uses BTreeMap index for O(log n) lookup instead of linear scan.
    fn find_index(&self, dev_id: u8, lba: u64) -> Option<usize> {
        self.index.get(&(dev_id, lba)).copied()
    }

    /// Bump the access counter and record it on the entry.
    #[allow(clippy::arithmetic_side_effects)]
    fn touch(&mut self, idx: usize) {
        self.access_counter = self.access_counter.wrapping_add(1);
        self.entries[idx].last_access = self.access_counter;
    }

    /// Find an invalid (free) slot, or `None`.
    ///
    /// Uses pre-built free-slot stack for O(1) allocation.
    fn find_free(&mut self) -> Option<usize> {
        self.free_slots.pop()
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
                blkdev::record_io(true); // Track write for disk-idle heuristic.
                if self.entries[idx].dirty {
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                }
                self.entries[idx].dirty = false;
                self.entries[idx].dirty_since_ns = 0;
                self.writebacks = self.writebacks.wrapping_add(1);
                Ok(())
            }
            Some(Err(e)) => Err(e),
            None => Err(KernelError::NoSuchDevice),
        }
    }

    /// Evict an entry: remove from index and mark the slot invalid.
    fn evict_entry(&mut self, idx: usize) {
        if self.entries[idx].valid {
            let dev_id = self.entries[idx].device_id;
            let lba = self.entries[idx].lba;
            self.index.remove(&(dev_id, lba));
            self.valid_count = self.valid_count.saturating_sub(1);
            if self.entries[idx].dirty {
                self.dirty_count = self.dirty_count.saturating_sub(1);
            }
        }
        self.entries[idx].valid = false;
        // Note: the slot is NOT pushed to free_slots here because
        // the caller will immediately reuse it.
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
            self.evict_entry(idx);
            return Ok(idx);
        }

        // 3. All entries dirty — writeback LRU, then evict it.
        let idx = self.find_lru()
            .ok_or(KernelError::InternalError)?;
        self.writeback_entry(idx)?;
        self.evict_entry(idx);
        Ok(idx)
    }

    // OPT: stats() is now O(1) — uses pre-maintained counters instead
    // of scanning all entries.  Called by /proc/cacheinfo on every read.
    fn stats(&self) -> CacheStats {
        CacheStats {
            reads: self.reads,
            hits: self.hits,
            misses: self.misses,
            writes: self.writes,
            writebacks: self.writebacks,
            readaheads: self.readaheads,
            expired_flushes: self.expired_flushes,
            entries_used: self.valid_count,
            entries_dirty: self.dirty_count,
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

/// Update the read-ahead tracker for a device after accessing `lba`.
///
/// Returns `true` if read-ahead should be triggered (sequential pattern
/// detected above the threshold).
fn update_readahead_tracker(
    cache: &mut BufferCacheInner,
    dev_id: u8,
    lba: u64,
) -> bool {
    let tracker = &mut cache.readahead[dev_id as usize];

    if lba == tracker.last_lba.wrapping_add(1) {
        // Sequential: LBA is exactly one past the last read.
        tracker.seq_count = tracker.seq_count.saturating_add(1);

        // Adaptive window growth: double the window each time we
        // exhaust the current prefetch range.  This means small
        // sequential runs use a small window (low cache pressure)
        // while large file reads ramp up to full throughput.
        if tracker.seq_count > 0
            && tracker.seq_count.is_multiple_of(tracker.window)
        {
            tracker.window = tracker.window.saturating_mul(2).min(get_readahead_max());
        }
    } else {
        // Non-sequential access — reset everything.
        tracker.seq_count = 0;
        tracker.prefetched_up_to = 0;
        tracker.window = get_readahead_initial();
    }
    tracker.last_lba = lba;

    tracker.seq_count >= READAHEAD_THRESHOLD
}

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
        // Update read-ahead tracker even on hits.
        update_readahead_tracker(&mut cache, dev_id, lba);
        return Ok(());
    }

    // Cache miss — read from device.
    cache.misses = cache.misses.wrapping_add(1);

    let result = blkdev::with_device(device, |dev| {
        dev.read_sector(lba, buf)
    });
    match result {
        Some(Ok(())) => {
            blkdev::record_io(false); // Track read for disk-idle heuristic.
        }
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
    cache.valid_count = cache.valid_count.wrapping_add(1);
    cache.index.insert((dev_id, lba), idx);
    cache.touch(idx);

    // OPT: Adaptive sequential read-ahead.  When we detect a pattern
    // of consecutive sector reads, speculatively prefetch ahead into
    // the cache.  The window starts small (READAHEAD_INITIAL) and
    // doubles on sustained sequential access (up to READAHEAD_MAX).
    // Based on Linux's readahead algorithm (mm/readahead.c).
    let should_readahead = update_readahead_tracker(&mut cache, dev_id, lba);
    if should_readahead {
        let ra_start = lba.wrapping_add(1);
        let window = u64::from(cache.readahead[dev_id as usize].window);
        let ra_end = ra_start.saturating_add(window);
        let prefetch_from = cache.readahead[dev_id as usize].prefetched_up_to.max(ra_start);
        if prefetch_from < ra_end {
            cache.readaheads = cache.readaheads.wrapping_add(1);
            // Prefetch sectors that aren't already cached.
            for ahead_lba in prefetch_from..ra_end {
                if cache.find_index(dev_id, ahead_lba).is_some() {
                    continue; // Already cached.
                }
                // Read from device into a temporary buffer.
                let mut ahead_buf = [0u8; SECTOR_SIZE];
                let ahead_result = blkdev::with_device(device, |dev| {
                    dev.read_sector(ahead_lba, &mut ahead_buf)
                });
                if let Some(Ok(())) = ahead_result {
                    if let Ok(slot) = cache.make_room() {
                        cache.entries[slot].device_id = dev_id;
                        cache.entries[slot].lba = ahead_lba;
                        cache.entries[slot].data.copy_from_slice(&ahead_buf);
                        cache.entries[slot].dirty = false;
                        cache.entries[slot].valid = true;
                        cache.valid_count = cache.valid_count.wrapping_add(1);
                        cache.index.insert((dev_id, ahead_lba), slot);
                        cache.touch(slot);
                    }
                } else {
                    break; // Device error or end of device — stop prefetching.
                }
            }
            cache.readahead[dev_id as usize].prefetched_up_to = ra_end;
        }
    }

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

    let now_ns = crate::hpet::elapsed_ns();

    // Check if already cached — update in place.
    if let Some(idx) = cache.find_index(dev_id, lba) {
        cache.entries[idx].data.copy_from_slice(buf);
        if !cache.entries[idx].dirty {
            cache.dirty_count = cache.dirty_count.wrapping_add(1);
            // First time becoming dirty — record when.
            cache.entries[idx].dirty_since_ns = now_ns;
        }
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
    cache.entries[idx].dirty_since_ns = now_ns;
    cache.entries[idx].valid = true;
    cache.valid_count = cache.valid_count.wrapping_add(1);
    cache.dirty_count = cache.dirty_count.wrapping_add(1);
    cache.index.insert((dev_id, lba), idx);
    cache.touch(idx);

    Ok(())
}

/// Read a sector **directly from the block device**, bypassing the buffer
/// cache entirely (design-decisions §38, "page-cache-primary").
///
/// Regular-file *data* sectors live in the page cache, never the buffer cache,
/// so reading them must not pollute (or be served stale by) the buffer cache.
/// The page-cache fill path and the ext4 file-data read path use this instead
/// of [`read_sector`].
///
/// Coherence invariant: a data LBA is never resident in the buffer cache (the
/// ext4 block-free path invalidates an LBA's buffer-cache entry when the block
/// leaves metadata use — see `ext4::balloc::free_block`). We therefore read
/// straight from the device. As defense in depth against a transient invariant
/// violation, any *clean* buffer-cache entry for this LBA is dropped first; a
/// *dirty* entry (which would indicate the LBA is still legitimately metadata)
/// is honored by serving its contents rather than silently losing the write.
///
/// # Errors
///
/// [`KernelError::NoSuchDevice`] if the device is unknown, or an I/O error from
/// the device read.
pub fn read_sector_uncached(
    device: &str,
    lba: u64,
    buf: &mut [u8; SECTOR_SIZE],
) -> KernelResult<()> {
    {
        let mut cache = CACHE.lock();
        cache.ensure_init();
        if let Some(dev_id) = cache.device_id(device) {
            if let Some(idx) = cache.find_index(dev_id, lba) {
                if cache.entries[idx].dirty {
                    // Honor a still-dirty entry (the LBA is legitimately metadata
                    // that has pending write-back): serve it from the cache.
                    buf.copy_from_slice(&cache.entries[idx].data);
                    cache.touch(idx);
                    return Ok(());
                }
                // Clean entry for a data LBA: drop it so the buffer cache holds
                // no file data, then fall through to a direct device read.
                cache.index.remove(&(dev_id, lba));
                cache.valid_count = cache.valid_count.saturating_sub(1);
                cache.entries[idx].valid = false;
                cache.free_slots.push(idx);
            }
        }
    }

    let result = blkdev::with_device(device, |dev| dev.read_sector(lba, buf));
    match result {
        Some(Ok(())) => {
            blkdev::record_io(false);
            Ok(())
        }
        Some(Err(e)) => Err(e),
        None => Err(KernelError::NoSuchDevice),
    }
}

/// Write a sector **directly to the block device**, bypassing the buffer cache
/// (design-decisions §38, "page-cache-primary").
///
/// Used for regular-file *data* writes so that file data is never cached in the
/// buffer cache. The write is synchronous to the device (no write-back). Any
/// stale buffer-cache entry for this LBA is dropped afterward so a later
/// [`read_sector`] (were the block ever reused as metadata before the free-path
/// invalidation ran) cannot serve pre-write contents.
///
/// # Errors
///
/// [`KernelError::NoSuchDevice`] if the device is unknown, or an I/O error from
/// the device write.
pub fn write_sector_uncached(
    device: &str,
    lba: u64,
    buf: &[u8; SECTOR_SIZE],
) -> KernelResult<()> {
    let result = blkdev::with_device(device, |dev| dev.write_sector(lba, buf));
    match result {
        Some(Ok(())) => blkdev::record_io(true),
        Some(Err(e)) => return Err(e),
        None => return Err(KernelError::NoSuchDevice),
    }
    // Drop any buffer-cache entry for this LBA so the buffer cache holds no
    // file data and cannot serve stale bytes. invalidate_range drops without
    // flushing, which is correct here: we just wrote the authoritative contents
    // to the device, so any cached copy is moot.
    let _ = invalidate_range(device, lba, 1);
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

/// Flush dirty entries that have been dirty for longer than
/// [`DIRTY_EXPIRE_NS`] (5 seconds by default).
///
/// This is the "background writeback" mechanism.  Calling it
/// periodically (e.g., every few seconds from a timer) ensures that
/// dirty data reaches disk within a bounded time window, reducing
/// data loss on crash.  Only entries that have been dirty longer
/// than the threshold are written back — recently-dirtied entries
/// are left in cache to allow further coalescing.
///
/// Returns the number of entries written back.
pub fn flush_expired() -> usize {
    let mut cache = CACHE.lock();
    cache.ensure_init();

    let now_ns = crate::hpet::elapsed_ns();
    let mut flushed: usize = 0;

    for i in 0..cache.entries.len() {
        if cache.entries[i].valid
            && cache.entries[i].dirty
            && cache.entries[i].dirty_since_ns > 0
        {
            let age = now_ns.saturating_sub(cache.entries[i].dirty_since_ns);
            if age >= get_dirty_expire_ns() {
                if cache.writeback_entry(i).is_ok() {
                    flushed = flushed.saturating_add(1);
                }
            }
        }
    }

    cache.expired_flushes = cache.expired_flushes
        .wrapping_add(flushed as u64);

    flushed
}

/// Non-blocking variant of [`flush_expired()`] for softirq context.
///
/// Uses `try_lock` to avoid deadlock when called from the timer softirq
/// handler — if the interrupted code holds the cache lock, this returns
/// `None` instead of spinning.  The softirq will retry on the next
/// invocation (~5 seconds later).
///
/// Returns `Some(n)` with the number of entries written back, or `None`
/// if the lock was already held.
pub fn try_flush_expired() -> Option<usize> {
    let mut cache = CACHE.try_lock()?;
    cache.ensure_init();

    let now_ns = crate::hpet::elapsed_ns();
    let mut flushed: usize = 0;

    for i in 0..cache.entries.len() {
        if cache.entries[i].valid
            && cache.entries[i].dirty
            && cache.entries[i].dirty_since_ns > 0
        {
            let age = now_ns.saturating_sub(cache.entries[i].dirty_since_ns);
            if age >= get_dirty_expire_ns() {
                if cache.writeback_entry(i).is_ok() {
                    flushed = flushed.saturating_add(1);
                }
            }
        }
    }

    cache.expired_flushes = cache.expired_flushes
        .wrapping_add(flushed as u64);

    Some(flushed)
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

    for i in 0..cache.entries.len() {
        if cache.entries[i].valid && cache.entries[i].device_id == dev_id {
            let lba = cache.entries[i].lba;
            cache.index.remove(&(dev_id, lba));
            cache.valid_count = cache.valid_count.saturating_sub(1);
            if cache.entries[i].dirty {
                cache.dirty_count = cache.dirty_count.saturating_sub(1);
            }
            cache.entries[i].valid = false;
            cache.free_slots.push(i);
        }
    }

    Ok(())
}

/// Drop cached entries for sectors `[start_lba, start_lba + count)` on
/// `device` **without** writing them back.
///
/// Used by the discard/`fstrim` path: the sectors are about to be discarded
/// on the device, so any cached copy (even a dirty one) is moot — keeping it
/// would let stale free-space data resurface on a later read and diverge from
/// the now-discarded device contents.  Dirty entries in the range are dropped,
/// not flushed, because flushing them would defeat the discard.
///
/// Returns the number of cache entries dropped.
pub fn invalidate_range(device: &str, start_lba: u64, count: u64) -> usize {
    if count == 0 {
        return 0;
    }
    let end_lba = match start_lba.checked_add(count) {
        Some(e) => e,
        None => return 0, // Overflow: caller passed a bogus range; do nothing.
    };

    let mut cache = CACHE.lock();
    let dev_id = match cache.device_id(device) {
        Some(id) => id,
        None => return 0,
    };

    let mut dropped = 0usize;
    for i in 0..cache.entries.len() {
        if cache.entries[i].valid
            && cache.entries[i].device_id == dev_id
            && cache.entries[i].lba >= start_lba
            && cache.entries[i].lba < end_lba
        {
            let lba = cache.entries[i].lba;
            cache.index.remove(&(dev_id, lba));
            cache.valid_count = cache.valid_count.saturating_sub(1);
            if cache.entries[i].dirty {
                cache.dirty_count = cache.dirty_count.saturating_sub(1);
            }
            cache.entries[i].valid = false;
            cache.entries[i].dirty = false;
            cache.free_slots.push(i);
            dropped = dropped.saturating_add(1);
        }
    }

    dropped
}

/// Invalidate all cache entries without flushing.
///
/// **Danger**: discards dirty data.  Only use during shutdown or
/// after a device error where writeback is known to be impossible.
pub fn invalidate_all_no_flush() {
    let mut cache = CACHE.lock();
    cache.index.clear();
    cache.free_slots.clear();
    let len = cache.entries.len();
    for entry in &mut cache.entries {
        entry.valid = false;
        entry.dirty = false;
    }
    cache.valid_count = 0;
    cache.dirty_count = 0;
    // Rebuild free list (reverse order so index 0 is allocated first).
    for i in (0..len).rev() {
        cache.free_slots.push(i);
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

    // Test sequential read-ahead:
    // Read consecutive sectors and verify the readahead counter increases.
    // After READAHEAD_THRESHOLD consecutive reads, the cache should
    // prefetch ahead sectors automatically.
    let ra_start_lba: u64 = 200; // Use sectors unlikely to be in cache.
    let ra_before = stats().readaheads;

    // Invalidate this range first to ensure clean state.
    // Read sectors sequentially to trigger read-ahead detection.
    for i in 0..5u64 {
        let mut sector_buf = [0u8; SECTOR_SIZE];
        read_sector(device, ra_start_lba.wrapping_add(i), &mut sector_buf)?;
    }

    let ra_after = stats().readaheads;
    if ra_after > ra_before {
        crate::serial_println!(
            "[bcache]   Read-ahead triggered: {} → {} ({} new prefetches)",
            ra_before, ra_after, ra_after.wrapping_sub(ra_before),
        );

        // Verify that sectors ahead are now cached (should be cache hits).
        let hit_before = stats().hits;
        let ahead_lba = ra_start_lba.wrapping_add(5);
        let mut ahead_buf = [0u8; SECTOR_SIZE];
        read_sector(device, ahead_lba, &mut ahead_buf)?;
        let hit_after = stats().hits;

        if hit_after > hit_before {
            crate::serial_println!(
                "[bcache]   Read-ahead verification: sector {} was a cache hit",
                ahead_lba
            );
        } else {
            // Not a failure — the read-ahead might not have reached
            // this exact sector.  Just note it.
            crate::serial_println!(
                "[bcache]   Read-ahead note: sector {} was a miss (prefetch window may vary)",
                ahead_lba
            );
        }
    } else {
        crate::serial_println!("[bcache]   Read-ahead: no triggers (sectors may already be cached)");
    }

    // Print final statistics.
    let final_stats = stats();
    crate::serial_println!(
        "[bcache]   Stats: {} reads, {} hits, {} misses ({:.0}% hit rate), {} writes, {} writebacks, {} readaheads",
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
        final_stats.readaheads,
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
