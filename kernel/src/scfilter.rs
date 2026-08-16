//! Syscall filtering (seccomp-equivalent) for container sandboxing.
//!
//! Provides per-process syscall allow/deny lists.  When a process has
//! a filter installed, every syscall is checked against the filter
//! before dispatch.  Denied syscalls return `PermissionDenied`.
//!
//! ## Design
//!
//! Unlike Linux seccomp-BPF which uses a bytecode VM, we use simple
//! bitmap-based filters — each filter is a 1000-bit bitmap (one bit
//! per syscall number).  This is:
//!
//! - O(1) per syscall check (single array index + bit test)
//! - Zero-allocation on the hot path
//! - Simple to audit and verify
//!
//! Filters are inherited on fork (child gets a copy of parent's filter).
//! Filters can only be tightened (a process can deny additional syscalls
//! but never re-allow one that was denied).
//!
//! ## Integration Points
//!
//! - **syscall/dispatch.rs**: Before looking up the handler, call
//!   `scfilter::check(task_id, syscall_nr)`.  If it returns `false`,
//!   return `PermissionDenied` without invoking the handler.
//! - **container.rs**: When creating a container, install a filter
//!   that allows only the syscalls the container needs.
//! - **proc/pcb.rs**: On fork, copy the parent's filter to the child.
//!
//! ## References
//!
//! - Linux seccomp(2), seccomp_rule_add(3)
//! - Design spec: capability-based security + container isolation

extern crate alloc;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of syscall numbers we track.
///
/// **Derived from `syscall::number::MAX_SYSCALL_NR`, never copied.**  This
/// used to be a hand-written `1000` under a comment claiming it matched the
/// dispatch table's bound.  It did not: the dispatch table had grown to 1100,
/// and nothing checked, because a comment asserting two numbers are equal is
/// not a check — it is a wish.
///
/// The consequence was not a slow path or a missed filter but a hard denial.
/// [`check`] denies any `nr >= MAX_SYSCALL_NR` (the bitmap cannot represent
/// it, and a filter that cannot represent a syscall must not claim to allow
/// it), so once [`init`] ran, *every* syscall in `1000..1100` returned
/// `PermissionDenied` for *every* process, filtered or not.  That range is
/// the entire DRM/graphics interface (`SYS_DRM_OPEN` = 1000 through
/// `SYS_DRM_ATOMIC_COMMIT` = 1060) plus `SYS_PROCESS_SET_EXEC_FDS` (1061),
/// `SYS_SIGNAL_STOP_SELF` (1062) and `SYS_PROCESS_WAIT_STATUS` (1063).
///
/// Deriving the constant makes the two impossible to drift apart, which a
/// `const assert!(a == b)` would only *detect*.  See `test_top_of_range`.
pub const MAX_SYSCALL_NR: usize = crate::syscall::number::MAX_SYSCALL_NR;

/// Maximum number of process filters.
///
/// One per active process.  Processes without a filter are unfiltered.
pub const MAX_FILTERS: usize = 128;

