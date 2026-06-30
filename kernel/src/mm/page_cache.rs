//! Read-only file page cache ("C-lite") — shared, refcounted file pages.
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
//! ## Keying
//!
//! Pages are keyed by [`PageKey`] = ([`FileId`], page index), where the page
//! index is `file_offset / FRAME_SIZE` (16 KiB pages).  [`FileId`] is the stable
//! `(fs_id, ino)` identity from [`crate::fs::vfs::Vfs::file_identity`].  Only
//! files with a stable identity are cacheable; callers that get `None` from
//! `file_identity` (FAT, ISO9660, pseudo-filesystems) must fall back to the
//! per-mapping read path and never enter this cache.
//!
//! ## Reference counting & lifetime
//!
//! Each [`CacheEntry`] carries a `refcount` of live references (one per mapping
//! or held lookup).  A reference is acquired by [`get_or_fill`] / [`lookup`] and
//! released by [`release`].  **Each successful acquire must be paired with
//! exactly one release** — [`CachedPage`] is a plain descriptor and does *not*
//! auto-release on drop, because the reference must outlive the mapping (the
//! page table holds the frame until the VMA is torn down, at which point the
//! unmap path releases).
//!
//! An entry stays resident at `refcount == 0` (that is the caching benefit: the
//! next mapper gets a hit).  Such idle entries are reclaimable — [`invalidate`]
//! and eviction (a later sub-task) drop them and free the frame.
//!
//! ## Lock ordering
//!
//! Frame allocation ([`crate::mm::frame::alloc_frame`]) and freeing take the
//! frame-allocator lock.  We **never** hold the page-cache lock across those
//! calls: `get_or_fill` allocates and fills the new frame with the cache lock
//! *dropped*, then re-acquires it to insert (handling the race where another
//! CPU inserted the same page meanwhile).

#![allow(clippy::module_name_repetitions)]

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::FileId;
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::mm::page_table;

// ---------------------------------------------------------------------------
// Keys & entries
// ---------------------------------------------------------------------------

/// Cache key: a stable file identity plus the 16 KiB page index within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageKey {
    /// Stable system-wide file identity (`(fs_id, ino)`).
    pub file: FileId,
    /// Page index = `file_offset / FRAME_SIZE`.
    pub page_index: u64,
}

/// A resident cached page: the physical frame plus its live-reference count.
#[derive(Debug)]
struct CacheEntry {
    /// The physical frame holding this page's bytes (zero-padded past EOF).
    frame: PhysFrame,
    /// Number of live references (mappings / held lookups).  May be 0 while the
    /// page stays resident for reuse.
    refcount: u64,
}

/// A handle to a cached page returned by [`get_or_fill`] / [`lookup`].
///
/// This is a plain copyable *descriptor*, not an owning guard: copying it does
/// **not** bump the refcount, and dropping it does **not** release.  The caller
/// owns exactly one logical reference per successful acquire and must hand the
/// key back to [`release`] when the mapping that holds the frame goes away.
#[derive(Debug, Clone, Copy)]
pub struct CachedPage {
    /// The shared, read-only physical frame.  Map it read-only; never write it.
    pub frame: PhysFrame,
    /// The key identifying this page (pass to [`release`] / [`invalidate`]).
    pub key: PageKey,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The cache map.  A `BTreeMap` keyed by `(FileId, page_index)` gives ordered
/// per-file ranges (used by [`invalidate_file`]) with `O(log n)` lookup.
static PAGE_CACHE: Mutex<BTreeMap<PageKey, CacheEntry>> = Mutex::new(BTreeMap::new());

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
    /// Reference releases.
    pub releases: u64,
    /// Entries evicted/invalidated (frame freed).
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

/// Compute the page key for a file offset, validating divisibility.
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
/// On a hit, the entry's refcount is incremented and a [`CachedPage`] returned;
/// the caller must later [`release`] it.  On a miss, returns `Ok(None)` (the
/// caller can then [`get_or_fill`]).  Frame-misaligned offsets are an error.
///
/// # Errors
///
/// Returns [`KernelError::InvalidArgument`] if `file_offset` is not a multiple
/// of [`FRAME_SIZE`].
pub fn lookup(file: FileId, file_offset: u64) -> KernelResult<Option<CachedPage>> {
    let key = key_for(file, file_offset)?;
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.get_mut(&key) {
        entry.refcount = entry.refcount.saturating_add(1);
        STAT_HITS.fetch_add(1, Ordering::Relaxed);
        return Ok(Some(CachedPage { frame: entry.frame, key }));
    }
    Ok(None)
}

/// Acquire a reference to the page for `(file, file_offset)`, filling it from
/// the backing store on a miss.
///
/// On a hit, increments the refcount and returns the shared frame.  On a miss,
/// allocates and zeroes a fresh frame, invokes `fill` to populate it (a short
/// fill leaves the tail zero, matching demand-paging EOF semantics), then
/// inserts it — racing CPUs are resolved so only one frame survives per key.
///
/// `fill` receives the full `FRAME_SIZE`-byte, already-zeroed page buffer and
/// should write the file's bytes for this page into it.
///
/// The returned [`CachedPage`] holds one reference; pair it with [`release`].
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] for a misaligned offset.
/// - Whatever `fill` returns on a read error (the just-allocated frame is freed
///   before propagating).
/// - Frame-allocation / HHDM errors.
pub fn get_or_fill<F>(file: FileId, file_offset: u64, fill: F) -> KernelResult<CachedPage>
where
    F: FnOnce(&mut [u8]) -> KernelResult<()>,
{
    let key = key_for(file, file_offset)?;

    // Fast path: already resident.
    {
        let mut cache = PAGE_CACHE.lock();
        if let Some(entry) = cache.get_mut(&key) {
            entry.refcount = entry.refcount.saturating_add(1);
            STAT_HITS.fetch_add(1, Ordering::Relaxed);
            return Ok(CachedPage { frame: entry.frame, key });
        }
    }

    STAT_MISSES.fetch_add(1, Ordering::Relaxed);

    // Allocate + fill the new frame with the cache lock dropped (frame alloc
    // takes the allocator lock; never nest it under the cache lock).
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
        // SAFETY: `new_frame` was just allocated and never mapped/shared.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Err(e);
    }

    // Re-acquire and insert, resolving the fill race.
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.get_mut(&key) {
        // Another CPU filled this page first; adopt theirs and discard ours.
        entry.refcount = entry.refcount.saturating_add(1);
        let winner = CachedPage { frame: entry.frame, key };
        drop(cache);
        STAT_RACES_LOST.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `new_frame` was never mapped or published to the cache.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Ok(winner);
    }
    cache.insert(key, CacheEntry { frame: new_frame, refcount: 1 });
    STAT_INSERTS.fetch_add(1, Ordering::Relaxed);
    Ok(CachedPage { frame: new_frame, key })
}

