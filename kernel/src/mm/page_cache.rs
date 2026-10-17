//! File page cache ("C-lite") — shared file pages via the frame allocator's
//! per-frame refcount.
//!
//! This module is the storage core of the page cache decided in
//! design-decisions §23/§36 and extended to shared mappings by §1572.  Its job
//! is to cache a file's pages **once** and share them across every process
//! that maps or reads them, so that:
//!
//! 1. **Shared-library `.text` is deduplicated.** N processes that map the same
//!    `.so` share one set of physical frames instead of N private copies.
//! 2. **The block buffer cache and per-mapping copies stop double-caching.** A
//!    page lives in exactly one place keyed by its stable file identity.
//!
//! A private (`MAP_PRIVATE`) writer copies-on-write to a fresh anonymous
//! frame; the cached frame it copied from is never mutated by it.
//!
//! ## Shared mappings (design-decisions §1572)
//!
//! A shared mapping of a file (`MAP_SHARED`) maps the file's cache frames
//! themselves, so it and every `read` see one copy. The cache counts a file's
//! shared mappings ([`shared_map_added`], [`shared_map_removed`]); while any
//! lives, a change to the file updates its cached pages in place instead of
//! dropping them ([`file_changed`]): a dropped page would stay mapped,
//! showing what the file held before.
//!
//! ## Changes racing fills
//!
//! A fill reads the file with the cache unlocked, and a change can land
//! between that read and the insert. Each fill notes the file's change count
//! ([`FileState::changes`]) when it begins and reads again if the count moved
//! by the insert, so no fill installs bytes a change replaced.
//!
//! ## Refcount model (design-decisions §37)
//!
//! Page lifetime rides the **frame allocator's existing per-frame refcount**
//! ([`crate::mm::frame::refcount`] / [`crate::mm::frame::ref_inc`] /
//! [`crate::mm::frame::free_frame`]) — the very mechanism CoW already uses — not
//! a separate mapper count inside the cache.  Concretely:
//!
//! - A resident [`CacheEntry`] holds **exactly one** frame reference (the entry's
//!   presence in the map *is* that reference; a freshly allocated frame starts at
//!   refcount 1).
//! - [`get_or_fill`] / [`lookup`] return a frame with **one additional reference
//!   already added on the caller's behalf** (so eviction cannot free it out from
//!   under a caller that is about to map it).  The caller either maps it — after
//!   which the standard `free_frame` teardown owns that reference — or, if it
//!   never maps it, hands it back to [`release`].
//! - Process unmap / exit frees mapped frames through the **standard
//!   `free_frame` path with no changes**: it decrements and only returns the
//!   frame to the allocator at refcount 0.
//! - "Is this page actively mapped?" is `frame::refcount(frame) > 1`.
//!
//! ## Keying
//!
//! Pages are keyed by [`PageKey`] = ([`FileId`], page index), where the page
//! index is `file_offset / FRAME_SIZE` (16 KiB pages).  [`FileId`] is the stable
//! `(fs_id, ino)` identity from [`crate::fs::vfs::Vfs::file_identity`].  Only
//! files with a stable identity are cacheable; callers that get `None` from
//! `file_identity` (FAT, ISO9660, pseudo-filesystems) must fall back to the
//! per-mapping read path and never enter this cache.
//!
//! ## Lock ordering
//!
//! The cache lock is taken **before** the frame-allocator lock (cache →
//! allocator).  We hold the cache lock across the cheap `ref_inc` /
//! `frame::refcount` calls (this closes the evict-vs-map race, since eviction
//! also takes the cache lock first), but **never** across the expensive
//! `alloc_frame` + fill on a miss — that runs with the cache lock dropped and a
//! re-lock resolves the fill race.

#![allow(clippy::module_name_repetitions)]

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::FileId;
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::mm::page_table;

// ---------------------------------------------------------------------------
// Keys & entries
// ---------------------------------------------------------------------------

/// Cache key: a stable file identity plus the 16 KiB page index within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PageKey {
    /// Stable system-wide file identity (`(fs_id, ino)`).
    file: FileId,
    /// Page index = `file_offset / FRAME_SIZE`.
    page_index: u64,
}

/// A resident cached page.  The entry's presence holds exactly one reference on
/// `frame` (see the module-level refcount model).
#[derive(Debug)]
struct CacheEntry {
    /// The physical frame holding this page's bytes (zero-padded past EOF).
    frame: PhysFrame,
    /// The frame holds bytes the file does not have yet: a writable shared
    /// mapping wrote it ([`mark_dirty`]), or a write to the file landed in
    /// it while such a mapping could ([`file_changed`]). Saved by
    /// [`writeback`]; never evicted while set. Changed only through
    /// [`set_dirty`](Self::set_dirty), which keeps [`DIRTY_COUNT`] in step.
    dirty: bool,
}

