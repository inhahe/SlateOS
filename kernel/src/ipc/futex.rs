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

        #[allow(clippy::indexing_slicing)]
        let bucket = &mut table.buckets[idx];

        // If we're still in the bucket, the timer woke us (not futex_wake).
        // Match on both addr and addr_space to avoid false positives from
        // other processes that collide into the same bucket.
        if let Some(pos) = bucket.iter().position(|w| {
            w.task_id == current_task && w.addr == addr && w.addr_space == addr_space
        }) {
            bucket.remove(pos);
            was_timed_out = true;
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
// ```text
// Bits 0-29:  Owner task ID (0 = unlocked)
// Bit 30:     FUTEX_WAITERS — set by kernel when PI waiters exist
// Bit 31:     Reserved (must be 0)
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
const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;

/// Bit flag indicating PI waiters exist (bit 30 of the futex word).
const FUTEX_WAITERS_BIT: u32 = 1 << 30;

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

/// Lock a PI futex.
///
/// Attempts to acquire the lock at `addr`.  If the lock is free (futex
/// word is 0), atomically sets the owner.  If contended, blocks the
/// caller and applies priority inheritance to the current lock holder.
///
/// The futex word uses bits 0–29 for the owner task ID and bit 30 as
/// a waiters flag.
///
/// # Returns
///
/// - `Ok(())` — lock acquired (either uncontended or after waiting).
/// - `Err(InvalidAddress)` — `addr` is null.
/// - `Err(BadAlignment)` — `addr` is not 4-byte aligned.
/// - `Err(WouldBlock)` — deadlock detected (caller already owns the lock).
///
/// # Safety contract
///
/// `addr` must point to a valid, aligned `AtomicU32`.
pub fn futex_lock_pi(addr: u64) -> KernelResult<()> {
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
            return Err(KernelError::WouldBlock); // Deadlock
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
                return Err(KernelError::WouldBlock);
            }
            o2
        } else {
            oid
        }
    };

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

    // Block until unlock_pi transfers the lock to us.
    sched::block_current();

    // We've been woken — we now own the lock.  Clear the blocked addr.
    sched::set_blocked_on_pi_addr(current_id, None);

    Ok(())
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
    test_priority_inheritance()?;

    serial_println!("[futex] Futex self-test PASSED");
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

    serial_println!("[futex]   Wait timeout: OK");
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
