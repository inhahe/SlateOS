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

use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use spin::Mutex;
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
    fn is_used(&self, slot: u32) -> bool {
        let wi = (slot / 64) as usize;
        let bit = slot % 64;
        self.bitmap
            .get(wi)
            .map_or(false, |w| *w & (1u64 << bit) != 0)
    }

    /// Number of free slots.
    #[must_use]
    fn free_count(&self) -> u32 {
        self.capacity.saturating_sub(self.used)
    }
}

// ---------------------------------------------------------------------------
// In-memory swap backend (for testing and zram-like use)
// ---------------------------------------------------------------------------

/// In-memory swap backend.
///
/// Stores page data in heap-allocated buffers.  Each slot is a
/// `[u8; FRAME_SIZE]` buffer.  This is useful for:
///
/// 1. **Testing**: validates the entire swap pipeline without disk I/O.
/// 2. **zram prototype**: with compression added later, this becomes a
///    compressed in-memory swap (zram-style).
///
/// Without compression, this backend doesn't save memory — it just
/// moves data from physical frames to heap buffers.  It exists to
/// prove that the swap infrastructure works correctly before adding
/// a disk backend or compression.
struct MemBackend {
    /// Slot storage.  `None` = slot never written.
    slots: Vec<Option<Vec<u8>>>,
    /// Total capacity (number of slots).
    capacity: u32,
}

impl MemBackend {
    /// Create a new in-memory backend with the given capacity.
    fn new(capacity: u32) -> Self {
        let mut slots = Vec::with_capacity(capacity as usize);
        for _ in 0..capacity {
            slots.push(None);
        }
        Self { slots, capacity }
    }

    /// Write a page's data into a swap slot.
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

