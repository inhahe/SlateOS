//! epoll — Linux-compatible I/O readiness multiplexing instance objects.
//!
//! An epoll instance is a kernel object that holds an **interest set**: a
//! collection of (file descriptor → monitored events + opaque user data)
//! registrations.  A Linux process creates one with `epoll_create(2)` /
//! `epoll_create1(2)`, mutates the interest set with `epoll_ctl(2)`
//! (`EPOLL_CTL_ADD` / `MOD` / `DEL`), and harvests ready descriptors with
//! `epoll_wait(2)`.
//!
//! This module owns only the **instance object and its interest set** — the
//! refcounted lifetime, and the add/modify/delete/list operations on the
//! registration map.  It deliberately knows nothing about *how* readiness
//! is computed: the `epoll_wait` implementation in
//! [`crate::syscall::linux`] walks the interest list returned by
//! [`interest_list`], looks each fd up in the caller's Linux fd table, and
//! reuses the existing `poll_revents_from_entry` readiness engine (the same
//! one `poll`/`select` use).  Keeping the readiness computation out of this
//! module means there is exactly one definition of "is this fd ready",
//! shared by `poll`, `select`, and `epoll`.
//!
//! ## Interest-set keying
//!
//! Linux keys the interest set by the pair `(fd number, struct file *)` so
//! that two `dup`-ed fds pointing at the same description can be registered
//! independently.  Our Linux fd model has no separate open-file-description
//! layer exposed here, so we key purely by **fd number**, which is the
//! overwhelmingly common case (one registration per fd).  A program that
//! registers two distinct fd numbers backed by the same underlying object
//! still gets two independent entries, exactly as on Linux; only the exotic
//! "same fd number, two descriptions" case (which cannot arise through our
//! fd table) is unrepresentable, and that is a deliberate, documented
//! simplification rather than a silent divergence.
//!
//! ## Refcounting and `fork`
//!
//! Like [`crate::ipc::eventfd`], an epoll instance is reference counted:
//! `create()` starts the count at 1, `dup()` bumps it (used when `fork`
//! duplicates the inheriting fd so a parent and child can each hold the
//! same epoll instance), and `close()` drops one reference — only the final
//! `close()` (count → 0) frees the instance and its interest set.  The
//! interest set is **shared** between all holders of the same instance,
//! matching Linux: an `epoll_ctl` from the child is visible to the parent
//! because they refer to the same kernel object.
//!
//! ## Lock ordering
//!
//! `EPOLL_TABLE` is a leaf lock — none of the operations here call into the
//! scheduler or any other subsystem while holding it, so it never
//! participates in a lock-ordering cycle.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for an epoll instance (the handle IS the ID).
type EpollId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_EPOLL_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_epoll_id() -> EpollId {
    NEXT_EPOLL_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an epoll instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::Epoll` variant added in the syscall-wiring layer); the
/// syscall layer reconstructs it with [`EpollHandle::from_raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpollHandle(u64);

impl EpollHandle {
    // `from_raw`/`raw` are the bridge to the Linux fd table: the syscall
    // layer (`HandleKind::Epoll`) stores the handle as a raw `u64` in an
    // `FdEntry` and reconstructs it on each epoll_ctl / epoll_wait call.
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

    fn id(self) -> EpollId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Interest entry
// ---------------------------------------------------------------------------

/// One registration in an epoll instance's interest set.
///
/// Mirrors the user's `struct epoll_event`: `events` is the requested
/// event mask (`EPOLLIN`, `EPOLLOUT`, plus behaviour flags like `EPOLLET`),
/// `data` is the opaque 64-bit cookie the kernel hands back verbatim in the
/// `epoll_wait` result for every ready report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterestEntry {
    /// Requested event mask + behaviour flags, exactly as the user wrote it.
    pub events: u32,
    /// Opaque user cookie, echoed back on every ready report.
    pub data: u64,
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A kernel epoll instance: an interest set plus a reference count.
struct Epoll {
    /// fd number → registration.  Keyed by fd; see module docs.
    interest: BTreeMap<i32, InterestEntry>,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
}

impl Epoll {
    fn new() -> Self {
        Self {
            interest: BTreeMap::new(),
            refcount: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live epoll instances, keyed by ID.
///
/// Leaf lock — no nested locking.
static EPOLL_TABLE: Mutex<BTreeMap<EpollId, Epoll>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new, empty epoll instance.
///
/// The returned handle owns one reference; the caller must `close()` it
/// (directly or via process-exit cleanup) exactly once for that reference.
#[must_use]
pub fn create() -> EpollHandle {
    let id = alloc_epoll_id();
    EPOLL_TABLE.lock().insert(id, Epoll::new());
    EpollHandle(id)
}

/// Add one reference to an epoll instance, returning the same handle.
///
/// Used when `fork` duplicates the inheriting fd: parent and child then
/// each hold a reference to the *same* instance (shared interest set), and
/// neither one's `close()` invalidates the other's.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: EpollHandle) -> KernelResult<EpollHandle> {
    let mut table = EPOLL_TABLE.lock();
    let ep = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    ep.refcount = ep
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to an epoll instance.
///
/// Only the final `close()` (refcount → 0) removes the instance and frees
/// its interest set.  A double-close is harmless: the saturating decrement
/// floors at 0 and an unknown handle is simply ignored.
pub fn close(handle: EpollHandle) {
    let mut table = EPOLL_TABLE.lock();
    if let Some(ep) = table.get_mut(&handle.id()) {
        ep.refcount = ep.refcount.saturating_sub(1);
        if ep.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Does this handle refer to a live epoll instance?
#[must_use]
pub fn exists(handle: EpollHandle) -> bool {
    EPOLL_TABLE.lock().contains_key(&handle.id())
}

// ---------------------------------------------------------------------------
// Interest-set API (the kernel half of epoll_ctl)
// ---------------------------------------------------------------------------

/// `EPOLL_CTL_ADD`: register `fd` with the given event mask and cookie.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is not a live instance.
/// - [`KernelError::AlreadyExists`] if `fd` is already registered (Linux
///   returns `EEXIST`).
pub fn ctl_add(handle: EpollHandle, fd: i32, events: u32, data: u64) -> KernelResult<()> {
    let mut table = EPOLL_TABLE.lock();
    let ep = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if ep.interest.contains_key(&fd) {
        return Err(KernelError::AlreadyExists);
    }
    ep.interest.insert(fd, InterestEntry { events, data });
    Ok(())
}

/// `EPOLL_CTL_MOD`: replace the event mask and cookie of an already-
/// registered `fd`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is not a live instance.
/// - [`KernelError::NotFound`] if `fd` is not registered (Linux returns
///   `ENOENT`).
pub fn ctl_mod(handle: EpollHandle, fd: i32, events: u32, data: u64) -> KernelResult<()> {
    let mut table = EPOLL_TABLE.lock();
    let ep = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    match ep.interest.get_mut(&fd) {
        Some(entry) => {
            entry.events = events;
            entry.data = data;
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// `EPOLL_CTL_DEL`: remove `fd` from the interest set.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is not a live instance.
/// - [`KernelError::NotFound`] if `fd` is not registered (Linux returns
///   `ENOENT`).
pub fn ctl_del(handle: EpollHandle, fd: i32) -> KernelResult<()> {
    let mut table = EPOLL_TABLE.lock();
    let ep = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if ep.interest.remove(&fd).is_some() {
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Snapshot the interest set as an ascending `(fd, events, data)` list.
///
/// The `epoll_wait` implementation iterates this snapshot, computes each
/// fd's current readiness via the shared poll engine, and builds the
/// result array.  Taking a snapshot (rather than holding `EPOLL_TABLE`
/// across the readiness scan) keeps this a leaf lock and lets the scan
/// touch the per-process fd table without nested locking.
///
/// Returns `None` if `handle` is not a live instance.
#[must_use]
pub fn interest_list(handle: EpollHandle) -> Option<Vec<(i32, u32, u64)>> {
    let table = EPOLL_TABLE.lock();
    let ep = table.get(&handle.id())?;
    Some(
        ep.interest
            .iter()
            .map(|(&fd, e)| (fd, e.events, e.data))
            .collect(),
    )
}

/// Look up a single registration (used by `epoll_ctl` to report `EEXIST`
/// pre-checks and by tests).  Returns `None` if the instance is gone or the
/// fd is not registered.
#[must_use]
pub fn lookup(handle: EpollHandle, fd: i32) -> Option<InterestEntry> {
    let table = EPOLL_TABLE.lock();
    table.get(&handle.id())?.interest.get(&fd).copied()
}

/// Number of registrations currently in the interest set, or 0 if the
/// instance does not exist.
#[must_use]
pub fn interest_count(handle: EpollHandle) -> usize {
    EPOLL_TABLE
        .lock()
        .get(&handle.id())
        .map_or(0, |ep| ep.interest.len())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the epoll instance object.
///
/// The kernel is `#![no_std]` / `#![no_main]`, so host `#[test]` functions
/// never run; verification happens here and returns `Err` (after a
/// `[epoll] FAIL: …` line) instead of panicking.  Covers: create/exists,
/// add/mod/del with the correct `AlreadyExists`/`NotFound` errors, interest
/// snapshot ordering and contents, dup/close refcount lifetime, and
/// operations on a stale handle.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[epoll] Running epoll instance self-test...");

    // 1. Create — a fresh instance exists and is empty.
    let ep = create();
    if !exists(ep) {
        serial_println!("[epoll]   FAIL: fresh instance does not exist");
        return Err(KernelError::InternalError);
    }
    if interest_count(ep) != 0 {
        serial_println!("[epoll]   FAIL: fresh instance not empty");
        close(ep);
        return Err(KernelError::InternalError);
    }

    // 2. ADD two fds; a duplicate ADD must report AlreadyExists.
    ctl_add(ep, 5, 0x1, 0xDEAD_BEEF)?;
    ctl_add(ep, 3, 0x4, 0x1234)?;
    if ctl_add(ep, 5, 0x1, 0).err() != Some(KernelError::AlreadyExists) {
        serial_println!("[epoll]   FAIL: duplicate ADD not AlreadyExists");
        close(ep);
        return Err(KernelError::InternalError);
    }
    if interest_count(ep) != 2 {
        serial_println!("[epoll]   FAIL: count after 2 ADDs != 2");
        close(ep);
        return Err(KernelError::InternalError);
    }

    // 3. The snapshot is ascending by fd and carries the right payloads.
    match interest_list(ep) {
        Some(list) => {
            if list != alloc::vec![(3, 0x4, 0x1234), (5, 0x1, 0xDEAD_BEEF)] {
                serial_println!("[epoll]   FAIL: interest_list contents/order wrong: {:?}", list);
                close(ep);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            serial_println!("[epoll]   FAIL: interest_list returned None for live instance");
            close(ep);
            return Err(KernelError::InternalError);
        }
    }

    // 4. MOD an existing fd updates events+data; MOD a missing fd -> NotFound.
    ctl_mod(ep, 5, 0x5, 0xCAFE)?;
    match lookup(ep, 5) {
        Some(e) if e.events == 0x5 && e.data == 0xCAFE => {}
        other => {
            serial_println!("[epoll]   FAIL: MOD did not update entry: {:?}", other);
            close(ep);
            return Err(KernelError::InternalError);
        }
    }
    if ctl_mod(ep, 99, 0x1, 0).err() != Some(KernelError::NotFound) {
        serial_println!("[epoll]   FAIL: MOD of missing fd not NotFound");
        close(ep);
        return Err(KernelError::InternalError);
    }

    // 5. DEL removes; DEL again -> NotFound.
    ctl_del(ep, 3)?;
    if interest_count(ep) != 1 {
        serial_println!("[epoll]   FAIL: count after DEL != 1");
        close(ep);
        return Err(KernelError::InternalError);
    }
    if ctl_del(ep, 3).err() != Some(KernelError::NotFound) {
        serial_println!("[epoll]   FAIL: DEL of missing fd not NotFound");
        close(ep);
        return Err(KernelError::InternalError);
    }

    // 6. Refcount lifetime: dup, then two closes are needed to free it.
    let ep2 = dup(ep)?;
    if ep2 != ep {
        serial_println!("[epoll]   FAIL: dup returned a different handle");
        close(ep);
        close(ep);
        return Err(KernelError::InternalError);
    }
    close(ep); // refcount 2 -> 1; instance must survive, interest intact.
    if !exists(ep) || interest_count(ep) != 1 {
        serial_println!("[epoll]   FAIL: instance freed/cleared after first of two closes");
        close(ep);
        return Err(KernelError::InternalError);
    }
    close(ep); // refcount 1 -> 0; instance freed.
    if exists(ep) {
        serial_println!("[epoll]   FAIL: instance still exists after final close");
        return Err(KernelError::InternalError);
    }

    // 7. Operations on a stale handle fail cleanly (no panic, right error).
    if ctl_add(ep, 1, 0x1, 0).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[epoll]   FAIL: ADD on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if interest_list(ep).is_some() {
        serial_println!("[epoll]   FAIL: interest_list on stale handle not None");
        return Err(KernelError::InternalError);
    }
    if dup(ep).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[epoll]   FAIL: dup on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    // close() on a stale handle must be a harmless no-op.
    close(ep);

    serial_println!("[epoll]   epoll instance object (create/ctl/dup/close): OK");
    Ok(())
}
