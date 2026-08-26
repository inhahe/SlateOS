//! Page Cache — file-backed page cache monitoring.
//!
//! Tracks page cache hits, misses, evictions, readahead
//! effectiveness, and dirty page ratios. Essential for
//! diagnosing I/O performance and memory pressure.
//!
//! ## Architecture
//!
//! ```text
//! Page cache monitoring
//!   → pagecache::record_hit() → cache hit
//!   → pagecache::record_miss() → cache miss (disk read)
//!   → pagecache::record_eviction(pages) → pages evicted
//!   → pagecache::record_readahead(requested, useful) → readahead stats
//!
//! Integration:
//!   → writeback (dirty page writeback)
//!   → inodestat (inode cache)
//!   → pagestat (page allocator)
//!   → fscache (filesystem cache)
//! ```
//!
//! ## Where the numbers actually come from
//!
//! There are two sources, and the distinction matters for reading `/proc`:
//!
//! 1. **The kernel's own page cache**, [`crate::mm::page_cache`], which is the
//!    real thing — it backs [`crate::fs::vfs`] reads and file-backed demand
//!    paging in [`crate::proc::pcb`]. It already keeps its hit/miss/eviction
//!    counters as relaxed atomics, and [`crate::mm::page_cache::stats`] reads
//!    them. This module **projects** that snapshot into a synthetic
//!    [`KERNEL_DEVICE`] row whenever a reader asks, rather than having the cache
//!    call in on every lookup — see below.
//! 2. **Registered devices**, whose counters move only when something calls
//!    [`record_hit`] and friends. Nothing in the tree does yet; those exist for
//!    a future per-device source and for the self-test.
//!
//! **Why projection rather than `record_*` calls on the cache's fast path.**
//! The obvious wiring — have `mm::page_cache::lookup` call `record_hit("…")` —
//! is wrong here for a reason that does not apply to slower accounting paths.
//! `record_hit` takes this module's spin lock and then does a *string compare
//! per registered device* to find its row; a page-cache lookup is a hot path on
//! every buffered read and every file-backed page fault, so that would put a
//! lock acquisition and a linear scan inside an operation whose whole purpose
//! is to be faster than touching the disk. The counters mm already keeps are
//! free by comparison, and its own comment above them says they exist to
//! "mirror what fs::pagecache exposes" — the two halves of one intent that were
//! never joined. Joining them at *read* time costs nothing on the fast path and
//! loses no information, because a monitoring reader only ever wanted the
//! totals.
//!
//! The projected row reports `dirty_pages`, `writeback_pages` and the readahead
//! pair as zero, because `mm::page_cache` genuinely does not track those — it
//! has no writeback state and no readahead heuristic. Those zeros are the one
//! remaining place in this file where zero does not mean "measured zero"; if
//! either is added to `mm::page_cache`, extend [`kernel_row`] with it.

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Cache operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOp {
    Hit,
    Miss,
    Eviction,
    Writeback,
    Readahead,
    Invalidate,
}

impl CacheOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Eviction => "eviction",
            Self::Writeback => "writeback",
            Self::Readahead => "readahead",
            Self::Invalidate => "invalidate",
        }
    }
}