impl CacheEntry {
    /// Set or clear the dirty mark, counting it in [`DIRTY_COUNT`]. An entry
    /// taken out of the index is cleared first, so that the count is of the
    /// index's dirty pages.
    fn set_dirty(&mut self, dirty: bool) {
        if self.dirty != dirty {
            self.dirty = dirty;
            if dirty {
                DIRTY_COUNT.fetch_add(1, Ordering::Relaxed);
            } else {
                DIRTY_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// What the cache knows about a file besides its pages: kept while the file
/// has a shared mapping or a fill in flight, and dropped when it has neither.
#[derive(Debug, Default)]
struct FileState {
    /// VMAs -- each piece a split leaves -- that map the file's cache frames
    /// directly: its shared mappings ([`shared_map_added`]).
    shared_maps: u32,
    /// Fills of the file's pages in flight ([`get_or_fill`]).
    fills: u32,
    /// Changes to the file while it has had state ([`file_changed`]). A fill
    /// that finds the count moved since it began read bytes a change
    /// replaced, and reads again.
    changes: u64,
    /// The shared mappings among [`shared_maps`](Self::shared_maps) whose
    /// handle may write: those that are, or may be made, writable.
    writable_maps: u32,
    /// A handle open for writing the file, a reference of the cache's own,
    /// that [`writeback`] saves dirty pages through: kept from the first
    /// writable mapping until the last is gone and its pages are saved.
    writer: Option<u64>,
}

impl FileState {
    /// Nothing left to keep: no shared mapping, no fill in flight, no writer.
    fn idle(&self) -> bool {
        self.shared_maps == 0 && self.fills == 0 && self.writer.is_none()
    }
}

/// The cache: its pages, a `BTreeMap` keyed by `(FileId, page_index)` so a
/// file's pages are one range ([`invalidate_file`]), and the state kept about
/// files ([`FileState`]).
struct CacheState {
    pages: BTreeMap<PageKey, CacheEntry>,
    files: BTreeMap<FileId, FileState>,
}

impl CacheState {
    /// File `file`'s state, made when it has none.
    fn file_mut(&mut self, file: FileId) -> &mut FileState {
        let files = &mut self.files;
        if !files.contains_key(&file) {
            TRACKED_FILES.fetch_add(1, Ordering::Relaxed);
        }
        files.entry(file).or_default()
    }

    /// Drop file `file`'s state if there is nothing left in it to keep.
    fn forget_if_idle(&mut self, file: FileId) {
        if self.files.get(&file).is_some_and(FileState::idle) {
            self.files.remove(&file);
            TRACKED_FILES.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Take every cached page of `file` out of the index, appending their
    /// frames -- each the cache's own reference -- to `frames`.
    fn take_file_pages(&mut self, file: FileId, frames: &mut alloc::vec::Vec<PhysFrame>) {
        self.take_pages_from(file, 0, frames);
    }

    /// Take `file`'s cached pages from page index `first` on out of the
    /// index, appending their frames to `frames`.
    fn take_pages_from(
        &mut self,
        file: FileId,
        first: u64,
        frames: &mut alloc::vec::Vec<PhysFrame>,
    ) {
        let lo = PageKey {
            file,
            page_index: first,
        };
        let hi = PageKey {
            file,
            page_index: u64::MAX,
        };
        let keys: alloc::vec::Vec<PageKey> = self.pages.range(lo..=hi).map(|(k, _)| *k).collect();
        for k in keys {
            if let Some(mut entry) = self.pages.remove(&k) {
                entry.set_dirty(false);
                frames.push(entry.frame);
            }
        }
    }
}

/// The cache's state: see [`CacheState`].
static PAGE_CACHE: Mutex<CacheState> = Mutex::new(CacheState {
    pages: BTreeMap::new(),
    files: BTreeMap::new(),
});

/// Files the cache keeps state about ([`FileState`]), mirrored like
/// [`RESIDENT_COUNT`], so that [`is_populated`] sees a file with a fill in
/// flight and nothing resident yet.
static TRACKED_FILES: AtomicUsize = AtomicUsize::new(0);

/// Resident-entry count, mirrored as a lock-free atomic so coherence hooks on
/// the (hot) write/truncate/unlink paths can ask "is anything cached at all?"
/// with a single relaxed load instead of taking the cache lock or resolving a
/// `FileId`.  Kept in lockstep with `PAGE_CACHE.len()`: bumped on insert,
/// dropped on every removal.
static RESIDENT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Dirty pages in the index ([`CacheEntry::dirty`]), kept by
/// [`CacheEntry::set_dirty`]: what [`dirty_pages`] answers without walking
/// the index under the cache's lock (`/proc/meminfo`, read once a second by
/// `top`).
static DIRTY_COUNT: AtomicUsize = AtomicUsize::new(0);

// Counters (monitoring; mirror what fs::pagecache exposes for stats).
static STAT_HITS: AtomicU64 = AtomicU64::new(0);
static STAT_MISSES: AtomicU64 = AtomicU64::new(0);
static STAT_INSERTS: AtomicU64 = AtomicU64::new(0);
static STAT_RACES_LOST: AtomicU64 = AtomicU64::new(0);
static STAT_RELEASES: AtomicU64 = AtomicU64::new(0);
static STAT_EVICTIONS: AtomicU64 = AtomicU64::new(0);
static STAT_REFILLS: AtomicU64 = AtomicU64::new(0);
static STAT_UPDATED_IN_PLACE: AtomicU64 = AtomicU64::new(0);
static STAT_WRITTEN_BACK: AtomicU64 = AtomicU64::new(0);

/// Snapshot of page-cache counters.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageCacheStats {
    /// Lookups satisfied by an already-resident page.
    pub hits: u64,
    /// Lookups that had to allocate + fill a new frame.
    pub misses: u64,
    /// New entries inserted into the map.
    pub inserts: u64,
    /// Misses that lost the fill race and freed their just-filled frame.
    pub races_lost: u64,
    /// Caller references dropped via [`release`].
    pub releases: u64,
    /// Entries evicted/invalidated (cache reference dropped).
    pub evictions: u64,
    /// Fills read again because the file changed while they read it.
    pub refills: u64,
    /// Cached pages of a shared-mapped file changed in place by a write or a
    /// truncate rather than dropped.
    pub updated_in_place: u64,
    /// Dirty pages saved to their files ([`writeback`]).
    pub written_back: u64,
    /// Entries currently resident.
    pub resident: u64,
}

/// Read a snapshot of the page-cache counters.
#[must_use]
pub fn stats() -> PageCacheStats {
    let resident = PAGE_CACHE.lock().pages.len() as u64;
    PageCacheStats {
        hits: STAT_HITS.load(Ordering::Relaxed),
        misses: STAT_MISSES.load(Ordering::Relaxed),
        inserts: STAT_INSERTS.load(Ordering::Relaxed),
        races_lost: STAT_RACES_LOST.load(Ordering::Relaxed),
        releases: STAT_RELEASES.load(Ordering::Relaxed),
        evictions: STAT_EVICTIONS.load(Ordering::Relaxed),
        refills: STAT_REFILLS.load(Ordering::Relaxed),
        updated_in_place: STAT_UPDATED_IN_PLACE.load(Ordering::Relaxed),
        written_back: STAT_WRITTEN_BACK.load(Ordering::Relaxed),
        resident,
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Compute the page key for a file offset, validating frame alignment.
fn key_for(file: FileId, file_offset: u64) -> KernelResult<PageKey> {
    // Offsets handed to the cache must be frame-aligned (the fault path always
    // faults whole 16 KiB pages).  Reject anything else rather than silently
    // truncating, which would alias two distinct offsets to one key.
    if !file_offset.is_multiple_of(FRAME_SIZE as u64) {
        return Err(KernelError::InvalidArgument);
    }
    // FRAME_SIZE is a nonzero constant, so this division never traps; checked_div
    // keeps the arithmetic-side-effects lint satisfied without an allow.
    let page_index = file_offset
        .checked_div(FRAME_SIZE as u64)
        .ok_or(KernelError::InvalidArgument)?;
    Ok(PageKey { file, page_index })
}

/// Acquire a reference to a cached page if it is already resident.
///
/// On a hit, adds one reference to the frame on the caller's behalf and returns
/// it; the caller must either map it (after which `free_frame` teardown owns the
/// reference) or hand it back to [`release`].  On a miss, returns `Ok(None)`.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] for a frame-misaligned offset, or a
/// frame-allocator error from the reference bump.
pub fn lookup(file: FileId, file_offset: u64) -> KernelResult<Option<PhysFrame>> {
    let key = key_for(file, file_offset)?;
    let cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.pages.get(&key) {
        let frame = entry.frame;
        // SAFETY: the entry is resident ⇒ `frame` is an allocated frame from the
        // frame allocator with refcount ≥ 1.  We hold the cache lock, and
        // eviction also takes the cache lock first, so the frame cannot be freed
        // concurrently between this check and the reference bump.
        unsafe { frame::ref_inc(frame)? };
        STAT_HITS.fetch_add(1, Ordering::Relaxed);
        return Ok(Some(frame));
    }
    Ok(None)
}

/// Acquire a reference to the page for `(file, file_offset)`, filling it from
/// the backing store on a miss.
///
/// Returns a frame with **one reference added for the caller** (see the
/// module-level refcount model).  On a hit, the shared resident frame is bumped
/// and returned.  On a miss, a fresh frame is allocated, zeroed, populated by
/// `fill` (a short fill leaves the tail zero, matching demand-paging EOF
/// semantics), inserted, and returned with the caller's reference; racing CPUs
/// are resolved so only one frame survives per key.
///
/// `fill` receives the full `FRAME_SIZE`-byte, already-zeroed page buffer. It
/// runs again, into a fresh buffer, when the file changed while it read
/// ([`file_changed`]): what it read may be what the change replaced.
///
/// The caller owns one reference: map the frame (teardown then owns it) or call
/// [`release`].
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] for a misaligned offset.
/// - Whatever `fill` returns on a read error (the just-allocated frame is freed
///   before propagating).
/// - Frame-allocation / HHDM / reference-bump errors.
pub fn get_or_fill<F>(file: FileId, file_offset: u64, mut fill: F) -> KernelResult<PhysFrame>
where
    F: FnMut(&mut [u8]) -> KernelResult<()>,
{
    let key = key_for(file, file_offset)?;
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;

    loop {
        // Fast path: already resident.  Bump under the cache lock (closes the
        // evict-vs-map race; eviction also locks the cache first).  On a
        // miss, the fill is counted under the same hold, with the file's
        // change count it starts from.
        let seen = {
            let mut cache = PAGE_CACHE.lock();
            if let Some(entry) = cache.pages.get(&key) {
                let frame = entry.frame;
                // SAFETY: resident entry ⇒ allocated frame, refcount ≥ 1; cache
                // lock held so eviction cannot free it concurrently.
                unsafe { frame::ref_inc(frame)? };
                STAT_HITS.fetch_add(1, Ordering::Relaxed);
                return Ok(frame);
            }
            let state = cache.file_mut(file);
            state.fills = state.fills.saturating_add(1);
            state.changes
        };

        STAT_MISSES.fetch_add(1, Ordering::Relaxed);
        let filled = fill_new_frame(hhdm, &mut fill);

        // Re-acquire, end this fill's count, and insert, resolving the races.
        let mut cache = PAGE_CACHE.lock();
        let moved = cache.files.get(&file).is_none_or(|st| st.changes != seen);
        if let Some(state) = cache.files.get_mut(&file) {
            state.fills = state.fills.saturating_sub(1);
        }
        cache.forget_if_idle(file);
        let new_frame = filled?;
        if let Some(entry) = cache.pages.get(&key) {
            // Another CPU filled this page first; adopt theirs and discard ours.
            let winner = entry.frame;
            // SAFETY: resident entry ⇒ allocated frame; cache lock held.
            let bumped = unsafe { frame::ref_inc(winner) };
            drop(cache);
            STAT_RACES_LOST.fetch_add(1, Ordering::Relaxed);
            // SAFETY: `new_frame` (refcount 1) was never mapped or published.
            let _ = unsafe { frame::free_frame(new_frame) };
            bumped?;
            return Ok(winner);
        }
        if moved {
            // The file changed while the fill read it: what it read may be
            // what the change replaced. Read again.
            drop(cache);
            STAT_REFILLS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: `new_frame` (refcount 1) was never mapped or published.
            let _ = unsafe { frame::free_frame(new_frame) };
            continue;
        }
        // We win the race.  `new_frame` carries refcount 1 (the cache's
        // reference); bump once more for the caller's returned reference (→ 2).
        // SAFETY: `new_frame` is a freshly allocated, valid frame; cache lock held.
        if let Err(e) = unsafe { frame::ref_inc(new_frame) } {
            drop(cache);
            // SAFETY: never mapped or published.
            let _ = unsafe { frame::free_frame(new_frame) };
            return Err(e);
        }
        cache.pages.insert(
            key,
            CacheEntry {
                frame: new_frame,
                dirty: false,
            },
        );
        RESIDENT_COUNT.fetch_add(1, Ordering::Relaxed);
        STAT_INSERTS.fetch_add(1, Ordering::Relaxed);
        return Ok(new_frame);
    }
}

/// A fresh frame, zeroed, then filled by `fill`: refcount 1, published nowhere.
/// The frame is freed again if `fill` fails.
fn fill_new_frame<F>(hhdm: u64, fill: &mut F) -> KernelResult<PhysFrame>
where
    F: FnMut(&mut [u8]) -> KernelResult<()>,
{
    // Allocated and filled with the cache lock dropped (alloc_frame is
    // expensive, and the fill reads the file; neither nests under the lock).
    let new_frame = frame::alloc_frame()?;
    // SAFETY: `new_frame` was just allocated, is exclusively owned by us, is not
    // mapped anywhere, and `to_virt(hhdm)` is its valid HHDM alias of exactly
    // FRAME_SIZE bytes.
    let buf = unsafe {
        let ptr = new_frame.to_virt(hhdm) as *mut u8;
        core::ptr::write_bytes(ptr, 0, FRAME_SIZE);
        core::slice::from_raw_parts_mut(ptr, FRAME_SIZE)
    };
    if let Err(e) = fill(buf) {
        // SAFETY: just allocated (refcount 1), never mapped or shared.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Err(e);
    }
    Ok(new_frame)
}

/// Copy `dest.len()` bytes starting at byte offset `file_offset` of `file` out
/// of the page cache, filling any missing 16 KiB pages via `fill_page`.
///
/// This is the **`read(2)` data path** for the page-cache-primary model
/// (design-decisions §38): a regular file's data is served from the single
/// shared cache frame, exactly as the mmap fault path serves it — so `read(2)`
/// and `mmap` share one copy with no separate read-side invalidation.
///
/// `file_offset` and `dest.len()` are arbitrary (no alignment requirement); the
/// range is split into the covering 16 KiB pages internally.  For each covered
/// page, `fill_page(page_file_off, page_buf)` is invoked **only on a cache
/// miss**, where `page_file_off` is the frame-aligned start of that page and
/// `page_buf` is the full, already-zeroed `FRAME_SIZE`-byte page buffer (a short
/// fill past EOF leaves the tail zero, matching demand-paging semantics).
///
/// The caller is responsible for clamping `dest.len()` to the bytes that
/// actually exist in the file (this routine zero-extends past EOF, like the
/// fault path, and does not itself know the file size).
///
/// Each page's caller reference is dropped here (the bytes are copied out, never
/// mapped), so no reference leaks.
///
/// # Errors
///
/// - [`KernelError::InternalError`] if the HHDM is unavailable or an internal
///   offset computation overflows.
/// - Whatever `fill_page` returns on a read error (propagated from the failing
///   page; the just-allocated frame is freed inside [`get_or_fill`]).
/// - Frame-allocation / reference-bump errors from [`get_or_fill`].
pub fn read_through<F>(
    file: FileId,
    file_offset: u64,
    dest: &mut [u8],
    fill_page: F,
) -> KernelResult<()>
where
    F: Fn(u64, &mut [u8]) -> KernelResult<()>,
{
    if dest.is_empty() {
        return Ok(());
    }
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    // FRAME_SIZE is a power of two, so `& !mask` floors to the page base and
    // `& mask` gives the in-page offset without arithmetic-side-effect ops.
    let mask = (FRAME_SIZE as u64).wrapping_sub(1);

    let mut copied: usize = 0;
    while copied < dest.len() {
        let cur = file_offset
            .checked_add(copied as u64)
            .ok_or(KernelError::InternalError)?;
        let page_off = cur & !mask;
        let in_page = (cur & mask) as usize;
        let remaining = dest.len().saturating_sub(copied);
        let avail_in_page = FRAME_SIZE.saturating_sub(in_page);
        let n = remaining.min(avail_in_page);
        if n == 0 {
            break;
        }

        // Obtain the page (fill on miss); one caller reference is returned.
        let frame = get_or_fill(file, page_off, |buf| fill_page(page_off, buf))?;

        // Copy [in_page, in_page + n) out of the frame's HHDM alias.
        // SAFETY: `frame` is a live cache frame on which we hold one reference,
        // so it cannot be freed under us; `to_virt(hhdm)` is its valid HHDM alias
        // spanning exactly FRAME_SIZE bytes, and `in_page + n <= FRAME_SIZE` by
        // construction (n <= avail_in_page = FRAME_SIZE - in_page).  `dest` is a
        // distinct caller buffer, so source and destination do not overlap.
        unsafe {
            let src = (frame.to_virt(hhdm) as *const u8).add(in_page);
            let dst = dest.as_mut_ptr().add(copied);
            core::ptr::copy_nonoverlapping(src, dst, n);
        }
        // Drop the caller reference (the page was copied out, never mapped).
        release(frame);

        copied = copied.checked_add(n).ok_or(KernelError::InternalError)?;
    }
    Ok(())
}

/// Drop one caller reference obtained from [`get_or_fill`] / [`lookup`] that was
/// **never mapped** (e.g. the fault path failed to install the PTE).
///
/// This decrements the frame's refcount via `free_frame`; if the cache entry and
/// all other mappers still hold references the frame survives, otherwise it is
/// returned to the allocator.  Do **not** call this for a reference you handed to
/// `map_frame` — that one is owned by the page table and freed on teardown.
pub fn release(frame: PhysFrame) {
    STAT_RELEASES.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `frame` came from get_or_fill/lookup, which added a reference for
    // the caller; dropping exactly that reference is balanced.
    let _ = unsafe { frame::free_frame(frame) };
}

/// Remove a page from the cache index and drop the cache's single reference.
///
/// Returns `true` if the page was resident.  Used for **coherence** (the file
/// changed): the page is always removed from the index so future mappers do not
/// see stale bytes.  If live mappers still reference the frame it survives for
/// them (their references keep it alive); otherwise it is freed.
pub fn invalidate(file: FileId, file_offset: u64) -> bool {
    let key = match key_for(file, file_offset) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let mut cache = PAGE_CACHE.lock();
    if let Some(mut entry) = cache.pages.remove(&key) {
        entry.set_dirty(false);
        let frame = entry.frame;
        RESIDENT_COUNT.fetch_sub(1, Ordering::Relaxed);
        drop(cache);
        STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: removing the index entry drops the cache's one reference;
        // free_frame decrements it.  Any live mappers keep the frame alive.
        let _ = unsafe { frame::free_frame(frame) };
        return true;
    }
    false
}

/// Evict a page **only if the cache holds the sole reference** (no live mappers).
///
/// Returns `true` if it was evicted.  This is the reclaim primitive: it never
/// reduces dedup for an actively-shared page (a page with `refcount > 1` is
/// left resident).
pub fn try_evict(file: FileId, file_offset: u64) -> bool {
    let key = match key_for(file, file_offset) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.pages.get(&key) {
        let frame = entry.frame;
        // refcount() takes the allocator lock; cache → allocator ordering holds.
        // A dirty page holds bytes the file lacks: never evicted.
        if !entry.dirty && frame::refcount(frame) <= 1 {
            cache.pages.remove(&key);
            RESIDENT_COUNT.fetch_sub(1, Ordering::Relaxed);
            drop(cache);
            STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: refcount ≤ 1 ⇒ only the cache references this frame, so
            // dropping that reference frees it with no live mapping left behind.
            let _ = unsafe { frame::free_frame(frame) };
            return true;
        }
    }
    false
}

/// Invalidate every cached page of a file, dropping the cache's reference on
/// each.
///
/// Returns the number of pages removed.  Frames with live mappers survive for
/// them; unreferenced frames are freed. For coherence after a change, use
/// [`file_changed`], which leaves a shared-mapped file's pages in place.
pub fn invalidate_file(file: FileId) -> usize {
    let mut frames = alloc::vec::Vec::new();
    PAGE_CACHE.lock().take_file_pages(file, &mut frames);
    RESIDENT_COUNT.fetch_sub(frames.len(), Ordering::Relaxed);
    let removed = frames.len();
    drop_cache_references(frames);
    removed
}

/// Drop the cache's own reference on each of `frames`, taken out of the index.
fn drop_cache_references(frames: alloc::vec::Vec<PhysFrame>) {
    for f in frames {
        STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: dropping the cache's one reference per entry; live mappers (if
        // any) keep their frame alive, otherwise it is freed.
        let _ = unsafe { frame::free_frame(f) };
    }
}

/// Is anything at all currently cached, or a fill in flight, or a shared
/// mapping counted?
///
/// Two relaxed atomic loads — the cheap gate for the coherence hook on the
/// write/truncate/unlink paths ([`file_changed`]). When the cache is empty
/// (the common case for a system with no active file mappings) callers skip
/// the cost of resolving a [`FileId`]. Racy by construction, which is fine: a
/// fill that starts after a change reads the changed file, and one that
/// started before is counted ([`FileState::fills`]) by then.
#[must_use]
pub fn is_populated() -> bool {
    RESIDENT_COUNT.load(Ordering::Relaxed) != 0 || TRACKED_FILES.load(Ordering::Relaxed) != 0
}

/// One more shared mapping of `file` maps its cache frames: a VMA made, or a
/// piece of one a split leaves, or a fork's copy. Until the matching
/// [`shared_map_removed`], changes to the file update its cached pages in
/// place ([`file_changed`]).
///
/// `writer` is the mapping's handle when it may write the file -- the
/// mapping is, or may be made, writable. The cache then keeps a reference
/// of its own on it, if it has no writer for the file yet, to save the
/// file's dirty pages through ([`writeback`]).
pub fn shared_map_added(file: FileId, writer: Option<u64>) {
    let wants_writer = {
        let mut cache = PAGE_CACHE.lock();
        let state = cache.file_mut(file);
        state.shared_maps = state.shared_maps.saturating_add(1);
        if writer.is_some() {
            state.writable_maps = state.writable_maps.saturating_add(1);
        }
        writer.is_some() && state.writer.is_none()
    };
    let Some(handle) = writer.filter(|_| wants_writer) else {
        return;
    };
    // The open-file table is not taken under the cache's lock (a leaf).
    if crate::fs::handle::dup_shared(handle).is_err() {
        return;
    }
    let spare = {
        let mut cache = PAGE_CACHE.lock();
        let state = cache.file_mut(file);
        if state.writer.is_none() {
            state.writer = Some(handle);
            None
        } else {
            Some(handle)
        }
    };
    if let Some(extra) = spare {
        let _ = crate::fs::handle::close(extra);
    }
}

/// One shared mapping of `file` is gone (see [`shared_map_added`]);
/// `writable` as it was added. When it was the last that may write, the
/// file's dirty pages are saved ([`writeback`]) and the cache's writer let go.
pub fn shared_map_removed(file: FileId, writable: bool) {
    let last_writer = {
        let mut cache = PAGE_CACHE.lock();
        let last = match cache.files.get_mut(&file) {
            Some(state) => {
                state.shared_maps = state.shared_maps.saturating_sub(1);
                if writable {
                    state.writable_maps = state.writable_maps.saturating_sub(1);
                }
                state.writable_maps == 0 && state.writer.is_some()
            }
            None => false,
        };
        cache.forget_if_idle(file);
        last
    };
    if !last_writer {
        return;
    }
    // A failure leaves the pages dirty, and the writer kept for the next
    // writeback (a sync, a later mapping's last unmap): nothing is dropped.
    if writeback(file, 0, u64::MAX).is_err() {
        return;
    }
    let done = {
        let mut cache = PAGE_CACHE.lock();
        let still_clean = !file_has_dirty(&cache, file);
        let writer = match cache.files.get_mut(&file) {
            Some(state) if state.writable_maps == 0 && still_clean => state.writer.take(),
            _ => None,
        };
        cache.forget_if_idle(file);
        writer
    };
    if let Some(handle) = done {
        let _ = crate::fs::handle::close(handle);
    }
}

/// Whether any page of `file` in the cache is dirty.
fn file_has_dirty(cache: &CacheState, file: FileId) -> bool {
    let lo = PageKey {
        file,
        page_index: 0,
    };
    let hi = PageKey {
        file,
        page_index: u64::MAX,
    };
    cache.pages.range(lo..=hi).any(|(_, e)| e.dirty)
}

/// A writable shared mapping is about to write the page of `file` at
/// `page_offset` (16 KiB-aligned), which it maps from `frame`: mark it dirty,
/// to be saved ([`writeback`]). False, and the write is not to be let
/// through, when the cache no longer holds that frame for that page -- the
/// mapping maps a page the file has dropped -- or holds no writer for the
/// file, which nothing could then save the page through.
#[must_use]
pub fn mark_dirty(file: FileId, page_offset: u64, frame: PhysFrame) -> bool {
    let Ok(key) = key_for(file, page_offset) else {
        return false;
    };
    let mut cache = PAGE_CACHE.lock();
    if cache.files.get(&file).is_none_or(|st| st.writer.is_none()) {
        return false;
    }
    match cache.pages.get_mut(&key) {
        Some(entry) if entry.frame == frame => {
            entry.set_dirty(true);
            true
        }
        _ => false,
    }
}

/// Save the dirty pages of `file` from byte `from` to byte `to` (exclusive;
/// `u64::MAX` for the rest of it) to the file, through the cache's writer
/// for it (design-decisions §1572). For each page: the dirty mark is cleared,
/// write permission taken back from every mapping of it
/// (`pcb::write_protect_file_page`, which flushes the TLBs), then its bytes
/// copied out -- up to the file's end, never past it -- and written. A store
/// after the copy faults and marks the page dirty again; one before it is in
/// the copy. Nothing to do, and `Ok`, for a file with no writer.
///
/// Called with no lock held: it takes the process table, address spaces'
/// page-table locks and the filesystem's.
///
/// # Errors
///
/// The filesystem's write error; the page is marked dirty again, and the
/// rest left for the next writeback.
pub fn writeback(file: FileId, from: u64, to: u64) -> KernelResult<()> {
    let page = FRAME_SIZE as u64;
    let first = from.checked_div(page).unwrap_or(0);
    let last = to.saturating_sub(1).checked_div(page).unwrap_or(0);
    let Some(hhdm) = page_table::hhdm() else {
        return Ok(());
    };
    // The writer, a reference of this call's own so a last unmap cannot
    // close it under the writes.
    let writer = {
        let cache = PAGE_CACHE.lock();
        cache.files.get(&file).and_then(|st| st.writer)
    };
    let Some(writer) = writer else {
        return Ok(());
    };
    crate::fs::handle::dup_shared(writer)?;
    let result = (|| {
        let mut buf = crate::mm::user::alloc_zeroed_vec(FRAME_SIZE)?;
        let mut next = first;
        while next <= last {
            // The next dirty page at or after `next`: its mark cleared, a
            // reference taken on its frame.
            let found = {
                let mut cache = PAGE_CACHE.lock();
                let lo = PageKey {
                    file,
                    page_index: next,
                };
                let hi = PageKey {
                    file,
                    page_index: last,
                };
                let hit = cache
                    .pages
                    .range_mut(lo..=hi)
                    .find(|(_, e)| e.dirty)
                    .map(|(k, e)| {
                        e.set_dirty(false);
                        (k.page_index, e.frame)
                    });
                match hit {
                    // SAFETY: resident entry ⇒ allocated frame; cache lock held.
                    Some((index, frame)) if unsafe { frame::ref_inc(frame) }.is_ok() => {
                        Some((index, frame))
                    }
                    Some((index, _)) => {
                        mark_dirty_again(&mut cache, file, index);
                        None
                    }
                    None => None,
                }
            };
            let Some((index, frame)) = found else {
                break;
            };
            let offset = index.saturating_mul(page);
            crate::proc::pcb::write_protect_file_page(file, offset, frame);
            let saved = (|| {
                let size = crate::fs::handle::fstat(writer)?.size;
                let len = usize::try_from(size.saturating_sub(offset))
                    .unwrap_or(usize::MAX)
                    .min(FRAME_SIZE);
                if len == 0 {
                    return Ok(());
                }
                let out = buf.get_mut(..len).ok_or(KernelError::InternalError)?;
                // SAFETY: a frame we hold a reference on; its HHDM alias is
                // FRAME_SIZE bytes and `len <= FRAME_SIZE`.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        frame.to_virt(hhdm) as *const u8,
                        out.as_mut_ptr(),
                        len,
                    );
                }
                crate::fs::handle::write_back_at(writer, offset, out)
            })();
            release(frame);
            if let Err(e) = saved {
                mark_dirty_again(&mut PAGE_CACHE.lock(), file, index);
                return Err(e);
            }
            STAT_WRITTEN_BACK.fetch_add(1, Ordering::Relaxed);
            next = index.saturating_add(1);
        }
        Ok(())
    })();
    let _ = crate::fs::handle::close(writer);
    result
}

/// Put back the dirty mark of page `index` of `file`, whose save failed.
fn mark_dirty_again(cache: &mut CacheState, file: FileId, index: u64) {
    let key = PageKey {
        file,
        page_index: index,
    };
    if let Some(entry) = cache.pages.get_mut(&key) {
        entry.set_dirty(true);
    }
}

/// Save every file's dirty pages ([`writeback`]): `sync(2)`. The first error
/// is answered; every file is tried.
///
/// # Errors
///
/// A filesystem's write error.
pub fn writeback_all() -> KernelResult<()> {
    let files: alloc::vec::Vec<FileId> = PAGE_CACHE
        .lock()
        .files
        .iter()
        .filter(|(_, st)| st.writer.is_some())
        .map(|(f, _)| *f)
        .collect();
    let mut first_error = Ok(());
    for file in files {
        if let Err(e) = writeback(file, 0, u64::MAX)
            && first_error.is_ok()
        {
            first_error = Err(e);
        }
    }
    first_error
}

/// How many cached pages are dirty now: `/proc/meminfo`'s `Dirty:`, in
/// frames.
#[must_use]
pub fn dirty_pages() -> usize {
    DIRTY_COUNT.load(Ordering::Relaxed)
}

/// Whether `file` has a shared mapping ([`shared_map_added`]).
#[must_use]
pub fn has_shared_maps(file: FileId) -> bool {
    PAGE_CACHE
        .lock()
        .files
        .get(&file)
        .is_some_and(|st| st.shared_maps > 0)
}

/// How many files have a shared mapping now: what a test compares before and
/// after a program that maps files shared, since a count a VMA took and
/// never gave back would keep its file's pages in place for good.
#[must_use]
pub fn shared_mapped_files() -> usize {
    PAGE_CACHE
        .lock()
        .files
        .values()
        .filter(|st| st.shared_maps > 0)
        .count()
}

/// How many files the cache keeps state about ([`FileState`]): what a test
/// compares before and after a program that maps files shared, since state
/// never dropped -- a mapping's count, a writer -- keeps a file's pages in
/// place, and a writer keeps the file open, for good.
#[must_use]
pub fn tracked_files() -> usize {
    PAGE_CACHE.lock().files.len()
}

/// How a file's bytes changed, for [`file_changed`].
#[derive(Clone, Copy, Debug)]
pub enum FileChange<'a> {
    /// `data` was written at byte `offset`.
    Written {
        /// Where the write began.
        offset: u64,
        /// What it wrote.
        data: &'a [u8],
    },
    /// The file was cut, or extended, to `size` bytes.
    Resized {
        /// Its length now.
        size: u64,
    },
    /// The whole file now holds `data` and nothing more.
    Replaced {
        /// Its contents now.
        data: &'a [u8],
    },
    /// A name of the file went -- an unlink, a rename over it -- while
    /// something may still hold it, which keeps its number its own.
    Unlinked,
    /// Its cached bytes are to be dropped whatever maps them: the file is
    /// gone for good and its number may be another file's, or it stopped
    /// being a file whose bytes the cache may hold (a swap file).
    Dropped,
}

/// What a caller of [`file_changed`] has left to do once it has let the
/// filesystem's lock go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum AfterChange {
    /// Nothing.
    Done,
    /// Unmap every shared mapping of the file from byte `from` on
    /// (`proc::pcb::unmap_file_from`): the file now ends before it, and an
    /// access there is `SIGBUS`. `from` is the file's length rounded up to a
    /// 4 KiB page, which is what a mapping maps.
    UnmapFrom {
        /// The file.
        file: FileId,
        /// The first byte no longer in the file, rounded up to 4 KiB.
        from: u64,
    },
}

/// Coherence: file `(fs_id, ino)` changed as `change` says. Called by the VFS
/// under the filesystem's lock, after the change: the lock puts every change
/// to the file, and so every in-place update below, in the filesystem's order.
///
/// A file with no shared mapping, and no writer kept to save dirty pages
/// through, has its cached pages dropped, as they no longer say what the file
/// holds. One with either keeps them --
/// the mappings map those frames, and a dropped page would stay mapped,
/// showing old bytes -- and they are changed to match: a write's bytes are
/// copied into each cached page it covers; a resize drops the pages wholly
/// past the new end and zeroes the rest of the last one; an unlink leaves
/// them, the mappings' holds keeping the file. Either way the file's change
/// count moves, so a fill reading the file meanwhile reads again.
///
/// A no-op for `ino == 0` (no stable identity: never cached) and, at the cost
/// of two atomic loads, when nothing is cached.
pub fn file_changed(fs_id: u64, ino: u64, change: FileChange<'_>) -> AfterChange {
    if ino == 0 || !is_populated() {
        return AfterChange::Done;
    }
    let file = FileId { fs_id, ino };
    let mut dropped = alloc::vec::Vec::new();
    // In-place changes, each a frame with a reference of ours on it, the
    // range of it to change, and the bytes to put there (`None`: zeros).
    let mut updates: alloc::vec::Vec<(PhysFrame, usize, usize, Option<&[u8]>)> =
        alloc::vec::Vec::new();
    let mut after = AfterChange::Done;
    {
        let mut cache = PAGE_CACHE.lock();
        let (shared, writable) = match cache.files.get_mut(&file) {
            Some(state) => {
                state.changes = state.changes.wrapping_add(1);
                // A writer outliving the last writable mapping is one whose
                // save failed: the pages it left dirty are kept, and changed
                // in place, as a mapped file's are, for the next writeback
                // to save.
                let pending = state.writer.is_some();
                (
                    state.shared_maps > 0 || pending,
                    state.writable_maps > 0 || pending,
                )
            }
            None => (false, false),
        };
        match change {
            FileChange::Dropped => cache.take_file_pages(file, &mut dropped),
            _ if !shared => cache.take_file_pages(file, &mut dropped),
            FileChange::Unlinked => {}
            FileChange::Written { offset, data } => {
                collect_writes(&mut cache, file, offset, data, writable, &mut updates);
            }
            FileChange::Resized { size } => {
                after = resize_in_place(&mut cache, file, size, &mut dropped, &mut updates);
            }
            FileChange::Replaced { data } => {
                after = resize_in_place(
                    &mut cache,
                    file,
                    data.len() as u64,
                    &mut dropped,
                    &mut updates,
                );
                collect_writes(&mut cache, file, 0, data, writable, &mut updates);
            }
        }
        RESIDENT_COUNT.fetch_sub(dropped.len(), Ordering::Relaxed);
    }
    if let Some(hhdm) = page_table::hhdm() {
        for &(frame, at, len, bytes) in &updates {
            // SAFETY: `frame` is a cache frame we hold a reference on, so it is
            // not freed under us; `to_virt(hhdm)` is its HHDM alias of
            // FRAME_SIZE bytes and `at + len <= FRAME_SIZE` by construction
            // (`collect_writes`, `resize_in_place`). A mapping may read or write
            // it meanwhile, as it may any shared page.
            unsafe {
                let dst = (frame.to_virt(hhdm) as *mut u8).add(at);
                match bytes {
                    Some(src) => core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len),
                    None => core::ptr::write_bytes(dst, 0, len),
                }
            }
        }
    }
    STAT_UPDATED_IN_PLACE.fetch_add(updates.len() as u64, Ordering::Relaxed);
    for (frame, ..) in updates {
        release(frame);
    }
    drop_cache_references(dropped);
    after
}

/// For a write of `data` at `offset` to `file`: each resident page it
/// covers, with a reference taken on it, the range of the page it covers,
/// and those bytes of `data`. Pages not resident are skipped: a fill reads
/// the file, which holds the write already.
///
/// With `dirty` (the file has a writable mapping) each page is marked dirty
/// too: a writeback that copied the page before this write's bytes landed
/// in it may write after the filesystem's write, putting the old bytes
/// back, and the mark has the page saved again.
fn collect_writes<'a>(
    cache: &mut CacheState,
    file: FileId,
    offset: u64,
    data: &'a [u8],
    dirty: bool,
    updates: &mut alloc::vec::Vec<(PhysFrame, usize, usize, Option<&'a [u8]>)>,
) {
    let size = FRAME_SIZE as u64;
    let mut done = 0usize;
    while done < data.len() {
        let Some(at) = offset.checked_add(done as u64) else {
            return;
        };
        let in_page = (at & size.wrapping_sub(1)) as usize;
        let len = (FRAME_SIZE.saturating_sub(in_page)).min(data.len().saturating_sub(done));
        let key = PageKey {
            file,
            page_index: at.checked_div(size).unwrap_or(0),
        };
        if let (Some(entry), Some(bytes)) = (
            cache.pages.get_mut(&key),
            data.get(done..done.saturating_add(len)),
        )
            // SAFETY: resident entry ⇒ allocated frame; cache lock held.
            && unsafe { frame::ref_inc(entry.frame) }.is_ok()
        {
            if dirty {
                entry.set_dirty(true);
            }
            updates.push((entry.frame, in_page, len, Some(bytes)));
        }
        done = done.saturating_add(len);
    }
}

/// For `file`, now `size` bytes long: take its pages wholly past the end out
/// of the index (into `dropped`), and note the rest of the last page past
/// the end, if that page is resident, to be zeroed (into `updates`) -- a
/// file cut short reads zeros past its end, and one extended reads zeros
/// where it grew, not what a mapping once stored past the old end. Answers
/// the unmapping the caller owes the shared mappings.
fn resize_in_place(
    cache: &mut CacheState,
    file: FileId,
    size: u64,
    dropped: &mut alloc::vec::Vec<PhysFrame>,
    updates: &mut alloc::vec::Vec<(PhysFrame, usize, usize, Option<&[u8]>)>,
) -> AfterChange {
    let page = FRAME_SIZE as u64;
    let first_past = size.div_ceil(page);
    cache.take_pages_from(file, first_past, dropped);
    let tail = (size & page.wrapping_sub(1)) as usize;
    if tail != 0 {
        let key = PageKey {
            file,
            page_index: size.checked_div(page).unwrap_or(0),
        };
        if let Some(entry) = cache.pages.get(&key)
            // SAFETY: resident entry ⇒ allocated frame; cache lock held.
            && unsafe { frame::ref_inc(entry.frame) }.is_ok()
        {
            updates.push((entry.frame, tail, FRAME_SIZE.saturating_sub(tail), None));
        }
    }
    let hw = crate::mm::page_table::HW_PAGE_SIZE as u64;
    AfterChange::UnmapFrom {
        file,
        from: size.div_ceil(hw).saturating_mul(hw),
    }
}

/// Memory-pressure shrinker: evict **idle** cached pages (those with no live
/// mapper, i.e. `refcount <= 1`) proportional to the pressure level.
///
/// Registered with [`crate::mm::pressure`] by [`init`].  Actively-shared pages
/// (`refcount > 1`) are never evicted — dropping them would not free the frame
/// anyway (a mapper still holds it) and would only force a re-fill on the next
/// fault.  Returns the number of pages evicted.
///
/// Holding the cache lock across the per-entry `frame::refcount` probe is the
/// established cache → allocator ordering; pressure events are rare, so the
/// `O(n)` sweep under the lock is acceptable.
pub fn shrink(level: crate::mm::pressure::PressureLevel) -> usize {
    use crate::mm::pressure::PressureLevel;
    let pct: usize = match level {
        PressureLevel::None => return 0,
        PressureLevel::Low => 25,
        PressureLevel::Medium => 50,
        PressureLevel::Critical => 90,
    };

    let mut frames = alloc::vec::Vec::new();
    {
        let mut cache = PAGE_CACHE.lock();
        let total = cache.pages.len();
        if total == 0 {
            return 0;
        }
        // Ceil(total * pct / 100); saturating/checked keep the arithmetic lints
        // satisfied (100 is a nonzero constant, so checked_div never fails).
        let target = total
            .saturating_mul(pct)
            .saturating_add(99)
            .checked_div(100)
            .unwrap_or(total);

        let mut victims = alloc::vec::Vec::new();
        for (k, entry) in cache.pages.iter() {
            if victims.len() >= target {
                break;
            }
            // refcount() takes the allocator lock; cache → allocator ordering.
            // A dirty page holds bytes the file lacks: never dropped here.
            if !entry.dirty && frame::refcount(entry.frame) <= 1 {
                victims.push(*k);
            }
        }
        for k in &victims {
            if let Some(entry) = cache.pages.remove(k) {
                frames.push(entry.frame);
            }
        }
        RESIDENT_COUNT.fetch_sub(frames.len(), Ordering::Relaxed);
    }

    let freed = frames.len();
    for f in frames {
        STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: each removed entry held the cache's sole reference (refcount
        // was ≤ 1 under the lock, and no get_or_fill/lookup can bump it without
        // first taking the cache lock we held); free_frame returns it.
        let _ = unsafe { frame::free_frame(f) };
    }
    freed
}

/// Register the page-cache shrinker with the memory-pressure subsystem.
///
/// Call once at boot (after `mm::pressure` is available) so the cache trims its
/// idle pages under memory pressure instead of growing without bound.
pub fn init() {
    let _ = crate::mm::pressure::register_shrinker("page_cache", shrink);
}

/// Test/diagnostic helper: is a page currently resident in the cache index?
#[must_use]
pub fn is_resident(file: FileId, file_offset: u64) -> bool {
    let Ok(key) = key_for(file, file_offset) else {
        return false;
    };
    PAGE_CACHE.lock().pages.contains_key(&key)
}

// ---------------------------------------------------------------------------
// Boot self-test
// ---------------------------------------------------------------------------

/// Boot self-test for the read-only page cache (design-decisions §23/§36/§37).
///
/// `#[cfg(test)]` unit tests do not run on the bare-metal target, so the cache
/// is exercised here against the real frame allocator during boot.  Validates
/// the frame-refcount sharing model: miss-fill + content, hit sharing + single
/// fill, per-caller reference bumps, distinct-key isolation, misaligned
/// rejection, refcount-aware eviction, forced invalidation, and reclaim — and
/// that no frame is leaked afterward.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on any failed assertion (logged with
/// a `FAIL:` line first); the wrapper in `main.rs` treats that as a non-fatal
/// self-test warning.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[page_cache] Running read-only page-cache self-test...");

