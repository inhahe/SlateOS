//! signalfd — Linux-compatible synchronous signal-acceptance instance objects.
//!
//! A signalfd is a kernel object that lets a process **accept signals via a
//! file descriptor** instead of an asynchronous handler.  A Linux process
//! creates one with `signalfd(2)` / `signalfd4(2)`, passing a signal mask;
//! `read(2)` on the fd then blocks until one of the masked signals is
//! pending for the process, and returns one or more fixed-size
//! `struct signalfd_siginfo` records describing the consumed signals.  The
//! fd is also pollable: it becomes readable exactly when a masked signal is
//! pending, so it can sit in a `poll`/`select`/`epoll` set alongside other
//! descriptors in an event loop.
//!
//! This module owns only the **instance object and its acceptance mask** —
//! the refcounted lifetime and the mask get/set operations.  It deliberately
//! knows nothing about *which signals are pending* or *how a read consumes
//! them*: that lives in the syscall layer (`crate::syscall::linux`), which
//! intersects this mask with the owning process's pending set from
//! [`crate::proc::signal`] and formats the `signalfd_siginfo` records.
//! Keeping the per-process signal state out of this object means a signalfd
//! can be `dup`-ed / inherited across `fork` while always reflecting the
//! *current* holder's pending signals at read time, exactly like Linux.
//!
//! ## What the mask means (and what it excludes)
//!
//! The mask is a 64-bit set: bit `n-1` set means "this signalfd accepts
//! signal `n`" (matching the 1-based numbering and the pending/blocked
//! representation in [`crate::proc::signal`]).  As on Linux, **`SIGKILL`
//! (9) and `SIGSTOP` (19) cannot be accepted via a signalfd** — they are
//! silently cleared from any mask supplied to [`create`] / [`set_mask`], so
//! a program can never use a signalfd to intercept them.
//!
//! For a signalfd read to actually consume a signal (rather than the signal
//! being delivered to a handler or taking its default action), the process
//! must have those signals **blocked** with `sigprocmask`/`rt_sigprocmask` —
//! that is the caller's responsibility and is not enforced here; this object
//! only records *which* signals the fd is interested in.
//!
//! ## Refcounting and `fork`
//!
//! Like [`crate::ipc::epoll`] and [`crate::ipc::eventfd`], a signalfd is
//! reference counted: `create()` starts the count at 1, `dup()` bumps it
//! (used when `fork` duplicates the inheriting fd so a parent and child can
//! each hold the same signalfd object), and `close()` drops one reference —
//! only the final `close()` (count → 0) frees the object.  The acceptance
//! mask is **shared** between all holders of the same object, matching
//! Linux: a `signalfd4()` mask update through one fd is visible through any
//! `dup`-ed fd referring to the same object.
//!
//! ## Lock ordering
//!
//! `SIGNALFD_TABLE` is a leaf lock — none of the operations here call into
//! the scheduler or any other subsystem while holding it, so it never
//! participates in a lock-ordering cycle.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Mask sanitization
// ---------------------------------------------------------------------------

/// Bit for `SIGKILL` (signal 9 → bit 8).  Never acceptable via signalfd.
const SIGKILL_BIT: u64 = 1u64 << (crate::proc::signal::SIGKILL - 1);
/// Bit for `SIGSTOP` (signal 19 → bit 18).  Never acceptable via signalfd.
const SIGSTOP_BIT: u64 = 1u64 << (crate::proc::signal::SIGSTOP - 1);

