//! Futex — fast userspace mutex kernel support.
//!
//! A futex (fast userspace mutex) allows efficient synchronization by
//! keeping the fast path entirely in userspace (atomic CAS, no syscall)
//! and only entering the kernel on contention.
//!
//! ## API
//!
//! - **`futex_wait(addr, expected)`** — if `*addr == expected`, block the
//!   calling task until woken.  If `*addr != expected`, return immediately
//!   (the value changed between the caller's CAS and the wait call).
//!
//! - **`futex_wake(addr, max_wake)`** — wake up to `max_wake` tasks
//!   blocked on `addr`.  Returns the number of tasks actually woken.
//!
//! ## Userspace Fast Path
//!
//! The uncontended case never enters the kernel:
//!
//! ```text
//! lock:   CAS(addr, 0 → 1)  →  success?  →  hold lock (no syscall)
//!                                  │
//!                                  └─ fail  →  futex_wait(addr, 1)
//!
//! unlock: STORE(addr, 0)  →  futex_wake(addr, 1)
//! ```
//!
//! ## Performance Targets
//!
//! - Uncontended: ~10ns (pure userspace atomic CAS, no syscall).
//! - Contended wake: < 5 µs (Linux: 1–3 µs).
//!
//! ## Implementation
//!
//! The kernel maintains a hash table mapping (`address_space`, `virtual_address`)
//! pairs to wait queues.  The address-space key is the PML4 physical address
//! (0 for kernel tasks).  This prevents aliasing: the same virtual address in
//! different processes maps to different physical pages, so they must not
//! share a futex queue.
//!
//! The hash table uses separate chaining with a fixed number of buckets.
//! Each bucket is a `VecDeque<Waiter>`.  The table is protected by a
//! single spinlock.
//!
//! Lock ordering: `FUTEX_TABLE` → `SCHED` (wake calls `sched::wake()`).

use alloc::collections::{BTreeMap, VecDeque};
use crate::error::{KernelError, KernelResult};
use crate::mm::user::{read_user, validate_user_write};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of hash buckets.  Must be a power of two.
///
/// 64 buckets is reasonable for early boot.  With per-process address
/// spaces, this should scale up.
const NUM_BUCKETS: usize = 64;

// ---------------------------------------------------------------------------
// Address-space identification
// ---------------------------------------------------------------------------

/// Get the current task's address-space key for futex hashing.
///
/// Returns the PML4 physical address for user processes, or 0 for
/// kernel tasks.  This prevents aliasing: the same virtual address
/// in different processes maps to different physical memory and must
/// not share a futex queue.
fn current_addr_space() -> u64 {
    let task_id = sched::current_task_id();
    let pid = crate::proc::thread::owner_process(task_id).unwrap_or(0);
    if pid == 0 {
        return 0; // Kernel task — no per-process address space.
    }
    crate::proc::pcb::get_pml4(pid).unwrap_or(0)
}

/// The owning user process id of the current task, or `0` for a kernel task.
///
/// Signal interruptibility only applies to user processes: kernel tasks (boot
/// self-tests, kworker-style threads) have no signal state, so a `0` here means
/// "park uninterruptibly, exactly as before".
fn current_user_pid() -> u64 {
    let task_id = sched::current_task_id();
    crate::proc::thread::owner_process(task_id).unwrap_or(0)
}

/// Remove this task's lingering futex waiter from whatever bucket holds it,
/// scoped to `addr_space`, and report whether one was found.
///
/// A waiter is normally dequeued by its waker (`futex_wake` / a successful
/// requeue), so finding ourselves *still* queued after a wake means the wake
/// came from a different source — a timeout or a signal — and we must evict the
/// stale entry ourselves.  We first probe the original `addr` bucket (the common
/// case), then fall back to a full scan because `FUTEX_CMP_REQUEUE` may have
/// moved us to a *different* address (hence a different bucket) before the wake.
///
/// Returns `true` iff a waiter for `task` was found and removed.
fn remove_self_waiter(addr: u64, addr_space: u64, task: u64) -> bool {
    let mut table = FUTEX_TABLE.lock();
    let idx = FutexTable::bucket_index(addr, addr_space);

    // SAFETY: idx is masked to NUM_BUCKETS-1 by bucket_index.
    #[allow(clippy::indexing_slicing)]
    {
        let bucket = &mut table.buckets[idx];
        if let Some(pos) = bucket
            .iter()
            .position(|w| w.task_id == task && w.addr == addr && w.addr_space == addr_space)
        {
            bucket.remove(pos);
            return true;
        }
    }

    // Requeued elsewhere: scan all buckets for this task within our address
    // space and evict any lingering entry.
    for bucket in &mut table.buckets {
        if let Some(pos) = bucket
            .iter()
            .position(|w| w.task_id == task && w.addr_space == addr_space)
        {
            bucket.remove(pos);
            return true;
        }
    }
    false
}

/// Remove **all** of `task`'s lingering futex waiter entries from every
/// bucket, returning the number removed.
///
/// A `futex_waitv` parker enqueues one [`Waiter`] per key (all sharing its
/// `task_id`); when it resumes it must evict every entry it queued — the
/// waker only dequeued the single key that fired (or, on a timeout/signal,
/// none were dequeued).  Matching on `task_id` alone is sufficient: a
/// `TaskId` is unique to one task within one process, so there is no
/// cross-address-space aliasing to guard against.
fn remove_all_self_waiters(task: u64) -> usize {
    let mut table = FUTEX_TABLE.lock();
    let mut removed = 0usize;
    for bucket in &mut table.buckets {
        // retain() can't easily count, so filter in place via a scan.
        let mut i = 0;
        while i < bucket.len() {
            if bucket.get(i).is_some_and(|w| w.task_id == task) {
                bucket.remove(i);
                #[allow(clippy::arithmetic_side_effects)]
                { removed += 1; }
                // Next element shifted down into slot `i`.
                continue;
            }
            #[allow(clippy::arithmetic_side_effects)]
            { i += 1; }
        }
    }
    removed
}

/// `true` if a deliverable (unblocked) signal is pending for `pid`.
fn deliverable_signal_pending(pid: u64) -> bool {
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::has_pending_in_mask(pid, deliverable)
}

// ---------------------------------------------------------------------------
// Waiter and hash table
// ---------------------------------------------------------------------------

/// Linux `FUTEX_BITSET_MATCH_ANY` — the wildcard bitset used by plain
/// `FUTEX_WAIT`/`FUTEX_WAKE`.
///
/// A waiter registered with this value matches every `FUTEX_WAKE_BITSET`
/// mask, and a `FUTEX_WAKE_BITSET` performed with this value wakes every
/// waiter regardless of the waiter's stored bitset.  Plain `FUTEX_WAIT`
/// and `FUTEX_WAKE` are exactly the bitset variants with this wildcard,
/// so routing them through the bitset path preserves their semantics.
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xffff_ffff;

/// A task waiting on a futex address.
struct Waiter {
    /// The virtual address being waited on.
    addr: u64,
    /// The address space identifier (PML4 physical address) that owns
    /// this futex address.  0 = kernel address space.
    ///
    /// This prevents aliasing: the same virtual address in different
    /// processes maps to different physical pages, so they must not
    /// share a futex queue.  Shared memory futexes (same physical page
    /// mapped into multiple address spaces) should use the same PML4 —
    /// which is correct because shared memory regions are mapped into
    /// the kernel address space (PML4 = 0) during early development.
    addr_space: u64,
    /// The blocked task's ID.
    task_id: TaskId,
    /// The 32-bit wakeup bitset this waiter registered with
    /// (`FUTEX_WAIT_BITSET`'s `val3`).  A `FUTEX_WAKE_BITSET(mask)` wakes
    /// this waiter only when `self.bitset & mask != 0`.  Plain
    /// `FUTEX_WAIT` stores [`FUTEX_BITSET_MATCH_ANY`], so it is woken by
    /// any wake.  A requeued waiter keeps its bitset (Linux requeue does
    /// not alter it); the requeue wake phase itself uses the wildcard, so
    /// the stored value only gates direct `FUTEX_WAKE_BITSET` calls.
    bitset: u32,
    /// For a `futex_waitv` (multi-key) parker, the index of this entry in
    /// the caller's waiter array (`0..nr`).  A single task enqueues one
    /// `Waiter` per key, all sharing its `task_id`; when any waker dequeues
    /// one, it records this index so the woken parker can report *which*
    /// futex woke it (Linux `futex_waitv` returns the woken index).  `None`
    /// for an ordinary single-key `FUTEX_WAIT`.
    multi_index: Option<u32>,
}

/// Global futex wait table.
///
/// Maps virtual addresses to lists of waiting tasks via a hash table
/// with separate chaining.
///
/// Lock ordering: `FUTEX_TABLE` → `SCHED`.
static FUTEX_TABLE: Mutex<FutexTable> = Mutex::new(FutexTable::new());

/// Woken-index registry for `futex_waitv` (multi-key) parkers.
///
/// Maps a multi-key parker's `TaskId` to the array index of the futex that
/// woke it.  A waker ([`futex_wake_bitset`]) inserts the index *before*
/// waking the task; the parker reads-and-clears its entry after
/// [`sched::block_current`] returns to learn which futex fired (Linux's
/// `futex_waitv` returns that index).  Insertion is first-writer-wins
/// (`or_insert`): if two futexes the task waited on are woken concurrently,
/// either index is an acceptable answer, matching Linux's "a futex that was
/// woken" contract.
///
/// Lock ordering: this lock is a leaf — it is taken only briefly to
/// insert/remove a single `u32`, never while holding `FUTEX_TABLE`, and
/// never around a `block_current`/`wake`.
static MULTI_WOKEN: Mutex<BTreeMap<TaskId, u32>> = Mutex::new(BTreeMap::new());

/// Hash table for futex waiters.
struct FutexTable {
    buckets: [VecDeque<Waiter>; NUM_BUCKETS],
}

impl FutexTable {
    const fn new() -> Self {
        // const initializer for array of VecDeques.
        const EMPTY: VecDeque<Waiter> = VecDeque::new();
        Self {
            buckets: [EMPTY; NUM_BUCKETS],
        }
    }

    /// Hash an address + address-space pair to a bucket index.
    ///
    /// Incorporates both the virtual address and the address-space key
    /// (PML4 physical address) to ensure that the same VA in different
    /// processes lands in a different bucket (reducing false sharing on
    /// bucket locks when multi-process futexes are common).
    #[allow(clippy::cast_possible_truncation)]
    fn bucket_index(addr: u64, addr_space: u64) -> usize {
        // Mix bits for better distribution.  Addresses are often
        // aligned, so we shift down and XOR.  The addr_space (PML4
        // physical address) is page-aligned, so shift it too.
        #[allow(clippy::arithmetic_side_effects)]
        let hash = addr ^ (addr >> 12) ^ (addr >> 24)
            ^ addr_space ^ (addr_space >> 12);
        #[allow(clippy::arithmetic_side_effects)]
        { (hash as usize) & (NUM_BUCKETS - 1) }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Block the current task if `*addr == expected`.
///
/// Atomically checks the 32-bit value at `addr`.  If it equals
/// `expected`, the current task is added to the wait queue for `addr`
/// and blocked.  If the value has changed (another thread modified it
/// between the caller's CAS and this call), returns immediately.
///
/// # Returns
///
/// - `Ok(true)` — the task was blocked and then woken.
/// - `Ok(false)` — the value didn't match; no blocking occurred.
/// - `Err(InvalidAddress)` — `addr` is null.
///
/// # Safety contract
///
/// `addr` must point to a valid, aligned `AtomicU32`.  The caller is
/// responsible for ensuring the memory is accessible.  When userspace
/// is implemented, the kernel must validate the pointer against the
/// caller's address space.
pub fn futex_wait(addr: u64, expected: u32) -> KernelResult<bool> {
    // Plain FUTEX_WAIT is the bitset wait with the wildcard mask: it is
    // woken by any FUTEX_WAKE / FUTEX_WAKE_BITSET.
    futex_wait_bitset(addr, expected, FUTEX_BITSET_MATCH_ANY)
}

/// Block the current task if `*addr == expected`, recording `bitset` as
/// the waiter's wakeup mask (Linux `FUTEX_WAIT_BITSET`).
///
/// Identical to [`futex_wait`] except the registered waiter is only woken
/// by a [`futex_wake_bitset`] whose mask shares at least one bit with
/// `bitset`.  `bitset` must be non-zero — a zero mask can never be matched
/// and the syscall layer rejects it with `EINVAL` before reaching here; as
/// a defensive measure we also reject it with [`KernelError::InvalidArgument`].
///
/// # Returns
///
/// - `Ok(true)` — the task was blocked and then woken.
/// - `Ok(false)` — the value didn't match; no blocking occurred.
/// - `Err(InvalidAddress)` — `addr` is null.
/// - `Err(BadAlignment)` — `addr` is not 4-byte aligned.
/// - `Err(InvalidArgument)` — `bitset` is zero.
pub fn futex_wait_bitset(addr: u64, expected: u32, bitset: u32) -> KernelResult<bool> {
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }

    // Check alignment (AtomicU32 requires 4-byte alignment).
    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }

    if bitset == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let current_task = sched::current_task_id();
    let addr_space = current_addr_space();

    {
        let mut table = FUTEX_TABLE.lock();

        // Atomically read the value at the address.
        //
        // SAFETY: Caller guarantees addr is valid and aligned.
        // We read atomically (Acquire) to see any concurrent writes.
        let actual = unsafe {
            let ptr = addr as *const AtomicU32;
            (*ptr).load(Ordering::Acquire)
        };

        // If the value changed, don't block — the condition the caller
        // was waiting for may already be satisfied.
        if actual != expected {
            super::stats::futex_spurious();
            return Ok(false);
        }

        super::stats::futex_wait();
        // Value matches — add to wait queue and block.
        let idx = FutexTable::bucket_index(addr, addr_space);

        // SAFETY: idx is masked to NUM_BUCKETS-1, which is < NUM_BUCKETS.
        #[allow(clippy::indexing_slicing)]
        table.buckets[idx].push_back(Waiter {
            addr,
            addr_space,
            task_id: current_task,
            bitset,
            multi_index: None,
        });

        // Drop the table lock before blocking.
    }

    let pid = current_user_pid();
    if pid == 0 {
        // Kernel task (boot self-test, etc.): no signal context — park
        // uninterruptibly.  Woken only by futex_wake.
        sched::block_current();
        return Ok(true);
    }

    // User process: make the park interruptible by signals.  Register as a
    // signal-waiter so a delivered signal wakes us, then recheck for a signal
    // that may have arrived between enqueue and registration (register-then-
    // recheck closes that race).
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, current_task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        crate::proc::signal::deregister_signalfd_waiter(pid, current_task);
        // A signal is already pending: unwind our queue entry and report the
        // interruption (unless a racing futex_wake already dequeued us).
        if remove_self_waiter(addr, addr_space, current_task) {
            return Err(KernelError::Interrupted);
        }
        return Ok(true);
    }

    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, current_task);

    // Woken.  If futex_wake removed us we are no longer queued -> a real wake.
    // If we are still queued, the wake came from a signal (or was spurious):
    // evict ourselves, then report Interrupted only when a deliverable signal
    // is actually pending; otherwise treat it as a spurious wake (the caller
    // re-checks the futex word).
    if remove_self_waiter(addr, addr_space, current_task) && deliverable_signal_pending(pid) {
        return Err(KernelError::Interrupted);
    }

    Ok(true)
}

/// Wait on a futex address with a timeout (nanoseconds).
///
/// Same as [`futex_wait`] but returns `Err(TimedOut)` if `timeout_ns`
/// elapses before a wake occurs.  `timeout_ns == 0` means check
/// `*addr == expected` but never block (returns Ok(false) if matched
/// but effectively a non-blocking check).
///
/// # Returns
///
/// - `Ok(true)` — blocked and woken by `futex_wake`.
/// - `Ok(false)` — `*addr != expected` (spurious non-match).
/// - `Err(TimedOut)` — timeout expired before a wake.
/// - `Err(InvalidAddress)` / `Err(BadAlignment)` — bad pointer.
pub fn futex_wait_timeout(addr: u64, expected: u32, timeout_ns: u64) -> KernelResult<bool> {
    // Plain timed FUTEX_WAIT is the bitset timed wait with the wildcard.
    futex_wait_bitset_timeout(addr, expected, timeout_ns, FUTEX_BITSET_MATCH_ANY)
}

/// Wait on a futex address with a timeout and a wakeup bitset
/// (Linux `FUTEX_WAIT_BITSET` with a finite deadline).
///
/// Same as [`futex_wait_timeout`] but the registered waiter is only woken
/// by a [`futex_wake_bitset`] whose mask shares a bit with `bitset`.
/// `bitset` must be non-zero (`Err(InvalidArgument)` otherwise).
pub fn futex_wait_bitset_timeout(
    addr: u64,
    expected: u32,
    timeout_ns: u64,
    bitset: u32,
) -> KernelResult<bool> {
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }

    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }

    if bitset == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let current_task = sched::current_task_id();
    let addr_space = current_addr_space();

    {
        let mut table = FUTEX_TABLE.lock();

        // SAFETY: Caller guarantees addr is valid and aligned.
        let actual = unsafe {
            let ptr = addr as *const AtomicU32;
            (*ptr).load(Ordering::Acquire)
        };

        if actual != expected {
            super::stats::futex_spurious();
            return Ok(false);
        }

        super::stats::futex_wait();

        // Non-blocking mode: if timeout is 0, treat as "try".
        if timeout_ns == 0 {
            return Err(KernelError::TimedOut);
        }

        let idx = FutexTable::bucket_index(addr, addr_space);

        #[allow(clippy::indexing_slicing)]
        table.buckets[idx].push_back(Waiter {
            addr,
            addr_space,
            task_id: current_task,
            bitset,
            multi_index: None,
        });
    }

    // Schedule a timer to wake us at the deadline.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, current_task);

    // Register as a signal-waiter (user processes only) so a delivered signal
    // wakes the park, then recheck for an already-pending signal that arrived
    // between enqueue and registration (register-then-recheck).
    let pid = current_user_pid();
    if pid != 0 {
        let deliverable = !crate::proc::signal::blocked(pid);
        crate::proc::signal::register_signalfd_waiter(pid, current_task, deliverable);
        if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
            crate::proc::signal::deregister_signalfd_waiter(pid, current_task);
            crate::hrtimer::cancel(timer_handle);
            if remove_self_waiter(addr, addr_space, current_task) {
                return Err(KernelError::Interrupted);
            }
            return Ok(true);
        }
    }

    // Block until woken (by futex_wake, the timer, or a signal).
    sched::block_current();

    if pid != 0 {
        crate::proc::signal::deregister_signalfd_waiter(pid, current_task);
    }
    crate::hrtimer::cancel(timer_handle);

    // Why did we wake?  A waker (futex_wake / requeue) dequeues us, so if we are
    // still queued the wake came from the timer or a signal.  remove_self_waiter
    // probes the original bucket then falls back to a full scan (handling a
    // requeue that moved us to a different bucket).
    let still_queued = remove_self_waiter(addr, addr_space, current_task);

    if !still_queued {
        // A futex_wake (or successful requeue+wake) released us — a real wake.
        return Ok(true);
    }

    // Still queued: prefer reporting a signal interruption over a timeout when
    // both happened (Linux delivers the signal; the timeout is restarted via the
    // restart_block in the kernel, here simplified to EINTR for the timed case).
    if pid != 0 && deliverable_signal_pending(pid) {
        return Err(KernelError::Interrupted);
    }

    if crate::hrtimer::now_ns() >= deadline_ns {
        Err(KernelError::TimedOut)
    } else {
        // Spurious early wake with neither signal nor deadline — report woken;
        // the caller re-checks the futex word and re-waits if needed.
        Ok(true)
    }
}

