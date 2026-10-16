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
//! (`design.txt` offers a swap partition at install time too; both are
//! areas here, below.)
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
//! Bit 0:      0 (not present — triggers page fault)
//! Bit 1:      1 (swap marker — distinguishes from truly-unmapped pages)
//! Bits 2–31:  swap slot index (30 bits, up to ~1 billion slots)
//! Bits 32–42: the 4 KiB page's flags when it was swapped out (PTE bits 0–10)
//! Bit 43:     the 4 KiB page's NO_EXECUTE (PTE bit 63)
//! Bits 44–63: reserved (zero)
//! ```
//!
//! A 16 KiB frame is swapped as a unit, and each of its four 4 KiB PTEs
//! becomes an entry naming the frame's one slot, with that part's own
//! flags kept beside it -- none, with no present bit, for a part that was
//! not mapped. Swap-in maps each part back with exactly those
//! ([`swap_in_page`]), so a read-only or executable part, or a gap
//! between two segments that share the frame, comes back as it was.
//!
//! A slot is counted by the entries that name it (`SwapState::part_refs`):
//! four when written, one fewer for each entry that goes -- at swap-in,
//! when its page is unmapped ([`release_swap_pte`]), or when the address
//! space is torn down ([`release_swap_parts`]) -- and the last frees it.
//!
//! ## Swap areas
//!
//! Pages go first to compressed RAM (zram, priority 100), then to the disk
//! areas: a block device, or a file, that says it is swap. Saying so is
//! Linux's `mkswap` header ([`parse_header`]): `SWAPSPACE2` at the end of the
//! area's first page, version 1, and the area's length. A disk without it is
//! never touched, nor one whose first bytes also say it is something else
//! ([`ambivalent`]).
//!
//! An area is a list of extents ([`AreaExtent`]): runs of its sectors that
//! are contiguous on the device. A whole device or a partition is one; a
//! swap file is the runs its filesystem placed it in, which the kernel then
//! reads and writes directly, past the filesystem, as Linux does. Slot 0
//! holds the header, and a slot holding a page `mkswap -c` found bad is
//! never used.
//!
//! Areas come and go: [`activate`] (`swapon`) and [`deactivate`]
//! (`swapoff`), which takes the area out of allocation, brings every page
//! in it back into memory -- walking every address space for the entries
//! that name it -- and only then lets it go. The boot activates the whole
//! devices that carry a header ([`activate_signed_devices`]); files and
//! partitions are userspace's to turn on.
//!
//! ## Thread Safety
//!
//! The swap subsystem uses its own spinlock.  Lock ordering:
//! SWAP → page table manipulation → frame allocator. `swapon` and `swapoff`
//! are serialised by a sleeping lock of their own ([`SWAP_ADMIN`]), held
//! across the drain, which can take a while and takes the address-space
//! locks; SWAP is never held across it.

use super::frame::{self, FRAME_SIZE};
use super::page_table::{self, PageFlags, PageTableEntry, VirtAddr};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::Mutex;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Swap marker bit in a non-present PTE (bit 1).
const SWAP_MARKER_BIT: u64 = 1 << 1;

/// Mask for the swap slot index (bits 2–31 = 30 bits).
const SWAP_SLOT_MASK: u64 = 0xFFFF_FFFC; // bits 2–31
/// Shift to extract the slot index from a raw PTE.
const SWAP_SLOT_SHIFT: u32 = 2;

/// Where a swap entry keeps the flags its 4 KiB page had when the frame was
/// swapped out: PTE bits 0-10 at bits 32-42 ([`SwapEntry::to_pte_raw_keeping`]).
const SWAP_KEPT_SHIFT: u32 = 32;
/// The PTE bits kept at [`SWAP_KEPT_SHIFT`]: 0-10, less accessed and dirty,
/// which describe the frame that is going and not the page.
const SWAP_KEPT_LOW: u64 = 0x7FF & !((1 << 5) | (1 << 6));
/// Where a swap entry keeps its page's `NO_EXECUTE` (PTE bit 63).
const SWAP_KEPT_NX: u64 = 1 << 43;
/// Where a swap entry keeps its page's [`PageFlags::MLOCKED`] (PTE bit 11).
///
/// A locked page is never swapped out, so no entry is made with it; it is
/// set on an entry already made, when a range holding a swapped-out page is
/// locked (`mm::mlock`), so the page comes back locked -- whichever path
/// brings it back: a fault, `mlock`'s own populating, `fork`. Without it an
/// `MLOCK_ONFAULT` lock, which brings nothing back itself, would leave such a
/// page unlocked for good.
pub(crate) const SWAP_KEPT_LOCKED: u64 = 1 << 44;

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

    /// [`Self::to_pte_raw`], keeping `kept` -- the flags the 4 KiB page had
    /// -- in the entry's high bits, where the CPU never looks (the entry is
    /// not present). Accessed and dirty are dropped. Pass
    /// [`PageFlags::empty()`] for a part of the frame that was not mapped: it
    /// stays unmapped at swap-in.
    #[must_use]
    pub const fn to_pte_raw_keeping(self, kept: PageFlags) -> u64 {
        let low = (kept.bits() & SWAP_KEPT_LOW) << SWAP_KEPT_SHIFT;
        let nx = if kept.bits() & PageFlags::NO_EXECUTE.bits() != 0 {
            SWAP_KEPT_NX
        } else {
            0
        };
        let locked = if kept.bits() & PageFlags::MLOCKED.bits() != 0 {
            SWAP_KEPT_LOCKED
        } else {
            0
        };
        self.to_pte_raw() | low | nx | locked
    }

    /// The flags a swap entry kept for its 4 KiB page
    /// ([`Self::to_pte_raw_keeping`]): no `PRESENT` for a part that was not
    /// mapped.
    #[must_use]
    pub const fn kept_flags(raw: u64) -> PageFlags {
        let low = (raw >> SWAP_KEPT_SHIFT) & SWAP_KEPT_LOW;
        let nx = if raw & SWAP_KEPT_NX != 0 {
            PageFlags::NO_EXECUTE.bits()
        } else {
            0
        };
        let locked = if raw & SWAP_KEPT_LOCKED != 0 {
            PageFlags::MLOCKED.bits()
        } else {
            0
        };
        PageFlags::from_bits(low | nx | locked)
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

    /// [`Self::new`], refusing rather than panicking when the bitmap cannot
    /// be allocated: an area added by `swapon` can be terabytes.
    fn try_new(capacity: u32) -> KernelResult<Self> {
        let words = capacity.div_ceil(64) as usize;
        let mut bitmap = Vec::new();
        bitmap
            .try_reserve_exact(words)
            .map_err(|_| KernelError::OutOfMemory)?;
        bitmap.resize(words, 0);
        Ok(Self {
            bitmap,
            capacity,
            used: 0,
            hint: 0,
        })
    }

    /// Mark `slot` used for good: never given out, never freed (a swap
    /// area's header, or a slot holding a bad page).
    fn reserve(&mut self, slot: u32) {
        if slot >= self.capacity {
            return;
        }
        let wi = (slot / 64) as usize;
        let bit = 1u64 << (slot % 64);
        if let Some(w) = self.bitmap.get_mut(wi)
            && *w & bit == 0
        {
            *w |= bit;
            self.used = self.used.saturating_add(1);
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
            let slot = (wi as u64).saturating_mul(64).saturating_add(bit as u64);

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
        self.bitmap.get(wi).is_some_and(|w| *w & (1u64 << bit) != 0)
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
                self.compressed_bytes = self.compressed_bytes.saturating_add(compressed_len);
                self.compressed_count = self.compressed_count.saturating_add(1);
                SlotData::Compressed(compressed)
            }
            None => {
                // Incompressible — store uncompressed.
                self.compressed_bytes = self.compressed_bytes.saturating_add(FRAME_SIZE as u64);
                self.uncompressed_count = self.uncompressed_count.saturating_add(1);
                SlotData::Uncompressed(data.to_vec())
            }
        };

        self.uncompressed_bytes = self.uncompressed_bytes.saturating_add(FRAME_SIZE as u64);
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
                let decompressed = super::compress::decompress(compressed, FRAME_SIZE)
                    .map_err(|_| KernelError::InternalError)?;
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
                self.uncompressed_bytes = self.uncompressed_bytes.saturating_sub(FRAME_SIZE as u64);
                self.compressed_bytes = self.compressed_bytes.saturating_sub(stored_size);
            }
            *slot_storage = None;
        }
    }
}

// ---------------------------------------------------------------------------
// The swap header: what says a disk area is swap
// ---------------------------------------------------------------------------

/// The signature at the end of a swap area's first page: version 1 of the
/// header, which Linux's `mkswap` writes. Version 0's `SWAP-SPACE` has been
/// refused by Linux since 2.5, and is here.
const SWAP_MAGIC: &[u8; 10] = b"SWAPSPACE2";

/// The first-page sizes a header is looked for at, smallest first: 4 KiB is
/// what `mkswap` writes on x86 Linux, 16 KiB is SlateOS's page
/// (`scripts/mkswap.py`), and 8 KiB is `mkswap -p 8192`'s.
const HEADER_PAGE_SIZES: [usize; 3] = [4096, 8192, 16_384];

/// `union swap_header`'s fields, by byte offset. The first 1 KiB is left for
/// a boot block or a disk label (`bootbits`).
const HDR_VERSION: usize = 1024;
/// The area's last page.
const HDR_LAST_PAGE: usize = 1028;
/// How many bad pages are listed.
const HDR_NR_BADPAGES: usize = 1032;
/// The bad-page list, `u32`s up to the signature.
const HDR_BADPAGES: usize = 1536;

/// How many bytes of an area's start [`activate`] reads to find its header:
/// one slot, which holds every header page size looked for.
const HEADER_READ_BYTES: usize = FRAME_SIZE;

/// A swap area's header, as `mkswap` wrote it ([`parse_header`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapHeader {
    /// The page size it was made with: the unit of `last_page` and of the
    /// bad pages.
    pub page_size: usize,
    /// The area's last page: it is `last_page + 1` pages, the first being
    /// the header.
    pub last_page: u32,
    /// Pages `mkswap -c` found unreadable, by number.
    pub bad_pages: Vec<u32>,
}

impl SwapHeader {
    /// The area's length in bytes, as the header gives it.
    #[must_use]
    pub fn area_bytes(&self) -> u64 {
        u64::from(self.last_page)
            .saturating_add(1)
            .saturating_mul(self.page_size as u64)
    }
}

/// Why an area's first bytes are not a swap header it can be used by
/// ([`parse_header`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    /// No `SWAPSPACE2` where a header page would end: not swap -- a
    /// filesystem, a partition table, a hibernation image (whose signature
    /// replaces it), or nothing at all.
    NoSignature,
    /// The signature, with a version other than 1.
    Version(u32),
    /// `last_page` is 0: no room after the header.
    Empty,
    /// More bad pages than the page holds, or one that is not in the area.
    BadPages,
}

/// The little-endian `u32` at `at` in `bytes`.
fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(field))
}

/// The swap header at the start of `first`, an area's first bytes -- at
/// least one header page; [`HEADER_READ_BYTES`] covers every size.
///
/// Linux's `read_swap_header` is the model: the signature, version 1, a
/// `last_page` that is not 0, and a bad-page list that fits its page and
/// names pages in the area. The smallest page size whose end holds the
/// signature is the header's. A header in the other byte order (which
/// Linux reads, swapping each field) is not taken: SlateOS is x86-64 only.
///
/// # Errors
///
/// [`HeaderError`]: what is wrong with it.
pub fn parse_header(first: &[u8]) -> Result<SwapHeader, HeaderError> {
    let page_size = HEADER_PAGE_SIZES
        .iter()
        .copied()
        .find(|&ps| {
            ps.checked_sub(SWAP_MAGIC.len())
                .and_then(|at| first.get(at..ps))
                .is_some_and(|m| m == SWAP_MAGIC)
        })
        .ok_or(HeaderError::NoSignature)?;
    let field = |at| le_u32(first, at).ok_or(HeaderError::NoSignature);
    let version = field(HDR_VERSION)?;
    if version != 1 {
        return Err(HeaderError::Version(version));
    }
    let last_page = field(HDR_LAST_PAGE)?;
    if last_page == 0 {
        return Err(HeaderError::Empty);
    }
    // The list runs from HDR_BADPAGES to the signature.
    let room = page_size
        .saturating_sub(SWAP_MAGIC.len())
        .saturating_sub(HDR_BADPAGES)
        / 4;
    let listed = usize::try_from(field(HDR_NR_BADPAGES)?).unwrap_or(usize::MAX);
    if listed > room {
        return Err(HeaderError::BadPages);
    }
    let mut bad_pages = Vec::with_capacity(listed);
    for i in 0..listed {
        let page = le_u32(first, HDR_BADPAGES.saturating_add(i.saturating_mul(4)))
            .ok_or(HeaderError::BadPages)?;
        if page == 0 || page > last_page {
            return Err(HeaderError::BadPages);
        }
        bad_pages.push(page);
    }
    Ok(SwapHeader {
        page_size,
        last_page,
        bad_pages,
    })
}

// ---------------------------------------------------------------------------
// Disk areas: extents, and the backend that reads and writes through them
// ---------------------------------------------------------------------------

/// A run of a swap area's sectors that is contiguous on its device: the
/// whole area for a device or a partition, one of several for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaExtent {
    /// Its first sector, counted from the area's start.
    pub area_sector: u64,
    /// Where that sector is on the device.
    pub dev_sector: u64,
    /// How many sectors.
    pub sectors: u64,
}

/// Number of 512-byte sectors per 16 KiB frame.
const SECTORS_PER_FRAME: u32 = (FRAME_SIZE / SECTOR_BYTES) as u32;

/// A sector's size: the block layer's.
const SECTOR_BYTES: usize = crate::blkdev::SECTOR_SIZE;

