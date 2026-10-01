// Slot counts and indices here are bounded by a table's capacity, itself
// bounded by the caller's limit; every sum made of them is checked against
// its bound first.  Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! Two pieces of plumbing for a process's tables of numbered objects -- the
//! System V IPC objects ([`crate::sysv_msg`], [`crate::sysv_sem`]) and
//! kernel AIO's contexts ([`crate::linux_aio_abi`]):
//!
//! - [`Slots`], a table of slots grown as needed, whose slot numbers stay
//!   put while the array behind them moves;
//! - [`Waits`], the counter blocked calls sleep on, advanced by every change
//!   one could be waiting for.  Sleepers are counted, so a change nobody
//!   waits for costs no system call.
//!
//! They were `sysv_ipc.rs`'s until kernel AIO needed both.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::interrupt::{Mark, Restart};
use crate::lowlevellock::Waited;

// ---------------------------------------------------------------------------
// The table of slots
// ---------------------------------------------------------------------------

/// A kind of object's slots, grown as needed.  Slots are stable; the array
/// moves, so nothing may hold a reference into it across a claim.
pub(crate) struct Slots<T> {
    items: *mut T,
    cap: usize,
    /// The slots below `cap` holding no object, as a stack of `nfree`.
    free: *mut usize,
    nfree: usize,
    live: usize,
}

impl<T> Slots<T> {
    pub(crate) const EMPTY: Self = Self {
        items: core::ptr::null_mut(),
        cap: 0,
        free: core::ptr::null_mut(),
        nfree: 0,
        live: 0,
    };

    /// How many slots there are, in use or not.
    pub(crate) fn cap(&self) -> usize {
        self.cap
    }

    /// How many hold an object.
    pub(crate) fn live(&self) -> usize {
        self.live
    }

    pub(crate) fn get(&mut self, slot: usize) -> Option<&mut T> {
        // SAFETY: `items` holds `cap` initialised entries.
        (slot < self.cap).then(|| unsafe { &mut *self.items.add(slot) })
    }

    /// A slot for a new object, growing the table (up to `max` slots) when
    /// none is free; counted live.  `None` when the table is full or memory
    /// runs out.
    pub(crate) fn claim(&mut self, max: usize, empty: impl Fn() -> T) -> Option<usize> {
        if self.nfree == 0 {
            let old = self.cap;
            let cap = old.checked_mul(2)?.clamp(8, max);
            if cap <= old {
                return None;
            }
            // SAFETY: `grow` keeps the old entries, and the free stack gets
            // room for every slot.
            unsafe {
                self.items = grow(self.items, old, cap, &empty)?;
                self.free = grow(self.free, self.nfree, cap, &|| 0)?;
            }
            self.cap = cap;
            // The new slots, lowest on top.
            for slot in (old..cap).rev() {
                // SAFETY: `free` holds `cap` entries and `nfree < cap`.
                unsafe { self.free.add(self.nfree).write(slot) };
                self.nfree += 1;
            }
        }
        self.nfree -= 1;
        self.live += 1;
        // SAFETY: `nfree` indexes a pushed entry.
        Some(unsafe { self.free.add(self.nfree).read() })
    }

    /// Give `slot` back; the caller has emptied it.
    pub(crate) fn release(&mut self, slot: usize) {
        // SAFETY: the stack has room for every slot below `cap`, and this
        // one was not on it.
        unsafe { self.free.add(self.nfree).write(slot) };
        self.nfree += 1;
        self.live -= 1;
    }
}