/// One key of a [`futex_wait_multiple`] (Linux `futex_waitv`) request: a
/// 32-bit futex word address and the value the caller expects to find there.
///
/// The caller (syscall layer) is responsible for validating that `uaddr` is
/// a readable, 4-byte-aligned user pointer before passing it here; this
/// mirrors the contract of the single-key [`futex_wait`] family.
#[derive(Clone, Copy, Debug)]
pub struct WaitvKey {
    /// Address of the 32-bit futex word.
    pub uaddr: u64,
    /// Expected current value at `uaddr`.
    pub expected: u32,
}

/// Outcome of a [`futex_wait_multiple`] call (Linux `futex_waitv` contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitvOutcome {
    /// A futex was woken; the payload is its index in the `keys` array.
    Woken(u32),
    /// At least one futex word did not hold its expected value at setup
    /// (Linux returns `-EAGAIN`).
    Mismatch,
    /// The timeout elapsed before any futex was woken (`-ETIMEDOUT`).
    TimedOut,
    /// A deliverable signal interrupted the wait (the syscall layer turns
    /// this into the restart sentinel / `-EINTR`).
    Interrupted,
}

/// Block the current task on **multiple** futex keys, waking when any one of
/// them is woken (Linux `futex_waitv`).
///
/// Semantics:
/// * Under a single `FUTEX_TABLE` critical section, every key's value is
///   compared against its `expected`.  If any differs, **nothing** is queued
///   and [`WaitvOutcome::Mismatch`] is returned (Linux `-EAGAIN`).  Holding
///   the table lock across all the compares makes the whole setup atomic
///   w.r.t. concurrent wakes — simpler than Linux's per-bucket-lock dance
///   because we have a single global table lock.
/// * Otherwise one [`Waiter`] per key is enqueued (all tagged with this
///   task's id and the key's index), and the task parks.
/// * A waker records the woken key's index in [`MULTI_WOKEN`] before waking;
///   on resume the index is read-and-cleared and **all** of this task's
///   queued entries are evicted ([`remove_all_self_waiters`]).
/// * A signal (user processes only) or the optional timeout can also end the
///   park.  A spurious wake (none of woken/signal/deadline) re-runs the whole
///   setup+park loop, re-validating values — so a value that changed during a
///   spurious wake correctly surfaces as `Mismatch`.
///
/// `timeout_ns` is a **relative** nanosecond budget (`None` = wait forever);
/// the syscall layer converts `futex_waitv`'s absolute deadline to a relative
/// value first.  `keys` must be non-empty (the syscall layer rejects `nr==0`).
pub fn futex_wait_multiple(keys: &[WaitvKey], timeout_ns: Option<u64>) -> WaitvOutcome {
    let current_task = sched::current_task_id();
    let addr_space = current_addr_space();
    let pid = current_user_pid();

    // Absolute deadline (if timed), computed once so the per-iteration timer
    // budget shrinks across spurious-wake retries instead of resetting.
    let deadline_ns = timeout_ns.map(|ns| crate::hrtimer::now_ns().saturating_add(ns));

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    loop {
        // ---- setup: value-check all keys, then enqueue all, atomically ----
        {
            let mut table = FUTEX_TABLE.lock();
            for key in keys {
                // SAFETY: the syscall layer validated each `uaddr` as a
                // readable, 4-byte-aligned user pointer; we read atomically
                // (Acquire) to observe concurrent writers, exactly as the
                // single-key path does.
                let actual = unsafe {
                    let ptr = key.uaddr as *const AtomicU32;
                    (*ptr).load(Ordering::Acquire)
                };
                if actual != key.expected {
                    super::stats::futex_spurious();
                    return WaitvOutcome::Mismatch;
                }
            }
            super::stats::futex_wait();
            for (i, key) in keys.iter().enumerate() {
                let bidx = FutexTable::bucket_index(key.uaddr, addr_space);
                #[allow(clippy::cast_possible_truncation)]
                let index = i as u32;
                // SAFETY: bidx is masked to NUM_BUCKETS-1 by bucket_index.
                #[allow(clippy::indexing_slicing)]
                table.buckets[bidx].push_back(Waiter {
                    addr: key.uaddr,
                    addr_space,
                    task_id: current_task,
                    bitset: FUTEX_BITSET_MATCH_ANY,
                    multi_index: Some(index),
                });
            }
        }

        // ---- arm timer (if any time left) ----
        let timer_handle = if let Some(dl) = deadline_ns {
            let now = crate::hrtimer::now_ns();
            if now >= dl {
                // Deadline already passed: unwind and report timeout.
                let _ = remove_all_self_waiters(current_task);
                MULTI_WOKEN.lock().remove(&current_task);
                return WaitvOutcome::TimedOut;
            }
            Some(crate::hrtimer::schedule_ns(dl.saturating_sub(now), timeout_wake, current_task))
        } else {
            None
        };

        // ---- register signal-waiter + recheck (user processes only) ----
        if pid != 0 {
            let deliverable = !crate::proc::signal::blocked(pid);
            crate::proc::signal::register_signalfd_waiter(pid, current_task, deliverable);
            if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
                crate::proc::signal::deregister_signalfd_waiter(pid, current_task);
                if let Some(h) = timer_handle {
                    crate::hrtimer::cancel(h);
                }
                let _ = remove_all_self_waiters(current_task);
                MULTI_WOKEN.lock().remove(&current_task);
                return WaitvOutcome::Interrupted;
            }
        }

        // ---- park ----
        sched::block_current();

        if pid != 0 {
            crate::proc::signal::deregister_signalfd_waiter(pid, current_task);
        }
        if let Some(h) = timer_handle {
            crate::hrtimer::cancel(h);
        }

        // ---- determine why we woke ----
        // Read-and-clear the woken index (a waker recorded it before waking),
        // then evict every entry we queued (the waker removed only the one key
        // that fired; a timeout/signal removed none).
        let woken = MULTI_WOKEN.lock().remove(&current_task);
        let _removed = remove_all_self_waiters(current_task);

        if let Some(index) = woken {
            return WaitvOutcome::Woken(index);
        }
        // No futex fired: prefer a signal over a timeout (Linux delivers the
        // signal; futex_waitv has no restart_block, so the syscall layer maps
        // Interrupted to the restart sentinel / EINTR).
        if pid != 0 && deliverable_signal_pending(pid) {
            return WaitvOutcome::Interrupted;
        }
        if let Some(dl) = deadline_ns
            && crate::hrtimer::now_ns() >= dl
        {
            return WaitvOutcome::TimedOut;
        }
        // Spurious wake: loop and re-run setup (re-validating values).
    }
}

/// Wake up to `max_wake` tasks blocked on `addr`.
///
/// Returns the number of tasks actually woken.
///
/// # Arguments
///
/// - `addr`: the virtual address to wake waiters on.
/// - `max_wake`: maximum number of tasks to wake (commonly 1 for
///   mutex unlock, `u32::MAX` for broadcast).
pub fn futex_wake(addr: u64, max_wake: u32) -> u32 {
    // Plain FUTEX_WAKE is the bitset wake with the wildcard mask: it wakes
    // any waiter regardless of the bitset it registered with.
    futex_wake_bitset(addr, max_wake, FUTEX_BITSET_MATCH_ANY)
}

/// Wake up to `max_wake` tasks blocked on `addr` whose registered bitset
/// shares at least one bit with `bitset` (Linux `FUTEX_WAKE_BITSET`).
///
/// Returns the number of tasks actually woken.  A waiter matches when
/// `waiter.bitset & bitset != 0`; since plain `FUTEX_WAIT` registers
/// [`FUTEX_BITSET_MATCH_ANY`] and plain `FUTEX_WAKE` passes the same
/// wildcard, the bitset gate is transparent to non-bitset callers.
///
/// `bitset == 0` matches nothing and returns 0 (the syscall layer rejects
/// a zero mask with `EINVAL` before reaching here).
///
/// # Arguments
///
/// - `addr`: the virtual address to wake waiters on.
/// - `max_wake`: maximum number of tasks to wake.
/// - `bitset`: the wakeup mask; only waiters with an overlapping bit wake.
pub fn futex_wake_bitset(addr: u64, max_wake: u32, bitset: u32) -> u32 {
    if addr == 0 || max_wake == 0 || bitset == 0 {
        return 0;
    }

    let addr_space = current_addr_space();

    // Collect (task, multi_index) pairs to wake while holding the table
    // lock, then record indices + wake outside the lock to respect lock
    // ordering.  `multi_index` is `Some` only for a `futex_waitv` parker.
    let mut to_wake: [(TaskId, Option<u32>); 32] = [(0, None); 32];
    let mut wake_count: usize = 0;

    {
        let mut table = FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr, addr_space);

        // SAFETY: idx is masked to NUM_BUCKETS-1.
        #[allow(clippy::indexing_slicing)]
        let bucket = &mut table.buckets[idx];

        // Remove up to max_wake waiters with matching address AND
        // address space AND an overlapping wakeup bitset.  This prevents
        // cross-process wake (a process can only wake tasks that share the
        // same address-space mapping) and honours FUTEX_WAKE_BITSET's
        // selective wakeup.
        let mut i = 0;
        while i < bucket.len() && wake_count < max_wake as usize && wake_count < to_wake.len() {
            if let Some(waiter) = bucket.get(i)
                && waiter.addr == addr
                && waiter.addr_space == addr_space
                && waiter.bitset & bitset != 0
                && let Some(removed) = bucket.remove(i)
            {
                if let Some(slot) = to_wake.get_mut(wake_count) {
                    *slot = (removed.task_id, removed.multi_index);
                }
                #[allow(clippy::arithmetic_side_effects)]
                { wake_count += 1; }
                // Don't increment i — the next element shifted down.
                continue;
            }
            #[allow(clippy::arithmetic_side_effects)]
            { i += 1; }
        }
    }

    // Record the woken index for any multi-key (`futex_waitv`) parker
    // *before* waking it, so the parker observes its index once it resumes.
    // First-writer-wins: a concurrent waker on another of the parker's keys
    // may also try to record — keep the earliest (either index is valid).
    for (task_id, multi_index) in to_wake.get(..wake_count).unwrap_or(&[]) {
        if let Some(index) = multi_index {
            MULTI_WOKEN.lock().entry(*task_id).or_insert(*index);
        }
    }

    // Wake the collected tasks outside the FUTEX_TABLE lock.
    for (task_id, _) in to_wake.get(..wake_count).unwrap_or(&[]) {
        sched::wake(*task_id);
    }

    #[allow(clippy::cast_possible_truncation)]
    let result = wake_count as u32;
    super::stats::futex_wake(result);
    result
}

/// Wake up to `max_wake` waiters on `addr1`, then requeue up to
/// `max_requeue` of the *remaining* waiters from `addr1` to `addr2`.
///
/// This is the kernel primitive behind Linux's `FUTEX_REQUEUE`,
/// `FUTEX_CMP_REQUEUE`, and the futex2 `futex_requeue` syscall.  Its
/// purpose is to avoid the "thundering herd" wakeup storm in
/// condition-variable implementations: `pthread_cond_broadcast` wakes one
/// waiter on the condvar's internal futex and *moves* the rest onto the
/// associated mutex's futex, where they will be woken one at a time as the
/// mutex is released.  Waking them all at once would have every waiter
/// race for the mutex, with all but one immediately re-blocking.
///
/// Both addresses are interpreted in the caller's address space, so a
/// process can only ever wake or move its own waiters (cross-process
/// aliasing is prevented by the `addr_space` key, exactly as in
/// [`futex_wake`]).
///
/// Returns the total number of tasks affected: woken + requeued (this
/// matches Linux's `futex_requeue` return value).
///
/// # Arguments
///
/// - `addr1`: source futex address; waiters here are woken then requeued.
/// - `addr2`: destination futex address; waiters are moved here.  If
///   `addr2 == 0`, the requeue phase is skipped (wake-only behaviour).
/// - `max_wake`: maximum number of waiters to wake from `addr1`.
/// - `max_requeue`: maximum number of *remaining* waiters to move to
///   `addr2`.
pub fn futex_requeue(addr1: u64, addr2: u64, max_wake: u32, max_requeue: u32) -> u32 {
    // No compare: requeue_inner never returns Err when `compare` is None,
    // so the unwrap_or is a safe default and never actually taken.
    requeue_inner(addr1, addr2, max_wake, max_requeue, None).unwrap_or(0)
}

/// Compare-and-requeue: the `FUTEX_CMP_REQUEUE` / futex2 variant.
///
/// Atomically (under the futex table lock) checks `*addr1 == expected`
/// before doing any wake or requeue.  If the value has changed, returns
/// `Err(WouldBlock)` (→ `EAGAIN`) and touches nothing — this is the race
/// detection that lets `pthread_cond_broadcast` retry safely when another
/// thread mutated the condvar word concurrently.
///
/// The caller must have validated that `addr1` is a readable user pointer.
///
/// Returns the total number of tasks affected on success.
pub fn futex_cmp_requeue(
    addr1: u64,
    addr2: u64,
    max_wake: u32,
    max_requeue: u32,
    expected: u32,
) -> KernelResult<u32> {
    requeue_inner(addr1, addr2, max_wake, max_requeue, Some(expected))
}

/// Shared body for [`futex_requeue`] and [`futex_cmp_requeue`].
///
/// When `compare` is `Some(v)`, `*addr1` is read under the table lock and
/// checked against `v` before any state change; a mismatch yields
/// `Err(WouldBlock)`.  When `compare` is `None`, no read occurs and the
/// function always returns `Ok`.
#[allow(clippy::arithmetic_side_effects)]
fn requeue_inner(
    addr1: u64,
    addr2: u64,
    max_wake: u32,
    max_requeue: u32,
    compare: Option<u32>,
) -> KernelResult<u32> {
    if addr1 == 0 {
        return Ok(0);
    }

    let addr_space = current_addr_space();

    // Collect task IDs to wake while holding the table lock, then wake
    // them outside the lock to respect lock ordering (FUTEX_TABLE → SCHED).
    let mut to_wake: [TaskId; 32] = [0; 32];
    let mut wake_count: usize = 0;
    let mut requeued: u32 = 0;

    {
        let mut table = FUTEX_TABLE.lock();
        let idx1 = FutexTable::bucket_index(addr1, addr_space);

        // Compare phase (CMP_REQUEUE only): the value check must happen
        // under the same lock that guards the wake/requeue, so that a
        // concurrent waker/setter cannot slip between the check and the
        // dequeue.  Done before touching any queue so a mismatch leaves
        // the table untouched.
        if let Some(expected) = compare {
            // SAFETY: the caller validated addr1 is a readable, aligned
            // user pointer (4-byte futex word), mirroring the load in
            // futex_wait_timeout which also reads under this lock.
            let actual = unsafe {
                let ptr = addr1 as *const AtomicU32;
                (*ptr).load(Ordering::Acquire)
            };
            if actual != expected {
                super::stats::futex_spurious();
                return Err(KernelError::WouldBlock);
            }
        }

        // Phase 1: wake up to max_wake waiters on addr1 (mirrors futex_wake).
        {
            // SAFETY: idx1 is masked to NUM_BUCKETS-1 by bucket_index.
            #[allow(clippy::indexing_slicing)]
            let bucket = &mut table.buckets[idx1];
            let mut i = 0;
            while i < bucket.len()
                && wake_count < max_wake as usize
                && wake_count < to_wake.len()
            {
                if let Some(waiter) = bucket.get(i)
                    && waiter.addr == addr1
                    && waiter.addr_space == addr_space
                    && let Some(removed) = bucket.remove(i)
                {
                    if let Some(slot) = to_wake.get_mut(wake_count) {
                        *slot = removed.task_id;
                    }
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        wake_count += 1;
                    }
                    // Don't increment i — the next element shifted down.
                    continue;
                }
                #[allow(clippy::arithmetic_side_effects)]
                {
                    i += 1;
                }
            }
        }

        // Phase 2: requeue up to max_requeue of the remaining addr1 waiters
        // onto addr2.  Re-borrow the source and destination buckets each
        // iteration: addr1 and addr2 may hash to the same bucket, so we
        // cannot hold two mutable borrows simultaneously.
        if addr2 != 0 && max_requeue > 0 {
            let idx2 = FutexTable::bucket_index(addr2, addr_space);
            while requeued < max_requeue {
                // Detach the next matching waiter from the source bucket.
                let moved = {
                    // SAFETY: idx1 is masked to NUM_BUCKETS-1.
                    #[allow(clippy::indexing_slicing)]
                    let b1 = &mut table.buckets[idx1];
                    match b1
                        .iter()
                        .position(|w| w.addr == addr1 && w.addr_space == addr_space)
                    {
                        Some(p) => b1.remove(p),
                        None => None,
                    }
                };
                let Some(mut waiter) = moved else { break };
                // Re-point the waiter at the destination futex so that a
                // later wake/timeout on addr2 will find and match it.
                waiter.addr = addr2;
                // SAFETY: idx2 is masked to NUM_BUCKETS-1.
                #[allow(clippy::indexing_slicing)]
                table.buckets[idx2].push_back(waiter);
                #[allow(clippy::arithmetic_side_effects)]
                {
                    requeued += 1;
                }
            }
        }
    }

    // Wake the collected tasks outside the FUTEX_TABLE lock.
    for task_id in to_wake.get(..wake_count).unwrap_or(&[]) {
        sched::wake(*task_id);
    }

    #[allow(clippy::cast_possible_truncation)]
    let woken = wake_count as u32;
    super::stats::futex_wake(woken);
    Ok(woken.saturating_add(requeued))
}

// ---------------------------------------------------------------------------
// FUTEX_WAKE_OP
// ---------------------------------------------------------------------------

/// Sign-extend a 12-bit field (held in the low bits of `v`) to a full
/// signed 32-bit value.
///
/// The `oparg`/`cmparg` fields of a `FUTEX_WAKE_OP` operation are encoded as
/// signed 12-bit integers (Linux: `(encoded_op << 8) >> 20`).  We extract the
/// field with a mask and shift, then replicate bit 11 into the upper bits so
/// negative operands (e.g. `ADD -1`) round-trip correctly.  Done with masks
/// rather than shift arithmetic to keep clear of `arithmetic_side_effects`.
#[allow(clippy::cast_possible_wrap)]
const fn sign_extend_12(v: u32) -> i32 {
    let field = v & 0x0fff;
    if field & 0x0800 != 0 {
        // Set the upper 20 bits, then reinterpret as i32.
        (field | 0xffff_f000) as i32
    } else {
        field as i32
    }
}