    // Two synthetic, never-colliding file identities (fs_id 0 is unused by real
    // mounts, which start at 1, so these can't clash with a live cache entry).
    let file_a = FileId {
        fs_id: 0,
        ino: 0xC0DE_0001,
    };
    let file_b = FileId {
        fs_id: 0,
        ino: 0xC0DE_0002,
    };
    let file_c = FileId {
        fs_id: 0,
        ino: 0xC0DE_0003,
    };
    let file_d = FileId {
        fs_id: 0,
        ino: 0xC0DE_0004,
    };
    let file_e = FileId {
        fs_id: 0,
        ino: 0xC0DE_0005,
    };
    let file_f = FileId {
        fs_id: 0,
        ino: 0xC0DE_0006,
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;

    // Helper: read a byte from a cached frame via its HHDM alias.
    let read_byte = |frame: PhysFrame, idx: usize| -> u8 {
        // SAFETY: `frame` is a resident cache frame; `to_virt(hhdm)` is its valid
        // HHDM alias and `idx < FRAME_SIZE`.
        unsafe { core::ptr::read_volatile((frame.to_virt(hhdm) as *const u8).add(idx)) }
    };

    // Run the body, always cleaning up the cache entries we created.
    let result = (|| -> KernelResult<()> {
        // (1) Miss: allocates, fills, returns a frame with refcount 2
        //     (cache's reference + this caller's reference).
        let mut fill_calls = 0u32;
        let f0 = get_or_fill(file_a, 0, |buf| {
            fill_calls = fill_calls.saturating_add(1);
            if let [b0, b1, .., last] = buf {
                *b0 = 0xAB;
                *b1 = 0xCD;
                *last = 0xEF; // whole-page fill marker
            }
            Ok(())
        })?;
        if fill_calls != 1 {
            serial_println!("[page_cache]   FAIL: expected 1 fill, got {fill_calls}");
            return Err(KernelError::InternalError);
        }
        if frame::refcount(f0) != 2 {
            serial_println!("[page_cache]   FAIL: refcount after miss != 2 (cache+caller)");
            return Err(KernelError::InternalError);
        }
        if read_byte(f0, 0) != 0xAB || read_byte(f0, 1) != 0xCD {
            serial_println!("[page_cache]   FAIL: filled content mismatch");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   miss fills + content + refcount: OK");

        // (2) Hit: same frame, no new fill, refcount 3 (cache + two callers).
        let f1 = get_or_fill(file_a, 0, |_buf| {
            fill_calls = fill_calls.saturating_add(1); // must NOT run on a hit
            Ok(())
        })?;
        if fill_calls != 1 {
            serial_println!("[page_cache]   FAIL: hit re-filled the page");
            return Err(KernelError::InternalError);
        }
        if f1 != f0 {
            serial_println!("[page_cache]   FAIL: hit returned a different frame");
            return Err(KernelError::InternalError);
        }
        if frame::refcount(f0) != 3 {
            serial_println!("[page_cache]   FAIL: refcount after hit != 3");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   hit shares frame, single fill, refcount: OK");

        // (3) A different page index of the same file is a distinct frame.
        let f2 = get_or_fill(file_a, FRAME_SIZE as u64, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x11;
            }
            Ok(())
        })?;
        if f2 == f0 {
            serial_println!("[page_cache]   FAIL: distinct page shares a frame");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   distinct page → distinct frame: OK");

        // (4) Misaligned offset is rejected.
        if lookup(file_a, 1).is_ok() {
            serial_println!("[page_cache]   FAIL: misaligned offset accepted");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   misaligned offset rejected: OK");

        // (5) Release the two caller references on page 0 (3 → 1 = cache only),
        //     then try_evict reclaims it (no live mappers).
        release(f0); // 3 -> 2
        release(f1); // 2 -> 1 (cache only)
        if frame::refcount(f0) != 1 {
            serial_println!("[page_cache]   FAIL: refcount after releases != 1");
            return Err(KernelError::InternalError);
        }
        if !try_evict(file_a, 0) {
            serial_println!("[page_cache]   FAIL: try_evict(sole reference) did not evict");
            return Err(KernelError::InternalError);
        }
        if is_resident(file_a, 0) {
            serial_println!("[page_cache]   FAIL: page still resident after eviction");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   release + refcount-aware eviction: OK");

        // (6) try_evict refuses a page with a live mapper (caller reference held).
        let live = get_or_fill(file_b, 0, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x22;
            }
            Ok(())
        })?;
        if try_evict(file_b, 0) {
            serial_println!("[page_cache]   FAIL: try_evict evicted a referenced page");
            return Err(KernelError::InternalError);
        }
        // But forced invalidate (coherence) removes it from the index; the
        // caller's reference keeps the frame alive until released.
        if !invalidate(file_b, 0) {
            serial_println!("[page_cache]   FAIL: invalidate did not remove resident page");
            return Err(KernelError::InternalError);
        }
        if is_resident(file_b, 0) {
            serial_println!("[page_cache]   FAIL: page resident after invalidate");
            return Err(KernelError::InternalError);
        }
        if frame::refcount(live) != 1 {
            serial_println!("[page_cache]   FAIL: caller ref not preserved across invalidate");
            return Err(KernelError::InternalError);
        }
        release(live); // drop the last reference → frame freed
        serial_println!("[page_cache]   try_evict spares + invalidate forces: OK");

        // Clean up the still-resident distinct page (f2): drop the caller ref,
        // then invalidate_file reclaims the cache's reference.
        release(f2);
        let n_a = invalidate_file(file_a);
        if n_a != 1 {
            serial_println!("[page_cache]   FAIL: invalidate_file removed {n_a}, expected 1");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   invalidate_file reclaims idle pages: OK");

        // (8) is_populated tracks residency; file_changed (the VFS coherence
        //     hook) drops an unshared file's pages by (fs_id, ino).
        let fc = get_or_fill(file_c, 0, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x33;
            }
            Ok(())
        })?;
        if !is_populated() {
            serial_println!("[page_cache]   FAIL: is_populated false with a resident page");
            return Err(KernelError::InternalError);
        }
        release(fc); // drop caller ref → cache holds the sole reference
        let _ = file_changed(
            file_c.fs_id,
            file_c.ino,
            FileChange::Written {
                offset: 0,
                data: b"x",
            },
        );
        if is_resident(file_c, 0) {
            serial_println!("[page_cache]   FAIL: a write left an unshared file's page resident");
            return Err(KernelError::InternalError);
        }
        // ino == 0 is never cacheable; the hook must be a safe no-op for it.
        let _ = file_changed(0, 0, FileChange::Dropped);
        serial_println!("[page_cache]   is_populated + file_changed drops an unshared file: OK");

        // (9) shrinker evicts idle pages (no live mapper) but spares pages that
        //     still have a caller reference (a live mapping).
        let idle = get_or_fill(file_d, 0, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x44;
            }
            Ok(())
        })?;
        release(idle); // now cache holds the sole reference (idle)
        let live = get_or_fill(file_d, FRAME_SIZE as u64, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x55;
            }
            Ok(())
        })?; // caller reference retained ⇒ refcount 2 (a "mapped" page)
        let freed = shrink(crate::mm::pressure::PressureLevel::Critical);
        if freed == 0 {
            serial_println!("[page_cache]   FAIL: shrink freed nothing");
            return Err(KernelError::InternalError);
        }
        if is_resident(file_d, 0) {
            serial_println!("[page_cache]   FAIL: shrink left an idle page resident");
            return Err(KernelError::InternalError);
        }
        if !is_resident(file_d, FRAME_SIZE as u64) {
            serial_println!("[page_cache]   FAIL: shrink evicted a live (mapped) page");
            return Err(KernelError::InternalError);
        }
        release(live); // drop the caller ref
        let _ = invalidate_file(file_d);
        serial_println!("[page_cache]   shrink spares live, evicts idle: OK");

        shared_file_changes(file_e, hhdm)?;
        serial_println!(
            "[page_cache]   a shared-mapped file's pages change in place, and stop when unshared: OK"
        );
        fill_racing_a_change(file_f)?;
        serial_println!("[page_cache]   a fill that raced a change reads again: OK");

        Ok(())
    })();

    // Defensive cleanup: if the body bailed early, drop any synthetic entries so
    // they do not linger in the live cache.
    let _ = invalidate_file(file_a);
    let _ = invalidate_file(file_b);
    let _ = invalidate_file(file_c);
    let _ = invalidate_file(file_d);
    let _ = invalidate_file(file_e);
    let _ = invalidate_file(file_f);
    // A body that bailed between adding and removing its shared mapping.
    while has_shared_maps(file_e) {
        shared_map_removed(file_e, false);
    }

    result?;

    // The cache must be empty again and counters must reflect the run.
    let s = stats();
    if s.misses < 1 || s.hits < 1 {
        serial_println!(
            "[page_cache]   FAIL: counters not updated (hits={}, misses={})",
            s.hits,
            s.misses
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[page_cache]   stats: hits={} misses={} inserts={} races_lost={} releases={} evictions={} \
         refills={} updated_in_place={} written_back={} resident={}",
        s.hits,
        s.misses,
        s.inserts,
        s.races_lost,
        s.releases,
        s.evictions,
        s.refills,
        s.updated_in_place,
        s.written_back,
        s.resident
    );

    serial_println!("[page_cache] Read-only page-cache self-test PASSED");
    Ok(())
}