/// `realloc` `old_len` entries at `p` to `new_len`, filling the new ones.
///
/// # Safety
///
/// `p` is null or a `malloc` block holding `old_len` valid `T`s.
pub(crate) unsafe fn grow<T>(
    p: *mut T,
    old_len: usize,
    new_len: usize,
    fill: &impl Fn() -> T,
) -> Option<*mut T> {
    let bytes = new_len.checked_mul(size_of::<T>())?;
    // SAFETY: the caller's contract; realloc(NULL, n) is malloc.
    let q = unsafe { crate::malloc::realloc(p.cast::<u8>(), bytes) }.cast::<T>();
    if q.is_null() {
        return None;
    }
    for i in old_len..new_len {
        // SAFETY: `q` holds `new_len` entries.
        unsafe { q.add(i).write(fill()) };
    }
    Some(q)
}

// ---------------------------------------------------------------------------
// Waiting
// ---------------------------------------------------------------------------

/// The counter one kind of object's blocked calls sleep on.
pub(crate) struct Waits {
    changes: AtomicI32,
    sleepers: AtomicI32,
}

impl Waits {
    /// No changes yet, nobody asleep.  A `const fn`, not a `const` item: a
    /// named constant with atomics in it would be a fresh copy at every use.
    pub(crate) const fn new() -> Self {
        Self {
            changes: AtomicI32::new(0),
            sleepers: AtomicI32::new(0),
        }
    }

    /// Tell the calls asleep on these objects that something changed.
    pub(crate) fn changed(&self) {
        self.changes.fetch_add(1, Ordering::SeqCst);
        if self.sleepers.load(Ordering::SeqCst) > 0 {
            crate::lowlevellock::futex_wake_all(&self.changes);
        }
    }

    /// The count a waiter compares against, read -- with the objects' lock
    /// held -- before it looks at one.
    pub(crate) fn seen(&self) -> i32 {
        self.changes.load(Ordering::SeqCst)
    }

    /// Sleep until the count moves from `seen`, or for at most `timeout_ns`.
    /// A signal does not end it: `io_destroy` waits so, for the requests
    /// still running.
    pub(crate) fn wait(&self, seen: i32, timeout_ns: Option<u64>) {
        self.sleepers.fetch_add(1, Ordering::SeqCst);
        match timeout_ns {
            None => crate::lowlevellock::futex_wait(&self.changes, seen),
            Some(ns) => crate::lowlevellock::futex_wait_timeout(&self.changes, seen, ns),
        }
        self.sleepers.fetch_sub(1, Ordering::SeqCst);
    }

    /// As [`Self::wait`], but any signal handler that runs on this thread
    /// since `mark` ends it, `SA_RESTART` or not: the calls that sleep here
    /// -- System V's and `io_getevents` -- are ones Linux never restarts
    /// ([`crate::interrupt`]).
    pub(crate) fn wait_interruptible(
        &self,
        seen: i32,
        timeout_ns: Option<u64>,
        mark: Mark,
    ) -> Waited {
        self.sleepers.fetch_add(1, Ordering::SeqCst);
        let waited = crate::lowlevellock::futex_wait_interruptible_since(
            &self.changes,
            seen,
            timeout_ns,
            Restart::Never,
            mark,
        );
        self.sleepers.fetch_sub(1, Ordering::SeqCst);
        waited
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn slots_claim_release_and_grow_to_the_limit() {
        let mut s: Slots<u32> = Slots::EMPTY;
        let a = s.claim(20, || 0).unwrap();
        let b = s.claim(20, || 0).unwrap();
        assert_ne!(a, b);
        assert_eq!(s.live(), 2);
        s.release(a);
        assert_eq!(
            s.claim(20, || 0),
            Some(a),
            "a released slot is reused first"
        );
        for _ in 2..20 {
            assert!(s.claim(20, || 0).is_some());
        }
        assert_eq!(s.claim(20, || 0), None, "full at the limit");
        assert_eq!(s.cap(), 20);
        *s.get(3).unwrap() = 7;
        assert_eq!(s.get(3).copied(), Some(7));
        assert!(s.get(20).is_none());
    }

    #[test]
    fn a_change_moves_the_count() {
        let w = Waits::new();
        let before = w.seen();
        w.changed();
        assert_ne!(w.seen(), before);
    }
}