/// Per-device cache stats.
#[derive(Debug, Clone)]
pub struct DeviceCacheStats {
    pub device: String,
    pub cached_pages: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_pages: u64,
    pub writeback_pages: u64,
    pub readahead_pages: u64,
    pub readahead_useful: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;

/// Row name under which the kernel's own page cache is reported.
///
/// Reserved: [`register_device`] refuses it, so a caller cannot create a second
/// row with this name and make the projected one ambiguous.
pub const KERNEL_DEVICE: &str = "kernel";

struct State {
    devices: Vec<DeviceCacheStats>,
    total_hits: u64,
    total_misses: u64,
    total_evictions: u64,
    total_readahead: u64,
    total_readahead_useful: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Projection of the kernel's own page cache
// ---------------------------------------------------------------------------

/// Snapshot the kernel page cache as a device row, or `None` if it has seen no
/// traffic at all.
///
/// Returning `None` on an all-zero snapshot is deliberate: a row of zeros in
/// `/proc/pagecache` before the cache has done anything is exactly the
/// "fabricated data" this module's `init_defaults` was cleaned up to stop
/// producing. A row appears the moment there is something real to report.
///
/// **Call this before taking `STATE`, never while holding it.** It takes the
/// page cache's own lock (to count residency), so calling it under `STATE`
/// would establish a `STATE` → `PAGE_CACHE` order. Nothing establishes the
/// reverse order today — `mm::page_cache` does not call into this module, which
/// is the whole reason this projection exists — but the readers below are
/// written to avoid the nesting entirely rather than to rely on that staying
/// true.
fn kernel_row() -> Option<DeviceCacheStats> {
    let s = crate::mm::page_cache::stats();
    if s.hits == 0 && s.misses == 0 && s.evictions == 0 && s.resident == 0 {
        return None;
    }
    Some(DeviceCacheStats {
        device: String::from(KERNEL_DEVICE),
        cached_pages: s.resident,
        hits: s.hits,
        misses: s.misses,
        evictions: s.evictions,
        // Not tracked by mm::page_cache; see the module docs.
        dirty_pages: 0,
        writeback_pages: 0,
        readahead_pages: 0,
        readahead_useful: 0,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** page-cache table.
///
/// Seeds NO devices and zero counters.  Real cache accounting is wired through
/// [`register_device`] (one row per backing device the page cache tracks) and
/// the `record_hit`/`record_miss`/`record_eviction`/`record_readahead`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/pagecache` and the `pagecache` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional devices ("sda": 500k cached pages /
/// 100M hits / 5M misses / 2M evictions / 5000 dirty / 200 writeback / 10M
/// readahead / 8M useful; "nvme0n1": 2M cached / 500M hits / 10M misses / 5M
/// evictions / 2000 dirty / 50 writeback / 50M readahead / 45M useful) plus
/// invented aggregate totals (total_hits 600M, total_misses 15M, total_evictions
/// 7M, total_readahead 60M, total_readahead_useful 53M), which `/proc/pagecache`
/// (and the `per_device`/`hit_rate`/`readahead_rate` views) then displayed as if
/// they were real measured cache traffic — a 97.5% hit rate and 88% readahead
/// effectiveness conjured from nothing.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).
///
/// "Empty" here means empty of *recorded* rows.  The kernel's own page cache is
/// not recorded into this table at all — it is projected from
/// [`crate::mm::page_cache::stats`] when a reader asks, so `/proc/pagecache`
/// shows real hit/miss traffic from the first buffered read onwards even though
/// nothing ever calls [`record_hit`].  See the module docs for why that is a
/// projection and not a fast-path call.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        devices: Vec::new(),
        total_hits: 0,
        total_misses: 0,
        total_evictions: 0,
        total_readahead: 0,
        total_readahead_useful: 0,
        ops: 0,
    });
}

/// Register a backing device for page-cache accounting.
///
/// Creates a zeroed [`DeviceCacheStats`] row.  Duplicate device names return
/// [`KernelError::AlreadyExists`]; exceeding [`MAX_DEVICES`] returns
/// [`KernelError::ResourceExhausted`].  [`KERNEL_DEVICE`] is reserved for the
/// projected kernel page-cache row and is refused with
/// [`KernelError::AlreadyExists`] too — a second row by that name would make it
/// impossible to tell projected traffic from recorded traffic.
pub fn register_device(device: &str) -> KernelResult<()> {
    if device == KERNEL_DEVICE {
        return Err(KernelError::AlreadyExists);
    }
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.devices.iter().any(|d| d.device == device) {
            return Err(KernelError::AlreadyExists);
        }
        state.devices.push(DeviceCacheStats {
            device: String::from(device),
            cached_pages: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            dirty_pages: 0,
            writeback_pages: 0,
            readahead_pages: 0,
            readahead_useful: 0,
        });
        Ok(())
    })
}

