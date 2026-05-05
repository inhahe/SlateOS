//! Kernel sleeping reader-writer lock.
//!
//! A `KRwLock` allows multiple concurrent readers OR a single exclusive
//! writer, blocking on contention instead of spinning.  This is ideal
//! for data structures that are read frequently but written rarely (e.g.,
//! mount tables, path caches, configuration registries).
//!
//! ## When to Use
//!
//! - **`spin::RwLock`** (if available): Very short critical sections,
//!   ISR/softirq context.
//! - **`KMutex`**: Exclusive access needed, reads and writes equally
//!   frequent, or critical section is short.
//! - **`KRwLock`**: Read-heavy workloads where multiple readers can
//!   proceed in parallel.  Must NOT be held in ISR/softirq context.
//!
//! ## Design
//!
//! State is encoded in a single `AtomicI64`:
//! - `0`: Unlocked (no readers, no writer).
//! - `N > 0`: N active readers.
//! - `-1`: One active writer.
//!
//! A separate `AtomicU32` counts pending writers.  When pending writers
//! exist, new readers block even if the lock is in read mode — this
//! prevents writer starvation under sustained read load.
//!
//! ## Writer Preference
//!
//! This lock uses writer-preference to avoid starvation:
//! - If a writer is waiting, incoming readers queue behind it.
//! - When the last reader exits, waiting writers are woken first.
//! - After a writer releases, all queued readers are woken (they can
//!   run concurrently until the next writer arrives).
//!
//! ## References
//!
//! - Linux `kernel/locking/rwsem.c` — reader-writer semaphore
//! - FreeBSD `sys/kern/kern_rwlock.c`
//! - Rust std `RwLock` (parking_lot variant)
//! - Fuchsia `kernel/lib/fbl/include/fbl/rw_lock.h`

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicI64, AtomicU32, Ordering};

use super::waitqueue::WaitQueue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// State value indicating a writer holds the lock.
const WRITE_LOCKED: i64 = -1;

/// State value indicating the lock is free.
const UNLOCKED: i64 = 0;

/// Number of adaptive spin iterations before blocking.
const SPIN_COUNT: u32 = 40;

// ---------------------------------------------------------------------------
// KRwLock
// ---------------------------------------------------------------------------

/// A sleeping reader-writer lock for kernel process context.
///
/// Multiple readers can hold the lock concurrently, or one writer can
/// hold it exclusively.  Writer-preference prevents reader starvation
/// of writers.
///
/// # Safety
///
/// Must NOT be acquired in ISR or softirq context (those contexts
/// cannot sleep).  Only use in normal kernel task context.
pub struct KRwLock<T> {
    /// Lock state: 0 = free, N>0 = N readers, -1 = writer.
    state: AtomicI64,
    /// Count of writers waiting to acquire.  Used to block new readers
    /// (writer preference).
    pending_writers: AtomicU32,
    /// Wait queue for tasks wanting read access.
    reader_wq: WaitQueue,
    /// Wait queue for tasks wanting write access.
    writer_wq: WaitQueue,
    /// The protected data.
    data: UnsafeCell<T>,
}

// SAFETY: KRwLock provides mutual exclusion via atomic ops + blocking.
// The UnsafeCell is only accessed through read/write guards.
unsafe impl<T: Send> Send for KRwLock<T> {}
unsafe impl<T: Send + Sync> Sync for KRwLock<T> {}

