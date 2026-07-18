//! Read-only file page cache ("C-lite") — shared file pages via the frame
//! allocator's per-frame refcount.
//!
//! This module is the storage core of the C-lite read-only page cache decided
//! in design-decisions §23/§36.  Its job is to cache a file's pages **once** and
//! share them — *read-only* — across every process that maps or reads them, so
//! that:
//!
//! 1. **Shared-library `.text` is deduplicated.** N processes that map the same
//!    `.so` share one set of physical frames instead of N private copies.
//! 2. **The block buffer cache and per-mapping copies stop double-caching.** A
//!    page lives in exactly one place keyed by its stable file identity.
//!
//! It is deliberately *read-only*: writable `MAP_SHARED` writeback (dirty
//! tracking / `msync`) stays out of scope and remains `ENOSYS` (§23).  A private
//! (`MAP_PRIVATE`) writer copies-on-write to a fresh anonymous frame; the cached
//! frame it copied from is never mutated.
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
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
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
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The cache map.  A `BTreeMap` keyed by `(FileId, page_index)` gives ordered
/// per-file ranges (used by [`invalidate_file`]) with `O(log n)` lookup.
static PAGE_CACHE: Mutex<BTreeMap<PageKey, CacheEntry>> = Mutex::new(BTreeMap::new());

/// Resident-entry count, mirrored as a lock-free atomic so coherence hooks on
/// the (hot) write/truncate/unlink paths can ask "is anything cached at all?"
/// with a single relaxed load instead of taking the cache lock or resolving a
/// `FileId`.  Kept in lockstep with `PAGE_CACHE.len()`: bumped on insert,
/// dropped on every removal.
static RESIDENT_COUNT: AtomicUsize = AtomicUsize::new(0);

// Counters (monitoring; mirror what fs::pagecache exposes for stats).
static STAT_HITS: AtomicU64 = AtomicU64::new(0);
static STAT_MISSES: AtomicU64 = AtomicU64::new(0);
static STAT_INSERTS: AtomicU64 = AtomicU64::new(0);
static STAT_RACES_LOST: AtomicU64 = AtomicU64::new(0);
static STAT_RELEASES: AtomicU64 = AtomicU64::new(0);
static STAT_EVICTIONS: AtomicU64 = AtomicU64::new(0);

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
    /// Entries currently resident.
    pub resident: u64,
}