/// Number of `u64` words needed for the bitmap.
///
/// Derived, so a change to `MAX_SYSCALL_NR` resizes the bitmap rather than
/// silently truncating it.  (At 1100 syscalls: 18 words, 144 bytes.)
#[allow(clippy::arithmetic_side_effects)]
const BITMAP_WORDS: usize = MAX_SYSCALL_NR.div_ceil(64);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Process ID type (matches scheduler's TaskId).
type Pid = u64;

/// A syscall filter bitmap.
///
/// Each bit position corresponds to a syscall number.
/// Bit set = allowed, bit clear = denied.
///
/// A fresh "allow-all" filter has all bits set.
/// A "deny-all" filter has all bits clear.
#[derive(Clone)]
struct FilterBitmap {
    /// Bitmap words.  Bit N corresponds to syscall number N.
    words: [u64; BITMAP_WORDS],
}

impl FilterBitmap {
    /// Create a filter that allows all syscalls.
    const fn allow_all() -> Self {
        Self {
            words: [u64::MAX; BITMAP_WORDS],
        }
    }

    /// Create a filter that denies all syscalls.
    const fn deny_all() -> Self {
        Self {
            words: [0; BITMAP_WORDS],
        }
    }

    /// Check if a syscall number is allowed.
    #[inline]
    fn is_allowed(&self, nr: usize) -> bool {
        if nr >= MAX_SYSCALL_NR {
            return false;
        }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        // SAFETY: word_idx < BITMAP_WORDS because nr < MAX_SYSCALL_NR
        // and BITMAP_WORDS = ceil(MAX_SYSCALL_NR / 64).
        (self.words[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Allow a specific syscall number.
    fn allow(&mut self, nr: usize) {
        if nr >= MAX_SYSCALL_NR {
            return;
        }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        self.words[word_idx] |= 1u64 << bit_idx;
    }

    /// Deny a specific syscall number.
    fn deny(&mut self, nr: usize) {
        if nr >= MAX_SYSCALL_NR {
            return;
        }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        self.words[word_idx] &= !(1u64 << bit_idx);
    }

    /// Count how many syscalls are allowed.
    fn count_allowed(&self) -> usize {
        let full_words = MAX_SYSCALL_NR / 64;
        let remaining_bits = MAX_SYSCALL_NR % 64;

        let mut count = 0usize;

        // Count all bits in fully-covered words.
        for word in self.words.iter().take(full_words) {
            count = count.saturating_add(word.count_ones() as usize);
        }

        // For the partial final word, mask off bits beyond MAX_SYSCALL_NR.
        if remaining_bits > 0 {
            if let Some(&last_word) = self.words.get(full_words) {
                // Keep only the lower `remaining_bits` bits.
                let mask = (1u64 << remaining_bits).wrapping_sub(1);
                count = count.saturating_add((last_word & mask).count_ones() as usize);
            }
        }

        count
    }

    /// Intersect two filters (AND).  The result allows only syscalls
    /// that both filters allow.
    fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::deny_all();
        for i in 0..BITMAP_WORDS {
            result.words[i] = self.words[i] & other.words[i];
        }
        result
    }
}

/// A filter entry attached to a process.
struct FilterEntry {
    /// Whether this slot is active.
    active: bool,
    /// Process (task) ID.
    pid: Pid,
    /// The filter bitmap.
    bitmap: FilterBitmap,
    /// How many syscalls were denied by this filter (audit counter).
    deny_count: u64,
}

impl FilterEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            pid: 0,
            bitmap: FilterBitmap::allow_all(),
            deny_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

// There used to be an `ENABLED: AtomicBool` here, set once by `init` and read
// by `check` as a "subsystem ready" gate.  It is gone because `check` no longer
// reads it, and a flag that is only ever *written* is worse than no flag: it
// reads like a kill switch, so a later change could plausibly add a `disable()`
// that turns nothing off while appearing to disable syscall filtering.  The two
// things it actually meant are both still checked — "table not yet allocated"
// by the `guard.as_ref()` match, and "nothing to check" by `ACTIVE_FILTERS`,
// which cannot be non-zero before `init` because nothing can be installed.

/// How many filter slots are currently active, mirrored outside the lock.
///
/// This exists purely so [`check`] can answer the overwhelmingly common
/// question — *does any process on this system have a filter at all?* — with a
/// single atomic load, taking no lock and touching no table.  It is maintained
/// by every mutation path while that path holds `TABLE`, so the lock still
/// serialises the writes; the atomic only publishes the result.
///
/// [`verify_index`] cross-checks it against a linear count of active slots, so
/// a mutation path that forgets to maintain it is caught by the self-test
/// rather than by a process silently escaping its sandbox.
static ACTIVE_FILTERS: AtomicUsize = AtomicUsize::new(0);

/// Buckets in the pid → slot index.  Sized for < 50% load at `MAX_FILTERS`.
const FILTER_HASH_SHIFT: u32 = 8;
const FILTER_HASH_BUCKETS: usize = 1 << FILTER_HASH_SHIFT;

const _: () = assert!(
    FILTER_HASH_BUCKETS >= MAX_FILTERS * 2,
    "filter index must stay under 50% load or probe runs grow without bound"
);
// A bucket stores `slot + 1` in a `u8`, so the largest storable slot is 254.
const _: () = assert!(
    MAX_FILTERS < 255,
    "a filter slot index plus one must fit in the u8 used by the hash index"
);

/// Map a pid to a starting bucket.
///
/// Fibonacci hashing (Knuth 6.4): multiply by 2^64/phi and keep the **high**
/// bits.  The high bits matter — pids are allocated as a dense ascending
/// sequence, so keeping the low bits would map the first 128 processes onto
/// 128 consecutive buckets and turn every miss into a long probe run, which is
/// the linear scan this index exists to remove, wearing a hash's clothes.
const fn filter_bucket(pid: Pid) -> usize {
    let mixed = pid.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (mixed >> (64 - FILTER_HASH_SHIFT)) as usize
}

struct FilterTable {
    filters: [FilterEntry; MAX_FILTERS],
    /// Open-addressed pid → slot index.  `0` = empty, `n` = slot `n - 1`.
    ///
    /// Rebuilt wholesale by [`FilterTable::rebuild_index`] after every
    /// mutation rather than maintained incrementally.  Incremental deletion
    /// from a linear-probe table needs either tombstones (which accumulate
    /// across install/remove cycles until the table degrades back to a scan)
    /// or backward-shift deletion (easy to get subtly wrong, and a subtle bug
    /// here means a filtered process stops being found — i.e. a sandbox
    /// escape, failing *open*).  Mutations happen at process create/exit;
    /// lookups happen on every syscall.  Paying 256 bytes of memset on the
    /// rare path to keep the hot path both O(1) and obviously correct is the
    /// right side of that trade.
    index: [u8; FILTER_HASH_BUCKETS],
}

impl FilterTable {
    // `const fn` so the all-empty table is materialized in read-only
    // static memory at compile time (see `init`), not built on the stack.
    const fn new() -> Self {
        // FilterEntry::empty() is a const fn producing a valid default
        // state.  This avoids a loop over MAX_FILTERS.
        Self {
            filters: [const { FilterEntry::empty() }; MAX_FILTERS],
            index: [0u8; FILTER_HASH_BUCKETS],
        }
    }

    /// Rebuild the pid → slot index from the filter array, and republish
    /// [`ACTIVE_FILTERS`].
    ///
    /// Must be called by every path that changes any slot's `active` or `pid`.
    /// Changing only a slot's `bitmap` or `deny_count` does not move it in the
    /// index, so `deny`/`allow`/`tighten` do not need it.
    fn rebuild_index(&mut self) {
        self.index = [0u8; FILTER_HASH_BUCKETS];
        let mut active = 0usize;
        for (slot, entry) in self.filters.iter().enumerate() {
            if !entry.active {
                continue;
            }
            active = active.saturating_add(1);
            let mut bucket = filter_bucket(entry.pid);
            // Terminates: the const assert above keeps load < 50%, so an empty
            // bucket always exists.  The bound is belt-and-braces against a
            // future MAX_FILTERS change that violates it.
            for _ in 0..FILTER_HASH_BUCKETS {
                let Some(cell) = self.index.get_mut(bucket) else {
                    break;
                };
                if *cell == 0 {
                    // `slot < MAX_FILTERS < 255` by the const assert above, so
                    // `slot + 1` fits in a u8 without truncation.
                    *cell = u8::try_from(slot.saturating_add(1)).unwrap_or(0);
                    break;
                }
                bucket = bucket.wrapping_add(1) & (FILTER_HASH_BUCKETS - 1);
            }
        }
        // Release: pairs with the Acquire load in `check`, so a CPU that sees a
        // non-zero count also sees the index and slots that explain it.
        ACTIVE_FILTERS.store(active, Ordering::Release);
    }

    /// Find the slot holding `pid`'s filter, in O(1) expected time.
    ///
    /// Returns `None` if `pid` is unfiltered.  Because entries are only ever
    /// inserted into a freshly-rebuilt table, probe runs are contiguous and
    /// stopping at the first empty bucket is correct.
    fn lookup(&self, pid: Pid) -> Option<usize> {
        let mut bucket = filter_bucket(pid);
        for _ in 0..FILTER_HASH_BUCKETS {
            let cell = *self.index.get(bucket)?;
            if cell == 0 {
                return None;
            }
            let slot = usize::from(cell).checked_sub(1)?;
            let entry = self.filters.get(slot)?;
            if entry.active && entry.pid == pid {
                return Some(slot);
            }
            bucket = bucket.wrapping_add(1) & (FILTER_HASH_BUCKETS - 1);
        }
        None
    }

    /// The linear scan this index replaced, kept as the reference
    /// implementation the index must agree with.  See [`verify_index`].
    fn lookup_by_scan(&self, pid: Pid) -> Option<usize> {
        self.filters.iter().position(|e| e.active && e.pid == pid)
    }
}

static TABLE: Mutex<Option<Box<FilterTable>>> = Mutex::new(None);

/// Initialize the syscall filter subsystem.
///
/// Uses heap allocation — `FilterTable` is ~19 KiB (128 entries × ~152 bytes),
/// too large for the boot stack under debug builds.
pub fn init() {
    let mut table = TABLE.lock();
    // Allocate on the heap to avoid stack overflow (FilterTable is ~19 KiB).
    //
    // `EMPTY` is a `const`, so the all-empty table lives in read-only
    // static memory; `Box::new` copies it straight to the heap without
    // first constructing a ~19 KiB temporary on the kernel stack
    // (a plain `Box::new(FilterTable::new())` would build that temporary
    // on the stack, which is what we must avoid here).
    const EMPTY: FilterTable = FilterTable::new();
    let fresh = table.insert(Box::new(EMPTY));
    // Republish the (zero) active count and the (empty) index from the table
    // that is actually installed, rather than assuming the statics already
    // agree with it.  `init` can in principle run over a populated table, and
    // an `ACTIVE_FILTERS` left non-zero across that would make every syscall
    // take the lock forever after; left too high it is merely slow, but the
    // symmetric mistake elsewhere is a sandbox that stops being enforced, so
    // the two are kept in sync at exactly one place: `rebuild_index`.
    fresh.rebuild_index();
    serial_println!("[scfilter] Initialized ({} max filters)", MAX_FILTERS);
}

// ---------------------------------------------------------------------------
// Hot path: check
// ---------------------------------------------------------------------------

/// Check if a syscall is allowed for a given task.
///
/// Returns `true` if the syscall should proceed, `false` if it should
/// be denied with `PermissionDenied`.
///
/// This is the hot-path function called on every syscall, so its cost is a
/// tax on the whole system whether or not anything is sandboxed:
///
/// - **Nothing on the system is filtered** (the usual case): one atomic load,
///   no lock, no table access.
/// - **Something is filtered**: one lock acquire plus an O(1) hash lookup and
///   a bit test — the same cost whether *this* task is the filtered one or not.
/// - **Subsystem uninitialised/disabled**: falls out of the same atomic load,
///   since nothing can be installed while it is off.
///
/// ## History
///
/// This function used to take the lock unconditionally and then walk **all
/// `MAX_FILTERS` (128) slots** of the ~19 KiB table, because the scan can only
/// conclude "unfiltered" by exhausting it.  With zero filters installed — the
/// state the kernel boots in and stays in — that was the cost of *every*
/// syscall.  Measured: 299 ns of a 525 ns `syscall_dispatch`, i.e. **57% of
/// kernel-side syscall dispatch was a linear scan over an empty table.**
///
/// After this rewrite: 44 ns of a 393 ns dispatch — 11% instead of 57%.  Read
/// the *share*, not the nanoseconds: the whole benchmark suite drifted +29%
/// (median over 64 benchmarks) between those two boots, so the raw 525→393
/// understates the change.  Drift-adjusted, dispatch fell to 0.58× — the
/// third-largest drop of 64 benchmarks in a boot where the median moved the
/// other way.
///
/// The doc comment it replaces claimed "returns `true` (O(1))" on one line and
/// "~5 ns (atomic load + **linear scan miss**)" four lines later.  Those cannot
/// both be true, and the discrepancy is the tell: the `~5 ns` was an estimate
/// typeset as a measurement, and nobody had run it.  Numbers in this comment
/// come from `bench_syscall_dispatch_breakdown`, which measures each dispatch
/// stage directly.
///
/// ## Failure direction
///
/// A bug in the index makes this function fail **open**: a filter that cannot
/// be found is a sandbox that is not enforced, silently.  That is why the
/// linear scan survives as [`FilterTable::lookup_by_scan`] and why
/// [`verify_index`] cross-checks every active pid against it on every boot —
/// a check that cannot fire is indistinguishable from a check that passes.
#[inline]
pub fn check(task_id: u64, syscall_nr: u64) -> bool {
    // Range check FIRST, deliberately ahead of the no-filters fast path below.
    // It is a compare against a constant — no memory access, no lock — so it
    // costs nothing to keep here, and putting it after the fast path would
    // quietly make the answer depend on unrelated state: an out-of-range
    // syscall number would be "allowed" or "denied" according to whether some
    // *other* process happened to be sandboxed.
    //
    // Hoisting it here is also what exposed a shipped bug. It previously sat
    // behind an `ENABLED` early-return, so it was dead code until
    // `scfilter::init()` ran — and `MAX_SYSCALL_NR` had drifted 100 below the
    // dispatch table's bound, meaning that from `init()` onward this branch
    // denied every syscall in 1000..1100 to every process. Because the branch
    // was unreachable during the early-boot self-tests, moving it in front of
    // the fast path turned a silent post-boot denial into an immediate,
    // reproducible self-test failure. The constant is now *derived* rather
    // than copied, so the drift cannot recur.
    let nr = syscall_nr as usize;
    if nr >= MAX_SYSCALL_NR {
        return false;
    }

    // Fast path: no process on this system has a filter, so there is nothing
    // to check.  `install`/`copy_filter` publish the count with Release after
    // filling the slot, so seeing a non-zero count here means the slot and the
    // index that finds it are both visible.
    //
    // Subsumes the old `ENABLED` check: a filter cannot be installed while the
    // subsystem is uninitialised, so `ACTIVE_FILTERS == 0` covers that case too
    // and this is one atomic load rather than two.
    if ACTIVE_FILTERS.load(Ordering::Acquire) == 0 {
        return true;
    }

    let mut guard = TABLE.lock();
    let Some(table) = guard.as_mut() else {
        return true; // Not initialized.
    };

    // O(1): hash lookup instead of walking every slot.
    let Some(slot) = table.lookup(task_id) else {
        return true; // No filter for this task.
    };
    let Some(entry) = table.filters.get_mut(slot) else {
        // `lookup` only returns in-range slots, so this is unreachable; allow
        // rather than panic, matching the "no filter" outcome for the same
        // task, and leave no `unwrap` on the syscall hot path.
        return true;
    };

    let allowed = entry.bitmap.is_allowed(nr);
    if !allowed {
        entry.deny_count = entry.deny_count.saturating_add(1);
    }
    allowed
}

// ---------------------------------------------------------------------------
// Public API: filter management
// ---------------------------------------------------------------------------

/// Install an allow-all filter for a process.
///
/// The process starts with everything allowed; use [`deny`] to
/// restrict specific syscalls.  This is the typical pattern:
///
/// ```ignore
/// scfilter::install(pid)?;
/// scfilter::deny(pid, SYS_PORT_READ);  // No raw port I/O
/// scfilter::deny(pid, SYS_PORT_WRITE);
/// scfilter::deny(pid, SYS_IRQ_REGISTER); // No direct IRQ access
/// ```
///
/// # Errors
///
/// - [`KernelError::AlreadyExists`] if a filter already exists.
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn install(pid: Pid) -> KernelResult<()> {
    install_with(pid, FilterBitmap::allow_all())
}

/// Shared body of [`install`] and [`install_deny_all`].
///
/// The two differ only in the starting bitmap, and factoring them together is
/// not cosmetic: each has to claim a slot *and* republish the index, and two
/// copies of that sequence is two places for a future edit to update one and
/// not the other — which would fail open (see [`check`]).
fn install_with(pid: Pid, bitmap: FilterBitmap) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    if table.lookup(pid).is_some() {
        return Err(KernelError::AlreadyExists);
    }

    // Find a free slot.
    let Some(slot) = table.filters.iter().position(|e| !e.active) else {
        return Err(KernelError::ResourceExhausted);
    };
    let Some(entry) = table.filters.get_mut(slot) else {
        return Err(KernelError::ResourceExhausted);
    };
    entry.active = true;
    entry.pid = pid;
    entry.bitmap = bitmap;
    entry.deny_count = 0;
    table.rebuild_index();
    Ok(())
}

/// Install a deny-all filter for a process.
///
/// The process starts with everything denied; use [`allow`] to
/// enable specific syscalls.  This is the restrictive pattern:
///
/// ```ignore
/// scfilter::install_deny_all(pid)?;
/// scfilter::allow(pid, SYS_EXIT);
/// scfilter::allow(pid, SYS_CONSOLE_WRITE);
/// scfilter::allow(pid, SYS_CONSOLE_READ_CHAR);
/// ```
///
/// # Errors
///
/// - [`KernelError::AlreadyExists`] if a filter already exists.
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn install_deny_all(pid: Pid) -> KernelResult<()> {
    install_with(pid, FilterBitmap::deny_all())
}

/// Remove the filter for a process.
///
/// Called on process exit.  After this, the process is unfiltered
/// (but typically it's exiting anyway).
pub fn remove(pid: Pid) {
    let mut guard = TABLE.lock();
    let Some(table) = guard.as_mut() else {
        return;
    };

    let Some(slot) = table.lookup(pid) else {
        return;
    };
    if let Some(entry) = table.filters.get_mut(slot) {
        entry.active = false;
        // Clear the identity too.  Leaving a stale `pid` on a freed slot is
        // harmless while `active` is false, but it means a debug dump of the
        // table shows a process that is no longer filtered as though it were,
        // and it is one `active` check away from being a real lookup hit.
        entry.pid = 0;
    }
    table.rebuild_index();
}

/// Deny a specific syscall for a process.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn deny(pid: Pid, syscall_nr: u64) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    // Bitmap-only change: the slot keeps its pid, so the index still points at
    // it and does not need rebuilding.
    let slot = table.lookup(pid).ok_or(KernelError::NotFound)?;
    let entry = table.filters.get_mut(slot).ok_or(KernelError::NotFound)?;
    entry.bitmap.deny(syscall_nr as usize);
    Ok(())
}

/// Allow a specific syscall for a process.
///
/// Note: this only works if the filter was installed with
/// [`install_deny_all`].  If the filter was installed with [`install`]
/// (allow-all), all syscalls are already allowed.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn allow(pid: Pid, syscall_nr: u64) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    let slot = table.lookup(pid).ok_or(KernelError::NotFound)?;
    let entry = table.filters.get_mut(slot).ok_or(KernelError::NotFound)?;
    entry.bitmap.allow(syscall_nr as usize);
    Ok(())
}

