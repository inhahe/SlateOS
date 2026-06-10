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

use alloc::collections::VecDeque;
use crate::error::{KernelError, KernelResult};
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

// ---------------------------------------------------------------------------
// Waiter and hash table
// ---------------------------------------------------------------------------

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
}

/// Global futex wait table.
///
/// Maps virtual addresses to lists of waiting tasks via a hash table
/// with separate chaining.
///
/// Lock ordering: `FUTEX_TABLE` → `SCHED`.
static FUTEX_TABLE: Mutex<FutexTable> = Mutex::new(FutexTable::new());

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
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }

    // Check alignment (AtomicU32 requires 4-byte alignment).
    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
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
        });

        // Drop the table lock before blocking.
    }

    // Block the current task.  The scheduler will switch to another
    // task.  When we're woken (by futex_wake), execution resumes here.
    sched::block_current();

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
    if addr == 0 {
        return Err(KernelError::InvalidAddress);
    }

    #[allow(clippy::arithmetic_side_effects)]
    if addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
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

    // Block until woken (by futex_wake or timer).
    sched::block_current();

    // We woke up — determine why.
    // If the timer expired, we're still in the bucket and need to
    // remove ourselves.  If futex_wake woke us, we were already
    // removed from the bucket by the waker.
    let mut was_timed_out = false;

    {
        let mut table = FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr, addr_space);

        // If we're still in the bucket, the timer woke us (not futex_wake).
        // Match on both addr and addr_space to avoid false positives from
        // other processes that collide into the same bucket.
        let found = {
            // SAFETY: idx is masked to NUM_BUCKETS-1 by bucket_index.
            #[allow(clippy::indexing_slicing)]
            let bucket = &mut table.buckets[idx];
            if let Some(pos) = bucket.iter().position(|w| {
                w.task_id == current_task && w.addr == addr && w.addr_space == addr_space
            }) {
                bucket.remove(pos);
                true
            } else {
                false
            }
        };

        if found {
            was_timed_out = true;
        } else {
            // Not in the original bucket.  Either futex_wake removed us, or
            // we were requeued to a *different* address (and thus a
            // different bucket) by futex_requeue before the timer fired.  A
            // requeued waiter carries the destination addr, so a search keyed
            // on the original `addr` would miss it and leave a stale entry.
            // Scan all buckets by task_id (scoped to our address space) and
            // evict any lingering entry for this task.
            for bucket in &mut table.buckets {
                if let Some(pos) = bucket
                    .iter()
                    .position(|w| w.task_id == current_task && w.addr_space == addr_space)
                {
                    bucket.remove(pos);
                    was_timed_out = true;
                    break;
                }
            }
        }
    }

    crate::hrtimer::cancel(timer_handle);

    if was_timed_out && crate::hrtimer::now_ns() >= deadline_ns {
        Err(KernelError::TimedOut)
    } else {
        Ok(true)
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
    if addr == 0 || max_wake == 0 {
        return 0;
    }

    let addr_space = current_addr_space();

    // Collect task IDs to wake while holding the table lock, then
    // wake them outside the lock to respect lock ordering.
    let mut to_wake: [TaskId; 32] = [0; 32];
    let mut wake_count: usize = 0;

    {
        let mut table = FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr, addr_space);

        // SAFETY: idx is masked to NUM_BUCKETS-1.
        #[allow(clippy::indexing_slicing)]
        let bucket = &mut table.buckets[idx];

        // Remove up to max_wake waiters with matching address AND
        // address space.  This prevents cross-process wake: a process
        // can only wake tasks that share the same address space mapping.
        let mut i = 0;
        while i < bucket.len() && wake_count < max_wake as usize && wake_count < to_wake.len() {
            if let Some(waiter) = bucket.get(i)
                && waiter.addr == addr
                && waiter.addr_space == addr_space
                && let Some(removed) = bucket.remove(i)
            {
                if let Some(slot) = to_wake.get_mut(wake_count) {
                    *slot = removed.task_id;
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

    // Wake the collected tasks outside the FUTEX_TABLE lock.
    for task_id in to_wake.get(..wake_count).unwrap_or(&[]) {
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
/// Matches Linux's `FUTEX_OWNER_DIED` (`0x4000_0000`).  We do not yet run
/// robust-list cleanup that would set this, but the constant reserves the
/// bit so it is never mistaken for part of the owner TID and so future
/// robust-futex support slots in without a layout change.
#[allow(dead_code)]
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
}

impl PiFutexTable {
    const fn new() -> Self {
        const EMPTY_W: VecDeque<PiWaiter> = VecDeque::new();
        const EMPTY_O: VecDeque<PiOwner> = VecDeque::new();
        Self {
            waiters: [EMPTY_W; NUM_BUCKETS],
            owners: [EMPTY_O; NUM_BUCKETS],
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

    // Fast path: CAS 0 → our tid (uncontended acquisition).
    if atomic
        .compare_exchange(0, current_tid, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        register_pi_owner(addr, addr_space, current_id);
        return Ok(());
    }

    // Slow (contended) path.
    //
    // Read the owner from the futex word.  Retry the CAS once if the
    // lock appears to have been released between our first CAS and this
    // read (race window on SMP; harmless retry on single-CPU).
    let owner_id = {
        let word = atomic.load(Ordering::Acquire);
        let oid = u64::from(word & FUTEX_TID_MASK);

        if oid == current_id {
            return Err(KernelError::Deadlock); // Caller already owns it.
        }
        if oid == 0 {
            // Lock was released between CAS and load — retry.
            if atomic
                .compare_exchange(0, current_tid, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
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
// Self-test
// ---------------------------------------------------------------------------

/// Run futex self-tests.
///
/// Tests:
/// 1. `futex_wait` with non-matching value (returns immediately).
/// 2. `futex_wake` with no waiters (returns 0).
/// 3. Blocking wait + wake via spawned task.
/// 4. Priority inheritance: high-prio task boosts low-prio lock holder.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[futex] Running futex self-test...");

    test_wait_value_mismatch()?;
    test_wake_no_waiters()?;
    test_blocking_wait_wake()?;
    test_requeue()?;
    test_wake_op()?;
    test_pi_trylock_deadlock()?;
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

    serial_println!("[futex]   Wait timeout: OK");
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

/// Task that wakes a futex after a short delay.
extern "C" fn timeout_waker_task(addr_raw: u64) {
    // Brief delay to let the main task block.
    sched::yield_now();
    sched::yield_now();

    // Wake the waiter.
    futex_wake(addr_raw, 1);
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