impl<T> KRwLock<T> {
    /// Create a new unlocked reader-writer lock protecting `value`.
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicI64::new(UNLOCKED),
            pending_writers: AtomicU32::new(0),
            reader_wq: WaitQueue::new(),
            writer_wq: WaitQueue::new(),
            data: UnsafeCell::new(value),
        }
    }

    /// Acquire the lock for reading (shared access).
    ///
    /// Multiple readers can hold the lock simultaneously.  Blocks if a
    /// writer is active or waiting (writer preference).
    ///
    /// Returns a guard that releases the read lock on drop.
    pub fn read(&self) -> KRwLockReadGuard<'_, T> {
        // Fast path: no writer active and no writers waiting.
        if self.try_read_fast() {
            return KRwLockReadGuard { lock: self };
        }

        // Slow path: block until readable.
        self.read_slow();
        KRwLockReadGuard { lock: self }
    }

    /// Try to acquire the lock for reading without blocking.
    ///
    /// Returns `Some(guard)` on success, `None` if a writer is active
    /// or pending writers would be starved.
    pub fn try_read(&self) -> Option<KRwLockReadGuard<'_, T>> {
        if self.try_read_fast() {
            Some(KRwLockReadGuard { lock: self })
        } else {
            None
        }
    }

    /// Acquire the lock for writing (exclusive access).
    ///
    /// Blocks until all readers exit and no other writer is active.
    ///
    /// Returns a guard that releases the write lock on drop.
    pub fn write(&self) -> KRwLockWriteGuard<'_, T> {
        // Fast path: lock is free — CAS from 0 to -1.
        if self.try_write_fast() {
            return KRwLockWriteGuard { lock: self };
        }

        // Slow path: register as pending writer and block.
        self.write_slow();
        KRwLockWriteGuard { lock: self }
    }

    /// Try to acquire the lock for writing without blocking.
    ///
    /// Returns `Some(guard)` on success, `None` if the lock is held
    /// by any reader or writer.
    pub fn try_write(&self) -> Option<KRwLockWriteGuard<'_, T>> {
        if self.try_write_fast() {
            Some(KRwLockWriteGuard { lock: self })
        } else {
            None
        }
    }

    /// Whether the lock is currently held by a writer.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_write_locked(&self) -> bool {
        self.state.load(Ordering::Relaxed) == WRITE_LOCKED
    }

    /// Current number of active readers (0 if write-locked or free).
    #[must_use]
    #[allow(dead_code)]
    pub fn reader_count(&self) -> u64 {
        let s = self.state.load(Ordering::Relaxed);
        if s > 0 { s as u64 } else { 0 }
    }

    // -----------------------------------------------------------------------
    // Internal: fast paths
    // -----------------------------------------------------------------------

    /// Try to increment reader count atomically.
    /// Fails if writer is active or pending writers exist.
    fn try_read_fast(&self) -> bool {
        // Don't proceed if writers are waiting (writer preference).
        if self.pending_writers.load(Ordering::Acquire) > 0 {
            return false;
        }

        loop {
            let current = self.state.load(Ordering::Acquire);
            if current < 0 {
                // Writer active.
                return false;
            }

            // Re-check pending_writers inside the CAS loop — a writer
            // may have arrived between our initial check and now.
            if self.pending_writers.load(Ordering::Acquire) > 0 {
                return false;
            }

            let new = current.saturating_add(1);
            if self
                .state
                .compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            // CAS failed due to concurrent modification — retry.
        }
    }

    /// Try to CAS from UNLOCKED to WRITE_LOCKED.
    fn try_write_fast(&self) -> bool {
        self.state
            .compare_exchange(UNLOCKED, WRITE_LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    // -----------------------------------------------------------------------
    // Internal: slow paths
    // -----------------------------------------------------------------------

    /// Slow path for read acquisition: adaptive spin then block.
    fn read_slow(&self) {
        // Brief spin in case the writer releases quickly.
        for _ in 0..SPIN_COUNT {
            if self.try_read_fast() {
                return;
            }
            core::hint::spin_loop();
        }

        // Block on reader wait queue until we can acquire.
        self.reader_wq.wait_until(|| self.try_read_fast());
    }

    /// Slow path for write acquisition: register as pending, spin, block.
    fn write_slow(&self) {
        // Register as a pending writer so new readers block.
        self.pending_writers.fetch_add(1, Ordering::Release);

        // Brief spin in case readers/writer release quickly.
        for _ in 0..SPIN_COUNT {
            if self.try_write_fast() {
                self.pending_writers.fetch_sub(1, Ordering::Release);
                return;
            }
            core::hint::spin_loop();
        }

        // Block until we can acquire the write lock.
        self.writer_wq.wait_until(|| self.try_write_fast());
        self.pending_writers.fetch_sub(1, Ordering::Release);
    }

    // -----------------------------------------------------------------------
    // Internal: release
    // -----------------------------------------------------------------------

    /// Release a read lock (called by ReadGuard drop).
    fn read_unlock(&self) {
        let prev = self.state.fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0, "read_unlock called but state was {}", prev);

        // If we were the last reader and writers are waiting, wake one.
        if prev == 1 {
            self.writer_wq.wake_one();
        }
    }

    /// Release the write lock (called by WriteGuard drop).
    fn write_unlock(&self) {
        let prev = self.state.swap(UNLOCKED, Ordering::Release);
        debug_assert_eq!(prev, WRITE_LOCKED, "write_unlock called but state was {}", prev);

        // If writers are waiting, wake one writer (writer preference).
        // Otherwise wake all queued readers (they can run concurrently).
        if self.pending_writers.load(Ordering::Acquire) > 0 {
            self.writer_wq.wake_one();
        } else {
            self.reader_wq.wake_all();
        }
    }
}

// ---------------------------------------------------------------------------
// Read guard
// ---------------------------------------------------------------------------

