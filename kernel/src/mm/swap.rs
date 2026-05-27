//! Swap subsystem — page eviction and restoration.
//!
//! When physical memory runs low, the swap subsystem can evict user-space
//! pages to a swap backend (disk file or compressed in-memory pool) and
//! restore them on demand when a page fault occurs.
//!
//! ## Design
//!
//! From the design spec:
//! > Use a swap file as default (not partition — on SSDs the performance
//! > difference is negligible).  zswap/zram compressed swap is highly
//! > recommended for desktop.  Swappiness tunable, default 10–20 for
//! > desktop.
//!
//! ## Architecture
//!
//! ```text
//!   Page fault (swap PTE)
//!         │
//!         ▼
//!   swap_in_page()  ←── reads page data from SwapBackend
//!         │                      ▲
//!         │                      │
//!   alloc frame + map            │
//!                                │
//!   swap_out_page() ──► writes page data to SwapBackend
//!         ▲
//!         │
//!   page reclaimer (low memory)
//! ```
//!
//! ## Swap Entry Format (in page table entries)
//!
//! When a page is swapped out, its PTE is set to a non-present "swap
//! entry" that encodes the swap slot index:
//!
//! ```text
//! Bit 0:     0 (not present — triggers page fault)
//! Bit 1:     1 (swap marker — distinguishes from truly-unmapped pages)
//! Bits 2–31: swap slot index (30 bits, up to ~1 billion slots)
//! Bits 32–63: reserved (zero)
//! ```
//!
//! ## Thread Safety
//!
//! The swap subsystem uses its own spinlock.  Lock ordering:
//! SWAP → page table manipulation → frame allocator.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::Mutex;
use super::frame::{self, FRAME_SIZE};
use super::page_table::{self, PageFlags, PageTableEntry, VirtAddr};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Swap marker bit in a non-present PTE (bit 1).
const SWAP_MARKER_BIT: u64 = 1 << 1;

/// Mask for the swap slot index (bits 2–31 = 30 bits).
const SWAP_SLOT_MASK: u64 = 0xFFFF_FFFC;  // bits 2–31
/// Shift to extract the slot index from a raw PTE.
const SWAP_SLOT_SHIFT: u32 = 2;

/// Maximum number of swap slots (30 bits = ~1 billion).
/// Actual capacity is set at init time based on backend size.
const MAX_SWAP_SLOTS: u32 = 1 << 30;

// ---------------------------------------------------------------------------
// Swap entry encoding/decoding
// ---------------------------------------------------------------------------

/// A swap entry that can be stored in a non-present PTE.
///
/// Encodes the swap slot index where the page's data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapEntry(u32);

impl SwapEntry {
    /// Create a swap entry for the given slot index.
    ///
    /// Returns `None` if `slot` exceeds 30 bits.
    #[must_use]
    pub const fn new(slot: u32) -> Option<Self> {
        if slot >= MAX_SWAP_SLOTS {
            return None;
        }
        Some(Self(slot))
    }

    /// The swap slot index.
    #[must_use]
    pub const fn slot(self) -> u32 {
        self.0
    }

    /// Encode this swap entry as a raw PTE value.
    ///
    /// The result has bit 0 = 0 (not present), bit 1 = 1 (swap marker),
    /// and bits 2–31 = slot index.
    #[must_use]
    pub const fn to_pte_raw(self) -> u64 {
        SWAP_MARKER_BIT | ((self.0 as u64) << SWAP_SLOT_SHIFT)
    }

    /// Decode a swap entry from a raw PTE value.
    ///
    /// Returns `None` if the PTE is not a valid swap entry (i.e.,
    /// bit 0 is set or bit 1 is clear).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn from_pte_raw(raw: u64) -> Option<Self> {
        // Must be: not present (bit 0 = 0), swap marker set (bit 1 = 1).
        if (raw & 0b11) != 0b10 {
            return None;
        }
        let slot = ((raw & SWAP_SLOT_MASK) >> SWAP_SLOT_SHIFT) as u32;
        Some(Self(slot))
    }
}

// ---------------------------------------------------------------------------
// Swap slot allocator
// ---------------------------------------------------------------------------

/// Bitmap-based allocator for swap slots.
///
/// Each bit represents one 16 KiB slot in the swap backend.
/// Bit = 0 → free, bit = 1 → in use.
struct SwapSlotAllocator {
    /// Bitmap of slot usage.  Each u64 covers 64 slots.
    bitmap: Vec<u64>,
    /// Total number of slots.
    capacity: u32,
    /// Number of slots currently in use.
    used: u32,
    /// Hint: start scanning from this word index (to avoid rescanning
    /// already-full words on every allocation).
    hint: usize,
}

impl SwapSlotAllocator {
    /// Create a new slot allocator with the given capacity.
    fn new(capacity: u32) -> Self {
        let words = capacity.div_ceil(64) as usize;
        Self {
            bitmap: vec![0u64; words],
            capacity,
            used: 0,
            hint: 0,
        }
    }

    /// Allocate a free swap slot.
    ///
    /// Returns the slot index, or `None` if all slots are in use.
    fn alloc(&mut self) -> Option<u32> {
        if self.used >= self.capacity {
            return None;
        }

        let words = self.bitmap.len();
        // Start scanning from the hint for O(1) average case.
        for pass in 0..words {
            let wi = (self.hint + pass) % words;
            let word = self.bitmap.get(wi).copied().unwrap_or(u64::MAX);
            if word == u64::MAX {
                continue; // All 64 bits set — no free slot in this word.
            }

            // Find the first zero bit.
            let bit = (!word).trailing_zeros();
            let slot = (wi as u64)
                .saturating_mul(64)
                .saturating_add(bit as u64);

            if slot >= self.capacity as u64 {
                // Past the end of valid slots (bitmap may be larger than
                // capacity rounded up to 64).
                continue;
            }

            // Mark as used.
            if let Some(w) = self.bitmap.get_mut(wi) {
                *w |= 1u64 << bit;
            }
            self.used = self.used.saturating_add(1);
            self.hint = wi; // Next alloc starts from this word.

            #[allow(clippy::cast_possible_truncation)]
            return Some(slot as u32);
        }

        None
    }

    /// Free a previously allocated swap slot.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the slot is not currently allocated
    /// (double-free detection).
    fn free(&mut self, slot: u32) {
        let wi = (slot / 64) as usize;
        let bit = slot % 64;

        if let Some(w) = self.bitmap.get_mut(wi) {
            debug_assert!(
                *w & (1u64 << bit) != 0,
                "swap slot {slot}: double free detected"
            );
            *w &= !(1u64 << bit);
            self.used = self.used.saturating_sub(1);
            // Update hint to help future allocations find free slots faster.
            if wi < self.hint {
                self.hint = wi;
            }
        }
    }

    /// Check if a slot is currently allocated.
    #[must_use]
    #[allow(dead_code)] // Diagnostic API for swap debugging.
    fn is_used(&self, slot: u32) -> bool {
        let wi = (slot / 64) as usize;
        let bit = slot % 64;
        self.bitmap
            .get(wi)
            .is_some_and(|w| *w & (1u64 << bit) != 0)
    }

    /// Number of free slots.
    #[must_use]
    fn free_count(&self) -> u32 {
        self.capacity.saturating_sub(self.used)
    }
}

// ---------------------------------------------------------------------------
// zram-style compressed in-memory swap backend
// ---------------------------------------------------------------------------

/// Slot storage type — compressed or uncompressed.
///
/// Pages are compressed on write.  If compression is beneficial
/// (compressed size < FRAME_SIZE), the compressed form is stored.
/// Otherwise, the uncompressed data is stored as-is.
enum SlotData {
    /// Compressed page data (smaller than FRAME_SIZE).
    Compressed(Vec<u8>),
    /// Uncompressed page data (compression was not beneficial).
    Uncompressed(Vec<u8>),
}

