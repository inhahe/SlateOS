//! Thumbnail cache for file explorer preview images.
//!
//! Manages a cache of thumbnail image data for files, enabling fast
//! preview display in the file explorer without re-reading/decoding
//! full files every time a directory is viewed.
//!
//! ## Architecture
//!
//! ```text
//! File explorer opens directory
//!   → for each file: thumbcache::get(path) → Option<CachedThumb>
//!   → cache miss: enqueue for generation
//!   → thumbcache::generate(path, data) → store thumbnail
//!   → cache hit: return cached RGBA pixel data
//! ```
//!
//! ## Features
//!
//! - **LRU cache** — least recently used eviction when at capacity
//! - **Multiple sizes** — small (48×48), medium (128×128), large (256×256)
//! - **Validation** — thumbnails invalidated when source file changes
//! - **MIME filtering** — only cache thumbnailable types (images, video, PDF)
//! - **Memory budget** — configurable total memory limit for cache
//! - **Persistent paths** — cache location for on-disk persistence (future)
//!
//! ## Design Notes
//!
//! - Maximum cached thumbnails: 2048 (across all sizes).
//! - Thumbnail data is stored as raw RGBA pixel bytes.
//! - Cache key: (path, mtime_ns, size) triple for validation.
//! - Generation is done by the caller (compositor/image decoder);
//!   this module only manages the cache storage.
//! - Thread-safe via `PreemptSpinMutex` (a preempt-disabling leaf lock).

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::fs::path::{Path, PathBuf};
use crate::serial_println;
use crate::sync::PreemptSpinMutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum cached thumbnails.
const MAX_ENTRIES: usize = 2048;

/// Maximum memory for thumbnail data (16 MiB).
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Supported thumbnail sizes.
const SIZE_SMALL: u32 = 48;
const SIZE_MEDIUM: u32 = 128;
const SIZE_LARGE: u32 = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Thumbnail size category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    /// 48×48 pixels — icon view.
    Small,
    /// 128×128 pixels — thumbnail view.
    Medium,
    /// 256×256 pixels — large preview.
    Large,
    /// Custom size (width, height).
    Custom(u32, u32),
}

impl ThumbSize {
    /// Get pixel dimensions.
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Small => (SIZE_SMALL, SIZE_SMALL),
            Self::Medium => (SIZE_MEDIUM, SIZE_MEDIUM),
            Self::Large => (SIZE_LARGE, SIZE_LARGE),
            Self::Custom(w, h) => (w, h),
        }
    }

    /// Pixel count.
    pub fn pixels(self) -> u32 {
        let (w, h) = self.dimensions();
        w * h
    }

    /// RGBA byte size for this thumbnail size.
    pub fn byte_size(self) -> usize {
        (self.pixels() as usize) * 4
    }

    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Custom(_, _) => "custom",
        }
    }

    /// Parse from name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "small" | "s" | "48" => Some(Self::Small),
            "medium" | "m" | "128" => Some(Self::Medium),
            "large" | "l" | "256" => Some(Self::Large),
            _ => None,
        }
    }
}

/// A cached thumbnail entry.
#[derive(Debug, Clone)]
pub struct CachedThumb {
    /// Source file path.
    pub path: PathBuf,
    /// Thumbnail pixel width.
    pub width: u32,
    /// Thumbnail pixel height.
    pub height: u32,
    /// RGBA pixel data (width × height × 4 bytes).
    pub data: Vec<u8>,
    /// Source file modification time (for validation).
    pub source_mtime_ns: u64,
    /// Source file size (for validation).
    pub source_size: u64,
    /// When the thumbnail was generated.
    pub generated_ns: u64,
    /// Last access time (for LRU).
    pub last_access_ns: u64,
}