/// `FUTEX_WAKE_OP` primitive: conditional double-wake with an atomic RMW.
///
/// This is the kernel primitive behind Linux's `FUTEX_WAKE_OP`.  In one
/// operation it:
///
/// 1. Atomically applies an arithmetic/bitwise operation to the 32-bit word
///    at `addr2`, capturing the *previous* value.
/// 2. Wakes up to `max_wake` waiters on `addr1`.
/// 3. If the previous value at `addr2` satisfies the encoded comparison,
///    wakes up to `max_wake2` waiters on `addr2`.
///
/// Returns the total number of tasks woken across both addresses.
///
/// Historically this powered glibc's condition-variable broadcast (wake the
/// waiters on the internal futex while atomically clearing the "wakeup
/// pending" flag in a single syscall).  Modern glibc no longer uses it, but
/// it remains part of the futex(2) ABI and other runtimes still rely on it.
///
/// # `encoded_op` layout (Linux ABI `val3`)
///
/// ```text
///   bit  31     FUTEX_OP_OPARG_SHIFT  (oparg is a shift count)
///   bits 30..28 op    (SET=0, ADD=1, OR=2, ANDN=3, XOR=4)
///   bits 27..24 cmp   (EQ=0, NE=1, LT=2, LE=3, GT=4, GE=5)
///   bits 23..12 oparg (signed 12-bit)
///   bits 11..0  cmparg (signed 12-bit)
/// ```
///
/// The word at `addr2` is treated as a *signed* `i32` for the comparison,
/// matching Linux semantics.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `addr2` is 0, the `op` or `cmp`
///   selector is unknown, or `FUTEX_OP_OPARG_SHIFT` is set with a shift
///   count outside `0..=31` (a negative or oversized shift is undefined in
///   the C ABI, so we reject it rather than invoke UB).
///
/// # Atomicity
///
/// The read-modify-write on `addr2` uses a single hardware atomic
/// (`swap`/`fetch_add`/`fetch_or`/`fetch_and`/`fetch_xor`), so it is atomic
/// against concurrent userspace CAS loops regardless of the table lock.  We
/// still perform it under [`FUTEX_TABLE`] so it serialises against the
/// compare-read in [`requeue_inner`] for the (rare) case where the same word
/// is the source of a concurrent requeue.  The wakes happen after the lock
/// is released to preserve the `FUTEX_TABLE → SCHED` lock order; futex
/// semantics permit the resulting spurious-wakeup window (callers re-check
/// their predicate).
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn futex_wake_op(
    addr1: u64,
    addr2: u64,
    max_wake: u32,
    max_wake2: u32,
    encoded_op: u32,
) -> KernelResult<u32> {
    // addr2 is always dereferenced (the RMW target), so it must be present.
    if addr2 == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // Decode the operation selector.  Bit 31 (FUTEX_OP_OPARG_SHIFT) is tested
    // separately; the op selector itself is the low 3 bits of the top nibble.
    const FUTEX_OP_OPARG_SHIFT: u32 = 0x8000_0000;
    let oparg_shift = encoded_op & FUTEX_OP_OPARG_SHIFT != 0;
    let op = (encoded_op >> 28) & 0x7;
    let cmp = (encoded_op >> 24) & 0xf;
    let oparg_raw = sign_extend_12((encoded_op >> 12) & 0x0fff);
    let cmparg = sign_extend_12(encoded_op & 0x0fff);

    // Resolve the operand: either the literal signed value, or — when
    // FUTEX_OP_OPARG_SHIFT is set — `1 << oparg`.  Linux invokes UB for an
    // out-of-range shift; we reject it as EINVAL instead.
    let oparg: u32 = if oparg_shift {
        if !(0..=31).contains(&oparg_raw) {
            return Err(KernelError::InvalidArgument);
        }
        // oparg_raw is in 0..=31, so the shift cannot overflow.
        1u32 << (oparg_raw as u32)
    } else {
        // Two's-complement reinterpretation: a negative operand wraps to the
        // matching u32 so fetch_add/fetch_xor/etc. compute the intended
        // signed result.
        oparg_raw as u32
    };

    // Validate the op/cmp selectors up front so an unknown selector cannot
    // mutate the word before erroring out.
    if op > 4 || cmp > 5 {
        return Err(KernelError::InvalidArgument);
    }

    // Phase 1: atomic RMW on *addr2, capturing the old value.
    let oldval = {
        let _table = FUTEX_TABLE.lock();
        // SAFETY: the caller validated addr2 as a writable, 4-byte-aligned
        // user word (validate_user_write).  AtomicU32 has the same layout as
        // a u32, and the RMW methods are the only access to this location
        // here, so there is no torn read/write.
        let atomic = unsafe { &*(addr2 as *const AtomicU32) };
        match op {
            0 => atomic.swap(oparg, Ordering::AcqRel),       // SET
            1 => atomic.fetch_add(oparg, Ordering::AcqRel),  // ADD (wrapping)
            2 => atomic.fetch_or(oparg, Ordering::AcqRel),   // OR
            3 => atomic.fetch_and(!oparg, Ordering::AcqRel), // ANDN
            // op is bounded to <=4 above; 4 is XOR and the only remaining arm.
            _ => atomic.fetch_xor(oparg, Ordering::AcqRel),  // XOR
        }
    };

    // Compare the *old* value (interpreted as signed) against cmparg.
    let old_signed = oldval as i32;
    let matched = match cmp {
        0 => old_signed == cmparg, // EQ
        1 => old_signed != cmparg, // NE
        2 => old_signed < cmparg,  // LT
        3 => old_signed <= cmparg, // LE
        4 => old_signed > cmparg,  // GT
        // cmp is bounded to <=5 above; 5 is GE and the only remaining arm.
        _ => old_signed >= cmparg, // GE
    };

    // Phase 2 + 3: wake addr1 unconditionally, addr2 on a comparison match.
    let mut woken = futex_wake(addr1, max_wake);
    if matched {
        woken = woken.saturating_add(futex_wake(addr2, max_wake2));
    }
    Ok(woken)
}

// ---------------------------------------------------------------------------
// Priority Inheritance (PI) Futex
// ---------------------------------------------------------------------------
//
// PI futexes solve the priority inversion problem.  When a high-priority
// task blocks on a lock held by a low-priority task, the holder is
// temporarily boosted to the blocked task's priority level.
//
// ## Futex Word Format (PI variant)
//
// This matches Linux's PI futex word layout exactly, so the same word can
// be shared between our native `SYS_FUTEX_*_PI` syscalls and the Linux-ABI
// `futex(2)` `FUTEX_LOCK_PI` / `FUTEX_UNLOCK_PI` / `FUTEX_TRYLOCK_PI` ops.
// A Linux thread's TID is what `gettid()` returns, which on this OS is the
// kernel `TaskId` (see `sys_gettid`), so the userspace fast-path CAS writes
// exactly the value the kernel records as the owner.
//
// ```text
// Bits 0-29:  Owner task ID (0 = unlocked)         (FUTEX_TID_MASK)
// Bit 30:     FUTEX_OWNER_DIED — owner exited holding the lock (reserved;
//             recognised but not yet produced by robust-list cleanup)
// Bit 31:     FUTEX_WAITERS — set by kernel when PI waiters exist
// ```
//
// ## Userspace Fast Path
//
// ```text
// lock:   CAS(addr, 0 → my_tid)  →  success?  →  hold lock (no syscall)
//                                      │
//                                      └─ fail  →  SYS_FUTEX_LOCK_PI(addr)
//
// unlock: If no waiters bit:  CAS(addr, my_tid → 0)  →  done
//         If waiters bit:     SYS_FUTEX_UNLOCK_PI(addr)
// ```

/// Mask for the owner task ID in a PI futex word (bits 0–29).
///
/// Identical to Linux's `FUTEX_TID_MASK`.  Since `gettid()` returns the
/// kernel `TaskId`, task IDs must stay within 30 bits for the userspace
/// fast-path CAS and the kernel owner record to agree.
const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;

/// Bit flag indicating PI waiters exist (bit 31 of the futex word).
///
/// Matches Linux's `FUTEX_WAITERS` (`0x8000_0000`).  Set by the kernel on
/// the contended path so the owner knows it must call into the kernel to
/// release rather than taking the userspace fast-path CAS.
const FUTEX_WAITERS_BIT: u32 = 1 << 31;

/// Bit flag indicating the owner died while holding the lock (bit 30).
///
/// Matches Linux's `FUTEX_OWNER_DIED` (`0x4000_0000`).  Set by the
/// thread-exit cleanup ([`exit_robust_list`] / [`exit_pi_owned_futexes`])
/// on every futex the dying thread still owned, so the next acquirer of a
/// `PTHREAD_MUTEX_ROBUST` lock observes the death and can recover the
/// protected state (`pthread_mutex_consistent`).  Kept distinct from the
/// owner-TID bits so it is never mistaken for part of the TID.
const FUTEX_OWNER_DIED_BIT: u32 = 1 << 30;

/// A task waiting on a PI futex address.
struct PiWaiter {
    /// The virtual address being waited on.
    addr: u64,
    /// Address-space key (PML4 physical address, 0 = kernel).
    addr_space: u64,
    /// The blocked task's ID.
    task_id: TaskId,
    /// The task's effective priority at the time of blocking.
    priority: u8,
}

/// A task parked on a condvar waiting to be requeued onto a PI mutex.
///
/// This backs the `FUTEX_WAIT_REQUEUE_PI` / `FUTEX_CMP_REQUEUE_PI` pair —
/// the condition-variable-to-PI-mutex handoff used by `pthread_cond_wait`
/// on a `PTHREAD_PRIO_INHERIT` mutex.  A waiter blocks on `cond_addr` (a
/// plain, non-PI futex word) but remembers the PI mutex (`target_addr`) it
/// must end up holding.  A later `futex_cmp_requeue_pi` either grants it
/// ownership of `target_addr` (if free) or moves it onto the PI waiter
/// queue for `target_addr`, where it waits for the holder to unlock.
///
/// Kept in its own queue (not [`FutexTable`]) so that a plain
/// `FUTEX_WAKE` on the condvar word cannot accidentally wake a requeue-PI
/// waiter into an inconsistent state: such waiters are serviced *only* by
/// `futex_cmp_requeue_pi`, matching the way glibc's PI condvar signals.
struct RequeuePiWaiter {
    /// The condvar (source) address the task is parked on.
    cond_addr: u64,
    /// Address-space key (PML4 physical address, 0 = kernel).
    addr_space: u64,
    /// The PI mutex (destination) address the task will be requeued onto.
    target_addr: u64,
    /// The blocked task's ID.
    task_id: TaskId,
    /// The task's effective priority captured at wait time (used to pick
    /// the top waiter to grant the mutex to, and to boost the holder).
    priority: u8,
}

/// An ownership record for a PI futex.
struct PiOwner {
    /// The futex address this task holds.
    addr: u64,
    /// Address-space key (PML4 physical address, 0 = kernel).
    addr_space: u64,
    /// The owning task's ID.
    owner_id: TaskId,
}

/// State for PI futex operations.
///
/// Separate from the non-PI [`FutexTable`] to avoid adding overhead
/// to the common non-PI fast path.
///
/// Lock ordering: `PI_FUTEX_TABLE` → `SCHED`.
struct PiFutexTable {
    /// PI waiters, bucketed by address hash.
    waiters: [VecDeque<PiWaiter>; NUM_BUCKETS],
    /// Ownership records, bucketed by address hash.
    owners: [VecDeque<PiOwner>; NUM_BUCKETS],
    /// Condvar waiters awaiting requeue onto a PI mutex, bucketed by the
    /// *condvar* address hash (the source key, not the PI mutex).
    requeue_waiters: [VecDeque<RequeuePiWaiter>; NUM_BUCKETS],
}

impl PiFutexTable {
    const fn new() -> Self {
        const EMPTY_W: VecDeque<PiWaiter> = VecDeque::new();
        const EMPTY_O: VecDeque<PiOwner> = VecDeque::new();
        const EMPTY_R: VecDeque<RequeuePiWaiter> = VecDeque::new();
        Self {
            waiters: [EMPTY_W; NUM_BUCKETS],
            owners: [EMPTY_O; NUM_BUCKETS],
            requeue_waiters: [EMPTY_R; NUM_BUCKETS],
        }
    }
}

/// Global PI futex state.
///
/// Lock ordering: `PI_FUTEX_TABLE` → `SCHED` (collect data under PI
/// table lock, then call sched functions outside the lock).
static PI_FUTEX_TABLE: Mutex<PiFutexTable> = Mutex::new(PiFutexTable::new());

/// Look up the current owner of a PI futex address.
///
/// Used as the `find_owner` callback for transitive PI chain walking.
/// Acquires `PI_FUTEX_TABLE` lock briefly.  Safe to call from
/// `pi_chain_boost` since that function does not hold any locks
/// (it alternates between PI_FUTEX_TABLE and SCHED without nesting).
///
/// Uses the caller's address space (addr_space = 0 during chain walks
/// within the kernel).  For cross-process PI (future), the chain walk
/// would need to carry the addr_space along.
fn find_pi_owner(addr: u64) -> Option<TaskId> {
    let addr_space = current_addr_space();
    let table = PI_FUTEX_TABLE.lock();
    let idx = FutexTable::bucket_index(addr, addr_space);
    // SAFETY: idx is masked to NUM_BUCKETS - 1.
    #[allow(clippy::indexing_slicing)]
    table.owners[idx]
        .iter()
        .find(|o| o.addr == addr && o.addr_space == addr_space)
        .map(|o| o.owner_id)
}

/// Register a task as the PI futex owner for an address.
fn register_pi_owner(addr: u64, addr_space: u64, owner_id: TaskId) {
    let mut table = PI_FUTEX_TABLE.lock();
    let idx = FutexTable::bucket_index(addr, addr_space);
    // SAFETY: idx is masked to NUM_BUCKETS - 1.
    #[allow(clippy::indexing_slicing)]
    table.owners[idx].push_back(PiOwner { addr, addr_space, owner_id });
}

/// Remove a task's PI ownership record for an address.
fn unregister_pi_owner(table: &mut PiFutexTable, addr: u64, addr_space: u64, owner_id: TaskId) {
    let idx = FutexTable::bucket_index(addr, addr_space);
    // SAFETY: idx is masked to NUM_BUCKETS - 1.
    #[allow(clippy::indexing_slicing)]
    table.owners[idx].retain(|o| !(o.addr == addr && o.addr_space == addr_space && o.owner_id == owner_id));
}

/// Recalculate the inherited priority for a task based on all PI
/// futexes it still owns.
///
/// Scans all PI waiter queues for addresses owned by `owner_id` and
/// returns the highest priority (lowest number) among all waiters,
/// or `None` if no PI waiters exist for any of this task's locks.
///
/// O(owned_locks × waiters_per_lock) — both are typically very small
/// (1–3 owned locks, 1–10 waiters each).
fn recalculate_inherited_for_owner(
    table: &PiFutexTable,
    owner_id: TaskId,
) -> Option<u8> {
    let mut best: Option<u8> = None;

    // Find all addresses still owned by this task.
    for bucket in &table.owners {
        for ownership in bucket {
            if ownership.owner_id != owner_id {
                continue;
            }
            // Check for PI waiters on this address + addr_space.
            let widx = FutexTable::bucket_index(ownership.addr, ownership.addr_space);
            // SAFETY: widx is masked to NUM_BUCKETS - 1.
            #[allow(clippy::indexing_slicing)]
            for waiter in &table.waiters[widx] {
                if waiter.addr == ownership.addr && waiter.addr_space == ownership.addr_space {
                    best = Some(match best {
                        Some(p) => p.min(waiter.priority),
                        None => waiter.priority,
                    });
                }
            }
        }
    }

    best
}

/// Lock a PI futex, blocking indefinitely until acquired.
///
/// Attempts to acquire the lock at `addr`.  If the lock is free (futex
/// word is 0), atomically sets the owner.  If contended, blocks the
/// caller and applies priority inheritance to the current lock holder.
///
/// The futex word uses the Linux PI layout: bits 0–29 for the owner
/// task ID (`FUTEX_TID_MASK`), bit 30 = `FUTEX_OWNER_DIED`, bit 31 =
/// `FUTEX_WAITERS`.
///
/// # Returns
///
/// - `Ok(())` — lock acquired (either uncontended or after waiting).
/// - `Err(InvalidAddress)` — `addr` is null.
/// - `Err(BadAlignment)` — `addr` is not 4-byte aligned.
/// - `Err(Deadlock)` — caller already owns the lock (maps to `EDEADLK`).
///
/// # Safety contract
///
/// `addr` must point to a valid, aligned `AtomicU32`.
pub fn futex_lock_pi(addr: u64) -> KernelResult<()> {
    lock_pi_inner(addr, None)
}

/// Lock a PI futex with a relative timeout.
///
/// Identical to [`futex_lock_pi`] except the caller gives up after
/// `timeout_ns` nanoseconds.  A `timeout_ns` of 0 means "try once"
/// (acquire if uncontended, otherwise return immediately).
///
/// # Returns
///
/// - `Ok(())` — lock acquired before the deadline.
/// - `Err(TimedOut)` — the deadline expired before acquisition (maps to
///   `ETIMEDOUT`).
/// - other errors as for [`futex_lock_pi`].
///
/// # Safety contract
///
/// `addr` must point to a valid, aligned `AtomicU32`.
pub fn futex_lock_pi_timeout(addr: u64, timeout_ns: u64) -> KernelResult<()> {
    lock_pi_inner(addr, Some(timeout_ns))
}

/// Try to claim an *ownerless* PI futex word for `current_tid`.
///
/// A word is ownerless when its owner-TID bits are clear — either a fully
/// zero word (normal uncontended lock) or a dead-owner word where
/// `FUTEX_OWNER_DIED` is set but no successor TID was written (left by
/// robust / PI exit cleanup with no waiter to inherit).  The CAS preserves
/// the `FUTEX_OWNER_DIED` and `FUTEX_WAITERS` bits so userspace robust
/// recovery (the `EOWNERDEAD` path) still sees the death.
///
/// Returns `true` if the word was claimed; `false` if a live owner holds it.
///
/// # Safety contract
///
/// `atomic` must reference a valid, aligned user/kernel `AtomicU32`.
fn try_acquire_ownerless(atomic: &AtomicU32, current_tid: u32) -> bool {
    loop {
        let w = atomic.load(Ordering::Acquire);
        if w & FUTEX_TID_MASK != 0 {
            return false; // a live owner holds it
        }
        let preserved = w & (FUTEX_OWNER_DIED_BIT | FUTEX_WAITERS_BIT);
        if atomic
            .compare_exchange(
                w,
                current_tid | preserved,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return true;
        }
        // Raced with another CAS on the word; loop re-reads and retries.
    }
}