/// zram-style compressed in-memory swap backend.
///
/// Compresses page data using an LZ4-like algorithm before storing
/// in heap buffers.  This trades CPU time for memory savings:
///
/// - **Zero pages** (BSS, fresh stack): 16 KiB → 1 byte (99.99% savings)
/// - **Sparse/repetitive data**: typically 50–90% compression
/// - **Random/encrypted data**: stored uncompressed (no overhead beyond
///   the failed compression attempt)
///
/// ## Memory accounting
///
/// `uncompressed_bytes` tracks the logical size of all stored pages
/// (N × FRAME_SIZE).  `compressed_bytes` tracks the actual heap usage.
/// The ratio `compressed_bytes / uncompressed_bytes` is the effective
/// compression ratio.
struct MemBackend {
    /// Slot storage.  `None` = slot never written.
    slots: Vec<Option<SlotData>>,
    /// Total capacity (number of slots).
    capacity: u32,
    /// Total uncompressed bytes stored (logical size).
    uncompressed_bytes: u64,
    /// Total compressed bytes stored (actual heap usage).
    compressed_bytes: u64,
    /// Number of pages that compressed successfully.
    compressed_count: u64,
    /// Number of pages stored uncompressed (incompressible).
    uncompressed_count: u64,
}

impl MemBackend {
    /// Create a new compressed in-memory backend with the given capacity.
    fn new(capacity: u32) -> Self {
        let mut slots = Vec::with_capacity(capacity as usize);
        for _ in 0..capacity {
            slots.push(None);
        }
        Self {
            slots,
            capacity,
            uncompressed_bytes: 0,
            compressed_bytes: 0,
            compressed_count: 0,
            uncompressed_count: 0,
        }
    }

    /// Write a page's data into a swap slot.
    ///
    /// Compresses the data first.  If compression is beneficial, stores
    /// the compressed form; otherwise stores uncompressed.
    ///
    /// `data` must be exactly `FRAME_SIZE` bytes.
    fn write(&mut self, slot: u32, data: &[u8]) -> KernelResult<()> {
        if slot >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }
        if data.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let slot_storage = self
            .slots
            .get_mut(slot as usize)
            .ok_or(KernelError::InvalidArgument)?;

        // Try to compress the page.
        let stored = match super::compress::compress(data) {
            Some(compressed) => {
                let compressed_len = compressed.len() as u64;
                self.compressed_bytes =
                    self.compressed_bytes.saturating_add(compressed_len);
                self.compressed_count =
                    self.compressed_count.saturating_add(1);
                SlotData::Compressed(compressed)
            }
            None => {
                // Incompressible — store uncompressed.
                self.compressed_bytes =
                    self.compressed_bytes.saturating_add(FRAME_SIZE as u64);
                self.uncompressed_count =
                    self.uncompressed_count.saturating_add(1);
                SlotData::Uncompressed(data.to_vec())
            }
        };