/// Where sectors `[start, start + count)` of an area are on its device:
/// `(device sector, sectors)` pieces, in order. `None` when the extents do
/// not cover them all.
fn area_pieces(extents: &[AreaExtent], start: u64, count: u64) -> Option<Vec<(u64, u64)>> {
    let end = start.checked_add(count)?;
    let mut pieces = Vec::new();
    let mut at = start;
    // The first extent that ends past `at`.
    let mut i = extents.partition_point(|e| e.area_sector.saturating_add(e.sectors) <= at);
    while at < end {
        let e = extents.get(i)?;
        let into = at.checked_sub(e.area_sector)?;
        let here = e.sectors.checked_sub(into)?.min(end.saturating_sub(at));
        if here == 0 {
            return None;
        }
        pieces.push((e.dev_sector.checked_add(into)?, here));
        at = at.saturating_add(here);
        i = i.saturating_add(1);
    }
    Some(pieces)
}

/// The sectors `extents` cover, when they describe an area: sorted, the
/// first at the area's sector 0, each starting where the last ended, none
/// empty, and every one inside a device of `device_sectors`. `None`
/// otherwise.
fn extents_span(extents: &[AreaExtent], device_sectors: u64) -> Option<u64> {
    let mut next = 0u64;
    for e in extents {
        if e.area_sector != next || e.sectors == 0 {
            return None;
        }
        if e.dev_sector.checked_add(e.sectors)? > device_sectors {
            return None;
        }
        next = next.checked_add(e.sectors)?;
    }
    (next > 0).then_some(next)
}

/// Whether two areas' extents share a device sector.
fn extents_overlap(a: &[AreaExtent], b: &[AreaExtent]) -> bool {
    a.iter().any(|x| {
        let x_end = x.dev_sector.saturating_add(x.sectors);
        b.iter().any(|y| {
            let y_end = y.dev_sector.saturating_add(y.sectors);
            x.dev_sector < y_end && y.dev_sector < x_end
        })
    })
}

/// Read or write `buf` (whole sectors) at sectors `[start, ...)` of the area
/// at `extents` on `device`, piece by piece, past every cache.
fn area_io(
    device: &str,
    extents: &[AreaExtent],
    start: u64,
    buf: &mut [u8],
    write: bool,
) -> KernelResult<()> {
    let sectors = (buf.len() / SECTOR_BYTES) as u64;
    let pieces = area_pieces(extents, start, sectors).ok_or(KernelError::InvalidArgument)?;
    let mut off = 0usize;
    for (dev_sector, n) in pieces {
        let count = u32::try_from(n).map_err(|_| KernelError::InvalidArgument)?;
        let len = usize::try_from(n)
            .ok()
            .and_then(|n| n.checked_mul(SECTOR_BYTES))
            .ok_or(KernelError::InvalidArgument)?;
        let end = off.checked_add(len).ok_or(KernelError::InvalidArgument)?;
        let chunk = buf.get_mut(off..end).ok_or(KernelError::InvalidArgument)?;
        crate::blkdev::with_device(device, |dev| {
            if write {
                dev.write_sectors(dev_sector, count, chunk)
            } else {
                dev.read_sectors(dev_sector, count, chunk)
            }
        })
        .ok_or(KernelError::NoSuchDevice)?
        .map_err(|_| KernelError::IoError)?;
        off = end;
    }
    Ok(())
}

/// A swap area on a block device, read and written past the filesystems and
/// their caches, through its extents.
///
/// Each slot is [`SECTORS_PER_FRAME`] sectors of the area. A page is
/// compressed first ([`super::compress`]), and only the sectors the result
/// needs are written and read back -- one for a page of zeros. What each slot
/// holds is kept here, in two bytes a slot ([`Self::meta`]).
///
/// I/O is synchronous, under the SWAP lock, as it has always been here.
struct DiskBackend {
    /// The block device, by its name in the registry (`"vdb"`).
    device_name: String,
    /// Where the area is on it.
    extents: Vec<AreaExtent>,
    /// Number of slots.
    capacity: u32,
    /// Per slot: 0 when nothing is written there; otherwise how many bytes
    /// it holds (1 to `FRAME_SIZE`) in bits 0-14, and [`META_COMPRESSED`]
    /// when they are compressed.
    meta: Vec<u16>,
}

/// [`DiskBackend::meta`]'s bit for a slot holding compressed bytes.
const META_COMPRESSED: u16 = 1 << 15;

impl DiskBackend {
    /// A backend for the area at `extents` on `device_name`, of `capacity`
    /// slots, nothing written yet. `OutOfMemory` when its two bytes a slot
    /// cannot be had.
    fn try_new(device_name: &str, extents: Vec<AreaExtent>, capacity: u32) -> KernelResult<Self> {
        let mut meta = Vec::new();
        meta.try_reserve_exact(capacity as usize)
            .map_err(|_| KernelError::OutOfMemory)?;
        meta.resize(capacity as usize, 0);
        Ok(Self {
            device_name: String::from(device_name),
            extents,
            capacity,
            meta,
        })
    }

    /// The area's first sector of `slot`.
    fn slot_start(slot: u32) -> u64 {
        u64::from(slot).saturating_mul(u64::from(SECTORS_PER_FRAME))
    }

    /// Write a page's data to a slot: compressed when that is smaller, and
    /// only the sectors it fills.
    fn write(&mut self, slot: u32, data: &[u8]) -> KernelResult<()> {
        if slot >= self.capacity || data.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        let (mut buf, meta) = match super::compress::compress(data) {
            // A slot holds FRAME_SIZE bytes; anything not smaller goes as it is.
            Some(compressed) if !compressed.is_empty() && compressed.len() < FRAME_SIZE => {
                let len = compressed.len();
                let mut padded = compressed;
                padded.resize(len.div_ceil(SECTOR_BYTES).saturating_mul(SECTOR_BYTES), 0);
                // len < FRAME_SIZE = 16384, below META_COMPRESSED.
                (padded, META_COMPRESSED | len as u16)
            }
            _ => (data.to_vec(), FRAME_SIZE as u16),
        };
        area_io(
            &self.device_name,
            &self.extents,
            Self::slot_start(slot),
            &mut buf,
            true,
        )?;
        let entry = self
            .meta
            .get_mut(slot as usize)
            .ok_or(KernelError::InternalError)?;
        *entry = meta;
        Ok(())
    }

    /// Read a slot's page back into `buf`, `FRAME_SIZE` bytes.
    fn read(&self, slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        if slot >= self.capacity || buf.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        let meta = self
            .meta
            .get(slot as usize)
            .copied()
            .filter(|&m| m != 0)
            .ok_or(KernelError::InvalidArgument)?;
        let len = usize::from(meta & !META_COMPRESSED);
        let mut disk = vec![0u8; len.div_ceil(SECTOR_BYTES).saturating_mul(SECTOR_BYTES)];
        area_io(
            &self.device_name,
            &self.extents,
            Self::slot_start(slot),
            &mut disk,
            false,
        )?;
        if meta & META_COMPRESSED != 0 {
            let compressed = disk.get(..len).ok_or(KernelError::InternalError)?;
            let page = super::compress::decompress(compressed, FRAME_SIZE)
                .map_err(|_| KernelError::InternalError)?;
            if page.len() != FRAME_SIZE {
                return Err(KernelError::InternalError);
            }
            buf.copy_from_slice(&page);
        } else {
            buf.copy_from_slice(disk.get(..FRAME_SIZE).ok_or(KernelError::InternalError)?);
        }
        Ok(())
    }

    /// Forget a slot's page: it holds nothing now.
    fn discard(&mut self, slot: u32) {
        if let Some(entry) = self.meta.get_mut(slot as usize) {
            *entry = 0;
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
    /// What `/proc/swaps` shows and `swapoff` names: the path `swapon` was
    /// given, as bytes (`zram` for the compressed tier).
    path: Vec<u8>,
    /// What the area is.
    kind: AreaKind,
    /// The storage backend.
    backend: SwapBackend,
    /// Per-device slot allocator.
    slots: SwapSlotAllocator,
    /// Global slot offset: this device owns global slots
    /// `[base_slot .. base_slot + capacity)`.
    base_slot: u32,
    /// Slots never given out: a disk area's header, and any holding a page
    /// its header lists as bad ([`SwapSlotAllocator::reserve`]). Counted in
    /// `slots.used`; not in what is reported as used.
    reserved: u32,
    /// `swapoff` is emptying it ([`deactivate`]): no slot is given out from
    /// it, while what it holds is still read back.
    draining: bool,
    /// Its priority is a default one ([`LEAST_PRIORITY`]), which `swapoff`
    /// gives back.
    default_priority: bool,
}

impl SwapDevice {
    /// Slots holding pages: used, less the reserved.
    fn pages(&self) -> u32 {
        self.slots.used.saturating_sub(self.reserved)
    }

    /// Slots that can hold pages: all, less the reserved.
    fn room(&self) -> u32 {
        self.slots.capacity.saturating_sub(self.reserved)
    }

    /// One past its last global slot.
    fn end_slot(&self) -> u32 {
        self.base_slot.saturating_add(self.slots.capacity)
    }
}

/// What a swap area is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaKind {
    /// Compressed RAM, the first tier.
    Memory,
    /// A block device, or a partition of one: Linux's `partition`.
    Device,
    /// A regular file, by its filesystem's identity and its inode: Linux's
    /// `file`. The filesystem refuses to change it while it is swap.
    File {
        /// The filesystem's identity, as the VFS gives it.
        fs_id: u64,
        /// The file's inode.
        ino: u64,
    },
}

/// Global swap subsystem state.
///
/// Supports multiple swap devices with priority-based allocation.
/// Devices are sorted by priority (descending) so iteration naturally
/// tries the fastest device first.
struct SwapState {
    /// All swap devices, sorted by priority (descending).
    devices: Vec<SwapDevice>,
    /// One past the highest global slot any area owns: `part_refs`' length.
    /// An area takes the lowest free run of global slots big enough
    /// ([`Self::find_gap`]), so `swapon` after `swapoff` reuses the space.
    slot_end: u32,
    /// Whether the subsystem is initialized (at least one device present).
    initialized: bool,
    /// How many PTEs name each global slot, by slot: four when
    /// [`swap_out_page`] writes it, one fewer for each entry that goes,
    /// and the slot is freed with the last ([`Self::release_parts`]).
    /// `slot_end` long.
    part_refs: Vec<u8>,
}

impl SwapState {
    const fn uninit() -> Self {
        Self {
            devices: Vec::new(),
            slot_end: 0,
            initialized: false,
            part_refs: Vec::new(),
        }
    }

    /// The lowest global slot where `len` slots are free of every area,
    /// below [`MAX_SWAP_SLOTS`].
    ///
    /// Allocates nothing: it runs under `SWAP`, and an allocation that fails
    /// its first try reclaims, which swaps out under `SWAP` again.
    fn find_gap(&self, len: u32) -> Option<u32> {
        let mut candidate = 0u32;
        loop {
            let end = candidate.checked_add(len)?;
            if end > MAX_SWAP_SLOTS {
                return None;
            }
            // Past every area the window meets: no start before the furthest
            // end among them has `len` free slots, since each such window
            // would meet that area too.
            match self
                .devices
                .iter()
                .filter(|d| d.base_slot < end && candidate < d.end_slot())
                .map(SwapDevice::end_slot)
                .max()
            {
                Some(next) => candidate = next,
                None => return Some(candidate),
            }
        }
    }

    /// Make `part_refs` reach `end`, and `slot_end` with it, in the room
    /// [`reserve_for_area`] made: growing it here would allocate under `SWAP`.
    fn grow_to(&mut self, end: u32) -> KernelResult<()> {
        if end <= self.slot_end {
            return Ok(());
        }
        if self.part_refs.capacity() < end as usize {
            return Err(KernelError::InternalError);
        }
        self.part_refs.resize(end as usize, 0);
        self.slot_end = end;
        Ok(())
    }

    /// Add `dev` among the areas, after every area of its priority or higher,
    /// in the room [`reserve_for_area`] made. Not a sort: a stable sort may
    /// allocate scratch space, and this runs under `SWAP`.
    fn insert_area(&mut self, dev: SwapDevice) -> KernelResult<()> {
        if self.devices.len() >= self.devices.capacity() {
            return Err(KernelError::InternalError);
        }
        let at = self
            .devices
            .iter()
            .position(|d| d.priority < dev.priority)
            .unwrap_or(self.devices.len());
        self.devices.insert(at, dev);
        Ok(())
    }