/// For [`self_test`]: report a failed check.
fn test_fail(what: &str) -> KernelResult<()> {
    crate::serial_println!("[page_cache]   FAIL: {}", what);
    Err(KernelError::InternalError)
}

/// For [`self_test`]: a shared-mapped file's cached pages follow its changes
/// in place -- a write's bytes copied in across a page boundary, a cut
/// dropping the pages wholly past the new end and zeroing the rest of the
/// last, an unlink leaving them -- and once its last shared mapping is gone,
/// a change drops them again. A caller reference on each page stands in for
/// a mapping's.
fn shared_file_changes(file: FileId, hhdm: u64) -> KernelResult<()> {
    let byte = |frame: PhysFrame, idx: usize| -> u8 {
        // SAFETY: `frame` is a frame we hold a reference on, `to_virt(hhdm)`
        // its HHDM alias, and every caller's `idx` is below FRAME_SIZE.
        unsafe { core::ptr::read_volatile((frame.to_virt(hhdm) as *const u8).add(idx)) }
    };
    let page = FRAME_SIZE as u64;
    let last = FRAME_SIZE.saturating_sub(1);
    shared_map_added(file, None);
    let mut frames: [Option<PhysFrame>; 3] = [None; 3];
    let mut result = Ok(());
    for (i, slot) in frames.iter_mut().enumerate() {
        let fill_byte = 0xA0u8.saturating_add(i as u8);
        match get_or_fill(file, (i as u64).saturating_mul(page), |buf| {
            buf.fill(fill_byte);
            Ok(())
        }) {
            Ok(f) => *slot = Some(f),
            Err(e) => result = Err(e),
        }
    }
    if let (true, [Some(f0), Some(f1), Some(_)]) = (result.is_ok(), frames) {
        result = (|| {
            let after = file_changed(
                file.fs_id,
                file.ino,
                FileChange::Written {
                    offset: page.saturating_sub(2),
                    data: b"wxyz",
                },
            );
            let in_place = after == AfterChange::Done
                && [byte(f0, last.saturating_sub(1)), byte(f0, last)] == *b"wx"
                && [byte(f1, 0), byte(f1, 1), byte(f1, 2)] == [b'y', b'z', 0xA1]
                && lookup(file, 0).ok().flatten().inspect(|f| release(*f)) == Some(f0);
            if !in_place {
                return test_fail(
                    "a write to a shared-mapped file did not land in its cached pages",
                );
            }
            let after = file_changed(
                file.fs_id,
                file.ino,
                FileChange::Resized {
                    size: page.saturating_add(10),
                },
            );
            let cut = after
                == AfterChange::UnmapFrom {
                    file,
                    from: page.saturating_add(4096),
                }
                && !is_resident(file, page.saturating_mul(2))
                && is_resident(file, page)
                && byte(f1, 9) == 0xA1
                && byte(f1, 10) == 0
                && byte(f1, last) == 0;
            if !cut {
                return test_fail(
                    "a cut of a shared-mapped file did not drop, zero and ask to unmap",
                );
            }
            let after = file_changed(file.fs_id, file.ino, FileChange::Unlinked);
            if after != AfterChange::Done || !is_resident(file, 0) {
                return test_fail("an unlink dropped a shared-mapped file's pages");
            }
            shared_map_removed(file, false);
            let _ = file_changed(
                file.fs_id,
                file.ino,
                FileChange::Written {
                    offset: 0,
                    data: b"q",
                },
            );
            if is_resident(file, 0) || is_resident(file, page) || byte(f0, 0) != 0xA0 {
                return test_fail(
                    "an unshared file's write did not drop its pages, or changed them",
                );
            }
            Ok(())
        })();
    }
    for f in frames.into_iter().flatten() {
        release(f);
    }
    result
}