/// Record a cache hit.
pub fn record_hit(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .devices
            .iter_mut()
            .find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.hits += 1;
        state.total_hits += 1;
        Ok(())
    })
}

/// Record a cache miss.
pub fn record_miss(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .devices
            .iter_mut()
            .find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.misses += 1;
        dev.cached_pages += 1; // Page now cached.
        state.total_misses += 1;
        Ok(())
    })
}

/// Record page eviction.
pub fn record_eviction(device: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .devices
            .iter_mut()
            .find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.evictions += pages;
        dev.cached_pages = dev.cached_pages.saturating_sub(pages);
        state.total_evictions += pages;
        Ok(())
    })
}

/// Record readahead pages.
pub fn record_readahead(device: &str, pages: u64, useful: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state
            .devices
            .iter_mut()
            .find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.readahead_pages += pages;
        dev.readahead_useful += useful;
        dev.cached_pages += pages;
        state.total_readahead += pages;
        state.total_readahead_useful += useful;
        Ok(())
    })
}

/// Get per-device cache stats.
///
/// Recorded devices first, in registration order, then the projected
/// [`KERNEL_DEVICE`] row if the kernel page cache has seen any traffic.  The
/// projected row is appended rather than prepended so that a caller holding an
/// index into a previous result is not silently re-pointed at a different
/// device when the kernel cache warms up.
pub fn per_device() -> Vec<DeviceCacheStats> {
    // Sampled before the lock: see `kernel_row`.
    let kernel = kernel_row();
    let mut rows = STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.devices.clone());
    if let Some(k) = kernel {
        rows.push(k);
    }
    rows
}

/// Cache hit rate as percentage * 100 (integer math).
///
/// Covers recorded devices and the kernel's own page cache together, because
/// that is the question the number is asked to answer — a "cache hit rate" that
/// excluded the actual cache would be the same defect this projection exists to
/// remove, just wearing a plausible number instead of a zero.
pub fn hit_rate() -> u64 {
    // Sampled before the lock: see `kernel_row`.
    let kernel = kernel_row();
    let guard = STATE.lock();
    let (rec_hits, rec_misses) = match guard.as_ref() {
        Some(s) => (s.total_hits, s.total_misses),
        None => (0, 0),
    };
    let hits = rec_hits.saturating_add(kernel.as_ref().map_or(0, |k| k.hits));
    let misses = rec_misses.saturating_add(kernel.as_ref().map_or(0, |k| k.misses));
    let total = hits.saturating_add(misses);
    if total == 0 {
        return 0;
    }
    hits.saturating_mul(10000) / total
}

/// Readahead effectiveness as percentage * 100.
///
/// Recorded devices only, and unlike [`hit_rate`] that is not a gap being
/// papered over: `mm::page_cache` performs no readahead, so there is no kernel
/// contribution to leave out.  If readahead is added there, this must start
/// including it or it will understate effectiveness rather than merely omit it.
pub fn readahead_rate() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            if s.total_readahead == 0 {
                return 0;
            }
            s.total_readahead_useful * 10000 / s.total_readahead
        }
        None => 0,
    }
}