/// Shared implementation of PI lock acquisition.
///
/// `timeout_ns`:
/// - `None` — block indefinitely until the lock is transferred to us.
/// - `Some(0)` — non-blocking: acquire only if uncontended.
/// - `Some(ns)` — block until acquired or until `ns` nanoseconds elapse.
///
/// The only events that wake a blocked PI waiter are (a) `futex_unlock_pi`
/// transferring ownership to us, or (b) our own timeout timer firing.  We
/// disambiguate by checking, under the `PI_FUTEX_TABLE` lock, whether an
/// ownership record now exists for us: holding the lock across both the
/// ownership check and our waiter removal closes the timeout-vs-transfer
/// race (either `unlock_pi` won the lock and made us owner, or we won the
/// lock and removed ourselves so `unlock_pi` can no longer pick us).
fn lock_pi_inner(addr: u64, timeout_ns: Option<u64>) -> KernelResult<()> {
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }

    let current_id = sched::current_task_id();
    let addr_space = current_addr_space();
    #[allow(clippy::cast_possible_truncation)]
    let current_tid = (current_id as u32) & FUTEX_TID_MASK;

    // SAFETY: Caller guarantees addr is valid and aligned.
    let atomic = unsafe { &*(addr as *const AtomicU32) };

    // Fast path: claim an ownerless word.  This covers the uncontended
    // case (word == 0) and the dead-owner case (FUTEX_OWNER_DIED set with
    // no TID) that robust/PI exit cleanup leaves behind, preserving the
    // OWNER_DIED bit so userspace robust recovery (EOWNERDEAD) still works.
    if try_acquire_ownerless(atomic, current_tid) {
        register_pi_owner(addr, addr_space, current_id);
        return Ok(());
    }

    // Slow (contended) path.
    //
    // Read the owner from the futex word.  Retry the claim if the lock
    // appears to have become ownerless between our first attempt and this
    // read (race window on SMP; harmless retry on single-CPU).
    let owner_id = {
        let word = atomic.load(Ordering::Acquire);
        let oid = u64::from(word & FUTEX_TID_MASK);

        if oid == current_id {
            return Err(KernelError::Deadlock); // Caller already owns it.
        }
        if oid == 0 {
            // Lock became ownerless between attempts — retry the claim.
            if try_acquire_ownerless(atomic, current_tid) {
                register_pi_owner(addr, addr_space, current_id);
                return Ok(());
            }
            let w2 = atomic.load(Ordering::Acquire);
            let o2 = u64::from(w2 & FUTEX_TID_MASK);
            if o2 == current_id {
                return Err(KernelError::Deadlock);
            }
            o2
        } else {
            oid
        }
    };

    // A zero timeout means "try once": contended, so fail without blocking.
    if matches!(timeout_ns, Some(0)) {
        return Err(KernelError::TimedOut);
    }

    // Get our effective priority for the PI donation.
    let our_priority = sched::get_effective_priority(current_id)
        .unwrap_or(sched::task::IDLE_PRIORITY);

    // Register as a PI waiter under the PI table lock.
    {
        let mut table = PI_FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr, addr_space);
        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        table.waiters[idx].push_back(PiWaiter {
            addr,
            addr_space,
            task_id: current_id,
            priority: our_priority,
        });
    }

    // Set the WAITERS bit so the unlocker knows to enter the kernel.
    atomic.fetch_or(FUTEX_WAITERS_BIT, Ordering::Release);

    // Boost the lock holder's priority if ours is higher (lower number),
    // then propagate transitively through the PI chain.
    if owner_id != 0 {
        sched::boost_priority(owner_id, our_priority);
        // Walk the chain: if owner_id is itself blocked on another PI
        // futex, boost that futex's owner, and so on.
        sched::pi_chain_boost(owner_id, our_priority, find_pi_owner);
    }

    // Record that we're blocked on this PI address (enables transitive
    // PI for tasks that later block on a lock we hold).
    sched::set_blocked_on_pi_addr(current_id, Some(addr));

    // Arm a one-shot timeout timer if requested.
    let timer_handle = match timeout_ns {
        Some(ns) => {
            // ns == 0 was handled above, so this is a real deadline.
            fn pi_timeout_wake(tid: u64) {
                if !sched::try_wake(tid) {
                    sched::defer_wake(tid);
                }
            }
            Some(crate::hrtimer::schedule_ns(ns, pi_timeout_wake, current_id))
        }
        None => None,
    };

    // Block until ownership is transferred to us, or the timer fires.
    // Deboost data is collected under the table lock and applied after
    // release to respect the PI_FUTEX_TABLE → SCHED lock ordering.
    let mut deboost: Option<(TaskId, Option<u8>)> = None;
    let outcome: KernelResult<()> = loop {
        sched::block_current();

        let mut table = PI_FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr, addr_space);

        // Did unlock_pi transfer ownership to us?
        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        let is_owner = table.owners[idx].iter().any(|o| {
            o.addr == addr && o.addr_space == addr_space && o.owner_id == current_id
        });

        if is_owner {
            break Ok(());
        }

        // Not the owner.  With no timeout, the only legitimate wake is an
        // ownership transfer, so any wake without ownership is spurious —
        // block again.  With a timeout, this wake is the timer firing.
        if timeout_ns.is_none() {
            drop(table);
            continue;
        }

        // Timed out: remove our waiter entry and clean up PI donation.
        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        if let Some(pos) = table.waiters[idx].iter().position(|w| {
            w.task_id == current_id && w.addr == addr && w.addr_space == addr_space
        }) {
            table.waiters[idx].remove(pos);
        }

        // If no waiters remain on this addr, clear the WAITERS bit.  Doing
        // this under the table lock serialises against new registrations
        // (which also take the lock before setting the bit), so we cannot
        // clear a bit that a freshly-queued waiter still needs.
        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        let more = table.waiters[idx]
            .iter()
            .any(|w| w.addr == addr && w.addr_space == addr_space);
        if !more {
            atomic.fetch_and(!FUTEX_WAITERS_BIT, Ordering::Release);
        }

        // Deboost the real current owner: with us gone, its inherited
        // priority may drop.
        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        let real_owner = table.owners[idx]
            .iter()
            .find(|o| o.addr == addr && o.addr_space == addr_space)
            .map(|o| o.owner_id);
        deboost = real_owner.map(|oid| (oid, recalculate_inherited_for_owner(&table, oid)));

        break Err(KernelError::TimedOut);
    };

    // Cancel the timer (no-op if it already fired).
    if let Some(handle) = timer_handle {
        crate::hrtimer::cancel(handle);
    }

    // We're no longer blocked on this PI address.
    sched::set_blocked_on_pi_addr(current_id, None);

    // Apply any deboost outside the table lock.
    if let Some((oid, recalc)) = deboost {
        sched::set_inherited_priority(oid, recalc);
    }

    outcome
}

/// Try to lock a PI futex without blocking.
///
/// Acquires the lock at `addr` if it is uncontended.  If the lock is
/// held by another task, returns `Err(WouldBlock)` (maps to `EAGAIN`).
/// If the caller already owns the lock, returns `Err(Deadlock)` (maps to
/// `EDEADLK`), matching Linux `FUTEX_TRYLOCK_PI`.
///
/// # Returns
///
/// - `Ok(())` — lock acquired.
/// - `Err(InvalidAddress)` — `addr` is null.
/// - `Err(BadAlignment)` — `addr` is not 4-byte aligned.
/// - `Err(Deadlock)` — caller already owns the lock.
/// - `Err(WouldBlock)` — lock is held by another task.
///
/// # Safety contract
///
/// `addr` must point to a valid, aligned `AtomicU32`.
pub fn futex_trylock_pi(addr: u64) -> KernelResult<()> {
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }

    let current_id = sched::current_task_id();
    let addr_space = current_addr_space();
    #[allow(clippy::cast_possible_truncation)]
    let current_tid = (current_id as u32) & FUTEX_TID_MASK;

    // SAFETY: Caller guarantees addr is valid and aligned.
    let atomic = unsafe { &*(addr as *const AtomicU32) };

    // Fast path: CAS 0 → our tid (uncontended acquisition).
    if atomic
        .compare_exchange(0, current_tid, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        register_pi_owner(addr, addr_space, current_id);
        return Ok(());
    }

    // Contended — distinguish self-deadlock from "held by another".
    let word = atomic.load(Ordering::Acquire);
    let oid = u64::from(word & FUTEX_TID_MASK);
    if oid == current_id {
        return Err(KernelError::Deadlock);
    }
    Err(KernelError::WouldBlock)
}

/// Unlock a PI futex.
///
/// Releases the lock at `addr` and transfers ownership to the highest-
/// priority waiting task (if any).  Restores the caller's priority to
/// its base level (or recalculates based on other held PI locks).
///
/// # Returns
///
/// - `Ok(())` — lock released successfully.
/// - `Err(InvalidAddress)` — `addr` is null.
/// - `Err(BadAlignment)` — `addr` is not 4-byte aligned.
/// - `Err(InvalidArgument)` — caller is not the lock owner.
pub fn futex_unlock_pi(addr: u64) -> KernelResult<()> {
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }

    let current_id = sched::current_task_id();
    let addr_space = current_addr_space();

    // SAFETY: Caller guarantees addr is valid and aligned.
    let atomic = unsafe { &*(addr as *const AtomicU32) };

    // Verify we're the owner.
    let word = atomic.load(Ordering::Acquire);
    let word_owner = u64::from(word & FUTEX_TID_MASK);
    if word_owner != current_id {
        return Err(KernelError::InvalidArgument);
    }

    // Find the highest-priority waiter and prepare ownership transfer.
    let waiter_to_wake: Option<TaskId>;
    let has_more_waiters: bool;
    let recalc_priority: Option<u8>;

    {
        let mut table = PI_FUTEX_TABLE.lock();

        // Remove our ownership record.
        unregister_pi_owner(&mut table, addr, addr_space, current_id);

        // Find the highest-priority (lowest number) waiter for this addr
        // in our address space.
        let idx = FutexTable::bucket_index(addr, addr_space);
        let mut best_idx: Option<usize> = None;
        let mut best_prio: u8 = u8::MAX;

        // SAFETY: idx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        for (i, w) in table.waiters[idx].iter().enumerate() {
            if w.addr == addr && w.addr_space == addr_space && w.priority < best_prio {
                best_prio = w.priority;
                best_idx = Some(i);
            }
        }

        // Remove the selected waiter.
        #[allow(clippy::indexing_slicing)]
        {
            waiter_to_wake = best_idx
                .and_then(|bi| table.waiters[idx].remove(bi))
                .map(|w| w.task_id);
        }

        // Check if more waiters remain for this address + addr_space.
        #[allow(clippy::indexing_slicing)]
        {
            has_more_waiters = table.waiters[idx]
                .iter()
                .any(|w| w.addr == addr && w.addr_space == addr_space);
        }

        // Recalculate our inherited priority based on remaining locks.
        recalc_priority = recalculate_inherited_for_owner(&table, current_id);
    }

    // Transfer ownership or clear the futex word.
    if let Some(new_owner_id) = waiter_to_wake {
        #[allow(clippy::cast_possible_truncation)]
        let new_tid = (new_owner_id as u32) & FUTEX_TID_MASK;
        let new_word = new_tid
            | if has_more_waiters { FUTEX_WAITERS_BIT } else { 0 };
        atomic.store(new_word, Ordering::Release);

        // Register the new owner (same addr_space — they're in the same
        // address space since they were waiting on the same futex).
        register_pi_owner(addr, addr_space, new_owner_id);

        // Wake the new owner.
        sched::wake(new_owner_id);
    } else {
        // No waiters — clear the word entirely.
        atomic.store(0, Ordering::Release);
    }

    // Restore our priority (clear or recalculate inherited).
    sched::set_inherited_priority(current_id, recalc_priority);

    Ok(())
}

// ---------------------------------------------------------------------------
// Thread-exit cleanup: robust-mutex list + owned PI futexes
// ---------------------------------------------------------------------------
//
// When a thread dies (cleanly or abruptly) it may still hold mutexes.  Two
// independent recovery paths run from the thread-exit hook, both while the
// dying thread's address space is still active (CR3 unchanged), so the
// userspace futex words resolve:
//
//   * `exit_robust_list` walks the thread's registered userspace robust
//     list (the `PTHREAD_MUTEX_ROBUST` protocol).  For every lock the
//     thread still owns it sets `FUTEX_OWNER_DIED` and, for *non-PI*
//     mutexes with waiters, wakes one so the next acquirer observes the
//     death.  This mirrors Linux's `exit_robust_list` /
//     `handle_futex_death` (kernel/futex/core.c).
//
//   * `exit_pi_owned_futexes` walks the *kernel* PI ownership records for
//     the dying thread and hands each held PI mutex to its highest-priority
//     blocked waiter (or clears it), setting `FUTEX_OWNER_DIED` so the new
//     owner knows it inherited a dead-owner lock.  This mirrors Linux's
//     `exit_pi_state_list`: without it, a thread blocked in `FUTEX_LOCK_PI`
//     on a mutex whose owner died would hang forever.
//
// Both must run: the robust-list walk handles the userspace-visible flag
// and non-PI wakeups; the PI-state walk unblocks kernel-parked PI waiters.

/// Maximum robust-list entries to walk before giving up.
///
/// Matches Linux's `ROBUST_LIST_LIMIT` (kernel/futex/core.c).  The list is
/// userspace-controlled and may be circular or corrupt; this bound
/// guarantees the exit path terminates regardless of what the (possibly
/// hostile) program put there.
const ROBUST_LIST_LIMIT: u32 = 2048;

/// Split a raw robust-list `next` pointer into `(masked_pointer, is_pi)`.
///
/// The glibc protocol overloads the low bit of every link as the PI flag,
/// so the real pointer is the value with bit 0 cleared.  Pure (no memory
/// access) so the masking is unit-testable.
fn split_robust_ptr(raw: u64) -> (u64, bool) {
    (raw & !1u64, raw & 1 != 0)
}

/// Read one robust-list `next` pointer from user memory and split off its
/// PI flag bit.
///
/// Returns `(masked_pointer, is_pi)`, or `None` if the user read faults
/// (which aborts the walk, exactly as Linux's `fetch_robust_entry` does).
fn fetch_robust_entry(ptr: u64) -> Option<(u64, bool)> {
    // SAFETY: `read_user` validates the range is user-space and mapped
    // before dereferencing; a fault returns `Err`, never UB.
    let raw = unsafe { read_user::<u64>(ptr) }.ok()?;
    Some(split_robust_ptr(raw))
}

/// The action the dead-owner protocol takes for a given futex word.
#[derive(Debug, PartialEq, Eq)]
enum RobustDeath {
    /// The dying thread does not own this lock — leave it untouched.
    Leave,
    /// The (non-PI, list-op-pending) word is already 0: just wake a waiter
    /// so a contender does not sleep on an unowned lock.
    WakeOnly,
    /// Store `new` into the word (OWNER_DIED set, WAITERS preserved, TID
    /// cleared); `wake` says whether to wake one waiter afterwards.
    SetDied { new: u32, wake: bool },
}

/// Pure decision for the robust dead-owner protocol given the current futex
/// `uval`.  Mirrors Linux's `handle_futex_death` (kernel/futex/core.c)
/// without any memory access, so the bit logic is unit-testable.
fn robust_death_transition(uval: u32, dying_tid: u32, pi: bool, pending_op: bool) -> RobustDeath {
    // Corner case: the thread died right after a userspace lock CAS but
    // before the word reflected ownership (word == 0).  Only the
    // list-op-pending, non-PI entry can legitimately be in this state.
    if pending_op && !pi && uval == 0 {
        return RobustDeath::WakeOnly;
    }
    // Only act on locks the dying thread actually owns.
    if uval & FUTEX_TID_MASK != dying_tid {
        return RobustDeath::Leave;
    }
    // Set OWNER_DIED, preserve WAITERS, drop the owner TID.  For non-PI
    // mutexes with waiters, wake one; PI mutexes are handed off by the
    // PI-state walk instead (matching Linux, which only wakes the `!pi`
    // case here).
    let new = (uval & FUTEX_WAITERS_BIT) | FUTEX_OWNER_DIED_BIT;
    let wake = !pi && uval & FUTEX_WAITERS_BIT != 0;
    RobustDeath::SetDied { new, wake }
}

/// Apply the dead-owner protocol to a single futex word in user memory.
///
/// Validates `futex_addr` (the offset comes from an attacker-controlled
/// robust list, so the resulting address may point anywhere) and then
/// applies [`robust_death_transition`] with a CAS so a concurrent acquirer
/// cannot be clobbered.
fn handle_futex_death(futex_addr: u64, dying_tid: u32, pi: bool, pending_op: bool) {
    // A malformed offset can yield a null or misaligned address; skip it
    // but keep walking (it cannot be one of our locks).
    #[allow(clippy::arithmetic_side_effects)]
    if futex_addr == 0 || futex_addr & 3 != 0 {
        return;
    }
    // The word must be a writable user address.  If it is not (page torn
    // down already, or a hostile offset pointing outside the mapping),
    // skip it — we cannot touch it, but the walk continues.
    if validate_user_write(futex_addr, 4).is_err() {
        return;
    }

    // SAFETY: validated as a writable, mapped, 4-byte-aligned user address
    // just above; the dying thread's address space is still active.
    let atomic = unsafe { &*(futex_addr as *const AtomicU32) };

    loop {
        let uval = atomic.load(Ordering::Acquire);
        match robust_death_transition(uval, dying_tid, pi, pending_op) {
            RobustDeath::Leave => return,
            RobustDeath::WakeOnly => {
                let _ = futex_wake(futex_addr, 1);
                return;
            }
            RobustDeath::SetDied { new, wake } => {
                if atomic
                    .compare_exchange(uval, new, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    if wake {
                        let _ = futex_wake(futex_addr, 1);
                    }
                    return;
                }
                // Raced with a concurrent CAS on the word; loop re-reads.
            }
        }
    }
}