    /// Shrink `slot_end` and `part_refs` to the areas that remain.
    fn shrink_to_areas(&mut self) {
        let end = self
            .devices
            .iter()
            .map(SwapDevice::end_slot)
            .max()
            .unwrap_or(0);
        self.part_refs.truncate(end as usize);
        self.slot_end = end;
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
        // try the highest-priority (fastest) device first. One `swapoff` is
        // emptying gives nothing out.
        for dev in self.devices.iter_mut().filter(|d| !d.draining) {
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
        let (dev_idx, local_slot) = self
            .find_device(global_slot)
            .ok_or(KernelError::InvalidArgument)?;
        let dev = self
            .devices
            .get_mut(dev_idx)
            .ok_or(KernelError::InternalError)?;
        dev.backend.write(local_slot, data)
    }

    /// Read data from a global swap slot.
    fn read_slot(&self, global_slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        let (dev_idx, local_slot) = self
            .find_device(global_slot)
            .ok_or(KernelError::InvalidArgument)?;
        let dev = self
            .devices
            .get(dev_idx)
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

    /// `count` of the entries naming `global_slot` are gone; the last frees
    /// the slot. A slot nothing names any more is left alone, rather than
    /// freed a second time: it may be someone else's by now.
    fn release_parts(&mut self, global_slot: u32, count: u8) {
        let Some(refs) = self.part_refs.get_mut(global_slot as usize) else {
            return;
        };
        if *refs == 0 {
            serial_println!(
                "[swap] WARNING: slot {} released by {} more entr(ies) than named it",
                global_slot,
                count
            );
            return;
        }
        *refs = refs.saturating_sub(count);
        if *refs == 0 {
            self.discard_slot(global_slot);
            self.free_slot(global_slot);
        }
    }

    /// Free slots a page can be given now: across every area but one being
    /// emptied.
    fn total_free(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| !d.draining)
            .map(|d| d.slots.free_count())
            .sum()
    }

    /// Slots holding pages, across every area.
    fn total_used(&self) -> u32 {
        self.devices.iter().map(SwapDevice::pages).sum()
    }

    /// Slots that can hold pages, across every area.
    fn total_room(&self) -> u32 {
        self.devices.iter().map(SwapDevice::room).sum()
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
///    (give it a "second chance") and advance.
///    b. If ACCESSED is clear, this page hasn't been touched recently
///    → select it for eviction.
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

            let virt = VirtAddr::new(entry.vaddr);

            // Locked (`mlock`): never a victim, and passed by rather than
            // taken off the list, so it is a candidate again the moment
            // `munlock` comes, with nothing to put back. (Linux moves such
            // pages to a list the reclaimer does not scan; here a sweep pays
            // a look at each, which with the default 8 MiB `RLIMIT_MEMLOCK`
            // is a few hundred frames a process.)
            if super::mlock::frame_is_locked(entry.pml4_phys, entry.vaddr) {
                continue;
            }

            // The clock test: used since the last sweep (any of its four
            // entries)? Each entry's accessed bit is cleared on its own,
            // nothing else in it touched.
            // SAFETY: the reclaim list names live address spaces: an exiting
            // process's entries go before its tables (`unregister_*`).
            match unsafe { page_table::frame_test_and_clear_accessed(entry.pml4_phys, virt) } {
                Some(true) => {
                    // Second chance. The CPU sets the bit again on the next
                    // use once the stale translation is gone.
                    // SAFETY: invlpg is always safe in ring 0.
                    unsafe {
                        page_table::flush_frame(virt);
                    }
                }
                // Not used since the last sweep: a victim.
                Some(false) => victims.push((idx, entry)),
                None => {
                    // Page is not present (already swapped or unmapped).
                    // Mark inactive.
                    if let Some(e) = state.pages.get_mut(idx) {
                        e.active = false;
                        state.active_count = state.active_count.saturating_sub(1);
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
    let batch_size =
        crate::sysctl::get(crate::sysctl::PARAM_MM_SWAP_BATCH_SIZE).unwrap_or(4) as usize;
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
                    state.active_count = state.active_count.saturating_sub(1);
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
            // In use right now (its address space's page-table lock was held,
            // or its entries changed after they were read): not a failure,
            // just not this time. It stays on the list.
            Err(KernelError::WouldBlock) => {}
            Err(KernelError::NotSupported) => {
                // A frame of a kind that is not swapped (shared memory, or a
                // group split across frames by a partial copy-on-write break:
                // `swap_out_page`). It will not become swappable by being
                // asked again, so it leaves the list rather than being picked,
                // refused and reported on every pass.
                let mut state = RECLAIM.lock();
                if let Some(e) = state.pages.get_mut(idx)
                    && e.active
                {
                    e.active = false;
                    state.active_count = state.active_count.saturating_sub(1);
                }
            }
            Err(e) => {
                serial_println!(
                    "[swap] Reclaim failed for virt={:#x}: {:?}",
                    victim.vaddr,
                    e
                );
                // Skip this page, try the next victim.
            }
        }
    }

    if reclaimed > 0 {
        serial_println!(
            "[swap] Reclaimed {} pages (batch_size={})",
            reclaimed,
            batch_size
        );
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
/// over disk swap when both are available.  Disk areas are added with
/// [`activate`].
///
/// Called during kernel boot, after the heap is available.
pub fn init(num_slots: u32) {
    // Boot: nothing else adds or removes an area yet, which is what
    // `reserve_for_area`'s caller must otherwise guarantee by `SWAP_ADMIN`.
    let Some(base_slot) = SWAP.lock().find_gap(num_slots) else {
        serial_println!("[swap] zram: no room for {} slots", num_slots);
        return;
    };
    if reserve_for_area(base_slot.saturating_add(num_slots)).is_err() {
        serial_println!("[swap] zram: no memory for {} slots' counts", num_slots);
        return;
    }
    let device = SwapDevice {
        priority: 100,
        path: b"zram".to_vec(),
        kind: AreaKind::Memory,
        backend: SwapBackend::Memory(MemBackend::new(num_slots)),
        slots: SwapSlotAllocator::new(num_slots),
        base_slot,
        reserved: 0,
        draining: false,
        default_priority: false,
    };
    let mut state = SWAP.lock();
    if let Err(e) = state
        .grow_to(base_slot.saturating_add(num_slots))
        .and_then(|()| state.insert_area(device))
    {
        serial_println!("[swap] zram: not added: {:?}", e);
        return;
    }
    state.initialized = true;

    serial_println!(
        "[swap] Initialized zram backend: {} slots ({} KiB), priority=100, base_slot={}",
        num_slots,
        (num_slots as u64).saturating_mul(FRAME_SIZE as u64) / 1024,
        base_slot
    );
}

/// `swapon` and `swapoff`, one at a time: Linux's `swapon_mutex`. A sleeping
/// lock, since `swapoff` holds it across the drain ([`deactivate`]).
static SWAP_ADMIN: crate::sched::kmutex::KMutex<()> = crate::sched::kmutex::KMutex::new(());

/// The most areas at once, the compressed tier among them (Linux allows 32,
/// less a few its own types take).
const MAX_AREAS: usize = 32;

/// The most slots one area may have: 2^28, 4 TiB of swap.
const MAX_AREA_SLOTS: u32 = 1 << 28;

/// How many times [`deactivate`] walks every address space for pages still
/// in the area before it gives up with `DeviceBusy`. A page caught being
/// swapped out by another CPU as a walk passes it is found by the next.
const DRAIN_PASSES: usize = 8;

/// The priority the last area added without one was given: Linux's
/// `least_priority`, from -1 down, so the first such area is -2, the next
/// -3, and an area added later is used after one added earlier.
static LEAST_PRIORITY: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(-1);

/// What `swapon` asks for ([`activate`]).
#[derive(Debug, Clone)]
pub struct AreaRequest {
    /// The path it was given: what `/proc/swaps` shows and `swapoff` names.
    pub path: Vec<u8>,
    /// A device or a file ([`AreaKind::Memory`] is refused).
    pub kind: AreaKind,
    /// The block device the area is on, by its name in the registry.
    pub device: String,
    /// Where the area is on it: one extent for a device, a file's for a file.
    pub extents: Vec<AreaExtent>,
    /// `None` for the next default ([`LEAST_PRIORITY`]).
    pub priority: Option<i32>,
}

/// An area [`activate`] added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveArea {
    /// Pages it can hold: its slots, less the header's and the bad ones.
    pub pages: u32,
    /// Its priority.
    pub priority: i32,
}

/// An area [`deactivate`] took away, for its caller to let go of.
#[derive(Debug, Clone)]
pub struct RemovedArea {
    /// What it was: a file's filesystem is told it is no longer swap.
    pub kind: AreaKind,
    /// The block device it was on.
    pub device: String,
    /// Where it was on it.
    pub extents: Vec<AreaExtent>,
}

/// Whether `request`'s path, or any of its sectors, is an area already.
fn area_taken(state: &SwapState, request: &AreaRequest) -> bool {
    state.devices.iter().any(|d| {
        d.path == request.path
            || matches!(&d.backend, SwapBackend::Disk(disk)
                if disk.device_name == request.device
                    && extents_overlap(&disk.extents, &request.extents))
    })
}

/// Whether a whole device's first bytes also hold another format's
/// signature: an ext4 or XFS superblock, a GPT header, or a DOS boot sector
/// (an MBR, a FAT or NTFS volume). A device that answers to two formats is
/// not used for swap -- `mkfs` or partitioning over an old swap area may
/// leave the old signature in place, and taking the device would then
/// destroy what is there now. (util-linux's `swapon` refuses such a device as
/// "ambivalent", through libblkid; the kernel itself does not check.)
fn ambivalent(first: &[u8]) -> bool {
    // ext4's magic, 0xEF53, at byte 56 of the superblock at byte 1024.
    let ext4 = first.get(1080..1082) == Some(&[0x53, 0xEF][..]);
    // A GPT header in sector 1.
    let gpt = first.get(512..520) == Some(&b"EFI PART"[..]);
    // A DOS boot sector's 0x55AA at byte 510: an MBR partition table, or a
    // FAT, exFAT or NTFS volume on the whole device. `mkswap` clears the
    // first 1 KiB unless a partition table is there, which it keeps: either
    // way the device is partitioned, or another volume took it since.
    let boot_sector = first.get(510..512) == Some(&[0x55, 0xAA][..]);
    // XFS's superblock magic at byte 0.
    let xfs = first.get(0..4) == Some(&b"XFSB"[..]);
    ext4 || gpt || boot_sector || xfs
}

/// Make room for one more area ending at global slot `end`: `part_refs`
/// able to reach `end`, and `devices` able to take one more, so that adding
/// it under `SWAP` ([`SwapState::grow_to`], [`SwapState::insert_area`])
/// allocates nothing. Each larger buffer is made outside the lock and swapped
/// in under it, the old one freed after.
///
/// The caller must be the only one adding or removing an area until it has
/// added this one -- by holding `SWAP_ADMIN`, or at boot -- since that is
/// what keeps the lengths read here true when the buffers are swapped in.
///
/// # Errors
///
/// `OutOfMemory`: a buffer could not be allocated.
fn reserve_for_area(end: u32) -> KernelResult<()> {
    let end = end as usize;
    let (refs_short, devices_full) = {
        let state = SWAP.lock();
        (
            state.part_refs.capacity() < end,
            state.devices.len() >= state.devices.capacity(),
        )
    };
    let mut refs: Vec<u8> = Vec::new();
    if refs_short {
        refs.try_reserve_exact(end)
            .map_err(|_| KernelError::OutOfMemory)?;
    }
    let mut devices: Vec<SwapDevice> = Vec::new();
    if devices_full {
        devices
            .try_reserve_exact(MAX_AREAS)
            .map_err(|_| KernelError::OutOfMemory)?;
    }
    let mut state = SWAP.lock();
    // Copies into room already made: neither line allocates.
    if refs_short && refs.capacity() >= state.part_refs.len() {
        refs.extend_from_slice(&state.part_refs);
        core::mem::swap(&mut state.part_refs, &mut refs);
    }
    if devices_full && devices.capacity() >= state.devices.len() {
        devices.append(&mut state.devices);
        core::mem::swap(&mut state.devices, &mut devices);
    }
    drop(state);
    // `refs` and `devices` hold the old buffers now, freed here.
    Ok(())
}

/// Add a swap area: `swapon`. The area's first slot must hold a swap
/// header ([`parse_header`]); its slots are what the header says, as far as
/// its extents reach, less the header's own and any holding a bad page.
///
/// # Errors
///
/// - `InvalidArgument`: no swap header, or one this cannot use; extents
///   that do not describe an area on the device; an area too small to hold
///   a page; a whole device that also holds a filesystem or a partition
///   table ([`ambivalent`]); [`AreaKind::Memory`].
/// - `NoSuchDevice`: the device is not registered.
/// - `ReadOnlyFilesystem`: the device cannot be written.
/// - `DeviceBusy`: the path is already an area, or the extents share
///   sectors with one.
/// - `ResourceExhausted`: [`MAX_AREAS`] areas already, or no room left in
///   the slot numbers.
/// - `OutOfMemory`: the area's bookkeeping cannot be allocated.
/// - `IoError`: the header could not be read.
pub fn activate(request: AreaRequest) -> KernelResult<ActiveArea> {
    let _admin = SWAP_ADMIN.lock();
    if request.kind == AreaKind::Memory {
        return Err(KernelError::InvalidArgument);
    }
    let (sector_count, read_only) = crate::blkdev::with_device(&request.device, |dev| {
        let info = dev.info();
        (info.sector_count, info.read_only)
    })
    .ok_or(KernelError::NoSuchDevice)?;
    if read_only {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    let span = extents_span(&request.extents, sector_count).ok_or(KernelError::InvalidArgument)?;
    let span_bytes = span.saturating_mul(SECTOR_BYTES as u64);
    if span_bytes < HEADER_READ_BYTES as u64 {
        return Err(KernelError::InvalidArgument);
    }
    // Swap already: refused before its sectors -- someone's swapped pages
    // -- are read for a header, as Linux's `claim_swapfile` answers EBUSY
    // first. Areas are added only under SWAP_ADMIN, held here, so the
    // answer holds until this one is added.
    if area_taken(&SWAP.lock(), &request) {
        return Err(KernelError::DeviceBusy);
    }

    // The header, in the area's first slot.
    let mut first = vec![0u8; HEADER_READ_BYTES];
    area_io(&request.device, &request.extents, 0, &mut first, false)?;
    let header = parse_header(&first).map_err(|_| KernelError::InvalidArgument)?;
    // From here a refusal is of something that says it is swap: said on the
    // serial line, since at boot nothing else would say why the area is not
    // there (a device with no header is the ordinary case, and silent).
    if request.kind == AreaKind::Device && ambivalent(&first) {
        serial_println!(
            "[swap] {}: a swap header, beside another format's mark; not used",
            request.device
        );
        return Err(KernelError::InvalidArgument);
    }

    // Its slots: as many whole ones as the header's length and the extents
    // both reach.
    let bytes = header.area_bytes().min(span_bytes);
    let slots = u32::try_from(bytes.checked_div(FRAME_SIZE as u64).unwrap_or(0))
        .unwrap_or(u32::MAX)
        .min(MAX_AREA_SLOTS);
    // Slot 0 holds the header; a slot holding a bad page is never used.
    let mut held: Vec<u32> = vec![0];
    for &page in &header.bad_pages {
        let slot = u64::from(page)
            .saturating_mul(header.page_size as u64)
            .checked_div(FRAME_SIZE as u64)
            .unwrap_or(0);
        if let Ok(slot) = u32::try_from(slot)
            && slot < slots
            && !held.contains(&slot)
        {
            held.push(slot);
        }
    }
    let reserved = u32::try_from(held.len()).unwrap_or(u32::MAX);
    if reserved >= slots {
        serial_println!(
            "[swap] {}: a swap header, but no slot past the header and the bad pages; not used",
            request.device
        );
        return Err(KernelError::InvalidArgument);
    }

    // Every allocation before the lock: under it, a failed first try would
    // reclaim, and reclaim swaps out under `SWAP`, which this task would then
    // hold twice. The area's place cannot change in between: only a
    // `SWAP_ADMIN` holder adds or removes one.
    let backend = DiskBackend::try_new(&request.device, request.extents.clone(), slots)?;
    let mut allocator = SwapSlotAllocator::try_new(slots)?;
    for &slot in &held {
        allocator.reserve(slot);
    }
    let base_slot = {
        let state = SWAP.lock();
        if state.devices.len() >= MAX_AREAS {
            return Err(KernelError::ResourceExhausted);
        }
        state
            .find_gap(slots)
            .ok_or(KernelError::ResourceExhausted)?
    };
    reserve_for_area(base_slot.saturating_add(slots))?;
    let default_priority = request.priority.is_none();
    let priority = request.priority.unwrap_or_else(|| {
        LEAST_PRIORITY
            .fetch_sub(1, core::sync::atomic::Ordering::Relaxed)
            .saturating_sub(1)
    });
    let device = SwapDevice {
        priority,
        path: request.path,
        kind: request.kind,
        backend: SwapBackend::Disk(backend),
        slots: allocator,
        base_slot,
        reserved,
        draining: false,
        default_priority,
    };

    let mut state = SWAP.lock();
    state.grow_to(base_slot.saturating_add(slots))?;
    // Highest priority first, an earlier area ahead of a later one of the
    // same priority.
    state.insert_area(device)?;
    state.initialized = true;
    Ok(ActiveArea {
        pages: slots.saturating_sub(reserved),
        priority,
    })
}

/// Which area [`deactivate`] takes away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AreaTarget {
    /// A block device's area, by the device's name in the registry.
    Device(String),
    /// A file's area, by the file's identity: any name for the file will do,
    /// as Linux's `swapoff` finds a file by its inode rather than its name.
    File {
        /// The filesystem's identity, as the VFS gives it.
        fs_id: u64,
        /// The file's inode.
        ino: u64,
    },
}

impl AreaTarget {
    /// Whether `dev` is the area this names. The compressed tier never is.
    fn names(&self, dev: &SwapDevice) -> bool {
        match self {
            _ if dev.kind == AreaKind::Memory => false,
            Self::Device(name) => {
                dev.kind == AreaKind::Device
                    && matches!(&dev.backend, SwapBackend::Disk(disk) if disk.device_name == *name)
            }
            Self::File { fs_id, ino } => {
                dev.kind
                    == AreaKind::File {
                        fs_id: *fs_id,
                        ino: *ino,
                    }
            }
        }
    }
}

/// Take a swap area away: `swapoff`. It gives no more slots out; every page
/// in it is brought back into memory, from every address space that has
/// one there; and only then is it gone. A failure leaves it as it was, in
/// use, with whatever pages were brought back staying in memory.
///
/// # Errors
///
/// - `InvalidArgument`: no such area (the compressed tier is not taken
///   away).
/// - `DeviceBusy`: `swapoff` is already emptying it, or pages were still
///   left after [`DRAIN_PASSES`] walks.
/// - `OutOfMemory`: memory, with the other areas' room, cannot hold what it
///   holds -- checked first -- or ran out on the way.
/// - `IoError`: a page could not be read back (it stays in the area).
pub fn deactivate(target: &AreaTarget) -> KernelResult<RemovedArea> {
    deactivate_also(target, &[])
}

/// [`deactivate`], walking the address spaces at `extra` too: for a
/// self-test's own, which no process holds.
pub(crate) fn deactivate_also(target: &AreaTarget, extra: &[u64]) -> KernelResult<RemovedArea> {
    let _admin = SWAP_ADMIN.lock();
    // An area is known by its first global slot from here on: no other area
    // can have it while this one does.
    let (base, end) = {
        let mut state = SWAP.lock();
        let others_free: u32 = state
            .devices
            .iter()
            .filter(|d| !target.names(d) && !d.draining)
            .map(|d| d.slots.free_count())
            .sum();
        let dev = state
            .devices
            .iter_mut()
            .find(|d| target.names(d))
            .ok_or(KernelError::InvalidArgument)?;
        if dev.draining {
            return Err(KernelError::DeviceBusy);
        }
        // Linux's `security_vm_enough_memory_mm` check: the pages must have
        // somewhere to go, in memory or in another area.
        let free_frames = frame::stats().map_or(0, |s| s.free_frames);
        let room = u64::try_from(free_frames)
            .unwrap_or(u64::MAX)
            .saturating_add(u64::from(others_free));
        if u64::from(dev.pages()) > room {
            return Err(KernelError::OutOfMemory);
        }
        dev.draining = true;
        (dev.base_slot, dev.end_slot())
    };

    let drained = drain(base, end, extra);

    let mut state = SWAP.lock();
    let index = state
        .devices
        .iter()
        .position(|d| d.base_slot == base && d.draining)
        .ok_or(KernelError::InternalError)?;
    if let Err(e) = drained {
        if let Some(dev) = state.devices.get_mut(index) {
            dev.draining = false;
        }
        return Err(e);
    }
    let dev = state.devices.remove(index);
    state.shrink_to_areas();
    // A default priority given back, as Linux's `swapoff` gives it: each
    // area given one after this one moves up one, and so does the next
    // default -- so on and off again lands an area where it was.
    if dev.default_priority {
        for d in state
            .devices
            .iter_mut()
            .filter(|d| d.default_priority && d.priority < dev.priority)
        {
            d.priority = d.priority.saturating_add(1);
        }
        LEAST_PRIORITY.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    state.initialized = !state.devices.is_empty();
    let SwapBackend::Disk(disk) = dev.backend else {
        return Err(KernelError::InternalError);
    };
    Ok(RemovedArea {
        kind: dev.kind,
        device: disk.device_name,
        extents: disk.extents,
    })
}

/// Read `buf` -- whole sectors -- from byte `offset`, a sector multiple, of
/// the area `target` names, through its extents and past every cache: for a
/// self-test checking that an area is where its filesystem says.
///
/// # Errors
///
/// `InvalidArgument` for no such disk area, or an offset or length that is
/// not whole sectors or runs past it; the device's own.
pub(crate) fn area_read(target: &AreaTarget, offset: u64, buf: &mut [u8]) -> KernelResult<()> {
    let (device, extents) = {
        let state = SWAP.lock();
        match state
            .devices
            .iter()
            .find(|d| target.names(d))
            .map(|d| &d.backend)
        {
            Some(SwapBackend::Disk(disk)) => (disk.device_name.clone(), disk.extents.clone()),
            _ => return Err(KernelError::InvalidArgument),
        }
    };
    let sector = SECTOR_BYTES as u64;
    if !offset.is_multiple_of(sector) || !buf.len().is_multiple_of(SECTOR_BYTES) {
        return Err(KernelError::InvalidArgument);
    }
    let first = offset.checked_div(sector).unwrap_or(0);
    area_io(&device, &extents, first, buf, false)
}

/// Pages the area at global slot `base` still holds; `None` when it is gone.
fn area_pages(base: u32) -> Option<u32> {
    SWAP.lock()
        .devices
        .iter()
        .find(|d| d.base_slot == base)
        .map(SwapDevice::pages)
}

/// Bring every page in global slots `[base, end)` back into memory: walk
/// every process's address space, pinned so it cannot go mid-walk (and the
/// address spaces at `extra`), until the area holds none.
fn drain(base: u32, end: u32, extra: &[u64]) -> KernelResult<()> {
    let in_area =
        |raw: u64| SwapEntry::from_pte_raw(raw).is_some_and(|e| (base..end).contains(&e.slot()));
    for pass in 0..DRAIN_PASSES {
        if area_pages(base).is_none_or(|n| n == 0) {
            return Ok(());
        }
        if pass > 0 {
            crate::sched::yield_now();
        }
        let empty = || area_pages(base).is_none_or(|n| n == 0);
        let mut walked: Vec<u64> = Vec::new();
        for &pml4 in extra {
            // SAFETY: the caller's own address spaces, alive for the call.
            unsafe { bring_back(pml4, &in_area)? };
            walked.push(pml4);
            if empty() {
                return Ok(());
            }
        }
        for pid in crate::proc::pcb::pids() {
            let Some(pin) = crate::proc::pcb::pin_address_space(pid) else {
                continue;
            };
            // Processes that share one address space are walked once.
            if walked.contains(&pin.pml4()) {
                continue;
            }
            walked.push(pin.pml4());
            // SAFETY: pinned: its tables stay until the pin goes.
            unsafe { bring_back(pin.pml4(), &in_area)? };
            // The last page back: the rest need not be walked.
            if empty() {
                return Ok(());
            }
        }
    }
    if area_pages(base).is_none_or(|n| n == 0) {
        Ok(())
    } else {
        Err(KernelError::DeviceBusy)
    }
}

/// Bring back every frame of the address space at `pml4` whose entries name
/// a slot `in_area` accepts, as a fault would ([`swap_in_page`]), and put it
/// on the reclaim list again.
///
/// # Safety
///
/// `pml4` must be a valid PML4 whose tables stay allocated for the call
/// ([`page_table::for_each_user_frame`]).
unsafe fn bring_back(pml4: u64, in_area: &dyn Fn(u64) -> bool) -> KernelResult<()> {
    // Frames found per walk: a list of fixed size, made once, since memory
    // is short while an area empties. The walk resumes where it stopped.
    const BATCH: usize = 256;
    let mut frames: Vec<VirtAddr> = Vec::new();
    frames
        .try_reserve_exact(BATCH)
        .map_err(|_| KernelError::OutOfMemory)?;
    let mut from = VirtAddr::new(0);
    loop {
        let mut resume = None;
        // SAFETY: the caller's contract.
        unsafe {
            page_table::for_each_user_frame(pml4, from, |va, parts| {
                if !parts.iter().any(|p| in_area(p.raw())) {
                    return true;
                }
                if frames.len() >= BATCH {
                    resume = Some(va);
                    return false;
                }
                frames.push(va); // within the room made above
                true
            });
        }
        for va in frames.drain(..) {
            // SAFETY: the caller's contract; `va` is a frame of this space.
            match unsafe { swap_in_page(pml4, va) } {
                Ok(Some(kept)) => register_reclaimable(pml4, va.as_u64(), kept),
                // Brought back by its own fault first, or unmapped, since the
                // walk read it.
                Ok(None) | Err(KernelError::InvalidArgument | KernelError::InvalidAddress) => {}
                Err(e) => return Err(e),
            }
        }
        match resume {
            Some(va) => from = va,
            None => return Ok(()),
        }
    }
}

/// Turn on, at boot, every whole block device whose first slot is a swap
/// header -- and nothing else ([`activate`] refuses the rest). Partitions
/// and files are userspace's to turn on, as Linux's `swapon -a` does from
/// its table. Returns how many were added.
pub fn activate_signed_devices() -> usize {
    let mut added = 0usize;
    for info in crate::blkdev::list_devices() {
        let mut path = b"/dev/".to_vec();
        path.extend_from_slice(info.name.as_bytes());
        let request = AreaRequest {
            path,
            kind: AreaKind::Device,
            device: info.name.clone(),
            extents: vec![AreaExtent {
                area_sector: 0,
                dev_sector: 0,
                sectors: info.sector_count,
            }],
            priority: None,
        };
        match activate(request) {
            Ok(area) => {
                serial_println!(
                    "[swap] /dev/{}: a swap area of {} pages ({} KiB), priority {}",
                    info.name,
                    area.pages,
                    u64::from(area.pages).saturating_mul(FRAME_SIZE as u64) / 1024,
                    area.priority
                );
                added = added.saturating_add(1);
            }
            // Not swap: a filesystem, a partition table, or nothing.
            Err(KernelError::InvalidArgument | KernelError::ReadOnlyFilesystem) => {}
            Err(e) => serial_println!("[swap] /dev/{}: not used for swap: {:?}", info.name, e),
        }
    }
    added
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
    summarise(&state)
}

/// Non-blocking variant of [`summary()`] for interrupt/softirq context.
///
/// Returns `None` if the SWAP lock is currently held rather than waiting for
/// it.  This exists because [`summary()`] is reachable from the periodic
/// metrics sampler, which runs in the timer softirq: if the timer fires on a
/// CPU whose interrupted code already holds SWAP, a blocking acquire can never
/// succeed — the only thing that could release the lock is the code the
/// interrupt just suspended.  `sync.rs` detects the recursive acquire and
/// panics, which is how this was found (lane B,
/// `requests/b-a-kstat-sample-calls-memory-info-from-the-timer-softirq-*`).
///
/// A dropped metrics sample costs nothing; the next tick gets it.
#[must_use]
pub fn try_summary() -> Option<(usize, usize, usize)> {
    let state = SWAP.try_lock()?;
    Some(summarise(&state))
}

/// Shared body of [`summary()`] and [`try_summary()`], so the two can never
/// drift into reporting different numbers from the same state.
fn summarise(state: &SwapState) -> (usize, usize, usize) {
    let total_bytes = (state.total_room() as usize).saturating_mul(FRAME_SIZE);
    let used_bytes = (state.total_used() as usize).saturating_mul(FRAME_SIZE);
    let devices = state.devices.len();
    (total_bytes, used_bytes, devices)
}

/// Information about a single swap device (for /proc/swaps).
#[derive(Debug, Clone)]
pub struct SwapDeviceInfo {
    /// The area's path for display: [`Self::path`] with every byte that is
    /// not printable ASCII, and space, tab, newline and backslash, written
    /// as a backslash and three octal digits -- `/proc/swaps`' escaping,
    /// applied beyond ASCII too so the text is valid UTF-8 and loses
    /// nothing.
    pub name: alloc::string::String,
    /// The area's path as `swapon` was given it (`zram` for the compressed
    /// tier).
    pub path: Vec<u8>,
    /// Device type: "memory" for zram, "disk" for a block device or
    /// partition, "file" for a swap file.
    pub device_type: &'static str,
    /// Slots that can hold pages (1 slot = 1 page = 16 KiB): the area's,
    /// less its header's and any bad ones.
    pub total_slots: u32,
    /// Slots holding pages.
    pub used_slots: u32,
    /// Priority (higher = preferred).
    pub priority: i32,
}

/// `path` as [`SwapDeviceInfo::name`] shows it.
fn escaped_path(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path {
        if b.is_ascii_graphic() && b != b'\\' {
            out.push(char::from(b));
        } else {
            out.push_str(&alloc::format!("\\{b:03o}"));
        }
    }
    out
}

/// List all active swap devices with their usage and type.
///
/// Used by `/proc/swaps` to show swap configuration.
pub fn list_devices() -> alloc::vec::Vec<SwapDeviceInfo> {
    let state = SWAP.lock();
    let mut result = alloc::vec::Vec::with_capacity(state.devices.len());
    for dev in &state.devices {
        let device_type = match dev.kind {
            AreaKind::Memory => "memory",
            AreaKind::Device => "disk",
            AreaKind::File { .. } => "file",
        };
        result.push(SwapDeviceInfo {
            name: escaped_path(&dev.path),
            path: dev.path.clone(),
            device_type,
            total_slots: dev.room(),
            used_slots: dev.pages(),
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

/// Pages read back from swap, whatever the backend: Linux's `pswpin`.
static PAGES_SWAPPED_IN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Pages written to swap, whatever the backend: Linux's `pswpout`.
static PAGES_SWAPPED_OUT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// `(pages read back from swap, pages written to it)` since boot -- each a
/// 16 KiB frame, the page size the system reports: what `/proc/vmstat`
/// gives as `pswpin` and `pswpout`, and `vmstat` converts with
/// `sysconf(_SC_PAGESIZE)`. The zram backend counts too, as Linux's swap
/// to a zram device does.
#[must_use]
pub fn swap_traffic() -> (u64, u64) {
    (
        PAGES_SWAPPED_IN.load(core::sync::atomic::Ordering::Relaxed),
        PAGES_SWAPPED_OUT.load(core::sync::atomic::Ordering::Relaxed),
    )
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
            stats.uncompressed_bytes = stats
                .uncompressed_bytes
                .saturating_add(m.uncompressed_bytes);
            stats.compressed_bytes = stats.compressed_bytes.saturating_add(m.compressed_bytes);
            stats.compressed_count = stats.compressed_count.saturating_add(m.compressed_count);
            stats.uncompressed_count = stats
                .uncompressed_count
                .saturating_add(m.uncompressed_count);
        }
    }
    stats
}

/// Swap out a page: evict a physical frame's contents to swap storage
/// and replace its page table entries with swap entries.
///
/// 1. Reads the frame's four 4 KiB entries. Every present one must map into
///    the same frame -- a group a partial copy-on-write break has split
///    across frames is not one frame to swap -- and none may be shared
///    memory, which other mappers hold too (both `NotSupported`).
/// 2. Reads the frame's 16 KiB of data via HHDM.
/// 3. Allocates a swap slot, writes the data to the backend, and counts the
///    four entries that will name it.
/// 4. Replaces each entry with a swap entry for the slot, keeping that part's
///    flags ([`SwapEntry::to_pte_raw_keeping`]); a part that was not mapped
///    keeps none.
/// 5. Flushes the TLB.
/// 6. Frees the physical frame.
///
/// The frame at `virt` must be a user-space frame with at least one part
/// mapped and present in the page table at `pml4_phys`.
///
/// Returns the `SwapEntry` that was written to the page table.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The page must not be actively accessed by another CPU/context during
///   the swap-out operation: the data is read before the entries change, so
///   a write in between is lost (known-issues
///   `TD-A-PAGE-MOVES-COPY-BEFORE-THEY-UNMAP`).
pub unsafe fn swap_out_page(pml4_phys: u64, virt: VirtAddr) -> KernelResult<SwapEntry> {
    use super::page_table::HW_PAGES_PER_FRAME;

    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    if !virt.is_user() {
        return Err(KernelError::InvalidArgument);
    }

    // Step 1: the frame's parts, and the one frame they map.
    let parts = page_table::read_frame_ptes(pml4_phys, virt).ok_or(KernelError::InvalidAddress)?;
    let frame_mask = (FRAME_SIZE as u64).wrapping_sub(1);
    let mut frame_base: Option<u64> = None;
    for part in &parts {
        if !part.is_present() {
            continue;
        }
        // Shared by design: not this path's to swap, ever.
        if part.flags().contains(PageFlags::SHARED) {
            return Err(KernelError::NotSupported);
        }
        // Locked (`mlock`): not while it is. `WouldBlock`, which leaves the
        // frame on the reclaim list for when `munlock` comes, not
        // `NotSupported`, which takes it off for good. A lock set after this
        // read changes the entry, which step 4 notices and gives up on.
        if part.flags().contains(PageFlags::MLOCKED) {
            return Err(KernelError::WouldBlock);
        }
        let base = part.phys_addr() & !frame_mask;
        match frame_base {
            None => frame_base = Some(base),
            Some(b) if b == base => {}
            Some(_) => return Err(KernelError::NotSupported),
        }
    }
    let frame_base = frame_base.ok_or(KernelError::InvalidAddress)?;
    let phys_frame = frame::PhysFrame::from_addr(frame_base).ok_or(KernelError::InternalError)?;

    // Step 2: read the frame's data via HHDM.
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let frame_virt = frame_base
        .checked_add(hhdm)
        .ok_or(KernelError::InternalError)?;
    let mut page_data = vec![0u8; FRAME_SIZE];
    // SAFETY: frame_virt is the HHDM address of a frame the page table maps;
    // FRAME_SIZE bytes from it are that one frame.
    unsafe {
        core::ptr::copy_nonoverlapping(frame_virt as *const u8, page_data.as_mut_ptr(), FRAME_SIZE);
    }

    // Step 3: allocate a swap slot, write the data, count the entries.
    // Multi-device: alloc_slot() tries the highest-priority device first,
    // overflowing to lower-priority devices when the preferred one is full.
    let swap_entry = {
        let mut state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }
        let global_slot = state.alloc_slot().ok_or(KernelError::OutOfMemory)?;
        let Some(entry) = SwapEntry::new(global_slot) else {
            state.free_slot(global_slot);
            return Err(KernelError::InternalError);
        };
        if let Err(e) = state.write_slot(global_slot, &page_data) {
            state.free_slot(global_slot);
            return Err(e);
        }
        PAGES_SWAPPED_OUT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let Some(refs) = state.part_refs.get_mut(global_slot as usize) else {
            state.free_slot(global_slot);
            return Err(KernelError::InternalError);
        };
        // One for each of the frame's parts: every one gets an entry.
        *refs = HW_PAGES_PER_FRAME as u8;
        entry
    };
    // Drop SWAP lock before page table manipulation (lock ordering).

    // Step 4: each part becomes a swap entry for the slot, keeping its own
    // flags -- under the address space's page-table lock, without waiting
    // for it (reclaim can run on an allocation failure, with interrupts
    // off), and only if the parts are still what step 1 read. Otherwise
    // the page is in use: the slot goes back, and reclaim picks another.
    let Some(held) = super::as_lock::try_lock(pml4_phys) else {
        SWAP.lock()
            .release_parts(swap_entry.slot(), HW_PAGES_PER_FRAME as u8);
        return Err(KernelError::WouldBlock);
    };
    if page_table::read_frame_ptes(pml4_phys, virt).is_none_or(|now| {
        now.iter()
            .zip(parts.iter())
            .any(|(a, b)| a.raw() != b.raw())
    }) {
        drop(held);
        SWAP.lock()
            .release_parts(swap_entry.slot(), HW_PAGES_PER_FRAME as u8);
        return Err(KernelError::WouldBlock);
    }
    for (i, part) in parts.iter().enumerate() {
        let kept = if part.is_present() {
            part.flags()
        } else {
            PageFlags::empty()
        };
        let entry = PageTableEntry::from_raw(swap_entry.to_pte_raw_keeping(kept));
        // SAFETY: pml4_phys is valid and virt is a user frame; the frame is
        // freed below only after the TLB flush.
        unsafe {
            page_table::exchange_frame_pte(pml4_phys, virt, i, entry)?;
        }
    }
    super::accounting::uncharge(pml4_phys, 1);
    drop(held);

    // Remove reverse mapping -- this frame is no longer mapped at this virt.
    super::rmap::remove(frame_base, pml4_phys, virt.as_u64());

    // Step 5: Flush the TLB.
    // SAFETY: invlpg is always safe in ring 0.
    unsafe {
        page_table::flush_frame(virt);
    }

    // Step 6: Free the physical frame.
    // SAFETY: The frame was just unmapped and the TLB flushed, so no
    // CPU holds a mapping to it.
    unsafe {
        frame::free_frame(phys_frame)?;
    }

    serial_println!(
        "[swap] Swapped out: virt={:#x} → slot={}",
        virt.as_u64(),
        swap_entry.slot()
    );

    crate::ktrace::record(
        crate::ktrace::Category::Mm,
        crate::ktrace::event::SWAP_OUT,
        virt.as_u64(),
        swap_entry.slot() as u64,
    );

    Ok(swap_entry)
}

/// Swap in a page: restore a previously swapped-out frame to physical
/// memory, each 4 KiB part as it was.
///
/// In two halves, like a copy-on-write break (`mm::cow`): [`prepare_swap_in`]
/// reads the frame's entries and the slot's data into a new frame, unlocked,
/// since the read can be a disk's; [`install_swap_in`] maps it under the
/// address space's page-table lock ([`super::as_lock`]), only over entries
/// that still name the slot. Two CPUs swapping the same frame in -- two
/// threads touching it at once -- install once: the second finds the frame
/// back, frees its copy, and the access runs again. Until 2026-10-03 the
/// second cleared the first's mapping and mapped its own.
///
/// 1. Reads the frame's four entries, finds the swap entry they name, and
///    for each part still naming it the flags it kept
///    ([`SwapEntry::kept_flags`]).
/// 2. Reads the data from the swap backend into a new frame (via HHDM).
/// 3. Under the lock: re-reads the entries; clears the parts naming the slot
///    and maps the frame part by part with their kept flags
///    (`page_table::map_frame_subpages`) -- a part with none, one that was
///    not mapped, stays unmapped. If the mapping fails the entries are put
///    back and the data stays in its slot.
/// 4. Gives the slot back for the entries that went (it is freed with the
///    last), and flushes the TLB.
///
/// Returns the flags of the first part mapped, for the reclaim list
/// ([`register_reclaimable`]); `None` when another CPU brought the frame
/// back first, which has registered it.
///
/// Until 2026-10-03 the entry was looked up with `read_leaf_pte`, which
/// answers present entries only, so swap-in never found it and failed every
/// time: a swapped-out page came back as a demand-paged page of zeros.
///
/// Waits for the address-space lock: for thread context, or the #PF handler
/// with interrupts on.
///
/// # Errors
///
/// `InvalidArgument` when no part of the frame holds a swap entry,
/// `InvalidAddress` when none that does was mapped; the backend's and the
/// allocator's own errors.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table and `virt` frame-aligned.
pub unsafe fn swap_in_page(pml4_phys: u64, virt: VirtAddr) -> KernelResult<Option<PageFlags>> {
    // SAFETY: the caller's contract.
    let prepared = unsafe { prepare_swap_in(pml4_phys, virt)? };
    let _held = super::as_lock::lock(pml4_phys);
    // SAFETY: the caller's contract; the lock is held.
    unsafe { install_swap_in(pml4_phys, virt, prepared) }
}

/// What the slow half of a swap-in found and made ([`prepare_swap_in`]).
pub(crate) struct SwapInPrep {
    /// The entry the frame's parts name.
    entry: SwapEntry,
    /// The data, in a new frame not yet mapped.
    frame: frame::PhysFrame,
}

/// The slow half of [`swap_in_page`], with no lock: find the swap entry the
/// frame's parts name, and read its slot into a new frame.
///
/// # Errors
///
/// As [`swap_in_page`].
///
/// # Safety
///
/// As [`swap_in_page`].
pub(crate) unsafe fn prepare_swap_in(pml4_phys: u64, virt: VirtAddr) -> KernelResult<SwapInPrep> {
    if !virt.is_frame_aligned() {
        return Err(KernelError::BadAlignment);
    }
    let parts = page_table::read_frame_ptes(pml4_phys, virt).ok_or(KernelError::InvalidAddress)?;
    let entry = parts
        .iter()
        .find_map(|part| SwapEntry::from_pte_raw(part.raw()))
        .ok_or(KernelError::InvalidArgument)?;

    // Read the data from the swap backend.
    // Multi-device: find_device() locates which backend owns this slot.
    let mut page_data = vec![0u8; FRAME_SIZE];
    {
        let state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }
        state.read_slot(entry.slot(), &mut page_data)?;
    }
    PAGES_SWAPPED_IN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // Drop SWAP lock before frame allocation (lock ordering).

    // A new frame, holding the data. (The HHDM first: a frame allocated
    // before an early return would leak.)
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let new_frame = frame::alloc_frame()?;
    let frame_virt = new_frame.addr().wrapping_add(hhdm);
    // SAFETY: new_frame is freshly allocated and mapped via HHDM.
    unsafe {
        core::ptr::copy_nonoverlapping(page_data.as_ptr(), frame_virt as *mut u8, FRAME_SIZE);
    }
    Ok(SwapInPrep {
        entry,
        frame: new_frame,
    })
}

/// The install half of [`swap_in_page`]: map the prepared frame over the
/// parts that still name its slot, each with the flags its entry kept.
/// `Ok(None)`, freeing the prepared frame, when no part names the slot any
/// more: another CPU brought the frame back first (or it was unmapped).
///
/// # Errors
///
/// `InvalidAddress` when only parts that were never mapped still name the
/// slot (their entries are given up with it); `map_frame_subpages`' own.
///
/// # Safety
///
/// As [`swap_in_page`], and the caller holds the address space's
/// page-table lock ([`super::as_lock`]).
pub(crate) unsafe fn install_swap_in(
    pml4_phys: u64,
    virt: VirtAddr,
    prep: SwapInPrep,
) -> KernelResult<Option<PageFlags>> {
    use super::page_table::HW_PAGES_PER_FRAME;

    let SwapInPrep {
        entry,
        frame: new_frame,
    } = prep;
    let discard = |result| {
        // SAFETY: prepared by prepare_swap_in and never mapped.
        let _ = unsafe { frame::free_frame(new_frame) };
        result
    };
    let Some(parts) = page_table::read_frame_ptes(pml4_phys, virt) else {
        return discard(Err(KernelError::InvalidAddress));
    };
    // The parts naming the slot now, and what each kept.
    let mut restore = [PageFlags::empty(); HW_PAGES_PER_FRAME];
    let mut held = [false; HW_PAGES_PER_FRAME];
    for (i, part) in parts.iter().enumerate() {
        if SwapEntry::from_pte_raw(part.raw()) == Some(entry) {
            if let (Some(h), Some(r)) = (held.get_mut(i), restore.get_mut(i)) {
                *h = true;
                *r = SwapEntry::kept_flags(part.raw());
            }
        }
    }
    let held_count = held.iter().filter(|&&h| h).count() as u8;
    if held_count == 0 {
        // Another CPU brought the frame back first, or it was unmapped.
        return discard(Ok(None));
    }
    let Some(representative) = restore
        .iter()
        .copied()
        .find(|flags| flags.contains(PageFlags::PRESENT))
    else {
        // Only parts that were never mapped still name the slot: nothing to
        // bring back. Give their entries up, and the slot with them.
        for (i, &h) in held.iter().enumerate() {
            if h {
                // SAFETY: pml4_phys is valid; the table exists (read above).
                let _ = unsafe {
                    page_table::exchange_frame_pte(pml4_phys, virt, i, PageTableEntry::EMPTY)
                };
            }
        }
        SWAP.lock().release_parts(entry.slot(), held_count);
        return discard(Err(KernelError::InvalidAddress));
    };

    // The parts naming the slot give it up, and the frame is mapped part by
    // part with what each kept. The table exists (read above), so no
    // exchange fails.
    for (i, &h) in held.iter().enumerate() {
        if h {
            // SAFETY: pml4_phys is valid; the table exists.
            let _ = unsafe {
                page_table::exchange_frame_pte(pml4_phys, virt, i, PageTableEntry::EMPTY)
            };
        }
    }
    // SAFETY: pml4_phys is valid, virt frame-aligned, new_frame valid and
    // the parts to be mapped were just cleared.
    if let Err(e) = unsafe { page_table::map_frame_subpages(pml4_phys, virt, new_frame, restore) } {
        // Put the entries back: the data stays in its slot.
        for (i, (&h, part)) in held.iter().zip(parts.iter()).enumerate() {
            if h {
                // SAFETY: as above.
                let _ = unsafe { page_table::exchange_frame_pte(pml4_phys, virt, i, *part) };
            }
        }
        return discard(Err(e));
    }

    // Register reverse mapping -- this frame is now mapped at this virt.
    super::rmap::add(new_frame.addr(), pml4_phys, virt.as_u64());

    // The entries that went give the slot back; flush the TLB.
    SWAP.lock().release_parts(entry.slot(), held_count);
    // SAFETY: invlpg is always safe.
    unsafe {
        page_table::flush_frame(virt);
    }

    serial_println!(
        "[swap] Swapped in: virt={:#x} \u{2190} slot={}",
        virt.as_u64(),
        entry.slot()
    );

    crate::ktrace::record(
        crate::ktrace::Category::Mm,
        crate::ktrace::event::SWAP_IN,
        virt.as_u64(),
        entry.slot() as u64,
    );

    Ok(Some(representative))
}

/// Whether any 4 KiB part of the 16 KiB frame holding `virt` is a swap
/// entry: the frame was swapped out, and swap-in is what brings it back,
/// never a fresh page over its entries.
///
/// Until 2026-10-03 this read the PTE with `read_leaf_pte`, which answers
/// present entries only, so it was false for every swapped page; and it
/// looked at one part, where a part unmapped since swap-out leaves the
/// others still naming the slot.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
#[must_use]
pub unsafe fn is_swapped(pml4_phys: u64, virt: VirtAddr) -> bool {
    let frame = VirtAddr::new(virt.as_u64() & !(FRAME_SIZE as u64).wrapping_sub(1));
    page_table::read_frame_ptes(pml4_phys, frame)
        .is_some_and(|parts| parts.iter().any(|part| part.is_swap()))
}

/// Give back what the swap entry of the 4 KiB page at `va` holds, as the
/// page is unmapped (`mm::user::unmap_user_range`): the entry is cleared,
/// and the frame's slot is freed with the last entry that named it.
/// `false` when the page's PTE is not a swap entry.
///
/// Until 2026-10-03 an unmap skipped a swapped page (its PTE is not present),
/// and the slot was never freed.
///
/// # Safety
///
/// `pml4_phys` must be a valid PML4 the caller may change, and `va` a user
/// address in it.
pub unsafe fn release_swap_pte(pml4_phys: u64, va: u64) -> bool {
    let hw = page_table::HW_PAGE_SIZE as u64;
    let frame_va = va & !(FRAME_SIZE as u64).wrapping_sub(1);
    let part = va.wrapping_sub(frame_va).checked_div(hw).unwrap_or(0) as usize;
    let virt = VirtAddr::new(frame_va);
    let is_swap = page_table::read_frame_ptes(pml4_phys, virt)
        .and_then(|parts| parts.get(part).copied())
        .is_some_and(|pte| pte.is_swap());
    if !is_swap {
        return false;
    }
    // The exchange decides: whatever it took out is what is given back, so
    // two unmaps of the same page cannot both release its entry.
    // SAFETY: the caller's contract; the table exists (read above).
    let Ok(was) =
        (unsafe { page_table::exchange_frame_pte(pml4_phys, virt, part, PageTableEntry::EMPTY) })
    else {
        return false;
    };
    match SwapEntry::from_pte_raw(was.raw()) {
        Some(entry) => {
            SWAP.lock().release_parts(entry.slot(), 1);
            true
        }
        None => {
            // Not a swap entry any more by the time it was taken: put back
            // what was there, which is not this caller's to clear.
            // SAFETY: as above.
            let _ = unsafe { page_table::exchange_frame_pte(pml4_phys, virt, part, was) };
            false
        }
    }
}

/// `count` swap entries naming `slot` are gone with the page table that held
/// them (`page_table::clear_user_address_space`); the slot is freed with the
/// last.
pub fn release_swap_parts(slot: u32, count: u8) {
    SWAP.lock().release_parts(slot, count);
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
pub fn self_test() -> crate::error::KernelResult<()> {
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
        assert_eq!(
            backend.compressed_count, 1,
            "should compress repeating data"
        );
        assert!(
            backend.compressed_bytes < FRAME_SIZE as u64,
            "compressed should be smaller than uncompressed"
        );

        // Read back and verify integrity.
        let mut buf = vec![0u8; FRAME_SIZE];
        backend.read(0, &mut buf).expect("read should succeed");
        assert_eq!(
            buf, data,
            "data integrity check failed after compress/decompress"
        );

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
            backend
                .uncompressed_bytes
                .saturating_sub(backend.compressed_bytes)
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
                    path: b"test-fast".to_vec(),
                    kind: AreaKind::Memory,
                    backend: SwapBackend::Memory(MemBackend::new(4)),
                    slots: SwapSlotAllocator::new(4),
                    base_slot: 0,
                    reserved: 0,
                    draining: false,
                    default_priority: false,
                },
                SwapDevice {
                    priority: 0,
                    path: b"test-slow".to_vec(),
                    kind: AreaKind::Memory,
                    backend: SwapBackend::Memory(MemBackend::new(4)),
                    slots: SwapSlotAllocator::new(4),
                    base_slot: 4,
                    reserved: 0,
                    draining: false,
                    default_priority: false,
                },
            ],
            slot_end: 8,
            initialized: true,
            part_refs: vec![0; 8],
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
            "overflow slot {} should be from device B (4..8)",
            overflow
        );

        // Free a slot from device A, next alloc should go back to A.
        test_state.free_slot(1);
        let refilled = test_state.alloc_slot().expect("refill from A");
        assert!(
            refilled < 4,
            "refilled slot {} should be from device A",
            refilled
        );

        // Write and read through the multi-device API.
        let test_data = vec![0x42u8; FRAME_SIZE];
        test_state
            .write_slot(overflow, &test_data)
            .expect("write to B");
        let mut read_buf = vec![0u8; FRAME_SIZE];
        test_state
            .read_slot(overflow, &mut read_buf)
            .expect("read from B");
        assert_eq!(read_buf, test_data, "multi-device read/write integrity");

        // Verify find_device routing.
        assert_eq!(
            test_state.find_device(0).map(|(d, _)| d),
            Some(0),
            "slot 0 → device 0"
        );
        assert_eq!(
            test_state.find_device(3).map(|(d, _)| d),
            Some(0),
            "slot 3 → device 0"
        );
        assert_eq!(
            test_state.find_device(4).map(|(d, _)| d),
            Some(1),
            "slot 4 → device 1"
        );
        assert_eq!(
            test_state.find_device(7).map(|(d, _)| d),
            Some(1),
            "slot 7 → device 1"
        );
        assert!(
            test_state.find_device(8).is_none(),
            "slot 8 should not belong to any device"
        );

        serial_println!(
            "[swap]   Multi-device priority: OK (alloc prefers priority=100, overflows to priority=0)"
        );
    }

    // --- Softirq-safety: the try_* chain refuses rather than blocks ---
    //
    // This is the regression test for the boot panic lane B reported in
    // `requests/b-a-kstat-sample-calls-memory-info-from-the-timer-softirq-*`:
    // the timer softirq called `mm::memory_pressure()` → `memory_info()` →
    // `swap::summary()` → `SWAP.lock()` while the interrupted code on the same
    // CPU held SWAP, and blocked for a release only the suspended code could
    // perform.
    //
    // Holding SWAP here and asserting the whole chain reports busy is the
    // property that makes that impossible.  Note it asserts about
    // `mm::try_memory_info()`, not just `try_summary()` — a fix that made only
    // the leaf non-blocking while some caller in the middle still took a
    // blocking lock would pass a leaf-only test and still panic on boot.
    {
        let held = SWAP.lock();

        assert!(
            try_summary().is_none(),
            "try_summary() must report busy while SWAP is held, not block"
        );
        assert!(
            crate::mm::try_memory_info().is_none(),
            "mm::try_memory_info() must report busy while SWAP is held — this is \
             the exact path the timer softirq takes"
        );
        assert!(
            crate::mm::try_memory_pressure().is_none(),
            "mm::try_memory_pressure() must report busy while SWAP is held"
        );

        drop(held);

        // And once released, the same calls must actually succeed — otherwise
        // the assertions above would be satisfied by a function that always
        // returns None, and the sampler would never record anything.
        //
        // Retried rather than asserted once: the negative direction above is
        // deterministic (we hold the lock ourselves), but the positive one
        // races against every other CPU, and ALLOCATOR in particular is hot.
        // A single-shot assert here would be a flaky boot failure. A bounded
        // retry still fails a chain that is *permanently* None, which is the
        // regression worth catching.
        let mut info = None;
        for _ in 0..1000 {
            if let Some(i) = crate::mm::try_memory_info() {
                info = Some(i);
                break;
            }
            core::hint::spin_loop();
        }
        // Succeeding proves every leaf in the chain — try_summary included —
        // can return Some, since try_memory_info requires all of them.
        let info =
            info.expect("try_memory_info() must succeed within 1000 tries with no lock held");
        assert!(
            info.total_bytes > 0,
            "a successful snapshot must carry real numbers, not zeros"
        );
        assert!(
            crate::mm::pressure_from_info(&info).score <= 100,
            "pressure score must be a percentage"
        );

        serial_println!("[swap]   Softirq-safe try_* chain: OK (refuses while SWAP held)");
    }

    // Note: disk backend test is in self_test_disk(), called separately
    // after the disk device is registered.

    test_round_trip_keeps_each_part()?;
    test_swap_in_twice()?;

    serial_println!("[swap] Self-test PASSED");
    Ok(())
}

/// Two CPUs swapping the same frame in -- two threads touching it at once:
/// both read the slot into a frame of their own, the first installs, the
/// second finds the frame back (`None`) and frees its copy. The slot is
/// given back once, and the page holds its data. Driven as the halves
/// interleave on two CPUs: prepare, prepare, install, install. Until
/// 2026-10-03 the second cleared the first's mapping and mapped its own.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn test_swap_in_twice() -> KernelResult<()> {
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[swap]   FAIL: swap-in twice: {}", what);
        Err(KernelError::InternalError)
    }

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let pml4 = page_table::alloc_pml4()?;
    let virt = VirtAddr::new(0x0000_0042_0000_0000);
    let pattern = |i: usize| (i % 239) as u8;
    let frame = frame::alloc_frame()?;
    for i in 0..FRAME_SIZE {
        // SAFETY: a freshly allocated frame, through the HHDM, in bounds.
        unsafe { ((frame.addr() + hhdm) as *mut u8).add(i).write(pattern(i)) };
    }
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    // SAFETY: this test's address space and a fresh frame.
    unsafe { page_table::map_frame(pml4, virt, frame, flags)? };
    let used_before = used_slots();
    // SAFETY: as above; the frame is mapped.
    unsafe { swap_out_page(pml4, virt)? };