/// RAII guard for shared (read) access to a `KRwLock`.
///
/// Provides immutable access to the protected data.  The read lock is
/// released when this guard is dropped.
pub struct KRwLockReadGuard<'a, T> {
    lock: &'a KRwLock<T>,
}

impl<T> Deref for KRwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: We hold a shared read lock — no writer can be active.
        // Multiple readers may coexist, but they only get & references.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> Drop for KRwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.read_unlock();
    }
}

// ---------------------------------------------------------------------------
// Write guard
// ---------------------------------------------------------------------------

/// RAII guard for exclusive (write) access to a `KRwLock`.
///
/// Provides mutable access to the protected data.  The write lock is
/// released when this guard is dropped.
pub struct KRwLockWriteGuard<'a, T> {
    lock: &'a KRwLock<T>,
}

impl<T> Deref for KRwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: We hold the exclusive write lock — no other accessor.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for KRwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: We hold the exclusive write lock — sole accessor.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for KRwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.write_unlock();
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the sleeping reader-writer lock.
///
/// Tests uncontended read/write, concurrent readers, try_* methods,
/// and writer preference semantics.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[krwlock] Running self-test...");

    // --- 1. Uncontended write ---
    {
        let lock = KRwLock::new(42u64);
        {
            let mut guard = lock.write();
            assert_eq!(*guard, 42);
            *guard = 100;
        }
        // Lock should be free now.
        assert!(!lock.is_write_locked());
        assert_eq!(lock.reader_count(), 0);

        // Verify the write persisted.
        let r = lock.read();
        assert_eq!(*r, 100);
    }
    serial_println!("[krwlock]   Uncontended write: OK");

    // --- 2. Uncontended read ---
    {
        let lock = KRwLock::new(7u64);
        let r = lock.read();
        assert_eq!(*r, 7);
        assert_eq!(lock.reader_count(), 1);
        drop(r);
        assert_eq!(lock.reader_count(), 0);
    }
    serial_println!("[krwlock]   Uncontended read: OK");

    // --- 3. Multiple concurrent readers ---
    {
        let lock = KRwLock::new(99u64);
        let r1 = lock.read();
        let r2 = lock.read();
        let r3 = lock.read();
        assert_eq!(*r1, 99);
        assert_eq!(*r2, 99);
        assert_eq!(*r3, 99);
        assert_eq!(lock.reader_count(), 3);
        drop(r1);
        assert_eq!(lock.reader_count(), 2);
        drop(r2);
        drop(r3);
        assert_eq!(lock.reader_count(), 0);
    }
    serial_println!("[krwlock]   Multiple readers: OK");

    // --- 4. try_write fails when readers hold ---
    {
        let lock = KRwLock::new(0u64);
        let _r = lock.read();
        assert!(lock.try_write().is_none());
        drop(_r);
        // Now it should succeed.
        let w = lock.try_write();
        assert!(w.is_some());
    }
    serial_println!("[krwlock]   try_write exclusion: OK");

    // --- 5. try_read fails when writer holds ---
    {
        let lock = KRwLock::new(0u64);
        let _w = lock.write();
        assert!(lock.try_read().is_none());
        drop(_w);
        // Now it should succeed.
        let r = lock.try_read();
        assert!(r.is_some());
    }
    serial_println!("[krwlock]   try_read exclusion: OK");

    // --- 6. Writer preference: try_read fails when writers pending ---
    {
        let lock = KRwLock::new(0u64);
        // Simulate a pending writer by incrementing the counter.
        lock.pending_writers.fetch_add(1, Ordering::Release);
        assert!(lock.try_read().is_none());
        lock.pending_writers.fetch_sub(1, Ordering::Release);
        // No pending writers — try_read should succeed.
        let r = lock.try_read();
        assert!(r.is_some());
    }
    serial_println!("[krwlock]   Writer preference: OK");

    // --- 7. State transitions ---
    {
        let lock = KRwLock::new(0u64);
        assert_eq!(lock.state.load(Ordering::Relaxed), UNLOCKED);

        let w = lock.write();
        assert_eq!(lock.state.load(Ordering::Relaxed), WRITE_LOCKED);
        drop(w);
        assert_eq!(lock.state.load(Ordering::Relaxed), UNLOCKED);

        let r1 = lock.read();
        assert_eq!(lock.state.load(Ordering::Relaxed), 1);
        let r2 = lock.read();
        assert_eq!(lock.state.load(Ordering::Relaxed), 2);
        drop(r1);
        assert_eq!(lock.state.load(Ordering::Relaxed), 1);
        drop(r2);
        assert_eq!(lock.state.load(Ordering::Relaxed), UNLOCKED);
    }
    serial_println!("[krwlock]   State transitions: OK");

    serial_println!("[krwlock] Self-test PASSED");
}