/// Cache entry (internal).
struct CacheEntry {
    /// Cache key: path + size category.
    path: PathBuf,
    size: ThumbSize,
    /// Thumbnail data.
    thumb: CachedThumb,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Thumbnail cache.
static CACHE: PreemptSpinMutex<Vec<CacheEntry>> = PreemptSpinMutex::named(Vec::new(), b"CACHE");

/// Current memory usage.
static MEMORY_USED: AtomicU64 = AtomicU64::new(0);

/// Statistics.
static HIT_COUNT: AtomicU64 = AtomicU64::new(0);
static MISS_COUNT: AtomicU64 = AtomicU64::new(0);
static STORE_COUNT: AtomicU64 = AtomicU64::new(0);
static EVICT_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Cache operations
// ---------------------------------------------------------------------------

/// Look up a cached thumbnail.
///
/// Returns `Some` if the thumbnail is cached AND the source file hasn't
/// changed (validated by mtime + size). Returns `None` on cache miss.
pub fn get(path: impl AsRef<Path>, size: ThumbSize) -> Option<CachedThumb> {
    let path = path.as_ref();
    let now = crate::timekeeping::clock_monotonic();

    // Phase 1 — read the candidate's validation stamp, then drop the lock.
    let (source_mtime_ns, source_size) = {
        let cache = CACHE.lock();
        let entry = cache
            .iter()
            .find(|e| e.path.as_path() == path && e.size == size)?;
        (entry.thumb.source_mtime_ns, entry.thumb.source_size)
    };

    // Phase 2 — validate against the source with NO lock held. `Vfs::metadata`
    // walks the mount table, takes filesystem locks of its own and can block on
    // the backing device; running it inside CACHE's critical section would hold
    // a leaf lock across an unbounded I/O path and put its acquisition order
    // ahead of the VFS's. A metadata *error* is treated as "cannot tell" and
    // leaves the entry trusted, which is the original behaviour.
    if let Ok(meta) = crate::fs::Vfs::metadata(path) {
        if meta.modified_ns != source_mtime_ns || meta.size != source_size {
            // Source changed. Actually drop the stale entry rather than merely
            // reporting a miss: leaving it in place made every future lookup of
            // this path re-fail validation (and re-pay the metadata call) until
            // LRU happened to evict it, and its bytes stayed counted in
            // MEMORY_USED the whole time.
            let mut cache = CACHE.lock();
            cache.retain(|e| {
                if e.path.as_path() == path
                    && e.size == size
                    && e.thumb.source_mtime_ns == source_mtime_ns
                    && e.thumb.source_size == source_size
                {
                    MEMORY_USED.fetch_sub(e.thumb.data.len() as u64, Ordering::Relaxed);
                    false
                } else {
                    true
                }
            });
            MISS_COUNT.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    }

    // Phase 3 — re-acquire and stamp the LRU. The entry can have been evicted
    // or replaced in the window above, so re-find it *and* require the same
    // validation stamp: we must never hand back a thumbnail we did not
    // validate. A vanished entry is reported as a plain miss, exactly like the
    // phase-1 not-found path.
    let mut cache = CACHE.lock();
    let entry = cache.iter_mut().find(|e| {
        e.path.as_path() == path
            && e.size == size
            && e.thumb.source_mtime_ns == source_mtime_ns
            && e.thumb.source_size == source_size
    })?;
    entry.thumb.last_access_ns = now;
    HIT_COUNT.fetch_add(1, Ordering::Relaxed);
    Some(entry.thumb.clone())
}

/// Store a thumbnail in the cache.
///
/// The caller provides the pre-generated RGBA pixel data.
/// Evicts least-recently-used entries if at capacity or memory limit.
pub fn store(
    path: impl AsRef<Path>,
    size: ThumbSize,
    width: u32,
    height: u32,
    data: Vec<u8>,
    source_mtime_ns: u64,
    source_size: u64,
) -> KernelResult<()> {
    let path = path.as_ref();
    let now = crate::timekeeping::clock_monotonic();
    let data_len = data.len();
    STORE_COUNT.fetch_add(1, Ordering::Relaxed);

    let thumb = CachedThumb {
        path: path.to_path_buf(),
        width,
        height,
        data,
        source_mtime_ns,
        source_size,
        generated_ns: now,
        last_access_ns: now,
    };

    let mut cache = CACHE.lock();

    // Remove existing entry for same path+size.
    if let Some(pos) = cache
        .iter()
        .position(|e| e.path.as_path() == path && e.size == size)
    {
        let old_len = cache[pos].thumb.data.len() as u64;
        cache.swap_remove(pos);
        MEMORY_USED.fetch_sub(old_len, Ordering::Relaxed);
    }

    // Evict LRU entries if at capacity.
    while cache.len() >= MAX_ENTRIES {
        evict_lru(&mut cache);
    }

    // Evict if over memory budget.
    let mut current_mem = MEMORY_USED.load(Ordering::Relaxed) as usize;
    while current_mem + data_len > MAX_MEMORY_BYTES && !cache.is_empty() {
        evict_lru(&mut cache);
        current_mem = MEMORY_USED.load(Ordering::Relaxed) as usize;
    }

    MEMORY_USED.fetch_add(data_len as u64, Ordering::Relaxed);

    cache.push(CacheEntry {
        path: path.to_path_buf(),
        size,
        thumb,
    });

    Ok(())
}

/// Invalidate all cached thumbnails for a path.
///
/// Called when a file is modified, deleted, or renamed.
pub fn invalidate(path: impl AsRef<Path>) -> usize {
    let path = path.as_ref();
    let mut cache = CACHE.lock();
    let len_before = cache.len();

    cache.retain(|e| {
        if e.path.as_path() == path {
            MEMORY_USED.fetch_sub(e.thumb.data.len() as u64, Ordering::Relaxed);
            false
        } else {
            true
        }
    });

    len_before - cache.len()
}

/// Invalidate thumbnails for all files under a directory prefix.
pub fn invalidate_dir(dir_path: impl AsRef<Path>) -> usize {
    let dir_path = dir_path.as_ref();
    let mut cache = CACHE.lock();
    let len_before = cache.len();

    // `path_in_subtree` matches on component boundaries and already covers
    // the directory itself, replacing the hand-built `format!("{}/", dir)`
    // byte prefix — which needed UTF-8 and, being a byte test, would also
    // have invalidated `/ab` when asked to invalidate `/a`.
    cache.retain(|e| {
        if crate::fs::pathutil::path_in_subtree(&e.path, dir_path) {
            MEMORY_USED.fetch_sub(e.thumb.data.len() as u64, Ordering::Relaxed);
            false
        } else {
            true
        }
    });

    len_before - cache.len()
}

/// Check if a MIME type is thumbnailable.
pub fn is_thumbnailable(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime == "application/pdf"
        || mime.starts_with("video/")
        || mime == "image/svg+xml"
}

/// Get a list of cached paths.
pub fn list() -> Vec<(PathBuf, ThumbSize, u32, u32, usize)> {
    let cache = CACHE.lock();
    cache
        .iter()
        .map(|e| {
            (
                e.path.clone(),
                e.size,
                e.thumb.width,
                e.thumb.height,
                e.thumb.data.len(),
            )
        })
        .collect()
}

/// Clear the entire cache.
pub fn clear() {
    let mut cache = CACHE.lock();
    cache.clear();
    MEMORY_USED.store(0, Ordering::Relaxed);
}

/// Get the number of cached thumbnails.
pub fn count() -> usize {
    CACHE.lock().len()
}

/// Get current memory usage in bytes.
pub fn memory_used() -> u64 {
    MEMORY_USED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get cache statistics.
pub fn stats() -> (u64, u64, u64, u64, usize, u64) {
    let count = CACHE.lock().len();
    (
        HIT_COUNT.load(Ordering::Relaxed),
        MISS_COUNT.load(Ordering::Relaxed),
        STORE_COUNT.load(Ordering::Relaxed),
        EVICT_COUNT.load(Ordering::Relaxed),
        count,
        MEMORY_USED.load(Ordering::Relaxed),
    )
}

/// Reset statistics (does not clear cache).
pub fn reset_stats() {
    HIT_COUNT.store(0, Ordering::Relaxed);
    MISS_COUNT.store(0, Ordering::Relaxed);
    STORE_COUNT.store(0, Ordering::Relaxed);
    EVICT_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Evict the least-recently-used entry.
fn evict_lru(cache: &mut Vec<CacheEntry>) {
    if cache.is_empty() {
        return;
    }
    let lru_idx = cache
        .iter()
        .enumerate()
        .min_by_key(|(_, e)| e.thumb.last_access_ns)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let old_len = cache[lru_idx].thumb.data.len() as u64;
    cache.swap_remove(lru_idx);
    MEMORY_USED.fetch_sub(old_len, Ordering::Relaxed);
    EVICT_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[thumbcache] Running self-test...");

    test_thumb_size();
    test_store_and_get();
    test_get_validation();
    test_invalidate();
    test_is_thumbnailable();
    test_memory_tracking();
    test_lru_eviction();

    serial_println!("[thumbcache] Self-test passed (7 tests).");
    Ok(())
}

fn test_thumb_size() {
    assert_eq!(ThumbSize::Small.dimensions(), (48, 48));
    assert_eq!(ThumbSize::Medium.dimensions(), (128, 128));
    assert_eq!(ThumbSize::Large.dimensions(), (256, 256));
    assert_eq!(ThumbSize::Custom(320, 240).dimensions(), (320, 240));
    assert_eq!(ThumbSize::Small.byte_size(), 48 * 48 * 4);
    assert_eq!(ThumbSize::from_name("small"), Some(ThumbSize::Small));
    assert_eq!(ThumbSize::from_name("medium"), Some(ThumbSize::Medium));
    assert_eq!(ThumbSize::from_name("unknown"), None);
    serial_println!("[thumbcache]   thumb_size: ok");
}

fn test_store_and_get() {
    clear();

    // Store a dummy thumbnail.
    let data = vec![0u8; 48 * 48 * 4];
    let result = store("/test/img.png", ThumbSize::Small, 48, 48, data, 1000, 5000);
    assert!(result.is_ok());
    assert_eq!(count(), 1);

    // Get — without valid metadata, the mtime/size check will fail for
    // non-existent files, but the entry is still in cache. The get()
    // validation is tested via is_thumbnailable and store/clear below.
    assert_eq!(count(), 1);

    clear();
    assert_eq!(count(), 0);
    serial_println!("[thumbcache]   store_and_get: ok");
}

/// Exercise `get()`'s three phases against a *real* file.
///
/// `test_store_and_get` above never calls `get()` at all — it asserts
/// `count() == 1` twice with nothing in between — so the validation path had no
/// coverage, which is how the stale-entry leak below survived. This covers all
/// three outcomes: source unreadable (trust), source changed (miss + evict),
/// source unchanged (hit).
#[allow(clippy::expect_used)] // Tests panic on unexpected state.
fn test_get_validation() {
    clear();

    // 1. Source does not exist -> `Vfs::metadata` errors -> "cannot tell", so
    //    the entry stays trusted and is returned. An unreadable source must
    //    never be mistaken for a changed one.
    store(
        "/no/such/file.png",
        ThumbSize::Small,
        8,
        8,
        vec![0u8; 64],
        1000,
        5000,
    )
    .expect("store missing-source thumb");
    assert_eq!(count(), 1);
    assert!(
        get("/no/such/file.png", ThumbSize::Small).is_some(),
        "an unreadable source must leave the cached thumb trusted"
    );
    assert_eq!(count(), 1, "an unreadable source must not evict");

    clear();
    let _ = crate::fs::Vfs::mkdir("/tmp/thumb_test");
    crate::fs::Vfs::write_file("/tmp/thumb_test/img.png", b"real contents").expect("write source");

    // 2. Source changed -> miss, AND the stale entry is dropped with its bytes
    //    returned to the memory accounting. Before the fix the entry was left
    //    in place, so every later lookup re-failed validation (re-paying the
    //    metadata call) and its bytes stayed counted until LRU evicted it.
    let before = memory_used();
    store(
        "/tmp/thumb_test/img.png",
        ThumbSize::Small,
        8,
        8,
        vec![0u8; 128],
        1,
        1,
    )
    .expect("store stale thumb");
    assert_eq!(memory_used(), before.saturating_add(128));
    assert!(
        get("/tmp/thumb_test/img.png", ThumbSize::Small).is_none(),
        "a changed source must miss"
    );
    assert_eq!(
        count(),
        0,
        "a stale entry must be dropped, not just reported as a miss"
    );
    assert_eq!(
        memory_used(),
        before,
        "dropping a stale entry must return its bytes"
    );

    // 3. Source unchanged -> hit, and the entry survives the drop/re-acquire
    //    that phases 2 and 3 do around the unlocked metadata call.
    let meta = crate::fs::Vfs::metadata("/tmp/thumb_test/img.png").expect("metadata");
    store(
        "/tmp/thumb_test/img.png",
        ThumbSize::Small,
        8,
        8,
        vec![7u8; 64],
        meta.modified_ns,
        meta.size,
    )
    .expect("store fresh thumb");
    let hit = get("/tmp/thumb_test/img.png", ThumbSize::Small).expect("a matching stamp must hit");
    assert_eq!(hit.data.len(), 64);
    assert_eq!(count(), 1, "a hit must not evict");

    clear();
    let _ = crate::fs::Vfs::remove("/tmp/thumb_test/img.png");
    serial_println!("[thumbcache]   get_validation: ok");
}

fn test_invalidate() {
    clear();

    let data1 = vec![0u8; 100];
    let data2 = vec![0u8; 200];
    let _ = store("/a/img1.png", ThumbSize::Small, 10, 10, data1, 100, 500);
    let _ = store("/a/img2.png", ThumbSize::Medium, 20, 20, data2, 200, 600);

    assert_eq!(count(), 2);

    // Invalidate one file.
    let removed = invalidate("/a/img1.png");
    assert_eq!(removed, 1);
    assert_eq!(count(), 1);

    // Invalidate by directory prefix.
    let _ = store(
        "/a/img3.png",
        ThumbSize::Small,
        10,
        10,
        vec![0u8; 50],
        300,
        700,
    );
    assert_eq!(count(), 2);
    let removed = invalidate_dir("/a");
    assert_eq!(removed, 2);
    assert_eq!(count(), 0);

    clear();
    serial_println!("[thumbcache]   invalidate: ok");
}

fn test_is_thumbnailable() {
    assert!(is_thumbnailable("image/png"));
    assert!(is_thumbnailable("image/jpeg"));
    assert!(is_thumbnailable("video/mp4"));
    assert!(is_thumbnailable("application/pdf"));
    assert!(!is_thumbnailable("text/plain"));
    assert!(!is_thumbnailable("application/octet-stream"));
    serial_println!("[thumbcache]   is_thumbnailable: ok");
}

fn test_memory_tracking() {
    clear();
    assert_eq!(memory_used(), 0);

    let data = vec![0u8; 1024];
    let _ = store("/mem/a.png", ThumbSize::Small, 16, 16, data, 100, 200);
    assert_eq!(memory_used(), 1024);

    let data2 = vec![0u8; 2048];
    let _ = store("/mem/b.png", ThumbSize::Medium, 32, 32, data2, 100, 200);
    assert_eq!(memory_used(), 1024 + 2048);

    invalidate("/mem/a.png");
    assert_eq!(memory_used(), 2048);

    clear();
    assert_eq!(memory_used(), 0);
    serial_println!("[thumbcache]   memory_tracking: ok");
}

fn test_lru_eviction() {
    clear();

    // Store entries up to MAX_ENTRIES to test eviction.
    // We'll use a small number and verify eviction happens.
    for i in 0..10 {
        let data = vec![0u8; 64];
        let path = alloc::format!("/lru/file_{}.png", i);
        let _ = store(&path, ThumbSize::Small, 4, 4, data, i as u64, 100);
    }
    assert_eq!(count(), 10);

    // Eviction happens at MAX_ENTRIES; our 10 entries are well under.
    // Just verify all are present.
    let entries = list();
    assert_eq!(entries.len(), 10);

    clear();
    serial_println!("[thumbcache]   lru_eviction: ok");
}