/// Strip the un-catchable signals (`SIGKILL`, `SIGSTOP`) from a mask.
///
/// Linux's `signalfd` silently ignores these bits rather than erroring, so a
/// caller that passes `~0` gets "every signal except KILL/STOP".
#[must_use]
pub const fn sanitize_mask(mask: u64) -> u64 {
    mask & !(SIGKILL_BIT | SIGSTOP_BIT)
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a signalfd instance (the handle IS the ID).
type SignalFdId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_SIGNALFD_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_signalfd_id() -> SignalFdId {
    NEXT_SIGNALFD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a signalfd instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::SignalFd` variant added in the syscall-wiring layer); the
/// syscall layer reconstructs it with [`SignalFdHandle::from_raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignalFdHandle(u64);

impl SignalFdHandle {
    // `from_raw`/`raw` are the bridge to the Linux fd table: the syscall
    // layer (`HandleKind::SignalFd`) stores the handle as a raw `u64` in an
    // `FdEntry` and reconstructs it on each signalfd read / mask update.
    /// Reconstruct a handle from its raw `u64` representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw `u64` representation (what gets stored in an `FdEntry`).
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    fn id(self) -> SignalFdId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A kernel signalfd instance: an acceptance mask plus a reference count.
struct SignalFd {
    /// Acceptance mask: bit `n-1` set means signal `n` is accepted.  Already
    /// sanitized (no `SIGKILL`/`SIGSTOP` bits).
    mask: u64,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
}

impl SignalFd {
    const fn new(mask: u64) -> Self {
        Self {
            mask: sanitize_mask(mask),
            refcount: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live signalfd instances, keyed by ID.
///
/// Leaf lock — no nested locking.
static SIGNALFD_TABLE: Mutex<BTreeMap<SignalFdId, SignalFd>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new signalfd instance with the given acceptance mask.
///
/// The mask is sanitized (`SIGKILL`/`SIGSTOP` bits cleared) before storage.
/// The returned handle owns one reference; the caller must `close()` it
/// (directly or via process-exit cleanup) exactly once for that reference.
#[must_use]
pub fn create(mask: u64) -> SignalFdHandle {
    let id = alloc_signalfd_id();
    SIGNALFD_TABLE.lock().insert(id, SignalFd::new(mask));
    SignalFdHandle(id)
}

/// Add one reference to a signalfd instance, returning the same handle.
///
/// Used when `fork` duplicates the inheriting fd: parent and child then each
/// hold a reference to the *same* instance (shared mask), and neither one's
/// `close()` invalidates the other's.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: SignalFdHandle) -> KernelResult<SignalFdHandle> {
    let mut table = SIGNALFD_TABLE.lock();
    let sfd = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    sfd.refcount = sfd
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to a signalfd instance.
///
/// Only the final `close()` (refcount → 0) removes the instance.  A
/// double-close is harmless: the saturating decrement floors at 0 and an
/// unknown handle is simply ignored.
pub fn close(handle: SignalFdHandle) {
    let mut table = SIGNALFD_TABLE.lock();
    if let Some(sfd) = table.get_mut(&handle.id()) {
        sfd.refcount = sfd.refcount.saturating_sub(1);
        if sfd.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Does this handle refer to a live signalfd instance?
#[must_use]
pub fn exists(handle: SignalFdHandle) -> bool {
    SIGNALFD_TABLE.lock().contains_key(&handle.id())
}

// ---------------------------------------------------------------------------
// Mask API
// ---------------------------------------------------------------------------

/// Get the (sanitized) acceptance mask of a signalfd instance.
///
/// Returns `None` if `handle` is not a live instance.
#[must_use]
pub fn mask(handle: SignalFdHandle) -> Option<u64> {
    SIGNALFD_TABLE.lock().get(&handle.id()).map(|s| s.mask)
}

/// Replace the acceptance mask of an existing signalfd instance.
///
/// The new mask is sanitized (`SIGKILL`/`SIGSTOP` cleared) before storage.
/// This is the kernel half of `signalfd(fd, mask, ...)` / `signalfd4` when
/// called with an existing signalfd fd to update its mask.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if `handle` is not a live instance.
pub fn set_mask(handle: SignalFdHandle, mask: u64) -> KernelResult<()> {
    let mut table = SIGNALFD_TABLE.lock();
    let sfd = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    sfd.mask = sanitize_mask(mask);
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the signalfd instance object.
///
/// The kernel is `#![no_std]` / `#![no_main]`, so host `#[test]` functions
/// never run; verification happens here and returns `Err` (after a
/// `[signalfd] FAIL: …` line) instead of panicking.  Covers: create with
/// mask sanitization, mask get/set, dup/close refcount lifetime (shared
/// mask), and operations on a stale handle.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[signalfd] Running signalfd instance self-test...");

    // 1. Create sanitizes SIGKILL/SIGSTOP out of the mask.
    let all = u64::MAX;
    let sfd = create(all);
    if !exists(sfd) {
        serial_println!("[signalfd]   FAIL: fresh instance does not exist");
        return Err(KernelError::InternalError);
    }
    let expected = all & !(SIGKILL_BIT | SIGSTOP_BIT);
    match mask(sfd) {
        Some(m) if m == expected => {}
        other => {
            serial_println!("[signalfd]   FAIL: create did not sanitize mask: {:?}", other);
            close(sfd);
            return Err(KernelError::InternalError);
        }
    }

    // 2. set_mask replaces and re-sanitizes.
    // SIGINT (2) + SIGTERM (15) + an attempt to set SIGKILL (must be dropped).
    let want = (1u64 << 1) | (1u64 << 14) | SIGKILL_BIT;
    set_mask(sfd, want)?;
    let want_clean = (1u64 << 1) | (1u64 << 14);
    match mask(sfd) {
        Some(m) if m == want_clean => {}
        other => {
            serial_println!("[signalfd]   FAIL: set_mask did not update/sanitize: {:?}", other);
            close(sfd);
            return Err(KernelError::InternalError);
        }
    }

    // 3. Refcount lifetime: dup, then two closes are needed to free it; the
    //    mask is shared (a set_mask through one handle shows on the other).
    let sfd2 = dup(sfd)?;
    if sfd2 != sfd {
        serial_println!("[signalfd]   FAIL: dup returned a different handle");
        close(sfd);
        close(sfd);
        return Err(KernelError::InternalError);
    }
    set_mask(sfd2, 1u64 << 5)?; // SIGABRT-ish, just a distinct bit.
    if mask(sfd) != Some(1u64 << 5) {
        serial_println!("[signalfd]   FAIL: mask not shared across dup");
        close(sfd);
        close(sfd);
        return Err(KernelError::InternalError);
    }
    close(sfd); // refcount 2 -> 1; instance survives.
    if !exists(sfd) {
        serial_println!("[signalfd]   FAIL: instance freed after first of two closes");
        close(sfd);
        return Err(KernelError::InternalError);
    }
    close(sfd); // refcount 1 -> 0; freed.
    if exists(sfd) {
        serial_println!("[signalfd]   FAIL: instance still exists after final close");
        return Err(KernelError::InternalError);
    }

    // 4. Operations on a stale handle fail cleanly (no panic, right error).
    if mask(sfd).is_some() {
        serial_println!("[signalfd]   FAIL: mask on stale handle not None");
        return Err(KernelError::InternalError);
    }
    if set_mask(sfd, 0).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[signalfd]   FAIL: set_mask on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if dup(sfd).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[signalfd]   FAIL: dup on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    // close() on a stale handle must be a harmless no-op.
    close(sfd);

    serial_println!("[signalfd]   signalfd instance object (create/mask/dup/close): OK");
    Ok(())
}