/// Statistics: (device_count, total_hits, total_misses, total_evictions, total_readahead, ops).
///
/// Hits, misses and evictions combine recorded devices with the projected
/// kernel page cache, and `device_count` counts the projected row when it is
/// present, so the tuple always describes exactly the rows [`per_device`]
/// returns.  `total_readahead` is recorded-only: `mm::page_cache` has no
/// readahead to report.  `ops` counts operations against *this* table and is
/// deliberately not inflated by projection — nothing "operated" on it.
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    // Sampled before the lock: see `kernel_row`.
    let kernel = kernel_row();
    let guard = STATE.lock();
    let (devs, hits, misses, evictions, readahead, ops) = match guard.as_ref() {
        Some(s) => (
            s.devices.len(),
            s.total_hits,
            s.total_misses,
            s.total_evictions,
            s.total_readahead,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0),
    };
    match kernel {
        Some(k) => (
            devs.saturating_add(1),
            hits.saturating_add(k.hits),
            misses.saturating_add(k.misses),
            evictions.saturating_add(k.evictions),
            readahead,
            ops,
        ),
        None => (devs, hits, misses, evictions, readahead, ops),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Fetch a recorded row by name, ignoring the projected kernel row.
///
/// The self-test used to index `per_device()[0]`, which was only safe while the
/// table could not contain anything the test had not itself put there.  Now
/// that a projected row may be present, position is not identity — look rows up
/// by name.
fn recorded_row(device: &str) -> Option<DeviceCacheStats> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.devices.iter().find(|d| d.device == device).cloned())
}

/// The recorded halves of the totals, with no projection mixed in.
///
/// Exists so the self-test can assert exact numbers.  The public [`stats`] adds
/// the kernel page cache, which is live and can advance between two calls, so
/// asserting equality against it would be a flake waiting for a machine fast
/// enough to page something in mid-test.
fn recorded_totals() -> (usize, u64, u64, u64, u64) {
    STATE.lock().as_ref().map_or((0, 0, 0, 0, 0), |s| {
        (
            s.devices.len(),
            s.total_hits,
            s.total_misses,
            s.total_evictions,
            s.total_readahead,
        )
    })
}