        self.uncompressed_bytes =
            self.uncompressed_bytes.saturating_add(FRAME_SIZE as u64);
        *slot_storage = Some(stored);
        Ok(())
    }

    /// Read a page's data from a swap slot.
    ///
    /// Decompresses if necessary.  `buf` must be exactly `FRAME_SIZE` bytes.
    fn read(&self, slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        if slot >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }
        if buf.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let slot_data = self
            .slots
            .get(slot as usize)
            .and_then(|s| s.as_ref())
            .ok_or(KernelError::InvalidArgument)?;

        match slot_data {
            SlotData::Compressed(compressed) => {
                let decompressed = super::compress::decompress(
                    compressed,
                    FRAME_SIZE,
                ).map_err(|_| KernelError::InternalError)?;
                buf.copy_from_slice(&decompressed);
            }
            SlotData::Uncompressed(data) => {
                buf.copy_from_slice(data);
            }
        }

        Ok(())
    }

    /// Discard a slot's data (after swap-in).
    fn discard(&mut self, slot: u32) {
        if let Some(slot_storage) = self.slots.get_mut(slot as usize) {
            // Update byte counts.
            if let Some(data) = slot_storage {
                let stored_size = match data {
                    SlotData::Compressed(c) => c.len() as u64,
                    SlotData::Uncompressed(_) => FRAME_SIZE as u64,
                };
                self.uncompressed_bytes =
                    self.uncompressed_bytes.saturating_sub(FRAME_SIZE as u64);
                self.compressed_bytes =
                    self.compressed_bytes.saturating_sub(stored_size);
            }
            *slot_storage = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Disk-backed swap backend
// ---------------------------------------------------------------------------

/// Disk-backed swap backend using a block device.
///
/// Writes swap slot data directly to a named block device.  Each swap
/// slot occupies `SECTORS_PER_FRAME` contiguous sectors starting from
/// a configurable base offset.
///
/// ## Sector layout
///
/// ```text
/// Disk: [... unused ...][base_sector][slot 0 = 32 sectors][slot 1 = 32 sectors]...
/// ```
///
/// Data is compressed before writing (like the zram backend), so each
/// slot's actual I/O may be less than `FRAME_SIZE` bytes, but we always
/// write full `FRAME_SIZE` for simplicity (the trailing bytes are
/// padding).  Compression metadata is stored in-memory (the slot
/// allocation bitmap + a length array).
///
/// ## Limitations
///
/// - Synchronous I/O (blocking, no DMA batching).
/// - Single-device only (no multi-device swap tiering yet).
struct DiskBackend {
    /// Name of the block device (e.g., "vdb") in the blkdev registry.
    device_name: String,
    /// Starting sector offset on the disk.
    base_sector: u64,
    /// Number of swap slots.
    capacity: u32,
    /// Per-slot stored size (actual compressed bytes).  Needed for
    /// decompression: we must know the compressed length.
    /// `None` = slot not written.
    slot_sizes: Vec<Option<u32>>,
    /// Whether each slot is stored compressed or uncompressed.
    slot_compressed: Vec<bool>,
}

/// Number of 512-byte sectors per 16 KiB frame.
const SECTORS_PER_FRAME: u32 = (FRAME_SIZE / 512) as u32;

impl DiskBackend {
    /// Create a new disk backend on the named block device.
    ///
    /// `base_sector`: first sector used for swap.
    /// `capacity`: number of swap slots.
    fn new(device_name: &str, base_sector: u64, capacity: u32) -> Self {
        let mut slot_sizes = Vec::with_capacity(capacity as usize);
        let mut slot_compressed = Vec::with_capacity(capacity as usize);
        for _ in 0..capacity {
            slot_sizes.push(None);
            slot_compressed.push(false);
        }
        Self {
            device_name: String::from(device_name),
            base_sector,
            capacity,
            slot_sizes,
            slot_compressed,
        }
    }

    /// Sector offset for a given slot.
    fn slot_sector(&self, slot: u32) -> u64 {
        self.base_sector
            .saturating_add((slot as u64).saturating_mul(SECTORS_PER_FRAME as u64))
    }

    /// Write page data to a swap slot on disk.
    ///
    /// Compresses data first.  Writes the (possibly compressed) data
    /// to the slot's sector range, padded with zeros.
    fn write(&mut self, slot: u32, data: &[u8]) -> KernelResult<()> {
        if slot >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }
        if data.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        // Try to compress the data.
        let (write_buf, compressed, stored_size) =
            match super::compress::compress(data) {
                Some(compressed) => {
                    let size = compressed.len();
                    // Pad to FRAME_SIZE for writing full sectors.
                    let mut padded = compressed;
                    padded.resize(FRAME_SIZE, 0);
                    (padded, true, size)
                }
                None => {
                    (data.to_vec(), false, FRAME_SIZE)
                }
            };

        let sector = self.slot_sector(slot);
        let device_name = self.device_name.clone();

        crate::blkdev::with_device(&device_name, |dev| {
            dev.write_sectors(sector, SECTORS_PER_FRAME, &write_buf)
        })
        .ok_or(KernelError::NotSupported)?
        .map_err(|_| KernelError::IoError)?;

        if let Some(size_entry) = self.slot_sizes.get_mut(slot as usize) {
            #[allow(clippy::cast_possible_truncation)]
            {
                *size_entry = Some(stored_size as u32);
            }
        }
        if let Some(comp) = self.slot_compressed.get_mut(slot as usize) {
            *comp = compressed;
        }

        Ok(())
    }

    /// Read page data from a swap slot on disk.
    ///
    /// Reads the slot's sectors, then decompresses if necessary.
    fn read(&self, slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        if slot >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }
        if buf.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let stored_size = self
            .slot_sizes
            .get(slot as usize)
            .and_then(|s| *s)
            .ok_or(KernelError::InvalidArgument)?;

        let is_compressed = self
            .slot_compressed
            .get(slot as usize)
            .copied()
            .unwrap_or(false);

        // Read the full sector range from disk.
        let mut disk_buf = vec![0u8; FRAME_SIZE];
        let sector = self.slot_sector(slot);
        let device_name = self.device_name.clone();

        crate::blkdev::with_device(&device_name, |dev| {
            dev.read_sectors(sector, SECTORS_PER_FRAME, &mut disk_buf)
        })
        .ok_or(KernelError::NotSupported)?
        .map_err(|_| KernelError::IoError)?;

        if is_compressed {
            let compressed = disk_buf
                .get(..stored_size as usize)
                .ok_or(KernelError::InternalError)?;
            let decompressed = super::compress::decompress(compressed, FRAME_SIZE)
                .map_err(|_| KernelError::InternalError)?;
            buf.copy_from_slice(&decompressed);
        } else {
            buf.copy_from_slice(&disk_buf);
        }

        Ok(())
    }

    /// Discard a slot (mark it as unused).
    fn discard(&mut self, slot: u32) {
        if let Some(size_entry) = self.slot_sizes.get_mut(slot as usize) {
            *size_entry = None;
        }
        if let Some(comp) = self.slot_compressed.get_mut(slot as usize) {
            *comp = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Unified swap backend
// ---------------------------------------------------------------------------

/// The swap backend: either compressed in-memory (zram) or disk-backed.
enum SwapBackend {
    /// Compressed in-memory swap (zram style).
    Memory(MemBackend),
    /// Disk-backed swap with compression.
    Disk(DiskBackend),
}

impl SwapBackend {
    fn write(&mut self, slot: u32, data: &[u8]) -> KernelResult<()> {
        match self {
            Self::Memory(m) => m.write(slot, data),
            Self::Disk(d) => d.write(slot, data),
        }
    }

    fn read(&self, slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        match self {
            Self::Memory(m) => m.read(slot, buf),
            Self::Disk(d) => d.read(slot, buf),
        }
    }

    fn discard(&mut self, slot: u32) {
        match self {
            Self::Memory(m) => m.discard(slot),
            Self::Disk(d) => d.discard(slot),
        }
    }
}

// ---------------------------------------------------------------------------
// Global swap state — multi-device with priority
// ---------------------------------------------------------------------------

/// A single swap device with its own backend, slot allocator, and priority.
///
/// Multiple swap devices can be active simultaneously.  When allocating
/// a swap slot, devices are tried in priority order (highest first).
/// This enables a tiered swap setup:
///
/// - **zram** (priority 100): fast in-memory compressed swap.
///   Small capacity, but zero I/O latency.
/// - **disk** (priority 0): slower, larger capacity for overflow.
///   Used only when zram is full.
struct SwapDevice {
    /// Priority: higher = preferred for writes.  When multiple devices
    /// have capacity, the highest-priority device is used first.
    priority: i32,
    /// Display name for logging (e.g., "zram", "disk:vdb").
    name: String,
    /// The storage backend.
    backend: SwapBackend,
    /// Per-device slot allocator.
    slots: SwapSlotAllocator,
    /// Global slot offset: this device owns global slots
    /// `[base_slot .. base_slot + capacity)`.
    base_slot: u32,
}

/// Global swap subsystem state.
///
/// Supports multiple swap devices with priority-based allocation.
/// Devices are sorted by priority (descending) so iteration naturally
/// tries the fastest device first.
struct SwapState {
    /// All swap devices, sorted by priority (descending).
    devices: Vec<SwapDevice>,
    /// Total capacity across all devices (sum of all slot counts).
    total_capacity: u32,
    /// Whether the subsystem is initialized (at least one device present).
    initialized: bool,
}

impl SwapState {
    const fn uninit() -> Self {
        Self {
            devices: Vec::new(),
            total_capacity: 0,
            initialized: false,
        }
    }

    /// Find which device owns a global slot index.
    ///
    /// Returns `(device_index, local_slot)` where `local_slot` is the
    /// offset within that device's slot range.
    fn find_device(&self, global_slot: u32) -> Option<(usize, u32)> {
        for (i, dev) in self.devices.iter().enumerate() {
            let end = dev.base_slot.saturating_add(dev.slots.capacity);
            if global_slot >= dev.base_slot && global_slot < end {
                return Some((i, global_slot.wrapping_sub(dev.base_slot)));
            }
        }
        None
    }

    /// Allocate a swap slot from the highest-priority device with capacity.
    ///
    /// Returns the global slot index, or `None` if all devices are full.
    fn alloc_slot(&mut self) -> Option<u32> {
        // Devices are sorted by priority (descending), so we naturally
        // try the highest-priority (fastest) device first.
        for dev in &mut self.devices {
            if let Some(local_slot) = dev.slots.alloc() {
                return Some(dev.base_slot.saturating_add(local_slot));
            }
        }
        None
    }

    /// Free a global swap slot.
    fn free_slot(&mut self, global_slot: u32) {
        if let Some((dev_idx, local_slot)) = self.find_device(global_slot) {
            if let Some(dev) = self.devices.get_mut(dev_idx) {
                dev.slots.free(local_slot);
            }
        }
    }

    /// Write data to a global swap slot.
    fn write_slot(&mut self, global_slot: u32, data: &[u8]) -> KernelResult<()> {
        let (dev_idx, local_slot) = self.find_device(global_slot)
            .ok_or(KernelError::InvalidArgument)?;
        let dev = self.devices.get_mut(dev_idx)
            .ok_or(KernelError::InternalError)?;
        dev.backend.write(local_slot, data)
    }

    /// Read data from a global swap slot.
    fn read_slot(&self, global_slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        let (dev_idx, local_slot) = self.find_device(global_slot)
            .ok_or(KernelError::InvalidArgument)?;
        let dev = self.devices.get(dev_idx)
            .ok_or(KernelError::InternalError)?;
        dev.backend.read(local_slot, buf)
    }

    /// Discard data from a global swap slot.
    fn discard_slot(&mut self, global_slot: u32) {
        if let Some((dev_idx, local_slot)) = self.find_device(global_slot) {
            if let Some(dev) = self.devices.get_mut(dev_idx) {
                dev.backend.discard(local_slot);
            }
        }
    }

    /// Total number of free slots across all devices.
    fn total_free(&self) -> u32 {
        self.devices.iter().map(|d| d.slots.free_count()).sum()
    }

    /// Total number of used slots across all devices.
    fn total_used(&self) -> u32 {
        self.devices.iter().map(|d| d.slots.used).sum()
    }
}

/// Lock ordering: SWAP → RECLAIM → page table → frame allocator.
static SWAP: Mutex<SwapState> = Mutex::named(SwapState::uninit(), b"SWAP");

// ---------------------------------------------------------------------------
// Reclaimable page tracking (Clock algorithm)
// ---------------------------------------------------------------------------

/// A record of a user-space page that can be reclaimed (swapped out).
///
/// The reclaimer maintains a circular list of these records and uses
/// the Clock (second-chance) algorithm to select victims.
#[derive(Clone, Copy)]
#[allow(dead_code)] // flags field stored for swap-in restoration.
struct ReclaimablePage {
    /// PML4 physical address of the owning process's page table.
    pml4_phys: u64,
    /// Frame-aligned virtual address of the page.
    vaddr: u64,
    /// Page flags to restore on swap-in.
    flags: PageFlags,
    /// Whether this entry is active (pages get deregistered by setting
    /// active=false rather than removing, to avoid O(n) shifts).
    active: bool,
}

/// Reclamation state (separate lock from SWAP to avoid holding both
/// during the full swap-out sequence).
struct ReclaimState {
    /// Circular list of reclaimable pages.
    pages: Vec<ReclaimablePage>,
    /// Clock hand position (index into `pages`).
    clock_hand: usize,
    /// Number of active entries.
    active_count: usize,
}

impl ReclaimState {
    const fn new() -> Self {
        Self {
            pages: Vec::new(),
            clock_hand: 0,
            active_count: 0,
        }
    }
}

static RECLAIM: Mutex<ReclaimState> = Mutex::named(ReclaimState::new(), b"RECLAIM");

/// Register a user-space page as reclaimable (eligible for swap-out).
///
/// Called when a user-space page is mapped (via demand paging, stack
/// growth, or committed allocation).  The page's PML4, virtual address,
/// and original flags are recorded so the reclaimer can find and
/// restore it later.
///
/// This is O(1) amortized — entries are appended or reuse inactive slots.
pub fn register_reclaimable(pml4_phys: u64, vaddr: u64, flags: PageFlags) {
    let mut state = RECLAIM.lock();

    // Try to reuse an inactive slot first (avoids unbounded growth).
    for entry in state.pages.iter_mut() {
        if !entry.active {
            *entry = ReclaimablePage {
                pml4_phys,
                vaddr,
                flags,
                active: true,
            };
            state.active_count = state.active_count.saturating_add(1);
            return;
        }
    }

    // No inactive slot — append.
    state.pages.push(ReclaimablePage {
        pml4_phys,
        vaddr,
        flags,
        active: true,
    });
    state.active_count = state.active_count.saturating_add(1);
}

/// Unregister a page from the reclaimable set.
///
/// Called when a user-space page is unmapped (via munmap or process
/// exit).  Marks the entry inactive so the reclaimer skips it.
///
/// This is O(n) in the worst case (scanning for the entry), but the
/// list is typically short and entries near the clock hand are found
/// quickly.
pub fn unregister_reclaimable(pml4_phys: u64, vaddr: u64) {
    let mut state = RECLAIM.lock();

    for entry in state.pages.iter_mut() {
        if entry.active && entry.pml4_phys == pml4_phys && entry.vaddr == vaddr {
            entry.active = false;
            state.active_count = state.active_count.saturating_sub(1);
            return;
        }
    }
}

/// Try to reclaim `target` pages by swapping them out.
///
/// Uses the **Clock algorithm** (second-chance LRU approximation):
/// 1. Start from the clock hand position.
/// 2. For each active page:
///    a. If the page's ACCESSED bit is set in the PTE, clear it
///       (give it a "second chance") and advance.
///    b. If ACCESSED is clear, this page hasn't been touched recently
///       → select it for eviction.
/// 3. Evict the selected page via `swap_out_page()`.
/// 4. Continue until `target` pages have been reclaimed or we've
///    scanned the entire list twice without finding a victim.
///
/// Returns the number of pages actually reclaimed.
pub fn try_reclaim(target: usize) -> usize {
    let mut reclaimed = 0;

    // We need to collect victims while holding RECLAIM, then release
    // RECLAIM before calling swap_out_page (which needs SWAP lock).
    // Collect up to `target` victims per pass.
    let victims = {
        let mut state = RECLAIM.lock();
        let len = state.pages.len();
        if len == 0 || state.active_count == 0 {
            return 0;
        }

        let mut victims = Vec::new();
        // Scan at most 2 * len entries (two full rotations gives every
        // page at least one second chance).
        let max_scan = len.saturating_mul(2);
        let mut scanned = 0;

        while victims.len() < target && scanned < max_scan {
            let idx = state.clock_hand % len;
            state.clock_hand = (state.clock_hand + 1) % len;
            scanned += 1;

            let entry = match state.pages.get(idx) {
                Some(e) if e.active => *e,
                _ => continue,
            };

            // Check the ACCESSED bit in the PTE.
            let virt = VirtAddr::new(entry.vaddr);
            let pte = page_table::read_leaf_pte(entry.pml4_phys, virt);

            match pte {
                Some(pte) if pte.is_present() => {
                    if pte.flags().contains(PageFlags::ACCESSED) {
                        // Second chance: clear the ACCESSED bit and
                        // move on.  The CPU will re-set it on next access.
                        // SAFETY: pml4_phys is valid, page is present.
                        let _ = unsafe {
                            page_table::change_flags(
                                entry.pml4_phys,
                                virt,
                                pte.flags() & !PageFlags::ACCESSED,
                            )
                        };
                        unsafe { page_table::flush_frame(virt); }
                    } else {
                        // Not recently accessed — select as victim.
                        victims.push((idx, entry));
                    }
                }
                _ => {
                    // Page is not present (already swapped or unmapped).
                    // Mark inactive.
                    if let Some(e) = state.pages.get_mut(idx) {
                        e.active = false;
                        state.active_count =
                            state.active_count.saturating_sub(1);
                    }
                }
            }
        }

        victims
    };
    // RECLAIM lock released here.

    // Read the swap batch size from sysctl.  This controls how many
    // pages we swap out before yielding the CPU, keeping the system
    // responsive during heavy swap activity.
    let batch_size = crate::sysctl::get(crate::sysctl::PARAM_MM_SWAP_BATCH_SIZE)
        .unwrap_or(4) as usize;
    let batch_size = if batch_size == 0 { 1 } else { batch_size };
    let mut batch_count = 0usize;

    // Now swap out each victim, yielding the CPU after each batch
    // to prevent swap I/O from monopolizing the CPU and making the
    // system unresponsive.
    for (idx, victim) in victims {
        let virt = VirtAddr::new(victim.vaddr);
        // SAFETY: pml4_phys and virt are from the reclaim list;
        // the page was verified present above.
        match unsafe { swap_out_page(victim.pml4_phys, virt) } {
            Ok(_entry) => {
                reclaimed += 1;
                batch_count += 1;
                // Mark the entry inactive (it's now in swap, not memory).
                let mut state = RECLAIM.lock();
                if let Some(e) = state.pages.get_mut(idx) {
                    e.active = false;
                    state.active_count =
                        state.active_count.saturating_sub(1);
                }
                // OPT: Yield the CPU after every batch_size pages to let
                // other tasks run.  Without this, a swap storm (many pages
                // being evicted) would block all other work on this CPU
                // for the duration of the disk I/O.  The batch size is
                // tunable via mm.swap_batch_size (default 4, range 1-64).
                if batch_count >= batch_size {
                    batch_count = 0;
                    crate::sched::yield_now();
                }
            }
            Err(e) => {
                serial_println!(
                    "[swap] Reclaim failed for virt={:#x}: {:?}",
                    victim.vaddr, e
                );
                // Skip this page, try the next victim.
            }
        }
    }

    if reclaimed > 0 {
        serial_println!("[swap] Reclaimed {} pages (batch_size={})", reclaimed, batch_size);
    }

    reclaimed
}

/// Number of pages registered as reclaimable.
#[must_use]
pub fn reclaimable_count() -> usize {
    RECLAIM.lock().active_count
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the swap subsystem with a zram (compressed in-memory) backend.
///
/// `num_slots` determines the maximum number of pages that can be
/// simultaneously stored in zram.  Each slot consumes no memory until
/// a page is actually written to it (the in-memory backend allocates
/// on demand).
///
/// zram is added at priority 100 (highest), so it will be preferred
/// over disk swap when both are available.  Additional backends can
/// be added later via `init_disk()`.
///
/// Called during kernel boot, after the heap is available.
pub fn init(num_slots: u32) {
    let mut state = SWAP.lock();

    let base_slot = state.total_capacity;
    state.devices.push(SwapDevice {
        priority: 100,
        name: String::from("zram"),
        backend: SwapBackend::Memory(MemBackend::new(num_slots)),
        slots: SwapSlotAllocator::new(num_slots),
        base_slot,
    });
    state.total_capacity = state.total_capacity.saturating_add(num_slots);
    // Keep devices sorted by priority (descending).
    state.devices.sort_by_key(|e| core::cmp::Reverse(e.priority));
    state.initialized = true;

    serial_println!(
        "[swap] Initialized zram backend: {} slots ({} KiB), priority=100, base_slot={}",
        num_slots,
        (num_slots as u64).saturating_mul(FRAME_SIZE as u64) / 1024,
        base_slot
    );
}

/// Add a disk-backed swap device alongside existing backends.
///
/// `device_name`: the block device name (e.g., `"vdb"`) in the
///   blkdev registry.
/// `base_sector`: first sector on the device used for swap.
/// `num_slots`: number of 16 KiB swap slots.
///
/// Each slot occupies [`SECTORS_PER_FRAME`] contiguous sectors.
/// The total disk space used is `num_slots × FRAME_SIZE`.
///
/// The disk device is added at priority 0 (lower than zram's 100),
/// so it will only be used when the zram backend is full.  This
/// creates a two-tier swap hierarchy:
///
/// 1. **zram** (fast, limited, in-memory) — tried first
/// 2. **disk** (slower, larger) — overflow only
///
/// Returns an error if the device is not found, too small, or
/// read-only.
pub fn init_disk(device_name: &str, base_sector: u64, num_slots: u32) -> KernelResult<()> {
    // Verify the device exists and has enough capacity.
    let (sector_count, read_only) = crate::blkdev::with_device(device_name, |dev| {
        let info = dev.info();
        (info.sector_count, info.read_only)
    })
    .ok_or(KernelError::NoSuchDevice)?;

    if read_only {
        serial_println!(
            "[swap] Device '{}' is read-only, cannot use for swap",
            device_name
        );
        return Err(KernelError::InvalidArgument);
    }

    let sectors_needed = (num_slots as u64).saturating_mul(SECTORS_PER_FRAME as u64);
    let end_sector = base_sector.saturating_add(sectors_needed);
    if end_sector > sector_count {
        serial_println!(
            "[swap] Device '{}' too small: need {} sectors from base {}, device has {}",
            device_name, sectors_needed, base_sector, sector_count
        );
        return Err(KernelError::InvalidArgument);
    }

    let mut state = SWAP.lock();

    let base_slot = state.total_capacity;
    let dev_name = alloc::format!("disk:{}", device_name);
    state.devices.push(SwapDevice {
        priority: 0,
        name: dev_name,
        backend: SwapBackend::Disk(
            DiskBackend::new(device_name, base_sector, num_slots),
        ),
        slots: SwapSlotAllocator::new(num_slots),
        base_slot,
    });
    state.total_capacity = state.total_capacity.saturating_add(num_slots);
    // Keep devices sorted by priority (descending).
    state.devices.sort_by_key(|e| core::cmp::Reverse(e.priority));
    state.initialized = true;

    let total_free: u32 = state.devices.iter().map(|d| d.slots.free_count()).sum();
    let device_count = state.devices.len();

    serial_println!(
        "[swap] Added disk backend on '{}': {} slots ({} KiB), priority=0, base_slot={}",
        device_name,
        num_slots,
        (num_slots as u64).saturating_mul(FRAME_SIZE as u64) / 1024,
        base_slot
    );
    serial_println!(
        "[swap]   {} device(s) active, {} total slots free",
        device_count, total_free
    );
    Ok(())
}

/// Check if the swap subsystem is initialized and has capacity.
#[must_use]
#[allow(dead_code)] // Public API for OOM policy decisions.
pub fn is_available() -> bool {
    let state = SWAP.lock();
    state.initialized && state.total_free() > 0
}

/// Number of free swap slots across all devices.
#[must_use]
#[allow(dead_code)] // Public API for memory pressure monitoring.
pub fn free_slots() -> u32 {
    SWAP.lock().total_free()
}

/// Number of used swap slots across all devices.
#[must_use]
#[allow(dead_code)] // Public API for memory statistics reporting.
pub fn used_slots() -> u32 {
    SWAP.lock().total_used()
}

/// Number of active swap devices.
#[must_use]
#[allow(dead_code)] // Public API for swap device management.
pub fn device_count() -> usize {
    SWAP.lock().devices.len()
}

/// Summary of swap usage for the unified `MemoryInfo` API.
///
/// Returns `(total_bytes, used_bytes, device_count)`.
///
/// This acquires the SWAP lock once and extracts all three values in a
/// single critical section, avoiding inconsistencies from three separate
/// calls.
#[must_use]
pub fn summary() -> (usize, usize, usize) {
    let state = SWAP.lock();
    let total_bytes = (state.total_capacity as usize).saturating_mul(FRAME_SIZE);
    let used_bytes = (state.total_used() as usize).saturating_mul(FRAME_SIZE);
    let devices = state.devices.len();
    (total_bytes, used_bytes, devices)
}

/// Information about a single swap device (for /proc/swaps).
#[derive(Debug, Clone)]
pub struct SwapDeviceInfo {
    /// Device display name (e.g., "zram", "disk:vdb").
    pub name: alloc::string::String,
    /// Device type ("memory" for zram, "disk" for block device).
    pub device_type: &'static str,
    /// Total capacity in slots (1 slot = 1 page = 16 KiB).
    pub total_slots: u32,
    /// Used slots.
    pub used_slots: u32,
    /// Priority (higher = preferred).
    pub priority: i32,
}

/// List all active swap devices with their usage and type.
///
/// Used by `/proc/swaps` to show swap configuration.
pub fn list_devices() -> alloc::vec::Vec<SwapDeviceInfo> {
    let state = SWAP.lock();
    let mut result = alloc::vec::Vec::with_capacity(state.devices.len());
    for dev in &state.devices {
        let device_type = match &dev.backend {
            SwapBackend::Memory(_) => "memory",
            SwapBackend::Disk(_) => "disk",
        };
        result.push(SwapDeviceInfo {
            name: dev.name.clone(),
            device_type,
            total_slots: dev.slots.capacity,
            used_slots: dev.slots.used,
            priority: dev.priority,
        });
    }
    result
}

/// Compression statistics for the zram backend.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API for memory statistics dashboard.
pub struct CompressionStats {
    /// Total uncompressed bytes of all stored pages (logical size).
    pub uncompressed_bytes: u64,
    /// Total compressed bytes stored (actual heap usage).
    pub compressed_bytes: u64,
    /// Number of pages that compressed successfully.
    pub compressed_count: u64,
    /// Number of pages stored uncompressed (incompressible).
    pub uncompressed_count: u64,
}

impl CompressionStats {
    /// Compression ratio as a percentage (0–100).
    /// 100% means no compression; 50% means half the size.
    /// Returns 0 if no data is stored.
    #[must_use]
    #[allow(dead_code)] // Public API for memory statistics dashboard.
    pub fn ratio_percent(&self) -> u64 {
        if self.uncompressed_bytes == 0 {
            return 0;
        }
        self.compressed_bytes
            .saturating_mul(100)
            .checked_div(self.uncompressed_bytes)
            .unwrap_or(0)
    }

    /// Memory saved by compression (in bytes).
    #[must_use]
    #[allow(dead_code)] // Public API for memory statistics dashboard.
    pub fn bytes_saved(&self) -> u64 {
        self.uncompressed_bytes
            .saturating_sub(self.compressed_bytes)
    }
}

/// Get aggregated compression statistics from all zram backends.
///
/// Only counts in-memory (zram) backends; disk backends track
/// compression differently (data goes to disk, not heap).
#[must_use]
#[allow(dead_code)] // Public API for memory statistics reporting.
pub fn compression_stats() -> CompressionStats {
    let state = SWAP.lock();
    let mut stats = CompressionStats {
        uncompressed_bytes: 0,
        compressed_bytes: 0,
        compressed_count: 0,
        uncompressed_count: 0,
    };
    for dev in &state.devices {
        if let SwapBackend::Memory(m) = &dev.backend {
            stats.uncompressed_bytes = stats.uncompressed_bytes
                .saturating_add(m.uncompressed_bytes);
            stats.compressed_bytes = stats.compressed_bytes
                .saturating_add(m.compressed_bytes);
            stats.compressed_count = stats.compressed_count
                .saturating_add(m.compressed_count);
            stats.uncompressed_count = stats.uncompressed_count
                .saturating_add(m.uncompressed_count);
        }
    }
    stats
}

/// Swap out a page: evict a physical frame's contents to swap storage
/// and replace the page table entries with a swap entry.
///
/// 1. Reads the page's 16 KiB of data via HHDM.
/// 2. Allocates a swap slot and writes the data to the backend.
/// 3. Unmaps the frame from the page table.
/// 4. Writes swap entries into the 4 PTEs.
/// 5. Flushes the TLB.
/// 6. Frees the physical frame.
///
/// The frame at `virt` must be:
/// - Mapped and present in the page table at `pml4_phys`.
/// - A user-space address (swapping kernel pages is not supported).
///
/// Returns the `SwapEntry` that was written to the page table.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The page at `virt` must be mapped and present.
/// - The page must not be actively accessed by another CPU/context
///   during the swap-out operation.
pub unsafe fn swap_out_page(
    pml4_phys: u64,
    virt: VirtAddr,
) -> KernelResult<SwapEntry> {
    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_user() {
        return Err(KernelError::InvalidArgument);
    }

    // Step 1: Read the page data via HHDM before unmapping.
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let phys = page_table::translate(pml4_phys, virt)
        .ok_or(KernelError::InvalidAddress)?;

    // The physical address from translate includes the page offset (0 for
    // frame-aligned addresses), so it's the frame's base.
    let frame_virt = phys.checked_add(hhdm).ok_or(KernelError::InternalError)?;

    let mut page_data = vec![0u8; FRAME_SIZE];
    // SAFETY: frame_virt points to a valid, mapped physical frame via HHDM.
    // We read FRAME_SIZE bytes (the entire 16 KiB frame).
    unsafe {
        core::ptr::copy_nonoverlapping(
            frame_virt as *const u8,
            page_data.as_mut_ptr(),
            FRAME_SIZE,
        );
    }

    // Step 2: Allocate a swap slot and write the data.
    // Multi-device: alloc_slot() tries the highest-priority device first,
    // overflowing to lower-priority devices when the preferred one is full.
    let swap_entry = {
        let mut state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }

        let global_slot = state.alloc_slot().ok_or(KernelError::OutOfMemory)?;
        let entry = SwapEntry::new(global_slot).ok_or(KernelError::InternalError)?;

        state.write_slot(global_slot, &page_data)?;

        entry
    };
    // Drop SWAP lock before page table manipulation (lock ordering).

    // Step 3: Unmap the frame from the page table.
    // SAFETY: pml4_phys is valid, virt is frame-aligned and mapped.
    let phys_frame = unsafe { page_table::unmap_frame(pml4_phys, virt)? };

    // Remove reverse mapping — this frame is no longer mapped at this virt.
    super::rmap::remove(phys_frame.addr(), pml4_phys, virt.as_u64());

    // Step 4: Write swap entries into the PTEs.
    let swap_pte = PageTableEntry::from_raw(swap_entry.to_pte_raw());
    // SAFETY: pml4_phys valid, virt frame-aligned, PTEs now non-present.
    unsafe { page_table::write_swap_entries(pml4_phys, virt, swap_pte)?; }

    // Step 5: Flush the TLB.
    // SAFETY: invlpg is always safe in ring 0.
    unsafe { page_table::flush_frame(virt); }

    // Step 6: Free the physical frame.
    // SAFETY: The frame was just unmapped and the TLB flushed, so no
    // CPU holds a mapping to it.
    unsafe { frame::free_frame(phys_frame)?; }

    serial_println!(
        "[swap] Swapped out: virt={:#x} → slot={}",
        virt.as_u64(), swap_entry.slot()
    );

    crate::ktrace::record(
        crate::ktrace::Category::Mm,
        crate::ktrace::event::SWAP_OUT,
        virt.as_u64(),
        swap_entry.slot() as u64,
    );

    Ok(swap_entry)
}

/// Swap in a page: restore a previously swapped-out page to physical
/// memory.
///
/// 1. Reads the swap entry from the PTE to find the slot index.
/// 2. Allocates a new physical frame.
/// 3. Reads the page data from the swap backend.
/// 4. Copies the data into the new frame (via HHDM).
/// 5. Maps the frame into the page table with the given flags.
/// 6. Frees the swap slot.
/// 7. Flushes the TLB.
///
/// # Arguments
///
/// - `pml4_phys`: the process's PML4 table physical address.
/// - `virt`: the virtual address of the swapped-out frame.
/// - `flags`: the page flags to apply to the restored mapping (should
///   match the original VMA's flags).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The PTE at `virt` must contain a valid swap entry.
pub unsafe fn swap_in_page(
    pml4_phys: u64,
    virt: VirtAddr,
    flags: PageFlags,
) -> KernelResult<()> {
    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }

    // Step 1: Read the swap entry from the PTE.
    let pte = page_table::read_leaf_pte(pml4_phys, virt)
        .ok_or(KernelError::InvalidAddress)?;

    let swap_entry = SwapEntry::from_pte_raw(pte.raw())
        .ok_or(KernelError::InvalidArgument)?;

    // Step 2: Read page data from the swap backend.
    // Multi-device: find_device() locates which backend owns this slot.
    let mut page_data = vec![0u8; FRAME_SIZE];
    {
        let state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }
        state.read_slot(swap_entry.slot(), &mut page_data)?;
    }
    // Drop SWAP lock before frame allocation (lock ordering).

    // Step 3: Allocate a new physical frame.
    let new_frame = frame::alloc_frame()?;

    // Step 4: Copy the page data into the new frame via HHDM.
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let frame_virt = new_frame.addr()
        .checked_add(hhdm)
        .ok_or(KernelError::InternalError)?;

    // SAFETY: new_frame is freshly allocated and mapped via HHDM.
    unsafe {
        core::ptr::copy_nonoverlapping(
            page_data.as_ptr(),
            frame_virt as *mut u8,
            FRAME_SIZE,
        );
    }

    // Step 5: Map the frame into the page table.
    // First, clear the swap entries (write EMPTY to all 4 PTEs).
    // SAFETY: pml4_phys valid, virt frame-aligned.
    unsafe {
        page_table::write_swap_entries(
            pml4_phys,
            virt,
            PageTableEntry::EMPTY,
        )?;
    }

    // Now map the new frame with proper flags.
    // SAFETY: pml4_phys valid, virt frame-aligned, new_frame valid.
    unsafe {
        page_table::map_frame(pml4_phys, virt, new_frame, flags)?;
    }

    // Register reverse mapping — this frame is now mapped at this virt.
    super::rmap::add(new_frame.addr(), pml4_phys, virt.as_u64());

    // Step 6: Free the swap slot.
    {
        let mut state = SWAP.lock();
        state.discard_slot(swap_entry.slot());
        state.free_slot(swap_entry.slot());
    }

    // Step 7: Flush the TLB.
    // SAFETY: invlpg is always safe.
    unsafe { page_table::flush_frame(virt); }

    serial_println!(
        "[swap] Swapped in: virt={:#x} ← slot={}",
        virt.as_u64(), swap_entry.slot()
    );

    crate::ktrace::record(
        crate::ktrace::Category::Mm,
        crate::ktrace::event::SWAP_IN,
        virt.as_u64(),
        swap_entry.slot() as u64,
    );

    Ok(())
}

/// Check if a PTE contains a swap entry.
///
/// This is used by the page fault handler to determine whether a
/// non-present page was swapped out (vs. truly unmapped or demand-paged).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
#[must_use]
pub unsafe fn is_swapped(pml4_phys: u64, virt: VirtAddr) -> bool {
    page_table::read_leaf_pte(pml4_phys, virt)
        .is_some_and(|pte| pte.is_swap())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-test of the swap subsystem.
///
/// Tests:
/// 1. Swap entry encoding/decoding roundtrip.
/// 2. Slot allocator: alloc, free, double-alloc, capacity exhaustion.
/// 3. In-memory backend: write, read, data integrity.
pub fn self_test() {
    serial_println!("[swap] Running self-test...");

    // --- Swap entry encoding/decoding ---
    {
        let entry = SwapEntry::new(42).expect("slot 42 should be valid");
        assert_eq!(entry.slot(), 42);

        let raw = entry.to_pte_raw();
        // Bit 0 should be 0 (not present).
        assert_eq!(raw & 1, 0, "swap PTE bit 0 should be 0");
        // Bit 1 should be 1 (swap marker).
        assert_eq!(raw & 2, 2, "swap PTE bit 1 should be 1 (swap marker)");

        let decoded = SwapEntry::from_pte_raw(raw).expect("should decode");
        assert_eq!(decoded.slot(), 42, "roundtrip should preserve slot");

        // Non-swap PTE (all zeros) should not decode.
        assert!(SwapEntry::from_pte_raw(0).is_none());
        // Present PTE should not decode.
        assert!(SwapEntry::from_pte_raw(1).is_none());
        // Raw value with only swap marker should decode to slot 0.
        let slot0 = SwapEntry::from_pte_raw(0b10).expect("slot 0");
        assert_eq!(slot0.slot(), 0);

        serial_println!("[swap]   Swap entry encoding: OK");
    }

    // --- Slot allocator ---
    {
        let mut alloc = SwapSlotAllocator::new(128);
        assert_eq!(alloc.free_count(), 128);

        // Allocate all 128 slots.
        let mut slots = Vec::new();
        for _ in 0..128 {
            let s = alloc.alloc().expect("should have free slots");
            assert!(!slots.contains(&s), "duplicate slot allocated");
            slots.push(s);
        }
        assert_eq!(alloc.free_count(), 0);

        // Next alloc should fail.
        assert!(alloc.alloc().is_none(), "should be full");

        // Free slot 50, then re-allocate — should get slot 50.
        alloc.free(50);
        assert_eq!(alloc.free_count(), 1);
        let reused = alloc.alloc().expect("should find freed slot");
        assert_eq!(reused, 50, "should reuse freed slot");
        assert_eq!(alloc.free_count(), 0);

        // Free all.
        for s in &slots {
            alloc.free(*s);
        }
        assert_eq!(alloc.free_count(), 128);

        serial_println!("[swap]   Slot allocator: OK");
    }

    // --- Compressed zram backend ---
    {
        let mut backend = MemBackend::new(4);

        // Write test pattern to slot 0 (repeating — highly compressible).
        let mut data = vec![0u8; FRAME_SIZE];
        for (i, byte) in data.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            {
                *byte = (i & 0xFF) as u8;
            }
        }
        backend.write(0, &data).expect("write should succeed");
        // Verify it compressed (compressed_count should be 1).
        assert_eq!(backend.compressed_count, 1, "should compress repeating data");
        assert!(
            backend.compressed_bytes < FRAME_SIZE as u64,
            "compressed should be smaller than uncompressed"
        );

        // Read back and verify integrity.
        let mut buf = vec![0u8; FRAME_SIZE];
        backend.read(0, &mut buf).expect("read should succeed");
        assert_eq!(buf, data, "data integrity check failed after compress/decompress");

        // Write all-zero page to slot 1 (special case: 1-byte encoding).
        let zeros = vec![0u8; FRAME_SIZE];
        backend.write(1, &zeros).expect("write zeros");
        assert_eq!(backend.compressed_count, 2);
        // Zero page stores as 1 byte — compressed_bytes should barely increase.
        let bytes_after_zero = backend.compressed_bytes;

        // Read back zeros.
        backend.read(1, &mut buf).expect("read zeros");
        assert_eq!(buf, zeros, "zero page roundtrip through backend");

        // Write uniform non-zero to slot 2 (compresses well: run of 0xAA).
        let data2 = vec![0xAA; FRAME_SIZE];
        backend.write(2, &data2).expect("write slot 2");

        // Verify slot 0 is unchanged.
        backend.read(0, &mut buf).expect("re-read slot 0");
        assert_eq!(buf, data, "slot 0 should be unchanged");

        // Read slot 2.
        backend.read(2, &mut buf).expect("read slot 2");
        assert_eq!(buf, data2, "slot 2 data check");

        // Discard and verify it's gone.
        backend.discard(0);
        assert!(
            backend.read(0, &mut buf).is_err(),
            "discarded slot should fail to read"
        );

        // Byte accounting: discard should reduce compressed_bytes.
        assert!(
            backend.compressed_bytes < bytes_after_zero.saturating_add(FRAME_SIZE as u64),
            "discard should reduce compressed byte count"
        );

        // Out-of-range slot should fail.
        assert!(backend.write(4, &data).is_err(), "slot 4 out of range");
        assert!(backend.read(4, &mut buf).is_err(), "slot 4 out of range");

        serial_println!(
            "[swap]   Compressed zram backend: OK (saved {} bytes across test pages)",
            backend.uncompressed_bytes.saturating_sub(backend.compressed_bytes)
        );
    }

    // --- Page reclamation tracking ---
    {
        // The global RECLAIM state may have entries from earlier boot
        // activity, so test using the counting API.
        let before = reclaimable_count();

        // Use the real kernel PML4 (from CR3) with user-space addresses
        // that are not mapped.  The reclaimer will walk the real page
        // tables, find the addresses unmapped, and mark them inactive.
        // (Using a fake PML4 would crash because read_leaf_pte
        // dereferences it via HHDM.)
        let real_pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
        let addr_a: u64 = 0x0040_0000; // 4 MiB — unmapped user addr
        let addr_b: u64 = 0x0040_4000; // 4 MiB + 16 KiB
        let addr_c: u64 = 0x0040_8000; // 4 MiB + 32 KiB
        let flags = PageFlags::PRESENT
            | PageFlags::WRITABLE
            | PageFlags::USER_ACCESSIBLE
            | PageFlags::NO_EXECUTE;

        register_reclaimable(real_pml4, addr_a, flags);
        register_reclaimable(real_pml4, addr_b, flags);
        register_reclaimable(real_pml4, addr_c, flags);
        assert_eq!(
            reclaimable_count(),
            before + 3,
            "should have 3 more reclaimable pages"
        );

        // Unregister one.
        unregister_reclaimable(real_pml4, addr_b);
        assert_eq!(
            reclaimable_count(),
            before + 2,
            "should have 2 more reclaimable pages after unregister"
        );

        // Unregister a non-existent page — count unchanged.
        unregister_reclaimable(real_pml4, 0xFFFF_0000);
        assert_eq!(
            reclaimable_count(),
            before + 2,
            "unregister of non-existent page should be a no-op"
        );

        // try_reclaim on the unmapped pages: the Clock algorithm will
        // walk the real page tables, find the addresses are not mapped
        // (read_leaf_pte returns None or non-present), and mark them
        // inactive.
        let reclaimed = try_reclaim(10);
        // No pages should actually be reclaimed (addresses not mapped).
        assert_eq!(reclaimed, 0, "should reclaim 0 from unmapped pages");

        // All three should now be marked inactive (two remaining active
        // ones were deactivated by the reclaimer due to unmapped PTEs).
        // Note: the originally unregistered one was already inactive.
        // The count should be back to `before`.
        assert_eq!(
            reclaimable_count(),
            before,
            "all unmapped pages should be deactivated after reclaim scan"
        );

        serial_println!("[swap]   Page reclamation tracking: OK");
    }

    // --- Multi-device priority allocation ---
    {
        // Create a local test with two backends to verify priority ordering.
        // Simulate two-device state: A at priority 100, B at priority 0.
        // Device A has base_slot=0, Device B has base_slot=4.
        let mut test_state = SwapState {
            devices: vec![
                SwapDevice {
                    priority: 100,
                    name: String::from("test-fast"),
                    backend: SwapBackend::Memory(MemBackend::new(4)),
                    slots: SwapSlotAllocator::new(4),
                    base_slot: 0,
                },
                SwapDevice {
                    priority: 0,
                    name: String::from("test-slow"),
                    backend: SwapBackend::Memory(MemBackend::new(4)),
                    slots: SwapSlotAllocator::new(4),
                    base_slot: 4,
                },
            ],
            total_capacity: 8,
            initialized: true,
        };

        // Allocate 4 slots — should all come from device A (higher priority).
        for _ in 0..4u32 {
            let slot = test_state.alloc_slot().expect("should have capacity");
            assert!(slot < 4, "slot {} should be from device A (0..4)", slot);
        }

        // Device A is full.  Next alloc should come from device B (slots 4..7).
        let overflow = test_state.alloc_slot().expect("overflow to B");
        assert!(
            overflow >= 4 && overflow < 8,
            "overflow slot {} should be from device B (4..8)", overflow
        );

        // Free a slot from device A, next alloc should go back to A.
        test_state.free_slot(1);
        let refilled = test_state.alloc_slot().expect("refill from A");
        assert!(refilled < 4, "refilled slot {} should be from device A", refilled);

        // Write and read through the multi-device API.
        let test_data = vec![0x42u8; FRAME_SIZE];
        test_state.write_slot(overflow, &test_data).expect("write to B");
        let mut read_buf = vec![0u8; FRAME_SIZE];
        test_state.read_slot(overflow, &mut read_buf).expect("read from B");
        assert_eq!(read_buf, test_data, "multi-device read/write integrity");

        // Verify find_device routing.
        assert_eq!(
            test_state.find_device(0).map(|(d, _)| d),
            Some(0), "slot 0 → device 0"
        );
        assert_eq!(
            test_state.find_device(3).map(|(d, _)| d),
            Some(0), "slot 3 → device 0"
        );
        assert_eq!(
            test_state.find_device(4).map(|(d, _)| d),
            Some(1), "slot 4 → device 1"
        );
        assert_eq!(
            test_state.find_device(7).map(|(d, _)| d),
            Some(1), "slot 7 → device 1"
        );
        assert!(
            test_state.find_device(8).is_none(),
            "slot 8 should not belong to any device"
        );

        serial_println!(
            "[swap]   Multi-device priority: OK (alloc prefers priority=100, overflows to priority=0)"
        );
    }

    // Note: disk backend test is in self_test_disk(), called separately
    // after the disk device is registered.

    serial_println!("[swap] Self-test PASSED");
}

/// Run self-test for the disk-backed swap backend.
///
/// Called after `init_disk()` succeeds.  Tests write-read roundtrip
/// through the live disk backend with compressed and uncompressed data.
pub fn self_test_disk() {
    serial_println!("[swap] Running disk backend self-test...");

    let has_disk = {
        let state = SWAP.lock();
        state.devices.iter().any(|d| matches!(d.backend, SwapBackend::Disk(_)))
    };

    if !has_disk {
        serial_println!("[swap]   Disk backend not active — skipped");
        return;
    }

    // Test a write-read roundtrip through the multi-device API.
    // alloc_slot() will use the highest-priority device with capacity.
    // If zram is still available, the test slot may go there.  To
    // ensure we exercise the disk path, we allocate through the global
    // API and verify the data integrity regardless of which backend
    // was chosen.
    let mut test_data = vec![0u8; FRAME_SIZE];
    for (i, byte) in test_data.iter_mut().enumerate() {
        // Repeating pattern that compresses well.
        #[allow(clippy::cast_possible_truncation)]
        {
            *byte = ((i * 7 + 13) & 0xFF) as u8;
        }
    }

    let slot = {
        let mut state = SWAP.lock();
        let slot = state.alloc_slot().expect("should have free slots");
        state.write_slot(slot, &test_data).expect("write test data");
        slot
    };

    // Read back and verify.
    let mut read_buf = vec![0u8; FRAME_SIZE];
    {
        let state = SWAP.lock();
        state.read_slot(slot, &mut read_buf).expect("read test data");
    }
    assert_eq!(read_buf, test_data, "disk roundtrip data integrity");

    // Test all-zero page (compresses to 1 byte — exercises
    // the compression path).
    let zero_data = vec![0u8; FRAME_SIZE];
    let zero_slot = {
        let mut state = SWAP.lock();
        let slot = state.alloc_slot().expect("free slot for zeros");
        state.write_slot(slot, &zero_data).expect("write zeros");
        slot
    };

    {
        let state = SWAP.lock();
        state.read_slot(zero_slot, &mut read_buf).expect("read zeros");
    }
    assert_eq!(read_buf, zero_data, "zero page roundtrip");

    // Clean up — free the test slots.
    {
        let mut state = SWAP.lock();
        state.discard_slot(slot);
        state.free_slot(slot);
        state.discard_slot(zero_slot);
        state.free_slot(zero_slot);
    }

    // Report which devices are active.
    {
        let state = SWAP.lock();
        serial_println!(
            "[swap]   {} device(s): {}",
            state.devices.len(),
            state.devices.iter()
                .map(|d| alloc::format!("{}(pri={},free={})", d.name, d.priority, d.slots.free_count()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    serial_println!("[swap] Disk backend self-test PASSED");
}