/// For [`self_test`]: a fill during which the file changes reads it again,
/// and what it inserts is the second read.
fn fill_racing_a_change(file: FileId) -> KernelResult<()> {
    let mut calls = 0u32;
    let frame = get_or_fill(file, 0, |buf| {
        calls = calls.saturating_add(1);
        let first = calls == 1;
        if first {
            // A write landing while this fill reads the file.
            let _ = file_changed(
                file.fs_id,
                file.ino,
                FileChange::Written {
                    offset: 0,
                    data: b"n",
                },
            );
        }
        if let Some(b) = buf.first_mut() {
            *b = if first { b'o' } else { b'n' };
        }
        Ok(())
    })?;
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    // SAFETY: a frame we hold a reference on; byte 0 of its HHDM alias.
    let got = unsafe { core::ptr::read_volatile(frame.to_virt(hhdm) as *const u8) };
    release(frame);
    let _ = invalidate_file(file);
    if calls != 2 || got != b'n' {
        return test_fail("a fill that raced a change kept what it read before the change");
    }
    Ok(())
}

/// Dirty pages and their writeback (design-decisions §1572, step 2), on a
/// real file in `/tmp`, a caller reference on a page standing in for a
/// mapping's: a page is marked dirty only through the frame the cache holds
/// for it; a dirty page is neither evicted nor shrunk away; a writeback saves
/// the range asked for and no more, and nothing past the file's end; a write
/// to the file while a mapping may write it marks the page it lands in dirty;
/// and the last writable mapping's going saves the rest and lets the cache's
/// writer go. Needs the VFS, so it runs with the ring-3 tests, not with
/// [`self_test`].
///
/// # Errors
///
/// `InternalError` for a failed check; the VFS's error when the file cannot
/// be made or read.
pub fn writeback_self_test() -> KernelResult<()> {
    const PATH: &str = "/tmp/page-cache-writeback";
    let size = FRAME_SIZE.saturating_mul(2).saturating_add(100);
    crate::serial_println!("[page_cache] Running dirty-page writeback self-test...");
    crate::fs::Vfs::write_file(PATH, &alloc::vec![b'a'; size])?;
    let result = writeback_checks(PATH, size);
    let removed = crate::fs::Vfs::remove(PATH);
    result?;
    removed?;
    crate::serial_println!("[page_cache] Dirty-page writeback self-test PASSED");
    Ok(())
}