/// Walk a dying thread's userspace robust-mutex list and apply the
/// dead-owner protocol to every lock it still holds.
///
/// `head_ptr` is the `struct robust_list_head*` the thread registered via
/// `set_robust_list(2)`; `dying_task` is its TID.  Must be called from the
/// dying thread's context so the user reads resolve.
///
/// All user pointers are validated before access (the list is
/// attacker-controlled) and the walk is bounded by [`ROBUST_LIST_LIMIT`],
/// so a corrupt or circular list cannot hang or corrupt the kernel.
pub fn exit_robust_list(head_ptr: u64, dying_task: TaskId) {
    if head_ptr == 0 {
        return;
    }
    #[allow(clippy::cast_possible_truncation)]
    let dying_tid = (dying_task as u32) & FUTEX_TID_MASK;
    // A TID of 0 can never appear as a futex owner (the boot thread does
    // not use robust mutexes), so there is nothing to clean up and the
    // compare below would spuriously match free words.
    if dying_tid == 0 {
        return;
    }

    // struct robust_list_head {
    //     struct robust_list *list;          // offset 0  (circular anchor)
    //     long                futex_offset;   // offset 8  (signed)
    //     struct robust_list *list_op_pending;// offset 16
    // };
    // `head->list.next` is at offset 0 (robust_list has a single `next`).
    let (mut entry, mut pi) = match fetch_robust_entry(head_ptr) {
        Some(v) => v,
        None => return,
    };
    // SAFETY: read_user validates the address.
    let futex_offset = match unsafe { read_user::<i64>(head_ptr.wrapping_add(8)) } {
        Ok(v) => v,
        Err(_) => return,
    };
    let (pending, pending_pi) =
        fetch_robust_entry(head_ptr.wrapping_add(16)).unwrap_or((0, false));

    let mut limit = ROBUST_LIST_LIMIT;
    // The list is circular: it terminates when we loop back to the anchor
    // (`&head->list`, whose address is `head_ptr`).
    while entry != head_ptr {
        // Fetch the next link BEFORE touching this entry's futex — once
        // OWNER_DIED is set, a waking thread may unlink and free the node.
        let next = fetch_robust_entry(entry);

        if entry != pending {
            let futex_addr = (entry as i64).wrapping_add(futex_offset) as u64;
            handle_futex_death(futex_addr, dying_tid, pi, false);
        }

        match next {
            Some((n, n_pi)) => {
                entry = n;
                pi = n_pi;
            }
            // Could not read the next link: stop (the rest is unreachable).
            None => return,
        }

        limit = limit.saturating_sub(1);
        if limit == 0 {
            break;
        }
    }

    // Finally, the list-op-pending entry (a lock mid acquire/release when
    // the thread died), if any.
    if pending != 0 {
        let futex_addr = (pending as i64).wrapping_add(futex_offset) as u64;
        handle_futex_death(futex_addr, dying_tid, pending_pi, true);
    }
}

/// Hand off every PI futex owned by a dying thread to its highest-priority
/// blocked waiter (or clear it), setting `FUTEX_OWNER_DIED` on the new
/// word.
///
/// Without this, a task blocked in `FUTEX_LOCK_PI` on a mutex whose owner
/// died (e.g. crashed, or exited holding a non-robust PI mutex) would never
/// be woken — `futex_unlock_pi` is the only other path that transfers
/// ownership, and a dead thread never calls it.
///
/// Runs from the dying thread's context.  The futex-word store is
/// best-effort (the page may already be unmapped); the authoritative state
/// is the kernel ownership record plus the `sched::wake`, which the woken
/// waiter re-checks under the table lock before returning from
/// `futex_lock_pi`.
pub fn exit_pi_owned_futexes(dying_task: TaskId) {
    // Process one owned mutex per iteration.  Each transfer mutates the
    // owners/waiters tables, so we re-scan from scratch until no ownership
    // record for the dying task remains.  The number of held mutexes is
    // small in practice and bounded by the number of registered owners.
    loop {
        // Collected under the PI table lock, applied after release to honour
        // the PI_FUTEX_TABLE → SCHED lock ordering.
        struct Handoff {
            addr: u64,
            new_owner: Option<TaskId>,
            has_more: bool,
        }

        let handoff = {
            let mut table = PI_FUTEX_TABLE.lock();

            // Find one ownership record for the dying task.
            let mut found: Option<(u64, u64)> = None;
            'scan: for bucket in &table.owners {
                for o in bucket {
                    if o.owner_id == dying_task {
                        found = Some((o.addr, o.addr_space));
                        break 'scan;
                    }
                }
            }
            let (addr, addr_space) = match found {
                Some(v) => v,
                None => break, // no more owned PI mutexes
            };

            // Drop the dead owner's record.
            unregister_pi_owner(&mut table, addr, addr_space, dying_task);

            // Pick the highest-priority (lowest number) waiter for this addr.
            let idx = FutexTable::bucket_index(addr, addr_space);
            let mut best_idx: Option<usize> = None;
            let mut best_prio: u8 = u8::MAX;
            // SAFETY: idx is masked to NUM_BUCKETS - 1 by bucket_index.
            #[allow(clippy::indexing_slicing)]
            for (i, w) in table.waiters[idx].iter().enumerate() {
                if w.addr == addr && w.addr_space == addr_space && w.priority < best_prio {
                    best_prio = w.priority;
                    best_idx = Some(i);
                }
            }
            // SAFETY: idx is masked to NUM_BUCKETS - 1.
            #[allow(clippy::indexing_slicing)]
            let new_owner = best_idx
                .and_then(|bi| table.waiters[idx].remove(bi))
                .map(|w| w.task_id);

            // Any waiters still queued after we removed the chosen one?
            // SAFETY: idx is masked to NUM_BUCKETS - 1.
            #[allow(clippy::indexing_slicing)]
            let has_more = table.waiters[idx]
                .iter()
                .any(|w| w.addr == addr && w.addr_space == addr_space);

            // Register the new owner while still holding the lock so a
            // concurrent locker cannot race in between.
            if let Some(new_id) = new_owner {
                // SAFETY: idx is masked to NUM_BUCKETS - 1.
                #[allow(clippy::indexing_slicing)]
                table.owners[idx].push_back(PiOwner {
                    addr,
                    addr_space,
                    owner_id: new_id,
                });
            }

            Handoff {
                addr,
                new_owner,
                has_more,
            }
        };

        // Apply the userspace word + wake outside the table lock.
        // The word lives in the dying thread's (still-active) address space.
        if validate_user_write(handoff.addr, 4).is_ok() {
            // SAFETY: validated writable user address; AS still active.
            let atomic = unsafe { &*(handoff.addr as *const AtomicU32) };
            match handoff.new_owner {
                Some(new_id) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let new_tid = (new_id as u32) & FUTEX_TID_MASK;
                    let word = new_tid
                        | FUTEX_OWNER_DIED_BIT
                        | if handoff.has_more { FUTEX_WAITERS_BIT } else { 0 };
                    atomic.store(word, Ordering::Release);
                }
                None => {
                    // No waiter: leave OWNER_DIED so the next userspace
                    // acquirer of a robust mutex still detects the death.
                    atomic.store(FUTEX_OWNER_DIED_BIT, Ordering::Release);
                }
            }
        }

        // Wake the new owner — even if the word store above failed, the
        // ownership record is registered, so the waiter returns Ok once it
        // re-checks the owners table.
        if let Some(new_id) = handoff.new_owner {
            sched::wake(new_id);
        }
    }
}

// ---------------------------------------------------------------------------
// Requeue-PI: condvar → PI mutex handoff (FUTEX_WAIT/CMP_REQUEUE_PI)
// ---------------------------------------------------------------------------
//
// These two operations implement the path `pthread_cond_wait` takes when
// the associated mutex is priority-inheriting:
//
//   * A waiter calls `futex_wait_requeue_pi(cond, val, pi_mutex, timeout)`.
//     It has *already* released `pi_mutex` in userspace.  It checks
//     `*cond == val`; if so it parks on the condvar's requeue queue,
//     remembering that it must wind up owning `pi_mutex`.
//
//   * A signaller calls `futex_cmp_requeue_pi(cond, pi_mutex, nr, val)`.
//     After verifying `*cond == val`, it takes the highest-priority parked
//     waiter and tries to acquire `pi_mutex` *for* it: if the mutex is
//     free, that waiter becomes the owner and is woken; otherwise it (and
//     up to `nr` further waiters) are moved onto the PI waiter queue for
//     `pi_mutex`, where the current holder's eventual `unlock_pi` hands the
//     lock down one at a time.  This avoids the thundering herd *and*
//     preserves priority inheritance across the handoff.
//
// Simplification vs. Linux: a plain `FUTEX_WAKE` on the condvar does not
// wake a requeue-PI waiter (they live on a private queue).  glibc's PI
// condvar only ever signals via `FUTEX_CMP_REQUEUE_PI`, so this is
// behaviourally complete for the intended caller.  Documented in todo.txt.

/// Remove and return the highest-priority requeue-PI waiter parked on
/// `cond_addr` whose destination matches `target_addr`.
///
/// "Highest priority" = lowest priority number; ties break FIFO (earliest
/// enqueued), matching the ordering `unlock_pi` uses for the PI queue.
fn take_best_requeue_waiter(
    table: &mut PiFutexTable,
    cidx: usize,
    cond_addr: u64,
    addr_space: u64,
    target_addr: u64,
) -> Option<RequeuePiWaiter> {
    // SAFETY: cidx is masked to NUM_BUCKETS - 1 by bucket_index.
    #[allow(clippy::indexing_slicing)]
    let q = &mut table.requeue_waiters[cidx];
    let mut best: Option<usize> = None;
    let mut best_prio: u8 = u8::MAX;
    for (i, w) in q.iter().enumerate() {
        if w.cond_addr == cond_addr
            && w.addr_space == addr_space
            && w.target_addr == target_addr
            && w.priority < best_prio
        {
            best_prio = w.priority;
            best = Some(i);
        }
    }
    best.and_then(|i| q.remove(i))
}

/// Wait on a condvar futex, to be requeued onto a PI mutex on wake.
///
/// Backs `FUTEX_WAIT_REQUEUE_PI`.  Atomically checks `*cond_addr == val`;
/// if it matches, the caller parks until a `futex_cmp_requeue_pi` (on
/// `cond_addr` targeting `pi_addr`) either grants it ownership of `pi_addr`
/// or moves it onto the PI waiter queue and a later `unlock_pi` transfers
/// ownership.  On success the caller returns owning `pi_addr`.
///
/// `timeout_ns`:
/// - `None` — wait indefinitely.
/// - `Some(0)` — value matched but do not block: returns `Err(TimedOut)`.
/// - `Some(ns)` — wait up to `ns` nanoseconds (covers *both* phases: while
///   parked on the condvar and after requeue while awaiting the mutex).
///
/// # Returns
///
/// - `Ok(())` — woken and now owns `pi_addr`.
/// - `Err(WouldBlock)` — `*cond_addr != val` (→ `EAGAIN`); no blocking.
/// - `Err(TimedOut)` — the deadline expired (→ `ETIMEDOUT`).
/// - `Err(InvalidAddress)` — either address is null.
/// - `Err(BadAlignment)` — either address is not 4-byte aligned.
/// - `Err(InvalidArgument)` — `cond_addr == pi_addr` (Linux requires they
///   differ).
///
/// # Safety contract
///
/// `cond_addr` must point to a valid, aligned readable `AtomicU32`;
/// `pi_addr` to a valid, aligned readable/writable `AtomicU32`.
pub fn futex_wait_requeue_pi(
    cond_addr: u64,
    val: u32,
    pi_addr: u64,
    timeout_ns: Option<u64>,
) -> KernelResult<()> {
    if cond_addr == 0 || pi_addr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if cond_addr & 3 != 0 || pi_addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }
    // Linux rejects uaddr == uaddr2 with EINVAL: requeuing onto the same
    // word makes no sense and would corrupt the queue bookkeeping.
    if cond_addr == pi_addr {
        return Err(KernelError::InvalidArgument);
    }

    let current_id = sched::current_task_id();
    let addr_space = current_addr_space();
    let our_priority =
        sched::get_effective_priority(current_id).unwrap_or(sched::task::IDLE_PRIORITY);

    // SAFETY: caller validated cond_addr as a readable, aligned user word.
    let cond = unsafe { &*(cond_addr as *const AtomicU32) };
    // SAFETY: caller validated pi_addr as a writable, aligned user word.
    let pi = unsafe { &*(pi_addr as *const AtomicU32) };

    // Park on the condvar queue iff *cond_addr == val.  The value check and
    // the enqueue happen under the same PI table lock that
    // futex_cmp_requeue_pi takes, so a concurrent signaller cannot slip
    // between the two.
    {
        let mut table = PI_FUTEX_TABLE.lock();
        let actual = cond.load(Ordering::Acquire);
        if actual != val {
            super::stats::futex_spurious();
            return Err(KernelError::WouldBlock);
        }
        // Zero timeout: value matched but we must not block.
        if matches!(timeout_ns, Some(0)) {
            return Err(KernelError::TimedOut);
        }
        super::stats::futex_wait();
        let cidx = FutexTable::bucket_index(cond_addr, addr_space);
        // SAFETY: cidx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        table.requeue_waiters[cidx].push_back(RequeuePiWaiter {
            cond_addr,
            addr_space,
            target_addr: pi_addr,
            task_id: current_id,
            priority: our_priority,
        });
    }

    // We are conceptually trying to acquire pi_addr; record it so that once
    // cmp_requeue_pi moves us onto the PI waiter queue, transitive PI sees
    // us as blocked on that lock.  Cleared on every exit path below.
    sched::set_blocked_on_pi_addr(current_id, Some(pi_addr));

    // Arm a one-shot timeout if requested.  The same timer covers both the
    // condvar-wait phase and the post-requeue PI-wait phase.
    let timer_handle = match timeout_ns {
        Some(ns) => {
            fn rq_timeout_wake(tid: u64) {
                if !sched::try_wake(tid) {
                    sched::defer_wake(tid);
                }
            }
            Some(crate::hrtimer::schedule_ns(ns, rq_timeout_wake, current_id))
        }
        None => None,
    };

    // Block until we own pi_addr, or the timer fires.  Deboost data is
    // gathered under the lock and applied after release (PI_FUTEX_TABLE →
    // SCHED order).
    let mut deboost: Option<(TaskId, Option<u8>)> = None;
    let outcome: KernelResult<()> = loop {
        sched::block_current();

        let mut table = PI_FUTEX_TABLE.lock();
        let pidx = FutexTable::bucket_index(pi_addr, addr_space);
        let cidx = FutexTable::bucket_index(cond_addr, addr_space);

        // (1) Did cmp_requeue_pi / unlock_pi make us the PI mutex owner?
        // SAFETY: pidx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        let is_owner = table.owners[pidx]
            .iter()
            .any(|o| o.addr == pi_addr && o.addr_space == addr_space && o.owner_id == current_id);
        if is_owner {
            break Ok(());
        }

        // Not the owner.  The only wakes that reach a requeue-PI waiter are
        // an ownership transfer (handled above) or the timeout timer —
        // cmp_requeue_pi requeues us *without* waking.  So with no timeout
        // this wake is spurious: re-block.
        if timeout_ns.is_none() {
            drop(table);
            continue;
        }

        // Timed out.  We are still queued in exactly one place; remove
        // ourselves and clean up.

        // Phase A: still parked on the condvar (never signalled).
        // SAFETY: cidx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        if let Some(pos) = table.requeue_waiters[cidx].iter().position(|w| {
            w.task_id == current_id && w.cond_addr == cond_addr && w.addr_space == addr_space
        }) {
            table.requeue_waiters[cidx].remove(pos);
            break Err(KernelError::TimedOut);
        }

        // Phase B: requeued onto the PI mutex, awaiting ownership transfer.
        // SAFETY: pidx is masked to NUM_BUCKETS - 1.
        #[allow(clippy::indexing_slicing)]
        if let Some(pos) = table.waiters[pidx].iter().position(|w| {
            w.task_id == current_id && w.addr == pi_addr && w.addr_space == addr_space
        }) {
            table.waiters[pidx].remove(pos);
            // Clear WAITERS if we were the last PI waiter on this address.
            // SAFETY: pidx is masked to NUM_BUCKETS - 1.
            #[allow(clippy::indexing_slicing)]
            let more = table.waiters[pidx]
                .iter()
                .any(|w| w.addr == pi_addr && w.addr_space == addr_space);
            if !more {
                pi.fetch_and(!FUTEX_WAITERS_BIT, Ordering::Release);
            }
            // Deboost the real owner now that our donation is gone.
            // SAFETY: pidx is masked to NUM_BUCKETS - 1.
            #[allow(clippy::indexing_slicing)]
            let real_owner = table.owners[pidx]
                .iter()
                .find(|o| o.addr == pi_addr && o.addr_space == addr_space)
                .map(|o| o.owner_id);
            deboost = real_owner.map(|oid| (oid, recalculate_inherited_for_owner(&table, oid)));
            break Err(KernelError::TimedOut);
        }

        // Not owner and in neither queue: unreachable, since every queue
        // transition happens under this lock.  Treat defensively as a
        // spurious wake and re-block rather than returning a bogus success.
        drop(table);
        continue;
    };

    if let Some(handle) = timer_handle {
        crate::hrtimer::cancel(handle);
    }
    sched::set_blocked_on_pi_addr(current_id, None);
    if let Some((oid, recalc)) = deboost {
        sched::set_inherited_priority(oid, recalc);
    }
    outcome
}

/// Signal a PI condvar: wake/requeue waiters from `cond_addr` onto the PI
/// mutex `pi_addr`.
///
/// Backs `FUTEX_CMP_REQUEUE_PI`.  After verifying `*cond_addr == val`
/// (race detection; mismatch → `Err(WouldBlock)`), at most one waiter is
/// granted ownership of `pi_addr` (only if the mutex is currently free)
/// and up to `max_requeue` further waiters are moved onto the PI waiter
/// queue for `pi_addr`.  The granted owner is woken; requeued waiters stay
/// blocked until the holder unlocks.
///
/// Returns the number of waiters affected (woken + requeued), matching
/// Linux's `futex_requeue` return convention.
///
/// # Errors
///
/// - `Err(WouldBlock)` — `*cond_addr != val` (→ `EAGAIN`); nothing touched.
/// - `Err(InvalidAddress)` / `Err(BadAlignment)` — bad address.
/// - `Err(InvalidArgument)` — `cond_addr == pi_addr`.
///
/// # Safety contract
///
/// `cond_addr` must point to a valid, aligned readable `AtomicU32`;
/// `pi_addr` to a valid, aligned readable/writable `AtomicU32`.
pub fn futex_cmp_requeue_pi(
    cond_addr: u64,
    pi_addr: u64,
    max_requeue: u32,
    val: u32,
) -> KernelResult<u32> {
    if cond_addr == 0 || pi_addr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if cond_addr & 3 != 0 || pi_addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }
    if cond_addr == pi_addr {
        return Err(KernelError::InvalidArgument);
    }

    let addr_space = current_addr_space();

    // SAFETY: caller validated cond_addr as a readable, aligned user word.
    let cond = unsafe { &*(cond_addr as *const AtomicU32) };
    // SAFETY: caller validated pi_addr as a writable, aligned user word.
    let pi = unsafe { &*(pi_addr as *const AtomicU32) };

    // Results collected under the table lock, applied after release.
    let mut owner_to_wake: Option<TaskId> = None;
    let mut woken: u32 = 0;
    let mut requeued: u32 = 0;
    let mut boost: Option<(TaskId, u8)> = None;

    {
        let mut table = PI_FUTEX_TABLE.lock();

        // Compare *cond_addr == val under the lock (mismatch → EAGAIN).
        let actual = cond.load(Ordering::Acquire);
        if actual != val {
            super::stats::futex_spurious();
            return Err(KernelError::WouldBlock);
        }

        let cidx = FutexTable::bucket_index(cond_addr, addr_space);
        let pidx = FutexTable::bucket_index(pi_addr, addr_space);

        // Budget: one implicit wake (the proxy-lock acquisition, Linux's
        // nr_wake which must be 1) plus max_requeue requeues.
        let budget = max_requeue.saturating_add(1);
        let mut processed: u32 = 0;
        let mut best_requeue_prio: u8 = u8::MAX;

        while processed < budget {
            let Some(w) =
                take_best_requeue_waiter(&mut table, cidx, cond_addr, addr_space, pi_addr)
            else {
                break;
            };
            processed = processed.saturating_add(1);

            // Re-read the owner each iteration (it changes after a grant).
            let word = pi.load(Ordering::Acquire);
            let owner_tid = word & FUTEX_TID_MASK;

            if owner_tid == 0 && owner_to_wake.is_none() {
                // Mutex free: grant ownership to this (highest-prio) waiter.
                #[allow(clippy::cast_possible_truncation)]
                let new_tid = (w.task_id as u32) & FUTEX_TID_MASK;
                pi.store(new_tid, Ordering::Release);
                // SAFETY: pidx is masked to NUM_BUCKETS - 1.
                #[allow(clippy::indexing_slicing)]
                table.owners[pidx].push_back(PiOwner {
                    addr: pi_addr,
                    addr_space,
                    owner_id: w.task_id,
                });
                owner_to_wake = Some(w.task_id);
                woken = 1;
            } else {
                // Mutex held: requeue as a PI waiter (do NOT wake — the
                // holder's unlock_pi will transfer ownership later).
                // SAFETY: pidx is masked to NUM_BUCKETS - 1.
                #[allow(clippy::indexing_slicing)]
                table.waiters[pidx].push_back(PiWaiter {
                    addr: pi_addr,
                    addr_space,
                    task_id: w.task_id,
                    priority: w.priority,
                });
                if w.priority < best_requeue_prio {
                    best_requeue_prio = w.priority;
                }
                requeued = requeued.saturating_add(1);
            }
        }

        // If we parked any PI waiters, set the WAITERS bit and arrange to
        // boost the current owner up to the best requeued priority.
        if requeued > 0 {
            pi.fetch_or(FUTEX_WAITERS_BIT, Ordering::Release);
            let owner_id = owner_to_wake.or_else(|| {
                let word = pi.load(Ordering::Acquire);
                let t = word & FUTEX_TID_MASK;
                if t == 0 { None } else { Some(u64::from(t)) }
            });
            if let Some(oid) = owner_id {
                boost = Some((oid, best_requeue_prio));
            }
        }
    }

    // Apply scheduler effects outside the PI table lock.  Requeued waiters
    // already recorded `blocked_on_pi_addr = pi_addr` in their wait path, so
    // only the boost and the wake of the new owner remain.
    if let Some((oid, prio)) = boost {
        sched::boost_priority(oid, prio);
        sched::pi_chain_boost(oid, prio, find_pi_owner);
    }
    if let Some(o) = owner_to_wake {
        sched::wake(o);
    }

    super::stats::futex_wake(woken);
    Ok(woken.saturating_add(requeued))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run futex self-tests.