/// Copy a filter from one process to another (fork inheritance).
///
/// The child gets a snapshot of the parent's filter.  If the parent
/// has no filter, the child gets no filter.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn copy_filter(parent_pid: Pid, child_pid: Pid) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    // Find parent's filter.
    let Some(parent_slot) = table.lookup(parent_pid) else {
        return Ok(()); // Parent has no filter — child inherits none.
    };
    let bitmap = table
        .filters
        .get(parent_slot)
        .ok_or(KernelError::NotFound)?
        .bitmap
        .clone();

    // Find a free slot for the child.
    let Some(slot) = table.filters.iter().position(|e| !e.active) else {
        return Err(KernelError::ResourceExhausted);
    };
    let entry = table
        .filters
        .get_mut(slot)
        .ok_or(KernelError::ResourceExhausted)?;
    entry.active = true;
    entry.pid = child_pid;
    entry.bitmap = bitmap;
    entry.deny_count = 0;
    table.rebuild_index();
    Ok(())
}

/// Tighten a filter by intersecting it with additional restrictions.
///
/// After this, only syscalls allowed by BOTH the existing filter
/// AND the new restrictions will be allowed.  This is the "only
/// tighten" invariant — a process can never re-allow a denied syscall.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn tighten(pid: Pid, restrictions: &[u64]) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    let slot = table.lookup(pid).ok_or(KernelError::NotFound)?;
    let entry = table.filters.get_mut(slot).ok_or(KernelError::NotFound)?;

    // Build a bitmap from the restriction list (deny these).
    let mut deny_mask = FilterBitmap::allow_all();
    for &nr in restrictions {
        deny_mask.deny(nr as usize);
    }
    entry.bitmap = entry.bitmap.intersect(&deny_mask);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Check if a process has a filter installed.
