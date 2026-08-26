//! Cgroup Memory Statistics — per-cgroup memory accounting.
//!
//! Tracks per-cgroup memory usage, limits, RSS, cache, swap,
//! and OOM events. Essential for container memory isolation
//! and resource management.
//!
//! ## Architecture
//!
//! ```text
//! Cgroup memory monitoring
//!   → cgmem::create(name, limit) → create cgroup
//!   → cgmem::record_charge(cg, pages) → charge pages
//!   → cgmem::record_uncharge(cg, pages) → uncharge pages
//!   → cgmem::per_cgroup() → per-cgroup stats
//!
//! Integration:
//!   → memcg (memory cgroup)
//!   → cgiostat (cgroup I/O)
//!   → oomkiller (OOM killer)
//!   → mempress (memory pressure)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-cgroup memory stats.
#[derive(Debug, Clone)]
pub struct CgroupMemStats {
    pub cg_id: u32,
    pub name: String,
    pub limit_pages: u64,
    pub usage_pages: u64,
    pub rss_pages: u64,
    pub cache_pages: u64,
    /// Pages currently in swap. Written only by [`record_swap`].
    pub swap_pages: u64,
    pub charges: u64,
    pub uncharges: u64,
    pub oom_kills: u64,
    /// Times a charge left `usage_pages` above `limit_pages`.
    ///
    /// This is the *only* consequence of the ceiling supplied to [`create`]:
    /// nothing rejects a charge that exceeds it. Until it was surfaced, the
    /// counter was incremented by [`record_charge`] and read by nothing outside
    /// this module's own self-test, which meant a cgroup could sit permanently
    /// over its limit with no reader anywhere able to say so. See
    /// `A-CGMEM-THE-LIMIT-IS-COMPARED-AND-THE-RESULT-IS-NEVER-SHOWN`.
    pub high_events: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CGROUPS: usize = 256;

struct State {
    cgroups: Vec<CgroupMemStats>,
    next_id: u32,
    total_charges: u64,
    total_uncharges: u64,
    total_oom_kills: u64,
    total_high_events: u64,
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
// Public API
// ---------------------------------------------------------------------------

/// Initialise the per-cgroup memory-statistics state.
///
/// Starts with no cgroups and all charge/uncharge/OOM totals at zero. The
/// `/proc/cgmem` generator and the `cgmem` kshell command surface this
/// table as if it reflects real per-cgroup page accounting, so seeding it
/// with invented cgroups and charge counts would be fabricated procfs
/// data. Cgroups are created at runtime through [`create`] and pages are
/// accounted only through real [`record_charge`] / [`record_uncharge`]
/// calls.
///
/// (Previously this seeded three fictional cgroups — "root" 500k usage
/// pages / 10M charges; "system" 1M limit / 400k usage / 5M charges / 2
/// OOM; "user" 2M limit / 800k usage / 20M charges / 5 OOM — plus invented
/// totals (35M charges, 33.3M uncharges, 7 OOM kills).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        cgroups: Vec::new(),
        next_id: 1,
        total_charges: 0,
        total_uncharges: 0,
        total_oom_kills: 0,
        total_high_events: 0,
        ops: 0,
    });
}

/// Create a cgroup.
pub fn create(name: &str, limit_pages: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.cgroups.len() >= MAX_CGROUPS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.cgroups.push(CgroupMemStats {
            cg_id: id,
            name: String::from(name),
            limit_pages,
            usage_pages: 0,
            rss_pages: 0,
            cache_pages: 0,
            swap_pages: 0,
            charges: 0,
            uncharges: 0,
            oom_kills: 0,
            high_events: 0,
        });
        Ok(id)
    })
}

/// Remove a cgroup.
pub fn remove(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state
            .cgroups
            .iter()
            .position(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        state.cgroups.remove(idx);
        Ok(())
    })
}

/// Charge pages to a cgroup.
pub fn record_charge(cg_id: u32, pages: u64, is_cache: bool) -> KernelResult<()> {
    with_state(|state| {
        let c = state
            .cgroups
            .iter_mut()
            .find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        c.usage_pages += pages;
        if is_cache {
            c.cache_pages += pages;
        } else {
            c.rss_pages += pages;
        }
        c.charges += 1;
        if c.usage_pages > c.limit_pages {
            c.high_events += 1;
            state.total_high_events += 1;
        }
        state.total_charges += 1;
        Ok(())
    })
}