///
/// Tests:
/// 1. `futex_wait` with non-matching value (returns immediately).
/// 2. `futex_wake` with no waiters (returns 0).
/// 3. Blocking wait + wake via spawned task.
/// 4. `futex_waitv` multi-key value-mismatch fast path (no block).  The
///    blocking multi-key outcomes (timeout / woken-by-index) need the
///    hrtimer and live in [`self_test_timeout`].
/// 5. Priority inheritance: high-prio task boosts low-prio lock holder.
/// 6. Robust dead-owner protocol (pure bit logic + OWNER_DIED relock).
/// 7. PI owner-death handoff: a blocked `FUTEX_LOCK_PI` waiter inherits a
///    dead owner's mutex instead of hanging.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[futex] Running futex self-test...");

    test_wait_value_mismatch()?;
    test_wake_no_waiters()?;
    test_blocking_wait_wake()?;
    test_wait_multiple()?;
    test_wake_bitset()?;
    test_requeue()?;
    test_wake_op()?;
    test_pi_trylock_deadlock()?;
    test_requeue_pi()?;
    test_robust_transition()?;
    test_owner_died_relock()?;
    test_pi_owner_death_handoff()?;
    test_priority_inheritance()?;

    serial_println!("[futex] Futex self-test PASSED");
    Ok(())
}

/// Result flag for the PI trylock/deadlock worker.
///
/// `0` = not yet finished, `1` = all checks passed, `>= 2` = a check
/// failed (the value is the 1-based index of the failing check).
static PI_TRYLOCK_RESULT: AtomicU32 = AtomicU32::new(0);

/// Worker that exercises `futex_trylock_pi` / `futex_lock_pi` ownership
/// semantics from a task with a **nonzero** TID.
///
/// The owner identity is packed into the low 30 bits of the futex word,
/// so a task with TID 0 (the boot thread during early self-tests) cannot
/// represent ownership — `CAS(0, 0)` is indistinguishable from acquiring
/// a free lock.  Real userspace threads never have TID 0, so these checks
/// must run in a spawned task to be meaningful.
///
/// `addr` points to a fresh PI word (initially 0) owned by the driver's
/// stack frame; the driver keeps it alive until this worker sets
/// `PI_TRYLOCK_RESULT`.
#[allow(clippy::cast_possible_truncation)]
extern "C" fn pi_trylock_worker(addr: u64) {
    let cur = (sched::current_task_id() as u32) & FUTEX_TID_MASK;
    // Each step returns false on the first failure; the driver records
    // `1` for pass and `2` for any failure.
    let run = || -> bool {
        // 1: uncontended trylock acquires.
        if futex_trylock_pi(addr).is_err() {
            return false;
        }
        // 2: re-acquire by the same owner → Deadlock.
        if !matches!(futex_trylock_pi(addr), Err(KernelError::Deadlock)) {
            return false;
        }
        // 3: release.
        if futex_unlock_pi(addr).is_err() {
            return false;
        }
        // 4: blocking lock acquires uncontended.
        if futex_lock_pi(addr).is_err() {
            return false;
        }
        // 5: blocking lock re-acquire by the same owner → Deadlock
        //    (must not block forever).
        if !matches!(futex_lock_pi(addr), Err(KernelError::Deadlock)) {
            return false;
        }
        // 6: release.
        if futex_unlock_pi(addr).is_err() {
            return false;
        }
        // 7: a lock held by another task → WouldBlock.  Plant a foreign
        //    owner tid that is nonzero and distinct from ours.
        let foreign_tid = {
            let t = (cur ^ 1) & FUTEX_TID_MASK;
            if t == 0 { 2 } else { t }
        };
        let foreign = AtomicU32::new(foreign_tid);
        let faddr = (&raw const foreign) as u64;
        matches!(futex_trylock_pi(faddr), Err(KernelError::WouldBlock))
    };
    PI_TRYLOCK_RESULT.store(if run() { 1 } else { 2 }, Ordering::SeqCst);
}