#[must_use]
pub fn has_filter(pid: Pid) -> bool {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else {
        return false;
    };

    table.lookup(pid).is_some()
}

/// Get the number of allowed syscalls for a process.
#[must_use]
pub fn allowed_count(pid: Pid) -> Option<usize> {
    let guard = TABLE.lock();
    let table = guard.as_ref()?;

    let slot = table.lookup(pid)?;
    Some(table.filters.get(slot)?.bitmap.count_allowed())
}

/// Get the deny count for a process (how many syscalls were blocked).
#[must_use]
pub fn deny_count(pid: Pid) -> u64 {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else {
        return 0;
    };

    table
        .lookup(pid)
        .and_then(|slot| table.filters.get(slot))
        .map_or(0, |e| e.deny_count)
}

/// Count active filters.
///
/// Deliberately still counts by walking the slots rather than reading
/// [`ACTIVE_FILTERS`]: this is the *oracle* the cached counter is checked
/// against in [`verify_index`], so it must not be reimplemented in terms of
/// the thing it validates.  It is not on any hot path.
#[must_use]
pub fn active_count() -> usize {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else {
        return 0;
    };

    table.filters.iter().filter(|e| e.active).count()
}

/// Cross-check the pid → slot index against the linear scan it replaced.
///
/// Verifies three things, and prints rather than returns so a failure is
/// visible in the boot log even when nothing is watching a return value:
///
/// 1. Every active slot's pid resolves through the hash to the same slot the
///    scan finds.  A hash *miss* here would mean a filtered process is treated
///    as unfiltered — a silent sandbox escape, failing open.
/// 2. A pid that is definitely not installed resolves to `None`, so the index
///    cannot manufacture a filter for an unfiltered process either.
/// 3. The cached [`ACTIVE_FILTERS`] counter equals the scan's count.  If it
///    over-counts, `check` takes the lock for nothing; if it *under*-counts
///    (worse), `check` returns `true` without looking, and every installed
///    filter silently stops being enforced.
///
/// # Panics
///
/// Panics if the index and the scan disagree.  That is deliberate: the
/// disagreement means syscall filtering is not enforcing what it reports, and
/// continuing to boot would ship a sandbox that is decorative.
pub fn verify_index(when: &str) {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else {
        serial_println!("[scfilter] index ({when}): SKIP (table not initialised)");
        return;
    };

    let mut checked = 0usize;
    for (slot, entry) in table.filters.iter().enumerate() {
        if !entry.active {
            continue;
        }
        assert_eq!(
            table.lookup(entry.pid),
            table.lookup_by_scan(entry.pid),
            "scfilter index disagrees with the linear scan for pid {} (slot {})",
            entry.pid,
            slot,
        );
        assert!(
            table.lookup(entry.pid).is_some(),
            "scfilter index cannot find pid {} which the scan says is at slot {} \
             -- a filtered process would run unfiltered",
            entry.pid,
            slot,
        );
        checked = checked.saturating_add(1);
    }

    // A pid no process can hold, so a hit here means the index invented one.
    const ABSENT_PID: Pid = 0xBADD_C0DE_BADD_C0DE;
    assert!(
        table.lookup(ABSENT_PID).is_none(),
        "scfilter index reports a filter for a pid that was never installed",
    );
    assert_eq!(
        table.lookup_by_scan(ABSENT_PID),
        None,
        "scfilter scan oracle reports a filter for a pid that was never installed",
    );

    let scanned = table.filters.iter().filter(|e| e.active).count();
    let cached = ACTIVE_FILTERS.load(Ordering::Acquire);
    assert_eq!(
        cached, scanned,
        "scfilter ACTIVE_FILTERS ({cached}) disagrees with the slot scan ({scanned}) \
         -- if cached is the smaller, every installed filter is silently unenforced",
    );

    serial_println!(
        "[scfilter] index ({when}): OK ({checked} filter(s) verified vs scan, \
         count {cached} matches)"
    );
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for syscall filtering.
pub fn self_test() {
    serial_println!("[scfilter] Running self-test...");

    // Test 1: No filters initially.
    assert_eq!(active_count(), 0);
    serial_println!("[scfilter]   Initial state: OK");

    // Test 2: Unfiltered process — all syscalls allowed.
    assert!(check(100, 0)); // SYS_YIELD
    assert!(check(100, 1)); // SYS_EXIT
    assert!(check(100, MAX_SYSCALL_NR.saturating_sub(1) as u64)); // Max valid
    serial_println!("[scfilter]   Unfiltered allows all: OK");

    // Test 3: Install allow-all filter.
    install(200).expect("install");
    assert!(has_filter(200));
    assert_eq!(active_count(), 1);
    assert!(check(200, 0));
    assert!(check(200, 100));
    assert_eq!(allowed_count(200), Some(MAX_SYSCALL_NR));
    serial_println!("[scfilter]   Allow-all filter: OK");

    // Test 4: Deny specific syscalls.
    deny(200, 10).expect("deny 10");
    deny(200, 11).expect("deny 11");
    deny(200, 12).expect("deny 12");
    assert!(!check(200, 10));
    assert!(!check(200, 11));
    assert!(!check(200, 12));
    assert!(check(200, 9)); // Adjacent — still allowed.
    assert!(check(200, 13));
    assert_eq!(allowed_count(200), Some(MAX_SYSCALL_NR - 3));
    serial_println!("[scfilter]   Deny specific syscalls: OK");

    // Test 5: Deny count tracking.
    let _ = check(200, 10); // denied
    let _ = check(200, 10); // denied again
    assert_eq!(deny_count(200), 5); // 3 from test 4 (check 10,11,12) + 2 from test 5
    serial_println!("[scfilter]   Deny count tracking: OK");

    // Test 6: Install deny-all filter.
    install_deny_all(300).expect("install deny-all");
    assert!(!check(300, 0));
    assert!(!check(300, 500));
    assert_eq!(allowed_count(300), Some(0));
    serial_println!("[scfilter]   Deny-all filter: OK");

    // Test 7: Allow specific syscalls.
    allow(300, 0).expect("allow 0"); // SYS_YIELD
    allow(300, 1).expect("allow 1"); // SYS_EXIT
    assert!(check(300, 0));
    assert!(check(300, 1));
    assert!(!check(300, 2)); // Still denied.
    assert_eq!(allowed_count(300), Some(2));
    serial_println!("[scfilter]   Allow specific syscalls: OK");

    // Test 8: Filter inheritance (copy).
    copy_filter(200, 400).expect("copy");
    assert!(has_filter(400));
    assert!(!check(400, 10)); // Inherited deny.
    assert!(check(400, 9)); // Inherited allow.
    serial_println!("[scfilter]   Filter inheritance: OK");

    // Test 9: Tighten filter.
    let restrictions = [9u64, 13]; // Deny 9 and 13 additionally.
    tighten(400, &restrictions).expect("tighten");
    assert!(!check(400, 9)); // Was allowed, now denied.
    assert!(!check(400, 13)); // Was allowed, now denied.
    assert!(check(400, 8)); // Not in restrictions — still allowed.
    serial_println!("[scfilter]   Tighten filter: OK");

    // Test 10: Duplicate install rejected.
    assert!(install(200).is_err());
    serial_println!("[scfilter]   Duplicate install rejected: OK");

    // Test 11: Operations on non-existent filter.
    assert!(deny(999, 0).is_err());
    assert!(allow(999, 0).is_err());
    assert!(!has_filter(999));
    serial_println!("[scfilter]   Non-existent filter rejected: OK");

    // Test 12: Out-of-range syscall number.
    //
    // This was written as a literal `check(200, 1000)` and passed for exactly
    // the wrong reason: 1000 was out of range only because this module's
    // `MAX_SYSCALL_NR` had drifted below the dispatch table's 1100, so the
    // assertion encoded the bug it should have caught.  Phrased against the
    // constant, it tests the boundary wherever the boundary actually is.
    assert!(!check(200, MAX_SYSCALL_NR as u64)); // First unrepresentable nr.
    assert!(!check(200, u64::MAX));
    serial_println!("[scfilter]   Out-of-range denied: OK");

    // Test 12b: the last *representable* syscall number is not swept up by the
    // boundary check — the off-by-one that would deny the top syscall to every
    // process.  200 is an allow-all filter here, so `true` is the only correct
    // answer, and `is_allowed` must have a bitmap word covering this bit.
    assert!(check(200, MAX_SYSCALL_NR.saturating_sub(1) as u64));
    serial_println!("[scfilter]   Top-of-range allowed: OK");

    // Test 13: Remove filter.
    remove(200);
    assert!(!has_filter(200));
    assert!(check(200, 10)); // No filter — allowed again.
    serial_println!("[scfilter]   Remove filter: OK");

    // Test 14: Copy from unfiltered parent.
    remove(400);
    copy_filter(999, 500).expect("copy from unfiltered");
    assert!(!has_filter(500)); // No filter installed.
    serial_println!("[scfilter]   Copy from unfiltered: OK");

    // Test 15: the pid → slot index agrees with the linear scan it replaced.
    //
    // Runs here, with filters installed, rather than only at boot with an empty
    // table: an index test over zero entries verifies nothing about lookup and
    // would pass on an index that is broken for every pid.  (Exactly that trap
    // was found in the lockdep class index, which was being verified when 3 of
    // its eventual 43 classes existed.)
    verify_index("self-test, populated");

    // Test 16: two pids that hash to the SAME bucket must both resolve.
    //
    // The whole risk of replacing a scan with a hash is the collision path: a
    // lookup that stops at the wrong entry reports "no filter", and a process
    // that should be sandboxed runs unsandboxed.  A test using arbitrary pids
    // almost certainly never collides, so it exercises only the happy path and
    // then reports success.  So search for a genuine collision rather than
    // hoping for one — and if none is found, say so instead of passing.
    let base_pid: Pid = 600;
    install_deny_all(base_pid).expect("install collision base");
    let target_bucket = filter_bucket(base_pid);
    let mut collider: Option<Pid> = None;
    for candidate in 601..200_000u64 {
        if filter_bucket(candidate) == target_bucket {
            collider = Some(candidate);
            break;
        }
    }
    if let Some(other) = collider {
        install(other).expect("install colliding pid");
        // Both must resolve to their own slot, and to the same answer the scan
        // gives — the collider must not shadow the entry already in the bucket.
        assert!(
            !check(base_pid, 5),
            "deny-all base lost its filter to a collision"
        );
        assert!(check(other, 5), "allow-all collider got the base's filter");
        assert!(has_filter(base_pid) && has_filter(other));
        verify_index("self-test, colliding pids");
        // Removing the *first* of a probe run must not orphan the second: with
        // tombstone-free rebuilding it cannot, and this asserts that.
        remove(base_pid);
        assert!(!has_filter(base_pid));
        assert!(
            has_filter(other),
            "removing a colliding pid orphaned the other"
        );
        assert!(check(other, 5));
        remove(other);
        serial_println!(
            "[scfilter]   Index collision (pids {base_pid} and {other} share bucket \
             {target_bucket}): OK"
        );
    } else {
        remove(base_pid);
        serial_println!(
            "[scfilter]   WARNING: no colliding pid found for {base_pid} in 200k \
             candidates — the index probe path is UNTESTED this boot"
        );
    }

    // Cleanup.
    remove(300);
    remove(400);
    remove(500);
    assert_eq!(active_count(), 0);
    // The cached counter must come back to zero with the slots, or the syscall
    // fast path stays switched off for the rest of the boot — the same class of
    // bug as the namespace self-tests, which left NS_FEATURES_ACTIVE armed and
    // silently cost every VFS path operation three spinlocks for the whole run.
    assert_eq!(
        ACTIVE_FILTERS.load(Ordering::Acquire),
        0,
        "a scfilter self-test leaked a filter: the syscall fast path would stay \
         disabled for the rest of the boot",
    );
    verify_index("self-test, drained");
    serial_println!("[scfilter]   Cleanup: OK");

    serial_println!("[scfilter] Self-test PASSED (16 tests)");
}