/// Release one reference previously acquired via [`get_or_fill`] / [`lookup`].
///
/// Decrements the entry's refcount (saturating at 0).  The entry stays resident
/// at refcount 0 for reuse; it is only freed by [`invalidate`] / eviction.  A
/// release for an already-evicted key is a harmless no-op.
pub fn release(key: PageKey) {
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.get_mut(&key) {
        entry.refcount = entry.refcount.saturating_sub(1);
        STAT_RELEASES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Drop a single resident page **only if it has no live references**, freeing
/// its frame.  Returns `true` if it was removed.
///
/// Used for coherence (the file changed) and as the eviction primitive.  An
/// entry with `refcount > 0` is left in place (live mappings still point at the
/// frame) and `false` is returned — the caller must not remove a page out from
/// under a live mapping.
pub fn invalidate(key: PageKey) -> bool {
    let mut cache = PAGE_CACHE.lock();
    let should_remove = matches!(cache.get(&key), Some(e) if e.refcount == 0);
    if should_remove {
        if let Some(entry) = cache.remove(&key) {
            drop(cache);
            STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: refcount was 0 ⇒ no mapping references this frame, so it
            // is safe to return it to the allocator.
            let _ = unsafe { frame::free_frame(entry.frame) };
            return true;
        }
    }
    false
}

/// Invalidate all idle (refcount 0) pages of a file, freeing their frames.
///
/// Returns the number of pages removed.  Pages with live references are left
/// resident.  Used when a file is unlinked/truncated/rewritten so its stale
/// cached pages are not served to future mappers.
pub fn invalidate_file(file: FileId) -> usize {
    let mut cache = PAGE_CACHE.lock();
    // Collect the idle keys for this file (BTreeMap range over the page index).
    let lo = PageKey { file, page_index: 0 };
    let hi = PageKey { file, page_index: u64::MAX };
    let mut victims = alloc::vec::Vec::new();
    for (k, entry) in cache.range(lo..=hi) {
        if entry.refcount == 0 {
            victims.push(*k);
        }
    }
    let mut frames = alloc::vec::Vec::with_capacity(victims.len());
    for k in &victims {
        if let Some(entry) = cache.remove(k) {
            frames.push(entry.frame);
        }
    }
    drop(cache);
    let removed = frames.len();
    for f in frames {
        STAT_EVICTIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: each frame had refcount 0 ⇒ no live mapping references it.
        let _ = unsafe { frame::free_frame(f) };
    }
    removed
}

/// Test/diagnostic helper: the current refcount of a key, or `None` if absent.
#[must_use]
pub fn refcount_of(key: PageKey) -> Option<u64> {
    PAGE_CACHE.lock().get(&key).map(|e| e.refcount)
}

// ---------------------------------------------------------------------------
// Boot self-test
// ---------------------------------------------------------------------------

/// Boot self-test for the read-only page cache (design-decisions §23/§36).
///
/// `#[cfg(test)]` unit tests do not run on the bare-metal target, so the cache
/// is exercised here against the real frame allocator during boot.  Validates
/// hit/miss sharing, single-fill, content correctness, refcounting, key
/// distinctness, race-free idempotence, and reclaim.
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

    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;

    // Helper: read a byte from a cached frame via its HHDM alias.
    let read_byte = |frame: PhysFrame, idx: usize| -> u8 {
        // SAFETY: `frame` is a resident cache frame; `to_virt(hhdm)` is its valid
        // HHDM alias and `idx < FRAME_SIZE`.
        unsafe { core::ptr::read_volatile((frame.to_virt(hhdm) as *const u8).add(idx)) }
    };

    // Run the body, always cleaning up the cache entries we created.
    let result = (|| -> KernelResult<()> {
        // (1) First acquire is a miss: allocates, fills, refcount 1.
        let mut fill_calls = 0u32;
        let p0 = get_or_fill(file_a, 0, |buf| {
            fill_calls = fill_calls.saturating_add(1);
            // Slice pattern avoids panicking index ops; a FRAME_SIZE buffer
            // always has ≥3 bytes, so this always matches.
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
        if refcount_of(p0.key) != Some(1) {
            serial_println!("[page_cache]   FAIL: refcount after miss != 1");
            return Err(KernelError::InternalError);
        }
        if read_byte(p0.frame, 0) != 0xAB || read_byte(p0.frame, 1) != 0xCD {
            serial_println!("[page_cache]   FAIL: filled content mismatch");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   miss fills + content correct: OK");

        // (2) Second acquire of the same page is a hit: same frame, no new fill,
        //     refcount 2.
        let p1 = get_or_fill(file_a, 0, |_buf| {
            // Must NOT be called on a hit.
            fill_calls = fill_calls.saturating_add(1);
            Ok(())
        })?;
        if fill_calls != 1 {
            serial_println!("[page_cache]   FAIL: hit re-filled the page");
            return Err(KernelError::InternalError);
        }
        if p1.frame != p0.frame {
            serial_println!("[page_cache]   FAIL: hit returned a different frame");
            return Err(KernelError::InternalError);
        }
        if refcount_of(p0.key) != Some(2) {
            serial_println!("[page_cache]   FAIL: refcount after hit != 2");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   hit shares frame, single fill: OK");

        // (3) A different page index of the same file is a distinct frame.
        let p2 = get_or_fill(file_a, FRAME_SIZE as u64, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x11;
            }
            Ok(())
        })?;
        if p2.frame == p0.frame {
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

        // (5) Release drops refcount; entry stays resident at 0; invalidate then
        //     reclaims it.
        release(p0.key); // 2 -> 1
        release(p1.key); // 1 -> 0
        if refcount_of(p0.key) != Some(0) {
            serial_println!("[page_cache]   FAIL: refcount after releases != 0");
            return Err(KernelError::InternalError);
        }
        // invalidate must refuse while p2 (different key) is irrelevant; p0 has
        // refcount 0 so it is removed.
        if !invalidate(p0.key) {
            serial_println!("[page_cache]   FAIL: invalidate(refcount 0) did not remove");
            return Err(KernelError::InternalError);
        }
        if refcount_of(p0.key).is_some() {
            serial_println!("[page_cache]   FAIL: entry still resident after invalidate");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   release + reclaim: OK");

        // (6) invalidate refuses to evict a page with live references.
        let live = get_or_fill(file_b, 0, |buf| {
            if let [b0, ..] = buf {
                *b0 = 0x22;
            }
            Ok(())
        })?;
        if invalidate(live.key) {
            serial_println!("[page_cache]   FAIL: invalidate evicted a live page");
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   invalidate spares live page: OK");

        // Clean up everything we still hold: p2 (file_a) and live (file_b).
        release(p2.key);
        release(live.key);
        let n_a = invalidate_file(file_a);
        let n_b = invalidate_file(file_b);
        if n_a != 1 || n_b != 1 {
            serial_println!(
                "[page_cache]   FAIL: invalidate_file removed {n_a}/{n_b}, expected 1/1"
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[page_cache]   invalidate_file reclaims idle pages: OK");

        Ok(())
    })();

    // Defensive cleanup: if the body bailed early, make sure no synthetic test
    // entries linger in the live cache.  (Idle ones are freed; live ones can't
    // be — but the test only leaves them on an assertion failure path.)
    let _ = invalidate_file(file_a);
    let _ = invalidate_file(file_b);

    result?;

    // The cache must be empty again (every synthetic entry reclaimed) and the
    // counters must reflect the run: at least one hit and one miss occurred.
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
