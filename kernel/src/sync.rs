//! Kernel synchronization primitives with lockdep integration.
//!
//! This module provides [`Mutex<T>`] — a wrapper around [`spin::Mutex<T>`]
//! that automatically reports lock acquisitions and releases to the lockdep
//! subsystem for deadlock detection.
//!
//! ## Migration
//!
//! To migrate a file from raw `spin::Mutex` to tracked locks:
//! ```ignore
//! // Before:
//! use spin::Mutex;
//!
//! // After:
//! use crate::sync::Mutex;
//! ```
//!
//! The API is identical to `spin::Mutex` — `lock()` returns a guard that
//! auto-unlocks on drop.
//!
//! ## Lock naming
//!
//! Each `Mutex` carries a static `&[u8]` name used in lockdep diagnostics.
//! Use `Mutex::named(value, b"SCHED")` for important locks, or
//! `Mutex::new(value)` which defaults to `b"?"`.

use crate::lockdep;
use core::ops::{Deref, DerefMut};

/// A mutual-exclusion spinlock with lockdep tracking.
///
/// Wraps `spin::Mutex<T>` and notifies the lock order validator on
/// every acquisition and release.
pub struct Mutex<T> {
    inner: spin::Mutex<T>,
    /// Human-readable name for lockdep diagnostics.
    name: &'static [u8],
}

// SAFETY: Mutex<T> is Send+Sync whenever T is Send (same as spin::Mutex).
unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Create a new tracked mutex with a default name.
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name: b"?",
        }
    }

    /// Create a new tracked mutex with a diagnostic name.
    ///
    /// The name appears in lockdep violation reports.  Keep it short
    /// (≤16 bytes — excess is truncated by lockdep).
    pub const fn named(value: T, name: &'static [u8]) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name,
        }
    }

    /// Acquire the lock, returning a guard that releases on drop.
    ///
    /// Notifies lockdep before spinning so the dependency edge is
    /// recorded even if the lock is uncontended.
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let addr = self.addr();
        lockdep::lock_acquire(addr, self.name);
        let guard = self.inner.lock();
        MutexGuard {
            guard,
            addr,
        }
    }

    /// Try to acquire the lock without blocking.
    ///
    /// If successful, records the acquisition with lockdep.
    /// If the lock is already held, returns `None` without recording.
    #[inline]
    #[allow(dead_code)]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let addr = self.addr();
        let guard = self.inner.try_lock()?;
        // Only record if we actually got the lock — try_lock doesn't
        // block, so there's no ordering issue to detect on failure.
        lockdep::lock_acquire(addr, self.name);
        Some(MutexGuard { guard, addr })
    }

    /// Get the address used as the lockdep class identifier.
    #[inline]
    fn addr(&self) -> usize {
        // Use the address of the inner spin::Mutex as the class ID.
        // This ensures each Mutex instance is its own class.
        &self.inner as *const _ as usize
    }
}

/// RAII guard that releases the lock and notifies lockdep on drop.
pub struct MutexGuard<'a, T> {
    guard: spin::MutexGuard<'a, T>,
    addr: usize,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Release the lockdep tracking BEFORE dropping the inner guard.
        // This way, if another CPU is spinning on this lock and acquires
        // it immediately after us, the ordering edges are correct.
        lockdep::lock_release(self.addr);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify the tracked Mutex works correctly with lockdep.
#[allow(dead_code)]
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[sync] Running self-test...");

    // Test 1: Basic lock/unlock.
    let m = Mutex::named(42u64, b"test-sync");
    {
        let mut g = m.lock();
        assert_eq!(*g, 42);
        *g = 99;
    }
    {
        let g = m.lock();
        assert_eq!(*g, 99);
    }
    serial_println!("[sync]   Basic lock/unlock: OK");

    // Test 2: try_lock succeeds when unlocked.
    let m2 = Mutex::named(7u32, b"test-try");
    {
        let g = m2.try_lock();
        assert!(g.is_some());
        assert_eq!(*g.unwrap(), 7);
    }
    serial_println!("[sync]   try_lock: OK");

    serial_println!("[sync] Self-test PASSED");
}