/// Uncharge pages from a cgroup.
///
/// Un-charges no more from `usage_pages` than actually left the named bucket.
///
/// The previous form subtracted `pages` from the aggregate and from the bucket
/// with two *independent* `saturating_sub` calls, which silently broke the
/// invariant [`record_charge`] maintains — `usage_pages == rss_pages +
/// cache_pages`, since those two are the only fields either function writes.
/// Un-charging 50 cache pages from a cgroup holding 100 rss and no cache
/// deflated the aggregate to 50 and the buckets not at all, leaving a cgroup
/// reporting 50 pages of usage of which 100 were resident.
///
/// That is not merely an inconsistent display. `usage_pages` is the number
/// [`record_charge`] compares against `limit_pages`, so the divergence bought
/// the cgroup headroom it did not have — an error in the permissive direction,
/// which is the direction that does not announce itself.
///
/// Clamping to the bucket rather than *rejecting* an over-large un-charge is a
/// real choice and the weaker one in one respect: under-un-charging is itself a
/// quiet substitution of a number the caller did not ask for. It is preferred
/// because this function has exactly one job — keeping the two views of the
/// same memory consistent — and a caller who un-charges more than a bucket
/// holds has already lost track; refusing would leave that caller's accounting
/// wrong with no way to resynchronise, whereas clamping restores the invariant
/// on the spot. See
/// `A-CGMEM-UNCHARGE-SATURATES-PER-BUCKET-AND-IN-AGGREGATE-INDEPENDENTLY`.
pub fn record_uncharge(cg_id: u32, pages: u64, is_cache: bool) -> KernelResult<()> {
    with_state(|state| {
        let c = state
            .cgroups
            .iter_mut()
            .find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        let actual = if is_cache {
            let n = pages.min(c.cache_pages);
            c.cache_pages -= n;
            n
        } else {
            let n = pages.min(c.rss_pages);
            c.rss_pages -= n;
            n
        };
        // `actual <= the bucket <= usage_pages` by the invariant above, so this
        // cannot underflow and the invariant still holds afterwards.
        c.usage_pages = c.usage_pages.saturating_sub(actual);
        c.uncharges += 1;
        state.total_uncharges += 1;
        Ok(())
    })
}

/// Record pages moving to or from swap.
///
/// `swap_in` false means pages were swapped *out* — they leave memory for swap,
/// so `swap_pages` rises and the resident buckets are untouched (the caller
/// reports the memory side with [`record_uncharge`], since only that caller
/// knows which bucket the pages came from). `swap_in` true means they came back
/// and `swap_pages` falls, clamped at zero.
///
/// This function exists because `swap_pages` was a field written by nothing.
/// `create` set it to 0 and no code path ever assigned it again, yet both
/// `cgmem list` and `/proc/cgmem` printed it on every line — so every cgroup
/// the system had ever displayed reported `swap=0`, not because nothing was
/// swapped but because nothing could ever make it say otherwise. A statistic
/// that is structurally constant is worse than an absent one: it reads as
/// evidence. The alternative was to delete the field and its two columns; see
/// `design-decisions.md` §608 for why completing it won.
pub fn record_swap(cg_id: u32, pages: u64, swap_in: bool) -> KernelResult<()> {
    with_state(|state| {
        let c = state
            .cgroups
            .iter_mut()
            .find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        if swap_in {
            c.swap_pages = c.swap_pages.saturating_sub(pages);
        } else {
            c.swap_pages = c.swap_pages.saturating_add(pages);
        }
        Ok(())
    })
}

/// Record an OOM kill in a cgroup.
pub fn record_oom(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state
            .cgroups
            .iter_mut()
            .find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        c.oom_kills += 1;
        state.total_oom_kills += 1;
        Ok(())
    })
}

/// Per-cgroup stats.
pub fn per_cgroup() -> Vec<CgroupMemStats> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.cgroups.clone())
}