pub fn self_test() {
    crate::serial_println!("pagecache::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/pagecache must never surface).  Resetting
    // first clears any residue from a prior `pagecache test` run so the totals
    // and rates asserted below are exact.
    //
    // "Exact" applies to the *recorded* side only.  The kernel page cache this
    // module projects is live: it is serving the VFS while this test runs, so
    // its counters can move between any two reads here.  Assertions against
    // `stats()`/`hit_rate()`/`per_device()` are therefore written as bounds and
    // as "contains", never as equality — and the exact arithmetic is checked
    // against `recorded_totals()`, which the projection cannot reach.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated *recorded* devices or counters, and
    // record on an unregistered device fails.  The projected kernel row may or
    // may not be present depending on whether anything has read a file yet,
    // which is exactly why this checks the recorded side.
    assert_eq!(recorded_totals(), (0, 0, 0, 0, 0));
    assert!(!per_device().iter().any(|d| d.device == "sda"));
    assert_eq!(readahead_rate(), 0);
    assert!(record_hit("sda").is_err()); // no phantom device exists yet
    crate::serial_println!("  [1/9] empty init: OK");

    // 2: Register — zeroed counters; dup fails.
    register_device("sda").expect("register");
    let d = recorded_row("sda").expect("find");
    assert_eq!(
        (d.hits, d.misses, d.evictions, d.cached_pages),
        (0, 0, 0, 0)
    );
    assert!(register_device("sda").is_err());
    crate::serial_println!("  [2/9] register: OK");

    // 3: Cache hit — per-device + total hits rise by one.
    record_hit("sda").expect("hit");
    assert_eq!(recorded_row("sda").expect("row").hits, 1);
    crate::serial_println!("  [3/9] hit: OK");

    // 4: Cache miss — miss caches the page (cached_pages +1).
    record_miss("sda").expect("miss");
    let d = recorded_row("sda").expect("row");
    assert_eq!(d.misses, 1);
    assert_eq!(d.cached_pages, 1);
    crate::serial_println!("  [4/9] miss: OK");

    // 5: Eviction — cached_pages drops, saturating at 0 on over-eviction.
    record_eviction("sda", 1).expect("evict");
    assert_eq!(recorded_row("sda").expect("row").cached_pages, 0);
    record_eviction("sda", 100).expect("evict over"); // saturating_sub guard
    assert_eq!(recorded_row("sda").expect("row").cached_pages, 0);
    crate::serial_println!("  [5/9] eviction: OK");

    // 6: Readahead + rate — 100 readahead / 80 useful = 80% effectiveness.
    // `readahead_rate` is recorded-only (mm::page_cache does no readahead), so
    // unlike `hit_rate` it is still exact here.
    record_readahead("sda", 100, 80).expect("readahead");
    let d = recorded_row("sda").expect("row");
    assert_eq!(d.readahead_pages, 100);
    assert_eq!(d.readahead_useful, 80);
    assert_eq!(readahead_rate(), 8000); // 80 * 10000 / 100
    crate::serial_println!("  [6/9] readahead + rate: OK");

    // 7: Unknown device → NotFound on every record path; the reserved kernel
    // row cannot be registered over.
    assert!(record_hit("fake").is_err());
    assert!(record_miss("fake").is_err());
    assert!(record_eviction("fake", 1).is_err());
    assert!(record_readahead("fake", 1, 1).is_err());
    assert!(register_device(KERNEL_DEVICE).is_err());
    crate::serial_println!("  [7/9] not found + reserved name: OK");

    // 8: Recorded totals are exact: 1 hit, 1 miss, 101 evicted pages (1 + 100
    // attempted; total counts all attempts), 100 readahead pages.
    assert_eq!(recorded_totals(), (1, 1, 1, 101, 100));
    crate::serial_println!("  [8/9] recorded totals: OK");

    // 9: Projection — the public views are the recorded numbers plus whatever
    // the kernel page cache reports, so each aggregate is at least its recorded
    // part, and the projected row appears in `per_device` exactly when the
    // kernel cache has traffic.  This is the rung that fails if the projection
    // is ever unwired again and /proc goes back to reporting only what nothing
    // writes.
    let (devs, hits, misses, evictions, readahead, ops) = stats();
    assert!(hits >= 1 && misses >= 1 && evictions >= 101);
    assert_eq!(readahead, 100); // recorded-only, so still exact
    assert!(ops > 0);
    let rows = per_device();
    assert!(rows.iter().any(|d| d.device == "sda"));
    let projected = rows.iter().find(|d| d.device == KERNEL_DEVICE);
    assert_eq!(devs, rows.len());
    match projected {
        Some(k) => {
            // A row exists only when it has something to say, and it must be
            // fully accounted for in the aggregate.
            assert!(k.hits > 0 || k.misses > 0 || k.evictions > 0 || k.cached_pages > 0);
            assert!(hits >= 1 + k.hits);
            crate::serial_println!(
                "  [9/9] projection: OK (kernel cache: {} hits, {} misses, {} resident)",
                k.hits,
                k.misses,
                k.cached_pages
            );
        }
        None => {
            // No file has been read through the cache yet on this boot.  Not a
            // failure, but say so plainly rather than passing silently: a run
            // that never sees the row has not exercised the projection.
            assert_eq!(devs, 1);
            crate::serial_println!("  [9/9] projection: OK (kernel cache idle, no row)");
        }
    }

    // Leave NO residue: clear the fixtures, then re-initialise so what is left
    // behind is an empty table rather than a dead one.  `init_defaults` runs
    // once at boot and nothing calls it again, so stopping at the reset would
    // leave `STATE` as `None` and make every later `record_*` return
    // `NotSupported` for the rest of the boot.  (The projected kernel row would
    // survive that, since it does not read `STATE` -- but recorded devices
    // would not, and silently losing half the table to having run a diagnostic
    // is not a tradeoff worth making.)
    {
        let mut guard = STATE.lock();
        *guard = None;
    }
    init_defaults();

    crate::serial_println!("pagecache::self_test() — all 9 tests passed");
}