        *slot_storage = Some(data.to_vec());
        Ok(())
    }

    /// Read a page's data from a swap slot.
    ///
    /// `buf` must be exactly `FRAME_SIZE` bytes.
    fn read(&self, slot: u32, buf: &mut [u8]) -> KernelResult<()> {
        if slot >= self.capacity {
            return Err(KernelError::InvalidArgument);
        }
        if buf.len() != FRAME_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let data = self
            .slots
            .get(slot as usize)
            .and_then(|s| s.as_ref())
            .ok_or(KernelError::InvalidArgument)?;

        buf.copy_from_slice(data);
        Ok(())
    }

    /// Discard a slot's data (after swap-in).
    fn discard(&mut self, slot: u32) {
        if let Some(slot_storage) = self.slots.get_mut(slot as usize) {
            *slot_storage = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Global swap state
// ---------------------------------------------------------------------------

/// Global swap subsystem state.
struct SwapState {
    /// Slot allocator.
    slots: SwapSlotAllocator,
    /// Storage backend.
    backend: MemBackend,
    /// Whether the subsystem is initialized.
    initialized: bool,
}

impl SwapState {
    const fn uninit() -> Self {
        Self {
            slots: SwapSlotAllocator {
                bitmap: Vec::new(),
                capacity: 0,
                used: 0,
                hint: 0,
            },
            backend: MemBackend {
                slots: Vec::new(),
                capacity: 0,
            },
            initialized: false,
        }
    }
}

/// Lock ordering: SWAP → RECLAIM → page table → frame allocator.
static SWAP: Mutex<SwapState> = Mutex::new(SwapState::uninit());

// ---------------------------------------------------------------------------
// Reclaimable page tracking (Clock algorithm)
// ---------------------------------------------------------------------------

/// A record of a user-space page that can be reclaimed (swapped out).
///
/// The reclaimer maintains a circular list of these records and uses
/// the Clock (second-chance) algorithm to select victims.
#[derive(Clone, Copy)]
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

static RECLAIM: Mutex<ReclaimState> = Mutex::new(ReclaimState::new());

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
            // SAFETY: pml4_phys is from a registered process.
            let pte = unsafe {
                page_table::read_leaf_pte(entry.pml4_phys, virt)
            };

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

    // Now swap out each victim.
    for (idx, victim) in victims {
        let virt = VirtAddr::new(victim.vaddr);
        // SAFETY: pml4_phys and virt are from the reclaim list;
        // the page was verified present above.
        match unsafe { swap_out_page(victim.pml4_phys, virt) } {
            Ok(_entry) => {
                reclaimed += 1;
                // Mark the entry inactive (it's now in swap, not memory).
                let mut state = RECLAIM.lock();
                if let Some(e) = state.pages.get_mut(idx) {
                    e.active = false;
                    state.active_count =
                        state.active_count.saturating_sub(1);
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
        serial_println!("[swap] Reclaimed {} pages", reclaimed);
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

/// Initialize the swap subsystem with the given number of 16 KiB slots.
///
/// `num_slots` determines the maximum number of pages that can be
/// simultaneously swapped out.  Each slot consumes no memory until
/// a page is actually written to it (the in-memory backend allocates
/// on demand).
///
/// Called during kernel boot, after the heap is available.
pub fn init(num_slots: u32) {
    let mut state = SWAP.lock();

    state.slots = SwapSlotAllocator::new(num_slots);
    state.backend = MemBackend::new(num_slots);
    state.initialized = true;

    serial_println!(
        "[swap] Initialized: {} slots ({} KiB max swap space)",
        num_slots,
        (num_slots as u64).saturating_mul(FRAME_SIZE as u64) / 1024
    );
}

/// Check if the swap subsystem is initialized and has capacity.
#[must_use]
pub fn is_available() -> bool {
    let state = SWAP.lock();
    state.initialized && state.slots.free_count() > 0
}

/// Number of free swap slots.
#[must_use]
pub fn free_slots() -> u32 {
    SWAP.lock().slots.free_count()
}

/// Number of used swap slots.
#[must_use]
pub fn used_slots() -> u32 {
    SWAP.lock().slots.used
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
    let swap_entry = {
        let mut state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }

        let slot = state.slots.alloc().ok_or(KernelError::OutOfMemory)?;
        let entry = SwapEntry::new(slot).ok_or(KernelError::InternalError)?;

        state.backend.write(slot, &page_data)?;

        entry
    };
    // Drop SWAP lock before page table manipulation (lock ordering).

    // Step 3: Unmap the frame from the page table.
    // SAFETY: pml4_phys is valid, virt is frame-aligned and mapped.
    let phys_frame = unsafe { page_table::unmap_frame(pml4_phys, virt)? };

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
    // SAFETY: pml4_phys is valid.
    let pte = unsafe { page_table::read_leaf_pte(pml4_phys, virt) }
        .ok_or(KernelError::InvalidAddress)?;

    let swap_entry = SwapEntry::from_pte_raw(pte.raw())
        .ok_or(KernelError::InvalidArgument)?;

    // Step 2: Read page data from the swap backend.
    let mut page_data = vec![0u8; FRAME_SIZE];
    {
        let state = SWAP.lock();
        if !state.initialized {
            return Err(KernelError::NotSupported);
        }
        state.backend.read(swap_entry.slot(), &mut page_data)?;
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

    // Step 6: Free the swap slot.
    {
        let mut state = SWAP.lock();
        state.backend.discard(swap_entry.slot());
        state.slots.free(swap_entry.slot());
    }

    // Step 7: Flush the TLB.
    // SAFETY: invlpg is always safe.
    unsafe { page_table::flush_frame(virt); }

    serial_println!(
        "[swap] Swapped in: virt={:#x} ← slot={}",
        virt.as_u64(), swap_entry.slot()
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
    unsafe { page_table::read_leaf_pte(pml4_phys, virt) }
        .map_or(false, |pte| pte.is_swap())
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

    // --- In-memory backend ---
    {
        let mut backend = MemBackend::new(4);

        // Write test pattern to slot 0.
        let mut data = vec![0u8; FRAME_SIZE];
        for (i, byte) in data.iter_mut().enumerate() {
            // Truncation: intentional — we want a repeating byte pattern.
            #[allow(clippy::cast_possible_truncation)]
            {
                *byte = (i & 0xFF) as u8;
            }
        }
        backend.write(0, &data).expect("write should succeed");

        // Read back and verify.
        let mut buf = vec![0u8; FRAME_SIZE];
        backend.read(0, &mut buf).expect("read should succeed");
        assert_eq!(buf, data, "data integrity check failed");

        // Write different pattern to slot 1.
        let data2 = vec![0xAA; FRAME_SIZE];
        backend.write(1, &data2).expect("write slot 1");

        // Verify slot 0 is unchanged.
        backend.read(0, &mut buf).expect("re-read slot 0");
        assert_eq!(buf, data, "slot 0 should be unchanged");

        // Read slot 1.
        backend.read(1, &mut buf).expect("read slot 1");
        assert_eq!(buf, data2, "slot 1 data check");

        // Discard and verify it's gone.
        backend.discard(0);
        assert!(
            backend.read(0, &mut buf).is_err(),
            "discarded slot should fail to read"
        );

        // Out-of-range slot should fail.
        assert!(backend.write(4, &data).is_err(), "slot 4 out of range");
        assert!(backend.read(4, &mut buf).is_err(), "slot 4 out of range");

        serial_println!("[swap]   In-memory backend: OK");
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

    serial_println!("[swap] Self-test PASSED");
}
