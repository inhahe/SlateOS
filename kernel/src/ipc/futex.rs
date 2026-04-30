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
//! The kernel maintains a hash table mapping virtual addresses to wait
//! queues.  When userspace processes are added, the key will become
//! (`address_space_id`, `virtual_address`) to distinguish the same virtual
//! address in different processes.
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
// Waiter and hash table
// ---------------------------------------------------------------------------

/// A task waiting on a futex address.
struct Waiter {
    /// The virtual address being waited on.
    addr: u64,
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

    /// Hash an address to a bucket index.
    #[allow(clippy::cast_possible_truncation)]
    fn bucket_index(addr: u64) -> usize {
        // Mix bits for better distribution.  Addresses are often
        // aligned, so we shift down and XOR.
        #[allow(clippy::arithmetic_side_effects)]
        let hash = addr ^ (addr >> 12) ^ (addr >> 24);
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
            return Ok(false);
        }

        // Value matches — add to wait queue and block.
        let idx = FutexTable::bucket_index(addr);

        // SAFETY: idx is masked to NUM_BUCKETS-1, which is < NUM_BUCKETS.
        #[allow(clippy::indexing_slicing)]
        table.buckets[idx].push_back(Waiter {
            addr,
            task_id: current_task,
        });

        // Drop the table lock before blocking.
    }

    // Block the current task.  The scheduler will switch to another
    // task.  When we're woken (by futex_wake), execution resumes here.
    sched::block_current();

    Ok(true)
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

    // Collect task IDs to wake while holding the table lock, then
    // wake them outside the lock to respect lock ordering.
    let mut to_wake: [TaskId; 32] = [0; 32];
    let mut wake_count: usize = 0;

    {
        let mut table = FUTEX_TABLE.lock();
        let idx = FutexTable::bucket_index(addr);

        // SAFETY: idx is masked to NUM_BUCKETS-1.
        #[allow(clippy::indexing_slicing)]
        let bucket = &mut table.buckets[idx];

        // Remove up to max_wake waiters with matching address.
        let mut i = 0;
        while i < bucket.len() && wake_count < max_wake as usize && wake_count < to_wake.len() {
            if let Some(waiter) = bucket.get(i)
                && waiter.addr == addr
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
    result
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
pub fn self_test() -> KernelResult<()> {
    serial_println!("[futex] Running futex self-test...");

    test_wait_value_mismatch()?;
    test_wake_no_waiters()?;
    test_blocking_wait_wake()?;

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