    // SAFETY: as above; the parts hold swap entries.
    let first = unsafe { prepare_swap_in(pml4, virt)? };
    // SAFETY: as above.
    let second = unsafe { prepare_swap_in(pml4, virt)? };
    let a = {
        let _held = super::as_lock::lock(pml4);
        // SAFETY: as above, with the lock held.
        unsafe { install_swap_in(pml4, virt, first)? }
    };
    let b = {
        let _held = super::as_lock::lock(pml4);
        // SAFETY: as above, with the lock held.
        unsafe { install_swap_in(pml4, virt, second)? }
    };
    let slot_back_once = used_slots() == used_before;
    let data_ok = page_table::translate(pml4, virt).is_some_and(|phys| {
        (0..FRAME_SIZE).all(|i| {
            // SAFETY: the mapped frame, through the HHDM, in bounds.
            unsafe { ((phys + hhdm) as *const u8).add(i).read() == pattern(i) }
        })
    });
    // SAFETY: never loaded in any CR3; nothing else uses it.
    unsafe { page_table::destroy_user_address_space(pml4) };

    let checks = [
        ("the first install mapped the frame", a.is_some()),
        ("the second install found it back", b.is_none()),
        ("the slot was given back once", slot_back_once),
        ("the page holds its data", data_ok),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        return fail(what);
    }
    serial_println!(
        "[swap]   swap-in twice: the first install maps the frame, the second finds it back \
         and frees its copy, and the slot goes back once: OK"
    );
    Ok(())
}

/// A frame swapped out and back in comes back as it was, and its slot is
/// given back however its entries go. Until 2026-10-03 nothing tested this,
/// and nothing about it worked: swap-in never found its entry, and the page
/// came back as zeros.
///
/// The frame's four parts are mapped differently -- read-write, read-only, a
/// gap, read-execute -- in an address space of its own, and filled with a
/// pattern. Then:
/// - swap-out leaves each part a swap entry for one slot, keeping that
///   part's flags (none for the gap), and uses one slot;
/// - swap-in maps each part back with its flags, leaves the gap unmapped,
///   brings the pattern back, and gives the slot back;
/// - unmapping the four parts of a swapped frame one by one gives the slot
///   back ([`release_swap_pte`]), and so does tearing the address space down
///   with a frame in swap ([`release_swap_parts`]).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn test_round_trip_keeps_each_part() -> KernelResult<()> {
    use super::page_table::{HW_PAGE_SIZE, HW_PAGES_PER_FRAME};

    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[swap]   FAIL: round trip: {}", what);
        Err(KernelError::InternalError)
    }
    /// The flags a part is compared by: what swap keeps, less accessed and dirty.
    fn kept(flags: PageFlags) -> u64 {
        flags.bits() & !(PageFlags::ACCESSED.bits() | PageFlags::DIRTY.bits())
    }

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let pml4 = page_table::alloc_pml4()?;
    let virt = VirtAddr::new(0x0000_0040_0000_0000);
    let user = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE;
    let parts = [
        user | PageFlags::WRITABLE | PageFlags::NO_EXECUTE,
        user | PageFlags::NO_EXECUTE,
        PageFlags::empty(),
        user,
    ];
    let pattern = |i: usize| (i % 251) as u8;

    let frame = frame::alloc_frame()?;
    for i in 0..FRAME_SIZE {
        // SAFETY: a freshly allocated frame, through the HHDM, in bounds.
        unsafe { ((frame.addr() + hhdm) as *mut u8).add(i).write(pattern(i)) };
    }
    // SAFETY: a fresh address space of this test's and a fresh frame.
    unsafe { page_table::map_frame_subpages(pml4, virt, frame, parts)? };
    let used_before = used_slots();

    // Out: each part an entry for one slot, keeping its own flags.
    // SAFETY: as above; the frame is mapped.
    let entry = unsafe { swap_out_page(pml4, virt)? };
    let out = page_table::read_frame_ptes(pml4, virt).ok_or(KernelError::InternalError)?;
    let out_ok = out.iter().zip(parts).all(|(pte, want)| {
        SwapEntry::from_pte_raw(pte.raw()) == Some(entry)
            && kept(SwapEntry::kept_flags(pte.raw())) == kept(want)
    });
    let one_slot = used_slots() == used_before + 1;
    // SAFETY: as above.
    let seen_swapped =
        unsafe { is_swapped(pml4, VirtAddr::new(virt.as_u64() + 3 * HW_PAGE_SIZE as u64)) };

    // In: each part back as it was, the gap unmapped, the data intact.
    // SAFETY: as above; the frame's parts hold swap entries.
    let representative = unsafe { swap_in_page(pml4, virt)? }.unwrap_or(PageFlags::empty());
    let back = page_table::read_frame_ptes(pml4, virt).ok_or(KernelError::InternalError)?;
    let in_ok = back.iter().zip(parts).all(|(pte, want)| {
        if want.contains(PageFlags::PRESENT) {
            pte.is_present() && kept(pte.flags()) == kept(want)
        } else {
            pte.raw() == 0
        }
    });
    let new_base = back
        .first()
        .map(|pte| pte.phys_addr() & !(FRAME_SIZE as u64 - 1))
        .unwrap_or(0);
    let data_ok = new_base != 0
        && (0..FRAME_SIZE).all(|i| {
            // SAFETY: the frame swap-in mapped, through the HHDM, in bounds.
            unsafe { ((new_base + hhdm) as *const u8).add(i).read() == pattern(i) }
        });
    let slot_back = used_slots() == used_before;
    let representative_ok = kept(representative) == kept(parts[0]);

    // Out again, then unmap the four parts one by one: the slot comes back.
    // SAFETY: as above; the frame is mapped again.
    unsafe { swap_out_page(pml4, virt)? };
    let in_use = used_slots() == used_before + 1;
    let mut released = 0;
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: this test's address space; a user address in it.
        if unsafe { release_swap_pte(pml4, virt.as_u64() + (i * HW_PAGE_SIZE) as u64) } {
            released += 1;
        }
    }
    let unmap_gives_back = released == HW_PAGES_PER_FRAME && used_slots() == used_before;

    // A frame in swap when the address space goes: the slot comes back.
    let frame2 = frame::alloc_frame()?;
    // SAFETY: this test's address space; the four parts were just cleared.
    unsafe {
        page_table::map_frame(
            pml4,
            virt,
            frame2,
            user | PageFlags::WRITABLE | PageFlags::NO_EXECUTE,
        )?;
        swap_out_page(pml4, virt)?;
    }
    let in_use_again = used_slots() == used_before + 1;
    // SAFETY: never loaded in any CR3; nothing else uses it.
    unsafe { page_table::destroy_user_address_space(pml4) };
    let teardown_gives_back = used_slots() == used_before;

    let checks = [
        (
            "swap-out wrote each part's entry with its own flags",
            out_ok,
        ),
        ("swap-out used one slot", one_slot),
        (
            "a part other than the first is seen as swapped",
            seen_swapped,
        ),
        (
            "swap-in mapped each part back as it was, the gap unmapped",
            in_ok,
        ),
        ("swap-in brought the data back", data_ok),
        ("swap-in gave the slot back", slot_back),
        (
            "swap-in answered the first mapped part's flags",
            representative_ok,
        ),
        ("a second swap-out used a slot", in_use),
        (
            "unmapping the four parts gave the slot back",
            unmap_gives_back,
        ),
        ("a third swap-out used a slot", in_use_again),
        ("teardown gave the slot back", teardown_gives_back),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        return fail(what);
    }
    serial_println!(
        "[swap]   round trip: each 4 KiB part comes back as it was, the data with it, and the \
         slot is given back by swap-in, by unmapping and by teardown: OK"
    );
    Ok(())
}