/// Statistics: (cgroup_count, total_charges, total_uncharges, total_oom_kills,
/// total_high_events, ops).
///
/// `total_high_events` was added when [`CgroupMemStats::high_events`] was
/// surfaced: a system-wide count of "a charge left some cgroup over its
/// ceiling" is the one number that makes the ceilings supplied to [`create`]
/// visible at a glance, rather than requiring a walk of every cgroup.
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.cgroups.len(),
            s.total_charges,
            s.total_uncharges,
            s.total_oom_kills,
            s.total_high_events,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cgmem::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live cgroup table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated cgroups, all totals zero.
    assert_eq!(per_cgroup().len(), 0);
    let (cg0, charges0, uncharges0, ooms0, high0, _) = stats();
    assert_eq!((cg0, charges0, uncharges0, ooms0, high0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/10] empty defaults: OK");

    // 2: Create — ids monotonic starting at 1; unknown-id charge errors.
    let id = create("test_cg", 10_000).expect("create");
    assert_eq!(id, 1);
    assert_eq!(per_cgroup().len(), 1);
    assert!(record_charge(99, 1, false).is_err());
    crate::serial_println!("  [2/10] create: OK");

    // 3: Charge RSS — usage and rss tracked, cache untouched.
    record_charge(id, 100, false).expect("charge_rss");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id)
        .expect("cg");
    assert_eq!(c.usage_pages, 100);
    assert_eq!(c.rss_pages, 100);
    assert_eq!(c.cache_pages, 0);
    crate::serial_println!("  [3/10] charge rss: OK");

    // 4: Charge cache — separate bucket, usage is the sum.
    record_charge(id, 50, true).expect("charge_cache");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id)
        .expect("cg");
    assert_eq!(c.cache_pages, 50);
    assert_eq!(c.usage_pages, 150);
    crate::serial_println!("  [4/10] charge cache: OK");

    // 5: Uncharge RSS — usage and rss drop, cache unaffected.
    record_uncharge(id, 30, false).expect("uncharge");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id)
        .expect("cg");
    assert_eq!(c.rss_pages, 70);
    assert_eq!(c.cache_pages, 50);
    assert_eq!(c.usage_pages, 120);
    crate::serial_println!("  [5/10] uncharge: OK");

    // 6: High-event detection when usage exceeds the page limit.
    let id2 = create("small_cg", 10).expect("create2");
    record_charge(id2, 25, false).expect("charge over limit");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id2)
        .expect("cg2");
    assert_eq!(c.high_events, 1);
    record_oom(id2).expect("oom");
    assert_eq!(
        per_cgroup()
            .into_iter()
            .find(|c| c.cg_id == id2)
            .expect("cg2")
            .oom_kills,
        1
    );
    crate::serial_println!("  [6/10] high event + oom: OK");

    // 7: Remove — gone, and double-remove errors.
    remove(id).expect("remove");
    remove(id2).expect("remove2");
    assert_eq!(per_cgroup().len(), 0);
    assert!(remove(id).is_err());
    crate::serial_println!("  [7/10] remove: OK");

    // 8: Stats reflect only the real activity above (3 charges, 1 uncharge,
    //    1 OOM; both cgroups removed).
    let (cgroups, charges, uncharges, ooms, high, ops) = stats();
    assert_eq!(cgroups, 0);
    assert_eq!(charges, 3);
    assert_eq!(uncharges, 1);
    assert_eq!(ooms, 1);
    // Exactly one charge above a ceiling: test 6's 25 pages into a 10-page
    // cgroup. The other two charges were inside `test_cg`'s 10000-page limit.
    assert_eq!(high, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/10] stats: OK");

    // 9: Un-charging more than a bucket holds keeps `usage == rss + cache`.
    //
    //    Both halves used to break it, because the aggregate and the bucket
    //    floored at zero independently: the aggregate fell by the full `pages`
    //    while the bucket fell by however much it had.
    let id3 = create("clamp_cg", 10_000).expect("create3");
    record_charge(id3, 100, false).expect("charge3");
    // (a) Wrong bucket entirely: no cache pages exist, so nothing may leave
    //     the aggregate either. This used to report 50 pages of usage in a
    //     cgroup with 100 resident.
    record_uncharge(id3, 50, true).expect("uncharge_empty_bucket");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id3)
        .expect("cg3");
    assert_eq!(c.usage_pages, 100);
    assert_eq!(c.rss_pages, 100);
    assert_eq!(c.cache_pages, 0);
    assert_eq!(c.usage_pages, c.rss_pages + c.cache_pages);
    // (b) Right bucket, over-large: the clamp applies to both sides equally,
    //     so the cgroup empties rather than going negative on one side only.
    record_uncharge(id3, 150, false).expect("uncharge_over");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id3)
        .expect("cg3");
    assert_eq!((c.usage_pages, c.rss_pages, c.cache_pages), (0, 0, 0));
    remove(id3).expect("remove3");
    crate::serial_println!("  [9/10] uncharge clamps to the bucket: OK");

    // 10: `swap_pages` can actually be written.
    //
    //     Until `record_swap` existed, this field was set to 0 by `create` and
    //     assigned by nothing, while both `cgmem list` and `/proc/cgmem`
    //     printed it on every line — so `swap=0` was not a measurement, it was
    //     a constant wearing a measurement's clothes.
    let id4 = create("swap_cg", 10_000).expect("create4");
    record_swap(id4, 40, false).expect("swap_out");
    record_swap(id4, 15, true).expect("swap_in");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id4)
        .expect("cg4");
    assert_eq!(c.swap_pages, 25);
    // Swapping in more than is out floors at zero rather than wrapping; the
    // resident buckets are the caller's business, so they stay untouched.
    record_swap(id4, 999, true).expect("swap_in_over");
    let c = per_cgroup()
        .into_iter()
        .find(|c| c.cg_id == id4)
        .expect("cg4");
    assert_eq!((c.swap_pages, c.usage_pages, c.rss_pages), (0, 0, 0));
    assert!(record_swap(99, 1, false).is_err());
    remove(id4).expect("remove4");
    crate::serial_println!("  [10/10] swap accounting is writable: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("cgmem::self_test() — all 10 tests passed");
}