/// For [`writeback_self_test`]: the checks, on `path`, `size` bytes of `a`.
fn writeback_checks(path: &str, size: usize) -> KernelResult<()> {
    use crate::fs::handle::{self, OpenFlags};

    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let page = FRAME_SIZE as u64;
    let file = crate::fs::Vfs::file_identity(path)?.ok_or(KernelError::InternalError)?;
    let tracked_before = tracked_files();
    let dirty_before = dirty_pages();
    let writer = handle::open(path, OpenFlags::READ.union(OpenFlags::WRITE))?;
    shared_map_added(file, Some(writer));
    // The cache took a reference of its own on the handle.
    let _ = handle::close(writer);
    let mut mapped = true;
    let mut held: [Option<PhysFrame>; 3] = [None; 3];
    // Store `bytes` at `at` in `frame`, as a mapping would.
    let poke = |frame: PhysFrame, at: usize, bytes: &[u8]| {
        if at.saturating_add(bytes.len()) <= FRAME_SIZE {
            // SAFETY: a frame we hold a reference on; its HHDM alias is
            // FRAME_SIZE bytes and the range is checked to be inside it.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    (frame.to_virt(hhdm) as *mut u8).add(at),
                    bytes.len(),
                );
            }
        }
    };
    // The file's bytes on the filesystem, read past the cache.
    let disk = || crate::fs::Vfs::read_at_uncached(path, 0, size.saturating_add(16));
    let byte = |d: &[u8], at: u64| usize::try_from(at).ok().and_then(|i| d.get(i).copied());
    let result = (|| {
        // The pages, filled by a read through the cache.
        let _ = crate::fs::Vfs::read_file(path)?;
        for (i, slot) in held.iter_mut().enumerate() {
            *slot = lookup(file, (i as u64).saturating_mul(page))?;
        }
        let [Some(f0), Some(_), Some(f2)] = held else {
            return test_fail("a read through the cache did not leave the file's pages in it");
        };

        if mark_dirty(file, page, f0) {
            return test_fail("a page was marked dirty through a frame that is not its own");
        }
        poke(f0, 5, b"X");
        poke(f2, 99, b"Y");
        poke(f2, 100, b"ZZZZ");
        if !mark_dirty(file, 0, f0) || !mark_dirty(file, page.saturating_mul(2), f2) {
            return test_fail("a page was not marked dirty through its own frame");
        }
        if dirty_pages() != dirty_before.saturating_add(2) {
            return test_fail("two pages marked dirty are not counted dirty");
        }

        // Only the cache holds pages 0 and 2 now.
        for slot in [0, 2] {
            if let Some(f) = held.get_mut(slot).and_then(Option::take) {
                release(f);
            }
        }
        if try_evict(file, 0) {
            return test_fail("a dirty page was evicted");
        }
        let _ = shrink(crate::mm::pressure::PressureLevel::Critical);
        if !is_resident(file, 0) || !is_resident(file, page.saturating_mul(2)) {
            return test_fail("the shrinker dropped a dirty page");
        }

        writeback(file, page.saturating_mul(2), u64::MAX)?;
        let d = disk()?;
        if d.len() != size
            || byte(&d, page.saturating_mul(2).saturating_add(99)) != Some(b'Y')
            || byte(&d, 5) != Some(b'a')
            || dirty_pages() != dirty_before.saturating_add(1)
        {
            return test_fail(
                "a writeback of the last page did not save it alone, or wrote past the end",
            );
        }

        crate::fs::Vfs::write_at(path, page.saturating_add(7), b"W")?;
        if dirty_pages() != dirty_before.saturating_add(2) {
            return test_fail(
                "a write to a file a mapping may write did not mark its cached page dirty",
            );
        }

        mapped = false;
        shared_map_removed(file, true);
        let d = disk()?;
        if d.len() != size
            || byte(&d, 5) != Some(b'X')
            || byte(&d, page.saturating_add(7)) != Some(b'W')
            || dirty_pages() != dirty_before
            || tracked_files() != tracked_before
        {
            return test_fail(
                "the last writable mapping's going did not save every page, or kept the writer",
            );
        }
        Ok(())
    })();
    for f in held.into_iter().flatten() {
        release(f);
    }
    if mapped {
        shared_map_removed(file, true);
    }
    let _ = invalidate_file(file);
    result
}