/// Test: PI trylock / lock ownership + self-deadlock detection.
///
/// Runs the checks in a spawned worker so the owner TID is nonzero (see
/// [`pi_trylock_worker`]).
fn test_pi_trylock_deadlock() -> KernelResult<()> {
    PI_TRYLOCK_RESULT.store(0, Ordering::SeqCst);

    let word = AtomicU32::new(0);
    let addr = (&raw const word) as u64;

    sched::spawn(b"pi-trylock", 16, pi_trylock_worker, addr, 0)?;

    // The worker never blocks, so it completes promptly once scheduled.
    // Keep `word` alive (it lives on this frame) until the worker reports.
    for _ in 0..20 {
        if PI_TRYLOCK_RESULT.load(Ordering::SeqCst) != 0 {
            break;
        }
        sched::yield_now();
    }

    let result = PI_TRYLOCK_RESULT.load(Ordering::SeqCst);
    sched::reap_dead_tasks();

    if result != 1 {
        serial_println!(
            "[futex]   FAIL: PI trylock/deadlock worker result {} (expected 1)",
            result
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Trylock/lock PI (acquire/deadlock/contended): OK");
    Ok(())
}

// --- Requeue-PI handoff test state ---------------------------------------
//
// Statics (not stack words) so they stay live across the whole handoff
// chain: a worker that acquires the PI mutex unlocks it to pass ownership
// to the next worker, which only then dereferences these words.

/// Condvar futex word for the requeue-PI test (expected value: 0).
static RPI_COND: AtomicU32 = AtomicU32::new(0);
/// PI mutex futex word for the requeue-PI test (0 = free).
static RPI_PI: AtomicU32 = AtomicU32::new(0);
/// Number of workers that have reached the requeue-PI wait.
static RPI_READY: AtomicU32 = AtomicU32::new(0);
/// Number of workers that completed the full acquire→unlock handoff.
static RPI_DONE: AtomicU32 = AtomicU32::new(0);
/// Number of workers that hit an unexpected error.
static RPI_FAIL: AtomicU32 = AtomicU32::new(0);

/// Worker for the requeue-PI test: parks on the condvar, and on wake must
/// own the PI mutex, which it then unlocks to hand off to the next waiter.
extern "C" fn requeue_pi_worker(_arg: u64) {
    let cond = (&raw const RPI_COND) as u64;
    let pi = (&raw const RPI_PI) as u64;
    RPI_READY.fetch_add(1, Ordering::SeqCst);
    match futex_wait_requeue_pi(cond, 0, pi, None) {
        Ok(()) => {
            // We now own the PI mutex.  Record progress, then unlock to
            // transfer ownership to the next requeued waiter (if any).
            RPI_DONE.fetch_add(1, Ordering::SeqCst);
            if futex_unlock_pi(pi).is_err() {
                RPI_FAIL.fetch_add(1, Ordering::SeqCst);
            }
        }
        Err(_) => {
            RPI_FAIL.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Test: `FUTEX_WAIT_REQUEUE_PI` / `FUTEX_CMP_REQUEUE_PI` condvar→PI-mutex
/// handoff.
///
/// Two workers park on the condvar (PI mutex initially free).  A
/// `cmp_requeue_pi` grants the mutex to one and requeues the other onto the
/// PI waiter queue; the chain of `unlock_pi` calls then walks ownership
/// down so both workers complete.  Also checks that a stale compare value
/// reports `WouldBlock` without disturbing the queue.
fn test_requeue_pi() -> KernelResult<()> {
    let cond = (&raw const RPI_COND) as u64;
    let pi = (&raw const RPI_PI) as u64;

    // (a) Stale compare must do nothing.  No waiters parked yet, but the
    //     value check happens first, so a mismatch short-circuits to EAGAIN.
    RPI_COND.store(0, Ordering::SeqCst);
    match futex_cmp_requeue_pi(cond, pi, u32::MAX, 7) {
        Err(KernelError::WouldBlock) => {}
        result => {
            serial_println!("[futex]   FAIL: cmp_requeue_pi stale compare {:?}", result);
            return Err(KernelError::InternalError);
        }
    }

    // (b) Functional handoff with two waiters.
    RPI_COND.store(0, Ordering::SeqCst);
    RPI_PI.store(0, Ordering::SeqCst);
    RPI_READY.store(0, Ordering::SeqCst);
    RPI_DONE.store(0, Ordering::SeqCst);
    RPI_FAIL.store(0, Ordering::SeqCst);

    sched::spawn(b"futex-rqpi0", 16, requeue_pi_worker, 0, 0)?;
    sched::spawn(b"futex-rqpi1", 16, requeue_pi_worker, 0, 0)?;

    // Let both workers reach the requeue-PI wait and block.
    for _ in 0..50 {
        if RPI_READY.load(Ordering::SeqCst) >= 2 {
            break;
        }
        sched::yield_now();
    }
    if RPI_READY.load(Ordering::SeqCst) < 2 {
        serial_println!("[futex]   FAIL: requeue-PI workers never parked");
        return Err(KernelError::InternalError);
    }
    // Extra yields so both are fully blocked inside futex_wait_requeue_pi.
    sched::yield_now();
    sched::yield_now();

    // Signal: grant the mutex to the top waiter, requeue the rest.  One is
    // woken (granted ownership) + one requeued = 2 affected.
    let affected = match futex_cmp_requeue_pi(cond, pi, u32::MAX, 0) {
        Ok(n) => n,
        Err(e) => {
            serial_println!("[futex]   FAIL: cmp_requeue_pi returned {:?}", e);
            return Err(KernelError::InternalError);
        }
    };
    if affected != 2 {
        serial_println!("[futex]   FAIL: cmp_requeue_pi affected={} (expected 2)", affected);
        return Err(KernelError::InternalError);
    }

    // Let the unlock_pi handoff chain run both workers to completion.
    for _ in 0..200 {
        if RPI_DONE.load(Ordering::SeqCst) >= 2 {
            break;
        }
        sched::yield_now();
    }
    let done = RPI_DONE.load(Ordering::SeqCst);
    let fail = RPI_FAIL.load(Ordering::SeqCst);
    let final_word = RPI_PI.load(Ordering::SeqCst);
    sched::reap_dead_tasks();

    if done != 2 || fail != 0 {
        serial_println!(
            "[futex]   FAIL: requeue-PI done={} fail={} (expected 2/0)",
            done,
            fail
        );
        return Err(KernelError::InternalError);
    }
    if final_word != 0 {
        serial_println!(
            "[futex]   FAIL: requeue-PI final word={:#x} (expected 0)",
            final_word
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Requeue-PI (condvar → PI mutex handoff): OK");
    Ok(())
}

// --- Robust-list / dead-owner cleanup tests ------------------------------

/// Test: pure decision logic for the robust dead-owner protocol.
///
/// Exercises [`split_robust_ptr`] (PI-flag bit extraction) and every branch
/// of [`robust_death_transition`].  No memory access, so it is a
/// deterministic unit test of the bit arithmetic that drives
/// `handle_futex_death` — the part that cannot be observed on the kernel
/// test words used elsewhere (the userspace word store is gated behind
/// `validate_user_write`, which rejects kernel addresses).
fn test_robust_transition() -> KernelResult<()> {
    // split_robust_ptr: bit 0 is the PI flag, the rest is the pointer.
    if split_robust_ptr(0x1000) != (0x1000, false)
        || split_robust_ptr(0x1001) != (0x1000, true)
        || split_robust_ptr(0) != (0, false)
    {
        serial_println!("[futex]   FAIL: split_robust_ptr");
        return Err(KernelError::InternalError);
    }

    let dying: u32 = 0x1234;

    // Not owned by the dying thread → leave untouched.
    if robust_death_transition(0x5678, dying, false, false) != RobustDeath::Leave {
        serial_println!("[futex]   FAIL: robust transition (not owner) != Leave");
        return Err(KernelError::InternalError);
    }

    // list-op-pending, non-PI, word still 0 (died mid-lock) → wake a waiter.
    if robust_death_transition(0, dying, false, true) != RobustDeath::WakeOnly {
        serial_println!("[futex]   FAIL: robust transition (pending op) != WakeOnly");
        return Err(KernelError::InternalError);
    }

    // Owned non-PI mutex, no waiters → set OWNER_DIED, no wake.
    match robust_death_transition(dying, dying, false, false) {
        RobustDeath::SetDied { new, wake } if new == FUTEX_OWNER_DIED_BIT && !wake => {}
        other => {
            serial_println!("[futex]   FAIL: robust transition (owned, no waiters) {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Owned non-PI mutex with waiters → OWNER_DIED, preserve WAITERS, wake.
    match robust_death_transition(dying | FUTEX_WAITERS_BIT, dying, false, false) {
        RobustDeath::SetDied { new, wake }
            if new == (FUTEX_OWNER_DIED_BIT | FUTEX_WAITERS_BIT) && wake => {}
        other => {
            serial_println!("[futex]   FAIL: robust transition (non-PI waiters) {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Owned PI mutex with waiters → set the flag but do NOT wake here (the
    // PI-state walk performs the authoritative kernel handoff instead).
    match robust_death_transition(dying | FUTEX_WAITERS_BIT, dying, true, false) {
        RobustDeath::SetDied { new, wake }
            if new == (FUTEX_OWNER_DIED_BIT | FUTEX_WAITERS_BIT) && !wake => {}
        other => {
            serial_println!("[futex]   FAIL: robust transition (PI waiters) {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[futex]   Robust death transition (pure logic): OK");
    Ok(())
}

/// Test: [`try_acquire_ownerless`] recovers dead-owner words while
/// preserving the recovery bits.
///
/// Deterministic (no scheduling): drives a local `AtomicU32` through the
/// states the robust/PI exit cleanup can leave behind, verifying that a
/// live owner blocks the claim and that `FUTEX_OWNER_DIED` / `FUTEX_WAITERS`
/// survive the CAS so userspace `EOWNERDEAD` recovery still works.
fn test_owner_died_relock() -> KernelResult<()> {
    let me: u32 = 0x0042;

    // Free word → claimed, owner stamped.
    let w = AtomicU32::new(0);
    if !try_acquire_ownerless(&w, me) || w.load(Ordering::SeqCst) != me {
        serial_println!("[futex]   FAIL: relock free word");
        return Err(KernelError::InternalError);
    }

    // Live owner → not claimed, word untouched.
    let w = AtomicU32::new(0x99);
    if try_acquire_ownerless(&w, me) || w.load(Ordering::SeqCst) != 0x99 {
        serial_println!("[futex]   FAIL: relock live owner");
        return Err(KernelError::InternalError);
    }

    // Dead owner (OWNER_DIED, no TID) → claimed, OWNER_DIED preserved.
    let w = AtomicU32::new(FUTEX_OWNER_DIED_BIT);
    if !try_acquire_ownerless(&w, me)
        || w.load(Ordering::SeqCst) != (me | FUTEX_OWNER_DIED_BIT)
    {
        serial_println!("[futex]   FAIL: relock OWNER_DIED");
        return Err(KernelError::InternalError);
    }

    // Dead owner with waiters → claimed, both recovery bits preserved.
    let w = AtomicU32::new(FUTEX_OWNER_DIED_BIT | FUTEX_WAITERS_BIT);
    if !try_acquire_ownerless(&w, me)
        || w.load(Ordering::SeqCst) != (me | FUTEX_OWNER_DIED_BIT | FUTEX_WAITERS_BIT)
    {
        serial_println!("[futex]   FAIL: relock OWNER_DIED|WAITERS");
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   OWNER_DIED relock (preserves recovery bits): OK");
    Ok(())
}

// --- PI owner-death handoff test state -----------------------------------
//
// Statics (not stack words) so they stay live across the whole handoff: the
// owner worker acquires the PI mutex, a waiter worker blocks on it, the
// driver simulates the owner's death via `exit_pi_owned_futexes`, and the
// waiter must inherit ownership instead of hanging.

/// PI mutex word for the owner-death test (0 = free).
static PID_PI: AtomicU32 = AtomicU32::new(0);
/// Set to 1 once the owner worker holds the PI mutex (2 on acquire error).
static PID_OWNED: AtomicU32 = AtomicU32::new(0);
/// Incremented by the waiter just before it parks on `futex_lock_pi`.
static PID_W_PARKED: AtomicU32 = AtomicU32::new(0);
/// Set to 1 once the waiter inherits ownership and returns from the lock.
static PID_RECOVERED: AtomicU32 = AtomicU32::new(0);
/// Set to 1 by either worker on an unexpected error.
static PID_FAIL: AtomicU32 = AtomicU32::new(0);
/// Driver sets this to release the owner worker after the handoff.
static PID_EXIT: AtomicU32 = AtomicU32::new(0);

/// Owner worker: acquires the PI mutex, then spins until released.  It does
/// NOT unlock — the driver simulates its death by transferring ownership
/// away with `exit_pi_owned_futexes`, so by exit time it no longer owns the
/// lock.
extern "C" fn pi_death_owner(_arg: u64) {
    let addr = (&raw const PID_PI) as u64;
    if futex_lock_pi(addr).is_err() {
        PID_FAIL.store(1, Ordering::SeqCst);
        PID_OWNED.store(2, Ordering::SeqCst);
        return;
    }
    PID_OWNED.store(1, Ordering::SeqCst);
    // Bounded spin so a missed signal can never hang the self-test.
    for _ in 0..100_000 {
        if PID_EXIT.load(Ordering::SeqCst) != 0 {
            break;
        }
        sched::yield_now();
    }
}

/// Waiter worker: blocks on the PI mutex the owner holds.  When the owner
/// "dies", the PI-state handoff must transfer ownership here so this returns
/// `Ok` instead of hanging forever.
#[allow(clippy::cast_possible_truncation)]
extern "C" fn pi_death_waiter(_arg: u64) {
    let addr = (&raw const PID_PI) as u64;
    PID_W_PARKED.fetch_add(1, Ordering::SeqCst);
    match futex_lock_pi(addr) {
        Ok(()) => {
            PID_RECOVERED.store(1, Ordering::SeqCst);
            // The kernel-memory test word was not rewritten by the handoff
            // (validate_user_write rejects kernel addresses), so stamp our
            // ownership before releasing to keep the table tidy.
            let me = (sched::current_task_id() as u32) & FUTEX_TID_MASK;
            PID_PI.store(me, Ordering::SeqCst);
            let _ = futex_unlock_pi(addr);
        }
        Err(_) => {
            PID_FAIL.store(1, Ordering::SeqCst);
        }
    }
}

/// Test: a `FUTEX_LOCK_PI` waiter inherits ownership when the owner dies.
///
/// Mirrors Linux's `exit_pi_state_list`: without the kernel-side handoff a
/// task blocked on a PI mutex whose owner exited would hang forever.  The
/// driver calls [`exit_pi_owned_futexes`] directly (the spawn-based kernel
/// workers do not run the userspace thread-exit hook), then verifies the
/// waiter recovered.
fn test_pi_owner_death_handoff() -> KernelResult<()> {
    PID_PI.store(0, Ordering::SeqCst);
    PID_OWNED.store(0, Ordering::SeqCst);
    PID_W_PARKED.store(0, Ordering::SeqCst);
    PID_RECOVERED.store(0, Ordering::SeqCst);
    PID_FAIL.store(0, Ordering::SeqCst);
    PID_EXIT.store(0, Ordering::SeqCst);

    let owner_id = sched::spawn(b"pi-death-own", 16, pi_death_owner, 0, 0)?;

    // Let the owner acquire the PI mutex.
    for _ in 0..50 {
        if PID_OWNED.load(Ordering::SeqCst) != 0 {
            break;
        }
        sched::yield_now();
    }
    if PID_OWNED.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: PI-death owner never acquired");
        return Err(KernelError::InternalError);
    }

    // Spawn the waiter; let it register as a PI waiter and block.
    sched::spawn(b"pi-death-wait", 16, pi_death_waiter, 0, 0)?;
    for _ in 0..50 {
        if PID_W_PARKED.load(Ordering::SeqCst) >= 1 {
            break;
        }
        sched::yield_now();
    }
    // Extra yields so it is fully blocked inside futex_lock_pi (the WAITERS
    // bit is set and the waiter record is registered) before we hand off.
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();

    // Simulate the owner's death: transfer its held PI mutexes to the
    // highest-priority blocked waiter.
    exit_pi_owned_futexes(owner_id);

    // Let the waiter wake and inherit ownership.
    for _ in 0..200 {
        if PID_RECOVERED.load(Ordering::SeqCst) != 0 {
            break;
        }
        sched::yield_now();
    }

    // Release the owner worker and let it (and the waiter) finish.
    PID_EXIT.store(1, Ordering::SeqCst);
    for _ in 0..100 {
        if PID_OWNED.load(Ordering::SeqCst) == 1
            && PID_RECOVERED.load(Ordering::SeqCst) != 0
        {
            // Both progressed; a few more yields lets them return.
        }
        sched::yield_now();
    }

    let recovered = PID_RECOVERED.load(Ordering::SeqCst);
    let fail = PID_FAIL.load(Ordering::SeqCst);
    sched::reap_dead_tasks();

    if recovered != 1 || fail != 0 {
        serial_println!(
            "[futex]   FAIL: PI owner-death handoff recovered={} fail={}",
            recovered,
            fail
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   PI owner-death handoff (waiter inherits): OK");
    Ok(())
}

/// Counter for self-test verification.
static FUTEX_TEST_RESULT: AtomicU32 = AtomicU32::new(0);

/// Test 1: `futex_wait` with value mismatch — should return immediately.
fn test_wait_value_mismatch() -> KernelResult<()> {
    let futex_word = AtomicU32::new(42);
    let addr = (&raw const futex_word) as u64;

    // Wait expecting 99, but actual value is 42 — should not block.
    let result = futex_wait(addr, 99)?;
    if result {
        serial_println!("[futex]   FAIL: wait blocked despite value mismatch");
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Value mismatch (no block): OK");
    Ok(())
}

/// Test 2: Wake with no waiters — should return 0.
fn test_wake_no_waiters() -> KernelResult<()> {
    let futex_word = AtomicU32::new(0);
    let addr = (&raw const futex_word) as u64;

    let woken = futex_wake(addr, 1);
    if woken != 0 {
        serial_println!("[futex]   FAIL: wake returned {} with no waiters", woken);
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Wake no waiters: OK");
    Ok(())
}

/// Task for the blocking wait/wake test.
///
/// Waits on the given futex address (passed as u64 arg).  When woken,
/// stores 42 to `FUTEX_TEST_RESULT`.
extern "C" fn futex_waiter_task(addr: u64) {
    // Wait for the value to be 1 (the initial value set by the test).
    let _ = futex_wait(addr, 1);
    // We've been woken — signal success.
    FUTEX_TEST_RESULT.store(42, Ordering::SeqCst);
}

/// Test 3: Blocking wait + wake via spawned task.
fn test_blocking_wait_wake() -> KernelResult<()> {
    FUTEX_TEST_RESULT.store(0, Ordering::SeqCst);

    // The futex word starts at 1 (locked).
    let futex_word = AtomicU32::new(1);
    let addr = (&raw const futex_word) as u64;

    // Spawn a task that will block on futex_wait(addr, 1).
    sched::spawn(b"futex-test", 16, futex_waiter_task, addr, 0)?;

    // Yield to let the waiter run and block.
    sched::yield_now();

    // "Unlock" the futex and wake the waiter.
    futex_word.store(0, Ordering::Release);
    let woken = futex_wake(addr, 1);

    if woken != 1 {
        serial_println!("[futex]   FAIL: wake returned {} (expected 1)", woken);
        return Err(KernelError::InternalError);
    }

    // Yield to let the waiter resume and store its result.
    sched::yield_now();
    sched::yield_now();

    let result = FUTEX_TEST_RESULT.load(Ordering::SeqCst);
    if result != 42 {
        serial_println!("[futex]   FAIL: waiter result={}, expected 42", result);
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Blocking wait + wake: OK");
    Ok(())
}

/// Test: `futex_waitv` multi-key value-mismatch fast path.
///
/// When any key's word does not hold its expected value at setup, the call
/// must return [`WaitvOutcome::Mismatch`] (Linux `EAGAIN`) immediately
/// without parking — safe to exercise inline on the boot thread.  The
/// blocking outcomes (TimedOut / Woken-by-index) require the hrtimer and a
/// real scheduler context, so they live in [`self_test_timeout`].
fn test_wait_multiple() -> KernelResult<()> {
    let wa = AtomicU32::new(7); // != expected 1
    let wb = AtomicU32::new(1);
    let mkeys = [
        WaitvKey { uaddr: (&raw const wa) as u64, expected: 1 },
        WaitvKey { uaddr: (&raw const wb) as u64, expected: 1 },
    ];
    match futex_wait_multiple(&mkeys, None) {
        WaitvOutcome::Mismatch => {}
        other => {
            serial_println!("[futex]   FAIL: waitv mismatch = {:?} (expected Mismatch)", other);
            return Err(KernelError::InternalError);
        }
    }

    // Also confirm the *second* key being the mismatching one is detected
    // (the setup loop checks every key, not just the first).
    let xa = AtomicU32::new(1);
    let xb = AtomicU32::new(9); // != expected 1
    let xkeys = [
        WaitvKey { uaddr: (&raw const xa) as u64, expected: 1 },
        WaitvKey { uaddr: (&raw const xb) as u64, expected: 1 },
    ];
    match futex_wait_multiple(&xkeys, None) {
        WaitvOutcome::Mismatch => {}
        other => {
            serial_println!("[futex]   FAIL: waitv 2nd-key mismatch = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[futex]   Multi-key waitv mismatch (EAGAIN, no block): OK");
    Ok(())
}

/// Bitset bits used by the `FUTEX_WAKE_BITSET` self-test.  Disjoint so a
/// wake targeting one cannot accidentally match the other.
const BITSET_TEST_A: u32 = 0x0000_0001;
const BITSET_TEST_B: u32 = 0x0000_0002;

/// Set to 1 by `bitset_waiter_task` when it is woken.
static BITSET_WOKEN: AtomicU32 = AtomicU32::new(0);

/// Waiter for the bitset test: parks on `addr` (value 1 at spawn) with
/// wakeup bitset `BITSET_TEST_A`.  On wake, sets `BITSET_WOKEN`.
extern "C" fn bitset_waiter_task(addr: u64) {
    let _ = futex_wait_bitset(addr, 1, BITSET_TEST_A);
    BITSET_WOKEN.store(1, Ordering::SeqCst);
}

/// Test: `FUTEX_WAKE_BITSET` selective matching.
///
/// Proves the per-waiter bitset gate in both directions: a waiter that
/// registered with bitset A is *not* woken by a wake whose mask is the
/// disjoint bitset B, but *is* woken by a wake whose mask overlaps A.
/// Also checks the zero-bitset API guards (wait rejects with
/// `InvalidArgument`, wake matches nothing).
fn test_wake_bitset() -> KernelResult<()> {
    // (a) API guards: a zero bitset is rejected on wait and wakes nobody.
    let probe = AtomicU32::new(1);
    let probe_addr = (&raw const probe) as u64;
    match futex_wait_bitset(probe_addr, 1, 0) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            serial_println!("[futex]   FAIL: wait_bitset(mask=0) = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }
    if futex_wake_bitset(probe_addr, u32::MAX, 0) != 0 {
        serial_println!("[futex]   FAIL: wake_bitset(mask=0) woke a waiter");
        return Err(KernelError::InternalError);
    }

    // (b) Selective wake.
    BITSET_WOKEN.store(0, Ordering::SeqCst);
    let word = AtomicU32::new(1);
    let addr = (&raw const word) as u64;
    sched::spawn(b"futex-bs", 16, bitset_waiter_task, addr, 0)?;
    // Let the waiter block.
    sched::yield_now();
    sched::yield_now();

    // Non-overlapping mask (B) must wake nothing and leave the waiter parked.
    let woken_b = futex_wake_bitset(addr, u32::MAX, BITSET_TEST_B);
    if woken_b != 0 {
        serial_println!(
            "[futex]   FAIL: non-overlap wake woke {} (expected 0)",
            woken_b
        );
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    if BITSET_WOKEN.load(Ordering::SeqCst) != 0 {
        serial_println!("[futex]   FAIL: waiter woke on a non-overlapping bitset");
        return Err(KernelError::InternalError);
    }

    // Overlapping mask (A) must wake the waiter.
    let woken_a = futex_wake_bitset(addr, u32::MAX, BITSET_TEST_A);
    if woken_a != 1 {
        serial_println!(
            "[futex]   FAIL: overlap wake woke {} (expected 1)",
            woken_a
        );
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    sched::yield_now();
    if BITSET_WOKEN.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: waiter did not wake on an overlapping bitset");
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Wake bitset selective match: OK");
    Ok(())
}

/// Counter for the requeue self-test: each woken waiter increments it.
static REQUEUE_WOKEN: AtomicU32 = AtomicU32::new(0);

/// Waiter task for the requeue test.  Blocks on `addr` (value 1 at spawn
/// time); on wake, bumps `REQUEUE_WOKEN`.
extern "C" fn requeue_waiter_task(addr: u64) {
    let _ = futex_wait(addr, 1);
    REQUEUE_WOKEN.fetch_add(1, Ordering::SeqCst);
}

/// Test 5: `futex_requeue` — wake one waiter on `addr1` and move the rest
/// onto `addr2`, then drain `addr2`.  Also checks that a `CMP_REQUEUE`
/// value mismatch reports `WouldBlock` (→ `EAGAIN`) without touching the
/// queues.
fn test_requeue() -> KernelResult<()> {
    // (a) CMP_REQUEUE with a stale compare value must do nothing.
    let guard = AtomicU32::new(5);
    let guard_addr = (&raw const guard) as u64;
    let sink = AtomicU32::new(0);
    let sink_addr = (&raw const sink) as u64;
    match futex_cmp_requeue(guard_addr, sink_addr, 1, u32::MAX, 6) {
        Err(KernelError::WouldBlock) => {}
        result => {
            serial_println!(
                "[futex]   FAIL: cmp_requeue mismatch returned {:?}",
                result
            );
            return Err(KernelError::InternalError);
        }
    }

    // (b) Functional requeue: three waiters on addr1, wake 1, requeue 2.
    REQUEUE_WOKEN.store(0, Ordering::SeqCst);
    let word1 = AtomicU32::new(1);
    let addr1 = (&raw const word1) as u64;
    let word2 = AtomicU32::new(1);
    let addr2 = (&raw const word2) as u64;

    sched::spawn(b"futex-rq0", 16, requeue_waiter_task, addr1, 0)?;
    sched::spawn(b"futex-rq1", 16, requeue_waiter_task, addr1, 0)?;
    sched::spawn(b"futex-rq2", 16, requeue_waiter_task, addr1, 0)?;

    // Let all three block on addr1.
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();

    // Wake exactly one; requeue the remaining two onto addr2.  The table
    // lock is held for the whole operation, so no waiter can run (and
    // self-remove) mid-requeue: all three are accounted for here.
    let affected = futex_requeue(addr1, addr2, 1, u32::MAX);
    if affected != 3 {
        serial_println!("[futex]   FAIL: requeue affected={} (expected 3)", affected);
        return Err(KernelError::InternalError);
    }

    // Let the single woken waiter run and report.
    sched::yield_now();
    sched::yield_now();
    let after_wake = REQUEUE_WOKEN.load(Ordering::SeqCst);
    if after_wake != 1 {
        serial_println!(
            "[futex]   FAIL: after requeue woken={} (expected 1)",
            after_wake
        );
        return Err(KernelError::InternalError);
    }

    // Drain addr2: the two requeued waiters must be wakeable there.
    let woken2 = futex_wake(addr2, u32::MAX);
    if woken2 != 2 {
        serial_println!("[futex]   FAIL: addr2 wake={} (expected 2)", woken2);
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    sched::yield_now();
    let total = REQUEUE_WOKEN.load(Ordering::SeqCst);
    if total != 3 {
        serial_println!("[futex]   FAIL: total woken={} (expected 3)", total);
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Requeue (wake 1 + requeue 2): OK");
    Ok(())
}

/// Test 3b: `FUTEX_WAKE_OP` — atomic RMW on the second word plus a
/// conditional double wake.
///
/// Covers the error gates (null target, unknown selector, out-of-range
/// shift) and both functional paths: a comparison that matches (so both
/// queues are woken and the RMW result is observed) and one that does not
/// (only the first queue is woken, the second is left intact).
#[allow(clippy::cast_possible_truncation)]
fn test_wake_op() -> KernelResult<()> {
    let scratch = AtomicU32::new(42);
    let scratch_addr = (&raw const scratch) as u64;

    // (a) addr2 == 0 → EINVAL (the RMW target is always dereferenced).
    match futex_wake_op(0, 0, 1, 1, 0) {
        Err(KernelError::InvalidArgument) => {}
        r => {
            serial_println!("[futex]   FAIL: wake_op null-target returned {:?}", r);
            return Err(KernelError::InternalError);
        }
    }

    // (b) Unknown op selector (op = 7) → EINVAL, and the word is untouched
    //     because the selector is validated before the RMW.
    let bad_op = 7u32 << 28;
    match futex_wake_op(0, scratch_addr, 0, 0, bad_op) {
        Err(KernelError::InvalidArgument) => {}
        r => {
            serial_println!("[futex]   FAIL: wake_op bad-op returned {:?}", r);
            return Err(KernelError::InternalError);
        }
    }
    if scratch.load(Ordering::SeqCst) != 42 {
        serial_println!("[futex]   FAIL: wake_op bad-op mutated word");
        return Err(KernelError::InternalError);
    }

    // (c) FUTEX_OP_OPARG_SHIFT with a shift count > 31 → EINVAL.
    let bad_shift = 0x8000_0000u32 | (40u32 << 12);
    match futex_wake_op(0, scratch_addr, 0, 0, bad_shift) {
        Err(KernelError::InvalidArgument) => {}
        r => {
            serial_println!("[futex]   FAIL: wake_op bad-shift returned {:?}", r);
            return Err(KernelError::InternalError);
        }
    }

    // (d) Functional path, comparison matches: ADD 4 to a word holding 1,
    //     compare old value GT 0 (true), so both queues are woken.
    REQUEUE_WOKEN.store(0, Ordering::SeqCst);
    let a1 = AtomicU32::new(1);
    let a1_addr = (&raw const a1) as u64;
    let a2 = AtomicU32::new(1);
    let a2_addr = (&raw const a2) as u64;
    sched::spawn(b"futex-wop-a1", 16, requeue_waiter_task, a1_addr, 0)?;
    sched::spawn(b"futex-wop-a2a", 16, requeue_waiter_task, a2_addr, 0)?;
    sched::spawn(b"futex-wop-a2b", 16, requeue_waiter_task, a2_addr, 0)?;
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();

    // op=ADD(1) << 28, cmp=GT(4) << 24, oparg=4 << 12, cmparg=0.
    let enc_match = (1u32 << 28) | (4u32 << 24) | (4u32 << 12);
    let woken = futex_wake_op(a1_addr, a2_addr, u32::MAX, u32::MAX, enc_match)?;
    if woken != 3 {
        serial_println!("[futex]   FAIL: wake_op matched woke {} (expected 3)", woken);
        return Err(KernelError::InternalError);
    }
    if a2.load(Ordering::SeqCst) != 5 {
        serial_println!(
            "[futex]   FAIL: wake_op ADD left word={} (expected 5)",
            a2.load(Ordering::SeqCst)
        );
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    sched::yield_now();
    if REQUEUE_WOKEN.load(Ordering::SeqCst) != 3 {
        serial_println!("[futex]   FAIL: wake_op matched waiters did not all run");
        return Err(KernelError::InternalError);
    }

    // (e) Functional path, comparison does NOT match: OR 0 (word stays 1),
    //     compare old value LT 0 (false), so only the first queue is woken
    //     and the second waiter is left blocked until we drain it.
    REQUEUE_WOKEN.store(0, Ordering::SeqCst);
    let b1 = AtomicU32::new(1);
    let b1_addr = (&raw const b1) as u64;
    let b2 = AtomicU32::new(1);
    let b2_addr = (&raw const b2) as u64;
    sched::spawn(b"futex-wop-b1", 16, requeue_waiter_task, b1_addr, 0)?;
    sched::spawn(b"futex-wop-b2", 16, requeue_waiter_task, b2_addr, 0)?;
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();
    sched::yield_now();

    // op=OR(2) << 28, cmp=LT(2) << 24, oparg=0, cmparg=0 → old(1) < 0 is false.
    let enc_nomatch = (2u32 << 28) | (2u32 << 24);
    let woken_b = futex_wake_op(b1_addr, b2_addr, u32::MAX, u32::MAX, enc_nomatch)?;
    if woken_b != 1 {
        serial_println!(
            "[futex]   FAIL: wake_op unmatched woke {} (expected 1)",
            woken_b
        );
        return Err(KernelError::InternalError);
    }
    if b2.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: wake_op OR 0 mutated word");
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    sched::yield_now();
    if REQUEUE_WOKEN.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: wake_op unmatched woke wrong waiter count");
        return Err(KernelError::InternalError);
    }
    // Drain the still-blocked b2 waiter so it does not linger.
    let drained = futex_wake(b2_addr, u32::MAX);
    if drained != 1 {
        serial_println!("[futex]   FAIL: wake_op leftover drain woke {}", drained);
        return Err(KernelError::InternalError);
    }
    sched::yield_now();
    sched::yield_now();
    if REQUEUE_WOKEN.load(Ordering::SeqCst) != 2 {
        serial_println!("[futex]   FAIL: wake_op leftover waiter did not run");
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   Wake-op (RMW + conditional double wake): OK");
    Ok(())
}

// -- PI self-test helpers ---------------------------------------------------

/// Stage counter for the PI test.
static PI_TEST_STAGE: AtomicU32 = AtomicU32::new(0);
/// Control word: L waits on this until the test driver wakes it.
static PI_CONTROL: AtomicU32 = AtomicU32::new(1);

/// Low-priority task for the PI test.
///
/// Locks the PI futex, signals stage 1, then blocks on the control
/// word.  When woken by the test driver, unlocks the PI futex
/// (transferring ownership to the high-priority waiter).
extern "C" fn pi_low_task(addr: u64) {
    let _ = futex_lock_pi(addr);
    PI_TEST_STAGE.store(1, Ordering::SeqCst);

    // Block on the control word until the test driver wakes us.
    // This lets the test driver spawn H and verify the PI boost
    // before we proceed.
    let ctrl_addr = (&raw const PI_CONTROL) as u64;
    let _ = futex_wait(ctrl_addr, 1);

    // Unlock the PI futex — transfers ownership to H.
    let _ = futex_unlock_pi(addr);
}

/// High-priority task for the PI test.
///
/// Signals stage 2, then tries to lock the PI futex (blocks because
/// L holds it, triggering priority inheritance).  When woken via
/// ownership transfer, signals stage 4 and unlocks.
extern "C" fn pi_high_task(addr: u64) {
    PI_TEST_STAGE.store(2, Ordering::SeqCst);
    // This will block because L holds the lock — PI boost applied to L.
    let _ = futex_lock_pi(addr);

    // Lock acquired via ownership transfer from L.
    PI_TEST_STAGE.store(4, Ordering::SeqCst);
    let _ = futex_unlock_pi(addr);
}

/// Test 4: Priority inheritance via PI futex.
///
/// Verifies that when a high-priority task (H, prio 8) blocks on a
/// lock held by a low-priority task (L, prio 24), L's effective
/// priority is boosted to 8 for the duration of the lock hold.
///
/// Sequence:
/// 1. Spawn L (prio 24) → L locks the PI futex, blocks on control word.
/// 2. Spawn H (prio 8)  → H tries to lock, blocks, boosts L.
/// 3. Verify L's effective priority is 8.
/// 4. Wake L → L unlocks, transfers lock to H, L's priority restored.
/// 5. H acquires, signals, unlocks.
/// 6. Verify everything completed and L's priority is back to 24.
fn test_priority_inheritance() -> KernelResult<()> {
    PI_TEST_STAGE.store(0, Ordering::SeqCst);
    PI_CONTROL.store(1, Ordering::SeqCst);

    // Create the PI futex word (initially unlocked).
    let futex_word = AtomicU32::new(0);
    let addr = (&raw const futex_word) as u64;

    // Spawn L at low priority (24).
    let l_id = sched::spawn(b"pi-low", 24, pi_low_task, addr, 0)?;

    // Yield to let L run: L locks the futex and blocks on PI_CONTROL.
    sched::yield_now();

    let stage = PI_TEST_STAGE.load(Ordering::SeqCst);
    if stage != 1 {
        serial_println!(
            "[futex]   PI FAIL: after L runs, expected stage 1, got {}",
            stage
        );
        return Err(KernelError::InternalError);
    }

    // Spawn H at high priority (8).
    let h_id = sched::spawn(b"pi-high", 8, pi_high_task, addr, 0)?;

    // Yield → H runs, sets stage 2, tries to lock → blocks with PI
    // boost on L.  Then idle/main resumes.
    sched::yield_now();

    let stage = PI_TEST_STAGE.load(Ordering::SeqCst);
    if stage != 2 {
        serial_println!(
            "[futex]   PI FAIL: after H blocks, expected stage 2, got {}",
            stage
        );
        return Err(KernelError::InternalError);
    }

    // Verify L's effective priority was boosted to 8.
    let l_eff = sched::get_effective_priority(l_id);
    if l_eff != Some(8) {
        serial_println!(
            "[futex]   PI FAIL: L effective priority should be 8, got {:?}",
            l_eff
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[futex]   PI boost verified: L prio 24 → effective 8");

    // Wake L from its control-word wait so it can unlock the PI futex.
    PI_CONTROL.store(0, Ordering::SeqCst);
    let ctrl_addr = (&raw const PI_CONTROL) as u64;
    futex_wake(ctrl_addr, 1);

    // Yield to let L run (boosted to 8), unlock, transfer to H.
    // Then H runs (prio 8), acquires, signals stage 4, unlocks, exits.
    // Then L's function returns and it exits too.
    for _ in 0..6 {
        sched::yield_now();
    }

    // Verify H completed.
    let stage = PI_TEST_STAGE.load(Ordering::SeqCst);
    if stage != 4 {
        serial_println!(
            "[futex]   PI FAIL: expected stage 4 (H done), got {}",
            stage
        );
        return Err(KernelError::InternalError);
    }

    // Verify L's priority was restored.  L may be Dead (returns None,
    // which is fine).  L's base priority is 24, but it may have been
    // detected as interactive (short burst before blocking → effective
    // priority = 24 - INTERACTIVE_BOOST = 22).  Either way, it must
    // NOT be the PI-boosted 8.
    if let Some(l_eff) = sched::get_effective_priority(l_id) {
        let base_min = 24u8.saturating_sub(sched::task::INTERACTIVE_BOOST);
        if l_eff < base_min || l_eff > 24 {
            serial_println!(
                "[futex]   PI FAIL: L priority not restored, got {} (expected {}-{})",
                l_eff, base_min, 24
            );
            return Err(KernelError::InternalError);
        }
    }

    // Clean up dead tasks.
    sched::reap_dead_tasks();

    serial_println!("[futex]   Priority inheritance: OK");

    // Suppress "unused variable" since we use h_id for spawning only.
    let _ = h_id;

    Ok(())
}

// ---------------------------------------------------------------------------
// Timeout self-test (requires hrtimer — runs late in boot)
// ---------------------------------------------------------------------------

/// Result storage for timeout test.
static TIMEOUT_TEST_RESULT: AtomicU32 = AtomicU32::new(0);

/// Late-boot self-test for `futex_wait_timeout`.
///
/// Requires hrtimer to be initialized.
pub fn self_test_timeout() -> KernelResult<()> {
    test_timeout_expires()?;
    test_timeout_woken_before_deadline()?;
    test_timeout_zero_nonblocking()?;
    test_lock_pi_timeout()?;
    test_wait_multiple_timeout()?;
    test_wait_multiple_woken()?;

    serial_println!("[futex]   Wait timeout: OK");
    Ok(())
}

// --- futex_waitv (multi-key) blocking self-tests (late boot) -------------

/// Two static words used by the multi-key woken-by-index test.  They must
/// be static (not stack locals) because the waker task wakes them by
/// address after this driver frame may have moved on.
static MWAITV_WORD_A: AtomicU32 = AtomicU32::new(1);
static MWAITV_WORD_B: AtomicU32 = AtomicU32::new(1);

/// Waker for [`test_wait_multiple_woken`]: after a brief delay (to let the
/// driver park on both keys), wakes the *second* key (`MWAITV_WORD_B`,
/// index 1) so the driver's `futex_wait_multiple` returns `Woken(1)`.
extern "C" fn waitv_multi_waker(_arg: u64) {
    sched::yield_now();
    sched::yield_now();
    MWAITV_WORD_B.store(0, Ordering::Release);
    futex_wake((&raw const MWAITV_WORD_B) as u64, 1);
}

/// Multi-key timeout: park on two matching keys with a 10ms timeout and no
/// waker — `futex_wait_multiple` must return [`WaitvOutcome::TimedOut`].
/// Runs directly on the driver thread (hrtimer is up by now), mirroring
/// [`test_timeout_expires`].
fn test_wait_multiple_timeout() -> KernelResult<()> {
    let wa = AtomicU32::new(1);
    let wb = AtomicU32::new(1);
    let keys = [
        WaitvKey { uaddr: (&raw const wa) as u64, expected: 1 },
        WaitvKey { uaddr: (&raw const wb) as u64, expected: 1 },
    ];
    match futex_wait_multiple(&keys, Some(10_000_000)) {
        WaitvOutcome::TimedOut => {}
        other => {
            serial_println!("[futex]   FAIL: waitv timeout = {:?} (expected TimedOut)", other);
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[futex]   Multi-key waitv timeout (ETIMEDOUT): OK");
    Ok(())
}

/// Multi-key woken-by-index: park indefinitely on two keys; a spawned
/// waker wakes the second key, so `futex_wait_multiple` must return
/// [`WaitvOutcome::Woken`] with index `1`.
fn test_wait_multiple_woken() -> KernelResult<()> {
    MWAITV_WORD_A.store(1, Ordering::SeqCst);
    MWAITV_WORD_B.store(1, Ordering::SeqCst);

    let keys = [
        WaitvKey { uaddr: (&raw const MWAITV_WORD_A) as u64, expected: 1 },
        WaitvKey { uaddr: (&raw const MWAITV_WORD_B) as u64, expected: 1 },
    ];
    sched::spawn(b"waitv-wk", 16, waitv_multi_waker, 0, 0)?;
    match futex_wait_multiple(&keys, None) {
        WaitvOutcome::Woken(1) => {}
        WaitvOutcome::Woken(other) => {
            serial_println!("[futex]   FAIL: waitv woken index={} (expected 1)", other);
            return Err(KernelError::InternalError);
        }
        other => {
            serial_println!("[futex]   FAIL: waitv woken = {:?} (expected Woken(1))", other);
            return Err(KernelError::InternalError);
        }
    }
    // Let the waker task exit, then reap it.
    sched::yield_now();
    sched::reap_dead_tasks();
    serial_println!("[futex]   Multi-key waitv woken-by-index: OK");
    Ok(())
}

/// Stage counter for the PI lock-timeout test.
static PI_TO_STAGE: AtomicU32 = AtomicU32::new(0);
/// Control word the PI-timeout owner blocks on until the driver wakes it.
static PI_TO_CONTROL: AtomicU32 = AtomicU32::new(1);

/// Owner task for the PI lock-timeout test.
///
/// Locks the PI futex, signals stage 1, then parks on the control word.
/// When the driver wakes it, unlocks the PI futex and signals stage 2.
extern "C" fn pi_timeout_owner_task(addr: u64) {
    let _ = futex_lock_pi(addr);
    PI_TO_STAGE.store(1, Ordering::SeqCst);

    let ctrl = (&raw const PI_TO_CONTROL) as u64;
    let _ = futex_wait(ctrl, 1);

    let _ = futex_unlock_pi(addr);
    PI_TO_STAGE.store(2, Ordering::SeqCst);
}

/// Timeout test D: `futex_lock_pi_timeout` against a held lock must time
/// out (returning `TimedOut`), clean up the waiter (clearing the WAITERS
/// bit), and leave the original owner in possession.  After the owner
/// releases, the word returns to 0.
#[allow(clippy::cast_possible_truncation)]
fn test_lock_pi_timeout() -> KernelResult<()> {
    PI_TO_STAGE.store(0, Ordering::SeqCst);
    PI_TO_CONTROL.store(1, Ordering::SeqCst);

    let word = AtomicU32::new(0);
    let addr = (&raw const word) as u64;

    // Spawn the owner; let it acquire the PI lock and park.
    let owner_id = sched::spawn(b"pi-to-own", 20, pi_timeout_owner_task, addr, 0)?;
    sched::yield_now();
    if PI_TO_STAGE.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: pi-timeout owner did not acquire lock");
        return Err(KernelError::InternalError);
    }

    // Driver tries to lock with a 10ms timeout; owner holds it → time out.
    match futex_lock_pi_timeout(addr, 10_000_000) {
        Err(KernelError::TimedOut) => {}
        other => {
            serial_println!("[futex]   FAIL: lock_pi_timeout returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // We were the only waiter, so the WAITERS bit must be cleared and the
    // owner must still hold the lock.
    let owner_tid = (owner_id as u32) & FUTEX_TID_MASK;
    let w = word.load(Ordering::SeqCst);
    if w != owner_tid {
        serial_println!(
            "[futex]   FAIL: after PI timeout word={:#x} (expected owner tid {:#x})",
            w, owner_tid
        );
        return Err(KernelError::InternalError);
    }

    // Wake the owner so it unlocks and exits cleanly.
    PI_TO_CONTROL.store(0, Ordering::SeqCst);
    futex_wake((&raw const PI_TO_CONTROL) as u64, 1);
    for _ in 0..4 {
        sched::yield_now();
    }
    if PI_TO_STAGE.load(Ordering::SeqCst) != 2 {
        serial_println!("[futex]   FAIL: pi-timeout owner did not release");
        return Err(KernelError::InternalError);
    }
    if word.load(Ordering::SeqCst) != 0 {
        serial_println!("[futex]   FAIL: pi-timeout word not cleared after unlock");
        return Err(KernelError::InternalError);
    }

    sched::reap_dead_tasks();
    serial_println!("[futex]   Lock PI timeout (held lock → ETIMEDOUT): OK");
    Ok(())
}

/// Timeout test A: Nobody wakes us — should time out.
fn test_timeout_expires() -> KernelResult<()> {
    let futex_word = AtomicU32::new(1);
    let addr = (&raw const futex_word) as u64;

    // Wait expecting 1 (matches), with 5ms timeout.
    // Nobody will wake us, so it should time out.
    match futex_wait_timeout(addr, 1, 5_000_000) {
        Err(KernelError::TimedOut) => {} // Expected.
        Ok(blocked) => {
            serial_println!("[futex]   FAIL: timeout_expires returned Ok({})", blocked);
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!("[futex]   FAIL: timeout_expires returned Err({:?})", e);
            return Err(KernelError::InternalError);
        }
    }

    Ok(())
}

/// Task that wakes a futex once the waiter is actually parked.
///
/// The test's waker does NOT change the futex word (it only calls
/// `futex_wake`), so correctness depends on the waiter being parked *before*
/// the wake — otherwise the waiter re-checks its (unchanged) expected value,
/// parks anyway, and the earlier wake is lost, making it wait the full timeout
/// and spuriously report `TimedOut`. A fixed number of `yield_now()`s cannot
/// guarantee that ordering under all boot-time interleavings (the switch-on
/// cutover shifts task-id/scheduler timing enough to occasionally lose it),
/// so instead retry `futex_wake` until it reports it actually woke a waiter.
/// `futex_wake` returns the number of waiters woken; a `0` means the waiter
/// has not parked yet, so we yield and try again. Bounded so a genuinely
/// broken wake path can never wedge boot.
extern "C" fn timeout_waker_task(addr_raw: u64) {
    let mut spins: u32 = 0;
    loop {
        if futex_wake(addr_raw, 1) >= 1 {
            break;
        }
        spins = spins.saturating_add(1);
        if spins >= 1_000_000 {
            // Waiter never parked — leave RESULT at 0 so the test fails loudly
            // rather than hanging.
            return;
        }
        sched::yield_now();
    }
    TIMEOUT_TEST_RESULT.store(1, Ordering::SeqCst);
}

/// Timeout test B: Woken before deadline — should return Ok(true).
fn test_timeout_woken_before_deadline() -> KernelResult<()> {
    let futex_word = AtomicU32::new(7);
    let addr = (&raw const futex_word) as u64;

    TIMEOUT_TEST_RESULT.store(0, Ordering::SeqCst);

    // Spawn a waker that will wake us quickly.
    sched::spawn(b"futex-waker", 16, timeout_waker_task, addr, 0)?;

    // Wait expecting 7 (matches), with 500ms timeout.
    // The waker should wake us well before 500ms.
    match futex_wait_timeout(addr, 7, 500_000_000) {
        Ok(true) => {} // Expected: blocked and woken.
        Ok(false) => {
            serial_println!("[futex]   FAIL: timeout_woken returned Ok(false)");
            return Err(KernelError::InternalError);
        }
        Err(KernelError::TimedOut) => {
            serial_println!("[futex]   FAIL: timeout_woken timed out");
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!("[futex]   FAIL: timeout_woken returned Err({:?})", e);
            return Err(KernelError::InternalError);
        }
    }

    // Verify the waker actually ran.
    sched::yield_now();
    if TIMEOUT_TEST_RESULT.load(Ordering::SeqCst) != 1 {
        serial_println!("[futex]   FAIL: waker task didn't run");
        return Err(KernelError::InternalError);
    }

    Ok(())
}

/// Timeout test C: Zero timeout — non-blocking, matches but doesn't block.
fn test_timeout_zero_nonblocking() -> KernelResult<()> {
    let futex_word = AtomicU32::new(5);
    let addr = (&raw const futex_word) as u64;

    // Value matches (5 == 5) and timeout is 0 → should return TimedOut
    // (non-blocking mode: value matches but we don't block).
    match futex_wait_timeout(addr, 5, 0) {
        Err(KernelError::TimedOut) => {} // Expected.
        other => {
            serial_println!("[futex]   FAIL: zero_timeout returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Value doesn't match → Ok(false) regardless of timeout.
    match futex_wait_timeout(addr, 99, 0) {
        Ok(false) => {} // Expected: value mismatch.
        other => {
            serial_println!("[futex]   FAIL: zero_timeout mismatch returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    Ok(())
}