/// Run self-test for the disk-backed swap backend.
///
/// Called after the boot's [`activate_signed_devices`] added a disk area.
/// Writes pages straight to that area's own slots -- a page of zeros, a
/// compressible one and an incompressible one -- reads each back, and gives
/// the slots back. Not through the global allocator, which gives zram's
/// slots first: the disk would not necessarily be what it tested.
pub fn self_test_disk() {
    serial_println!("[swap] Running disk backend self-test...");

    let mut pattern = vec![0u8; FRAME_SIZE];
    for (i, byte) in pattern.iter_mut().enumerate() {
        // Repeating: compresses well.
        #[allow(clippy::cast_possible_truncation)]
        {
            *byte = ((i.wrapping_mul(7)).wrapping_add(13) & 0xFF) as u8;
        }
    }
    let mut noise = vec![0u8; FRAME_SIZE];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for byte in &mut noise {
        // xorshift64: does not compress, so it is stored as it is.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        #[allow(clippy::cast_possible_truncation)]
        {
            *byte = x as u8;
        }
    }
    let pages = [vec![0u8; FRAME_SIZE], pattern, noise];

    let mut state = SWAP.lock();
    let Some(dev) = state
        .devices
        .iter_mut()
        .find(|d| matches!(d.backend, SwapBackend::Disk(_)) && !d.draining)
    else {
        serial_println!("[swap]   Disk backend not active — skipped");
        return;
    };
    let name = escaped_path(&dev.path);
    let mut ok = true;
    let mut taken = Vec::new();
    for page in &pages {
        let Some(slot) = dev.slots.alloc() else {
            serial_println!("[swap]   {}: no free slot to test with", name);
            ok = false;
            break;
        };
        taken.push(slot);
        let mut back = vec![0u8; FRAME_SIZE];
        let round_trip = dev
            .backend
            .write(slot, page)
            .and_then(|()| dev.backend.read(slot, &mut back));
        if round_trip.is_err() || back != *page {
            serial_println!(
                "[swap]   FAIL: {} slot {}: {:?}, data {}",
                name,
                slot,
                round_trip,
                if back == *page { "intact" } else { "differs" }
            );
            ok = false;
        }
    }
    for slot in taken {
        dev.backend.discard(slot);
        dev.slots.free(slot);
    }
    let report = state
        .devices
        .iter()
        .map(|d| {
            alloc::format!(
                "{}(pri={},free={})",
                escaped_path(&d.path),
                d.priority,
                d.slots.free_count()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let count = state.devices.len();
    drop(state);
    serial_println!("[swap]   {} device(s): {}", count, report);
    if ok {
        serial_println!("[swap] Disk backend self-test PASSED");
    } else {
        serial_println!("[swap] Disk backend self-test FAILED");
    }
}

/// Swap areas end to end, on RAM disks of the test's own: the header
/// ([`parse_header`]), extents ([`area_pieces`], [`extents_span`],
/// [`extents_overlap`]), and an area added, used, emptied and taken away
/// ([`activate`], [`deactivate`]) -- a page swapped out to it from an address
/// space of the test's lands where the extents say, and the drain brings it
/// back, data and flags intact.
///
/// # Errors
///
/// `InternalError` naming what failed.
pub fn self_test_areas() -> KernelResult<()> {
    serial_println!("[swap] Running swap-area self-test...");
    test_parse_header()?;
    test_extents()?;
    test_whole_device()?;
    test_area_round_trip()?;
    serial_println!("[swap] Swap-area self-test PASSED");
    Ok(())
}

/// Write a `mkswap` header for an area whose last page is `last_page`, of
/// `page_size` pages, listing `bad`, into `buf`: for the self-tests here and
/// in [`super::swapctl`], so the format has one encoder beside its parser.
pub(crate) fn make_header(buf: &mut [u8], page_size: usize, last_page: u32, bad: &[u32]) {
    let mut put = |at: usize, value: u32| {
        if let Some(field) = buf.get_mut(at..at.saturating_add(4)) {
            field.copy_from_slice(&value.to_le_bytes());
        }
    };
    put(HDR_VERSION, 1);
    put(HDR_LAST_PAGE, last_page);
    put(
        HDR_NR_BADPAGES,
        u32::try_from(bad.len()).unwrap_or(u32::MAX),
    );
    for (i, &page) in bad.iter().enumerate() {
        put(HDR_BADPAGES.saturating_add(i.saturating_mul(4)), page);
    }
    let at = page_size.saturating_sub(SWAP_MAGIC.len());
    if let Some(magic) = buf.get_mut(at..page_size) {
        magic.copy_from_slice(SWAP_MAGIC);
    }
}

/// [`parse_header`] takes what `mkswap` writes, at 4 KiB and at 16 KiB, and
/// refuses the rest -- each refusal for its own reason.
fn test_parse_header() -> KernelResult<()> {
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[swap]   FAIL: header: {}", what);
        Err(KernelError::InternalError)
    }
    let fresh = || vec![0u8; HEADER_READ_BYTES];

    let mut linux = fresh();
    make_header(&mut linux, 4096, 1023, &[]);
    let linux_ok = parse_header(&linux).is_ok_and(|h| {
        h.page_size == 4096
            && h.last_page == 1023
            && h.area_bytes() == 4 << 20
            && h.bad_pages.is_empty()
    });

    let mut own = fresh();
    make_header(&mut own, 16_384, 63, &[5, 9]);
    let own_ok = parse_header(&own)
        .is_ok_and(|h| h.page_size == 16_384 && h.last_page == 63 && h.bad_pages == [5, 9]);

    let mut hibernated = fresh();
    make_header(&mut hibernated, 4096, 1023, &[]);
    if let Some(magic) = hibernated.get_mut(4086..4096) {
        magic.copy_from_slice(b"S1SUSPEND\0");
    }

    let mut version0 = fresh();
    make_header(&mut version0, 4096, 1023, &[]);
    if let Some(field) = version0.get_mut(HDR_VERSION..HDR_VERSION.saturating_add(4)) {
        field.copy_from_slice(&0u32.to_le_bytes());
    }

    let mut empty = fresh();
    make_header(&mut empty, 4096, 0, &[]);
    let mut outside = fresh();
    make_header(&mut outside, 4096, 15, &[16]);
    let mut page_zero = fresh();
    make_header(&mut page_zero, 4096, 15, &[0]);
    let mut too_many = fresh();
    make_header(&mut too_many, 4096, 15, &[]);
    if let Some(field) = too_many.get_mut(HDR_NR_BADPAGES..HDR_NR_BADPAGES.saturating_add(4)) {
        field.copy_from_slice(&638u32.to_le_bytes());
    }

    let checks = [
        (
            "a 4 KiB header, as mkswap writes it on x86, is read",
            linux_ok,
        ),
        ("a 16 KiB header with two bad pages is read", own_ok),
        (
            "nothing at all has no signature",
            parse_header(&fresh()) == Err(HeaderError::NoSignature),
        ),
        (
            "a hibernation image's signature is not swap's",
            parse_header(&hibernated) == Err(HeaderError::NoSignature),
        ),
        (
            "version 0 is refused",
            parse_header(&version0) == Err(HeaderError::Version(0)),
        ),
        (
            "an empty area is refused",
            parse_header(&empty) == Err(HeaderError::Empty),
        ),
        (
            "a bad page past the end is refused",
            parse_header(&outside) == Err(HeaderError::BadPages),
        ),
        (
            "a bad page 0 (the header) is refused",
            parse_header(&page_zero) == Err(HeaderError::BadPages),
        ),
        (
            "more bad pages than a 4 KiB header holds is refused",
            parse_header(&too_many) == Err(HeaderError::BadPages),
        ),
        (
            "a header shorter than its page is not read",
            parse_header(linux.get(..4000).unwrap_or_default()) == Err(HeaderError::NoSignature),
        ),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        return fail(what);
    }
    serial_println!("[swap]   header: mkswap's at 4 KiB and 16 KiB read, eight others refused: OK");
    Ok(())
}

/// An area's sectors map through its extents onto the device, across an
/// extent's edge; extents with a gap, an empty one, or one past the device
/// are not an area; and two areas sharing a sector overlap.
fn test_extents() -> KernelResult<()> {
    let at = |area_sector, dev_sector, sectors| AreaExtent {
        area_sector,
        dev_sector,
        sectors,
    };
    let extents = [at(0, 1000, 40), at(40, 5000, 24), at(64, 3000, 136)];
    let checks = [
        (
            "a run across two edges is three pieces",
            area_pieces(&extents, 30, 40) == Some(vec![(1030, 10), (5000, 24), (3000, 6)]),
        ),
        (
            "the whole area is the three extents",
            area_pieces(&extents, 0, 200) == Some(vec![(1000, 40), (5000, 24), (3000, 136)]),
        ),
        (
            "a run past the end has no pieces",
            area_pieces(&extents, 190, 20).is_none(),
        ),
        (
            "the area is 200 sectors",
            extents_span(&extents, 10_000) == Some(200),
        ),
        (
            "an extent past the device is no area",
            extents_span(&extents, 3100).is_none(),
        ),
        (
            "a gap is no area",
            extents_span(&[at(0, 0, 8), at(9, 100, 8)], 10_000).is_none(),
        ),
        (
            "an empty extent is no area",
            extents_span(&[at(0, 0, 8), at(8, 100, 0)], 10_000).is_none(),
        ),
        ("no extents is no area", extents_span(&[], 10_000).is_none()),
        (
            "a shared sector overlaps",
            extents_overlap(&extents, &[at(0, 3135, 4)]),
        ),
        (
            "neighbours do not",
            !extents_overlap(&extents, &[at(0, 1040, 4), at(4, 4996, 4)]),
        ),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        serial_println!("[swap]   FAIL: extents: {}", what);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[swap]   extents: pieces across edges, gaps and overruns refused, overlaps seen: OK"
    );
    Ok(())
}

/// A scratch RAM disk, unregistered when it goes.
struct ScratchDisk(&'static str);

impl ScratchDisk {
    fn new(name: &'static str, sectors: u64) -> Self {
        crate::blkdev::register(
            name,
            alloc::boxed::Box::new(crate::blkdev::RamBlockDevice::new(sectors)),
        );
        Self(name)
    }
}

impl Drop for ScratchDisk {
    /// Unregister the disk -- unless an area is still on it, which a failed
    /// test can leave: a real process's page reclaim put there would then be
    /// unreadable, so the disk stays, its memory spent, and the boot says so.
    fn drop(&mut self) {
        let in_use =
            SWAP.lock().devices.iter().any(
                |d| matches!(&d.backend, SwapBackend::Disk(disk) if disk.device_name == self.0),
            );
        if in_use {
            serial_println!(
                "[swap]   WARNING: {} still holds a swap area; left registered",
                self.0
            );
            return;
        }
        crate::blkdev::unregister(self.0);
    }
}

/// A whole device: taken when its first slot is a swap header and nothing
/// else; refused when it also holds another format's mark ([`ambivalent`]).
fn test_whole_device() -> KernelResult<()> {
    const DEV: &str = "swaptest1";
    let disk = ScratchDisk::new(DEV, 256);
    let whole = vec![AreaExtent {
        area_sector: 0,
        dev_sector: 0,
        sectors: 256,
    }];
    let request = |path: &[u8]| AreaRequest {
        path: path.to_vec(),
        kind: AreaKind::Device,
        device: String::from(DEV),
        extents: whole.clone(),
        priority: Some(-1000),
    };
    let unsigned = activate(request(b"/dev/swaptest1"));

    let mut head = vec![0u8; HEADER_READ_BYTES];
    make_header(&mut head, 4096, 31, &[]);
    // An ext4 superblock's magic as well: two formats at once.
    if let Some(magic) = head.get_mut(1080..1082) {
        magic.copy_from_slice(&[0x53, 0xEF]);
    }
    area_io(DEV, &whole, 0, &mut head, true)?;
    let both = activate(request(b"/dev/swaptest1"));

    if let Some(magic) = head.get_mut(1080..1082) {
        magic.copy_from_slice(&[0, 0]);
    }
    area_io(DEV, &whole, 0, &mut head, true)?;
    let taken = activate(request(b"/dev/swaptest1"));
    let removed = deactivate(&AreaTarget::Device(String::from(DEV)));
    // A default priority is given back: on, off and on again lands where it
    // was, as Linux's does.
    let target = AreaTarget::Device(String::from(DEV));
    let by_default = || AreaRequest {
        priority: None,
        ..request(b"/dev/swaptest1")
    };
    let once = activate(by_default()).map(|a| a.priority);
    let off_once = deactivate(&target).is_ok();
    let twice = activate(by_default()).map(|a| a.priority);
    let off_twice = deactivate(&target).is_ok();
    let priority_back = once.is_ok() && once == twice && off_once && off_twice;
    drop(disk);

    // Each other format's mark beside the header makes the device another's.
    let marked = |at: usize, mark: &[u8]| {
        let mut first = head.clone();
        first
            .get_mut(at..at.saturating_add(mark.len()))
            .map(|dst| dst.copy_from_slice(mark))
            .is_some()
            && ambivalent(&first)
    };
    let others_marks = marked(512, b"EFI PART")
        && marked(510, &[0x55, 0xAA])
        && marked(0, b"XFSB")
        && !ambivalent(&head);

    let checks = [
        (
            "a GPT, a DOS boot sector (MBR, FAT, NTFS) or XFS beside the header is another's",
            others_marks,
        ),
        ("a default priority is given back at swapoff", priority_back),
        (
            "a device with no header is refused",
            unsigned == Err(KernelError::InvalidArgument),
        ),
        (
            "a device that is swap and ext4 at once is refused",
            both == Err(KernelError::InvalidArgument),
        ),
        (
            // 32 pages of 4 KiB are 128 KiB: eight slots, the header's one.
            "a device that is only swap is taken, header slot held back",
            taken
                == Ok(ActiveArea {
                    pages: 7,
                    priority: -1000,
                }),
        ),
        (
            "and taken away again",
            removed
                .as_ref()
                .is_ok_and(|r| r.kind == AreaKind::Device && r.device == DEV),
        ),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        serial_println!(
            "[swap]   FAIL: whole device: {} ({:?} / {:?} / {:?} / {:?})",
            what,
            unsigned,
            both,
            taken,
            removed.map(|r| r.device)
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[swap]   whole device: unsigned and ambivalent refused, signed taken and given back, a \
         default priority given back too: OK"
    );
    Ok(())
}

/// An area made of three extents scattered over a RAM disk -- one slot
/// straddling two of them, one holding a bad page -- added above every
/// other area; a page swapped out from a test address space lands in it,
/// its bytes where the extents put that slot; `swapoff` brings the page back
/// mapped as it was and takes the area away; the slot numbers and counts
/// are as before.
// A self-test's fixed numbers: frame addresses plus the direct map's offset,
// a slot's sector (slot * 32 for 16 KiB slots), and a pattern byte cut from
// a counter -- none can overflow or lose bits that matter.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn test_area_round_trip() -> KernelResult<()> {
    const DEV: &str = "swaptest0";
    const PATH: &[u8] = b"/swaptest-area";
    let disk = ScratchDisk::new(DEV, 4096);
    let at = |area_sector, dev_sector, sectors| AreaExtent {
        area_sector,
        dev_sector,
        sectors,
    };
    // 256 sectors, eight slots: slot 1 (sectors 32-63) is 16 sectors at
    // 2032 and 16 at 100.
    let extents = vec![at(0, 2000, 48), at(48, 100, 48), at(96, 3000, 160)];
    let mut head = vec![0u8; HEADER_READ_BYTES];
    make_header(&mut head, 16_384, 7, &[6]);
    area_io(DEV, &extents, 0, &mut head, true)?;

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let pml4 = page_table::alloc_pml4()?;
    let virt = VirtAddr::new(0x0000_0044_0000_0000);
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    let mut noise = vec![0u8; FRAME_SIZE];
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for byte in &mut noise {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *byte = x as u8;
    }

    let result = (|| -> Result<(), &'static str> {
        let used_before = used_slots();
        let area = activate(AreaRequest {
            path: PATH.to_vec(),
            kind: AreaKind::File { fs_id: 0, ino: 0 },
            device: String::from(DEV),
            extents: extents.clone(),
            priority: Some(i32::MAX),
        })
        .map_err(|_| "the area was not taken")?;
        if area.pages != 6 {
            return Err("eight slots less the header's and the bad one should hold six pages");
        }
        let again = activate(AreaRequest {
            path: b"/swaptest-again".to_vec(),
            kind: AreaKind::File { fs_id: 0, ino: 0 },
            device: String::from(DEV),
            extents: vec![at(0, 3100, 64)],
            priority: None,
        });
        if again != Err(KernelError::DeviceBusy) {
            return Err("sectors already swap were taken twice");
        }
        let listed = list_devices()
            .into_iter()
            .find(|d| d.path == PATH)
            .ok_or("the area is not listed")?;
        if listed.device_type != "file" || listed.total_slots != 6 || listed.used_slots != 0 {
            return Err("the area is listed wrong");
        }
        let base = SWAP
            .lock()
            .devices
            .iter()
            .find(|d| d.path == PATH)
            .map(|d| d.base_slot)
            .ok_or("the area has no slots")?;

        // A page out to it: the highest priority takes it.
        let frame = frame::alloc_frame().map_err(|_| "no frame")?;
        // SAFETY: a fresh frame, through the HHDM, FRAME_SIZE bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                noise.as_ptr(),
                (frame.addr() + hhdm) as *mut u8,
                FRAME_SIZE,
            );
        }
        // SAFETY: the test's own address space and a fresh frame.
        unsafe { page_table::map_frame(pml4, virt, frame, flags) }.map_err(|_| "map")?;
        // SAFETY: as above; the frame is mapped.
        let entry = unsafe { swap_out_page(pml4, virt) }.map_err(|_| "swap-out failed")?;
        let local = entry
            .slot()
            .checked_sub(base)
            .filter(|&s| s > 0 && s < 8 && s != 6)
            .ok_or("the page did not go to the area's usable slots")?;
        // It does not compress, so it is there as it was, where the extents
        // put its slot.
        let mut raw = vec![0u8; FRAME_SIZE];
        area_io(DEV, &extents, u64::from(local) * 32, &mut raw, false).map_err(|_| "read raw")?;
        if raw != noise {
            return Err("the page is not where the extents put its slot");
        }

        // swapoff: the page comes back and the area goes.
        let removed = deactivate_also(&AreaTarget::File { fs_id: 0, ino: 0 }, &[pml4])
            .map_err(|_| "swapoff failed")?;
        if removed.device != DEV || removed.extents != extents {
            return Err("swapoff gave back the wrong area");
        }
        let back = page_table::read_frame_ptes(pml4, virt).ok_or("no table")?;
        let phys = back
            .first()
            .filter(|pte| pte.is_present())
            .map(|pte| pte.phys_addr() & !(FRAME_SIZE as u64 - 1))
            .ok_or("the page was not brought back")?;
        let mut now = vec![0u8; FRAME_SIZE];
        // SAFETY: the mapped frame, through the HHDM, FRAME_SIZE bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (phys + hhdm) as *const u8,
                now.as_mut_ptr(),
                FRAME_SIZE,
            );
        }
        if now != noise {
            return Err("the page came back with other data");
        }
        let kept =
            |f: PageFlags| f.bits() & !(PageFlags::ACCESSED.bits() | PageFlags::DIRTY.bits());
        if back.iter().any(|pte| kept(pte.flags()) != kept(flags)) {
            return Err("the page came back with other flags");
        }
        if list_devices().iter().any(|d| d.path == PATH) {
            return Err("the area is still listed");
        }
        if !matches!(
            deactivate(&AreaTarget::File { fs_id: 0, ino: 0 }),
            Err(KernelError::InvalidArgument)
        ) {
            return Err("a second swapoff was not refused");
        }
        if used_slots() != used_before {
            return Err("the slot counts are not as before");
        }
        Ok(())
    })();

    // Whatever happened: the area goes (with the address space first, whose
    // teardown gives back any slot still named), then the disk.
    // SAFETY: never loaded in any CR3; nothing else uses it.
    unsafe { page_table::destroy_user_address_space(pml4) };
    // Gone already on success, and then `InvalidArgument`: nothing to do.
    let _ = deactivate(&AreaTarget::File { fs_id: 0, ino: 0 });
    drop(disk);

    match result {
        Ok(()) => {
            serial_println!(
                "[swap]   area round trip: added over three extents, a page out to it and on \
                 the disk where they say, swapoff brings it back and takes the area away: OK"
            );
            Ok(())
        }
        Err(what) => {
            serial_println!("[swap]   FAIL: area round trip: {}", what);
            Err(KernelError::InternalError)
        }
    }
}