/// Read a snapshot of the page-cache counters.
#[must_use]
pub fn stats() -> PageCacheStats {
    let resident = PAGE_CACHE.lock().len() as u64;
    PageCacheStats {
        hits: STAT_HITS.load(Ordering::Relaxed),
        misses: STAT_MISSES.load(Ordering::Relaxed),
        inserts: STAT_INSERTS.load(Ordering::Relaxed),
        races_lost: STAT_RACES_LOST.load(Ordering::Relaxed),
        releases: STAT_RELEASES.load(Ordering::Relaxed),
        evictions: STAT_EVICTIONS.load(Ordering::Relaxed),
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
    if let Some(entry) = cache.get(&key) {
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
/// `fill` receives the full `FRAME_SIZE`-byte, already-zeroed page buffer.
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
pub fn get_or_fill<F>(file: FileId, file_offset: u64, fill: F) -> KernelResult<PhysFrame>
where
    F: FnOnce(&mut [u8]) -> KernelResult<()>,
{
    let key = key_for(file, file_offset)?;

    // Fast path: already resident.  Bump under the cache lock (closes the
    // evict-vs-map race; eviction also locks the cache first).
    {
        let cache = PAGE_CACHE.lock();
        if let Some(entry) = cache.get(&key) {
            let frame = entry.frame;
            // SAFETY: resident entry ⇒ allocated frame, refcount ≥ 1; cache lock
            // held so eviction cannot free it concurrently.
            unsafe { frame::ref_inc(frame)? };
            STAT_HITS.fetch_add(1, Ordering::Relaxed);
            return Ok(frame);
        }
    }

    STAT_MISSES.fetch_add(1, Ordering::Relaxed);

    // Allocate + fill the new frame with the cache lock dropped (alloc_frame is
    // expensive; never nest it under the cache lock).  A fresh frame has
    // refcount 1 — that becomes the cache's reference once inserted.
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let new_frame = frame::alloc_frame()?;

    // Zero the frame, then let `fill` populate it.
    // SAFETY: `new_frame` was just allocated, is exclusively owned by us, is not
    // mapped anywhere, and `to_virt(hhdm)` is its valid HHDM alias of exactly
    // FRAME_SIZE bytes.
    let buf = unsafe {
        let ptr = new_frame.to_virt(hhdm) as *mut u8;
        core::ptr::write_bytes(ptr, 0, FRAME_SIZE);
        core::slice::from_raw_parts_mut(ptr, FRAME_SIZE)
    };
    if let Err(e) = fill(buf) {
        // Fill failed — free the frame we never published.
        // SAFETY: `new_frame` was just allocated (refcount 1) and never mapped or
        // shared; free_frame returns it to the allocator.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Err(e);
    }

    // Re-acquire and insert, resolving the fill race.
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.get(&key) {
        // Another CPU filled this page first; adopt theirs and discard ours.
        let winner = entry.frame;
        // SAFETY: resident entry ⇒ allocated frame; cache lock held.
        unsafe { frame::ref_inc(winner)? };
        drop(cache);
        STAT_RACES_LOST.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `new_frame` (refcount 1) was never mapped or published.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Ok(winner);
    }
    // We win the race.  `new_frame` carries refcount 1 (the cache's reference);
    // bump once more for the caller's returned reference (→ 2).
    // SAFETY: `new_frame` is a freshly allocated, valid frame; cache lock held.
    unsafe { frame::ref_inc(new_frame)? };
    cache.insert(key, CacheEntry { frame: new_frame });
    RESIDENT_COUNT.fetch_add(1, Ordering::Relaxed);
    STAT_INSERTS.fetch_add(1, Ordering::Relaxed);
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
    if let Some(entry) = cache.remove(&key) {
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
    if let Some(entry) = cache.get(&key) {
        let frame = entry.frame;
        // refcount() takes the allocator lock; cache → allocator ordering holds.
        if frame::refcount(frame) <= 1 {
            cache.remove(&key);
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
/// each (coherence: the file was unlinked / truncated / rewritten).
///
/// Returns the number of pages removed.  Frames with live mappers survive for
/// them; unreferenced frames are freed.
pub fn invalidate_file(file: FileId) -> usize {
    let mut cache = PAGE_CACHE.lock();
    let lo = PageKey { file, page_index: 0 };
    let hi = PageKey { file, page_index: u64::MAX };
    let mut victims = alloc::vec::Vec::new();
    for (k, _entry) in cache.range(lo..=hi) {
        victims.push(*k);
    }
    let mut frames = alloc::vec::Vec::with_capacity(victims.len());
    for k in &victims {
        if let Some(entry) = cache.remove(k) {
            frames.push(entry.frame);
        }
    }
    RESIDENT_COUNT.fetch_sub(frames.len(), Ordering::Relaxed);
    drop(cache);
    let removed = frames.len();
    for f in frames {
        STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: dropping the cache's one reference per entry; live mappers (if
        // any) keep their frame alive, otherwise it is freed.
        let _ = unsafe { frame::free_frame(f) };
    }
    removed
}

/// Is anything at all currently cached?
///
/// A single relaxed atomic load — the cheap gate for coherence hooks on the
/// write/truncate/unlink paths.  When the cache is empty (the common case for
/// a system with no active file mappings) callers skip the cost of resolving a
/// [`FileId`] and invalidating.  Racy by construction (an entry may appear or
/// vanish immediately after the load), which is fine: the hooks run *after* the
/// mutation, so a page inserted concurrently is filled from the post-mutation
/// file contents anyway, and a stale "empty" reading can only occur if nothing
/// was cached at the moment of the write — exactly when there is nothing to
/// invalidate.
#[must_use]
pub fn is_populated() -> bool {
    RESIDENT_COUNT.load(Ordering::Relaxed) != 0
}

/// Coherence hook: invalidate every cached page of a file **if** the cache is
/// non-empty, given the file's `(fs_id, ino)` identity components.
///
/// This is the entry point the VFS mutation paths call after a `write`,
/// `truncate`, `unlink`, or `rename`-over-existing.  It is a no-op (one atomic
/// load) when nothing is cached, so the common write path pays almost nothing.
/// `ino == 0` (no stable identity) is never cacheable, so it is skipped.
pub fn invalidate_identity(fs_id: u64, ino: u64) {
    if ino == 0 || !is_populated() {
        return;
    }
    let _ = invalidate_file(FileId { fs_id, ino });
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
        let total = cache.len();
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
        for (k, entry) in cache.iter() {
            if victims.len() >= target {
                break;
            }
            // refcount() takes the allocator lock; cache → allocator ordering.
            if frame::refcount(entry.frame) <= 1 {
                victims.push(*k);
            }
        }
        for k in &victims {
            if let Some(entry) = cache.remove(k) {
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
    PAGE_CACHE.lock().contains_key(&key)
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
    let file_a = FileId { fs_id: 0, ino: 0xC0DE_0001 };
    let file_b = FileId { fs_id: 0, ino: 0xC0DE_0002 };
    let file_c = FileId { fs_id: 0, ino: 0xC0DE_0003 };
    let file_d = FileId { fs_id: 0, ino: 0xC0DE_0004 };

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

        // (8) is_populated tracks residency; invalidate_identity (the VFS
        //     coherence hook) drops a file's pages by (fs_id, ino).
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
        invalidate_identity(file_c.fs_id, file_c.ino);
        if is_resident(file_c, 0) {
            serial_println!("[page_cache]   FAIL: invalidate_identity left the page resident");
            return Err(KernelError::InternalError);
        }
        // ino == 0 is never cacheable; the hook must be a safe no-op for it.
        invalidate_identity(0, 0);
        serial_println!("[page_cache]   is_populated + invalidate_identity: OK");

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

        Ok(())
    })();

    // Defensive cleanup: if the body bailed early, drop any synthetic entries so
    // they do not linger in the live cache.
    let _ = invalidate_file(file_a);
    let _ = invalidate_file(file_b);
    let _ = invalidate_file(file_c);
    let _ = invalidate_file(file_d);

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
        "[page_cache]   stats: hits={} misses={} inserts={} races_lost={} releases={} evictions={} resident={}",
        s.hits,
        s.misses,
        s.inserts,
        s.races_lost,
        s.releases,
        s.evictions,
        s.resident
    );

    serial_println!("[page_cache] Read-only page-cache self-test PASSED");
    Ok(())
}
