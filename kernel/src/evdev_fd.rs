//! evdev file-descriptor instance objects — the per-open kernel state behind a
//! `/dev/input/eventN` file descriptor.
//!
//! When a Linux input client (a Wayland compositor, X.Org's `evdev`/`libinput`
//! driver, SDL, `evtest`) opens `/dev/input/event0` it gets a file descriptor
//! it drives with `read(2)` — a stream of whole 24-byte `struct input_event`
//! records — plus `poll(2)` for readiness and `ioctl(2)` for device
//! identification.  Every open is an independent *reader*: two processes that
//! both open the keyboard each see every keystroke, rather than one stealing
//! the other's.
//!
//! This module owns the **instance object** that a `HandleKind::Evdev`
//! [`crate::proc::linux_fd::FdEntry`] points at: which device the fd refers to
//! and where in that device's event stream this open has read up to.  It
//! mirrors the refcounted-instance pattern of [`crate::drm::card_fd`]:
//! [`create`] starts the count at 1, [`dup`] bumps it (so `fork`/`dup` share
//! one cursor between the two fds, matching Linux's shared `struct
//! evdev_client`), and only the final [`close`] (count → 0) removes it.
//!
//! ## Why the cursor lives here and not in the ring
//!
//! [`crate::evdev`] keeps exactly one *source ring* per physical device,
//! written by that device's ISR and never consumed — a reader holds a cursor
//! into it.  That inversion is what makes multiple independent readers work
//! and keeps the ISR side single-producer and lock-free.  The consequence is
//! that "how far has this fd read" is per-open state, which is precisely what
//! this table stores.
//!
//! A reader that falls more than `RING_CAP` events behind is *lapped*: its
//! cursor names events the producer has already overwritten.  [`crate::evdev`]
//! detects that and resynchronises to the oldest surviving event, delivering
//! one `EV_SYN`/`SYN_DROPPED` record first so the client knows its view of the
//! device state (which keys are down, where the pointer is) may be stale and
//! must be re-queried.  That is exactly Linux's contract, and it is why a slow
//! reader degrades into a gap rather than into unbounded kernel memory.

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::evdev::{EvdevClient, InputDevice};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for an evdev client instance (the handle IS the ID).
type EvdevId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_EVDEV_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_evdev_id() -> EvdevId {
    NEXT_EVDEV_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an open evdev client instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::Evdev` variant); the syscall layer reconstructs it with
/// [`EvdevHandle::from_raw`] on each read / poll / ioctl / close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvdevHandle(u64);

impl EvdevHandle {
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

    fn id(self) -> EvdevId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// The per-open state of one `/dev/input/eventN` fd.
struct Instance {
    /// The reader cursor into the device's source ring.
    client: EvdevClient,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
}

/// Global table of all live evdev client instances, keyed by ID.
///
/// Leaf lock — no other lock is taken while it is held.  In particular the
/// read path copies out of the ring while holding it, which is safe because
/// the ring's producer side is lock-free (an ISR never blocks on this lock).
static EVDEV_TABLE: Mutex<BTreeMap<EvdevId, Instance>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new evdev client instance reading `device`.
///
/// The new client starts at the *current* end of the ring: an open does not
/// deliver keystrokes that happened before it, which is what a client expects
/// (Linux's `evdev_open_device` likewise starts an empty client buffer).
///
/// The returned handle owns one reference; the caller must [`close`] it
/// (directly or via process-exit cleanup) exactly once for that reference.
#[must_use]
pub fn create(device: InputDevice) -> EvdevHandle {
    let id = alloc_evdev_id();
    EVDEV_TABLE.lock().insert(
        id,
        Instance {
            client: EvdevClient::new(device),
            refcount: 1,
        },
    );
    EvdevHandle(id)
}

/// Add one reference to an instance, returning the same handle.
///
/// Used when `fork`/`dup` duplicates the inheriting fd: both fds then share
/// the *same* cursor, so an event read through one is not re-delivered through
/// the other — matching Linux, where a dup'd evdev fd shares one
/// `struct evdev_client`.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: EvdevHandle) -> KernelResult<EvdevHandle> {
    let mut table = EVDEV_TABLE.lock();
    let inst = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    inst.refcount = inst
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to an instance.
///
/// Only the final [`close`] (refcount → 0) removes it.  A double-close is
/// harmless: the saturating decrement floors at 0 and an unknown handle is
/// simply ignored.
pub fn close(handle: EvdevHandle) {
    let mut table = EVDEV_TABLE.lock();
    if let Some(inst) = table.get_mut(&handle.id()) {
        inst.refcount = inst.refcount.saturating_sub(1);
        if inst.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Does this handle refer to a live instance?
#[must_use]
pub fn exists(handle: EvdevHandle) -> bool {
    EVDEV_TABLE.lock().contains_key(&handle.id())
}

/// Which input device this fd reads, or `None` if stale.
#[must_use]
pub fn device(handle: EvdevHandle) -> Option<InputDevice> {
    EVDEV_TABLE
        .lock()
        .get(&handle.id())
        .map(|i| i.client.device())
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

/// Is at least one whole event available to read?
///
/// Returns `false` for a stale handle (nothing to read).  Used by the `poll` /
/// `epoll` / `select` paths to set `POLLIN`, and by the blocking read loop as
/// its wake condition.
#[must_use]
pub fn readable(handle: EvdevHandle) -> bool {
    EVDEV_TABLE
        .lock()
        .get(&handle.id())
        .is_some_and(|i| i.client.readable())
}

/// Read whole `input_event` records into `buf`, returning the byte count.
///
/// Returns `Ok(0)` when nothing is pending — the caller decides whether that
/// means `EAGAIN` (`O_NONBLOCK`) or "block and retry".  Only complete 24-byte
/// records are delivered, so a partial event is never returned; a buffer
/// smaller than one record is [`KernelError::InvalidArgument`] (`EINVAL`),
/// matching Linux's `evdev_read`.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if `buf` cannot hold one whole event.
pub fn read(handle: EvdevHandle, buf: &mut [u8]) -> KernelResult<usize> {
    let mut table = EVDEV_TABLE.lock();
    let inst = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    inst.client.read_into(buf)
}

/// How many events this fd has lost to being lapped by the producer.
///
/// Diagnostic only: a non-zero count means the client is not keeping up and
/// has seen at least one `SYN_DROPPED`.
#[must_use]
pub fn drops(handle: EvdevHandle) -> Option<u64> {
    EVDEV_TABLE
        .lock()
        .get(&handle.id())
        .map(|i| i.client.drop_count())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the evdev fd instance lifecycle.
///
/// Exercises create → dup → close (twice) refcounting, the device accessor,
/// the shared-cursor semantics of `dup`, and the stale-handle paths, leaving
/// no instances behind.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on the first failed invariant.
pub fn self_test() -> KernelResult<()> {
    macro_rules! check {
        ($cond:expr, $msg:expr) => {
            if !($cond) {
                serial_println!("[evdev_fd] SELF-TEST FAILED: {}", $msg);
                return Err(KernelError::InternalError);
            }
        };
    }

    let h = create(InputDevice::Keyboard);
    check!(exists(h), "new instance must exist");
    check!(device(h) == Some(InputDevice::Keyboard), "device recorded");
    check!(drops(h) == Some(0), "fresh client has lost nothing");

    // A fresh open starts at the current end of the ring, so it is not
    // readable until something new arrives.
    check!(!readable(h), "fresh open sees no backlog");

    // One synthetic keystroke = MSC_SCAN + EV_KEY + SYN = three records.
    crate::evdev::push_key(InputDevice::Keyboard, 30, 0x1E, true);
    check!(readable(h), "keystroke makes the fd readable");

    // A buffer that cannot hold one whole record is EINVAL, not a short read.
    let mut tiny = [0u8; crate::evdev::INPUT_EVENT_SIZE - 1];
    check!(
        read(h, &mut tiny) == Err(KernelError::InvalidArgument),
        "sub-record buffer is EINVAL"
    );

    // dup shares the cursor: reading through one fd consumes for both.
    let h2 = dup(h)?;
    check!(h2 == h, "dup returns the same handle");
    let mut buf = [0u8; 4 * crate::evdev::INPUT_EVENT_SIZE];
    let n = read(h2, &mut buf)?;
    check!(
        n == 3 * crate::evdev::INPUT_EVENT_SIZE,
        "one keystroke reads back as three whole records"
    );
    check!(!readable(h), "shared cursor: drained for both fds");

    // Two closes to free, because dup took a reference.
    close(h);
    check!(exists(h), "still alive after one of two closes");
    close(h);
    check!(!exists(h), "freed after the second close");

    // A second, independent open sees its own copy of a later event: this is
    // the property that lets a compositor and `evtest` coexist.
    let a = create(InputDevice::Keyboard);
    let b = create(InputDevice::Keyboard);
    check!(a != b, "independent opens get distinct handles");
    crate::evdev::push_key(InputDevice::Keyboard, 30, 0x1E, false);
    check!(readable(a) && readable(b), "both opens see the event");
    let na = read(a, &mut buf)?;
    check!(
        na == 3 * crate::evdev::INPUT_EVENT_SIZE,
        "first reader gets the whole keystroke"
    );
    check!(
        readable(b),
        "one reader draining does not steal from the other"
    );
    let nb = read(b, &mut buf)?;
    check!(nb == na, "second reader gets the same bytes");
    close(a);
    close(b);
    check!(!exists(a) && !exists(b), "both freed");

    // Stale-handle operations are inert, never a panic.
    check!(device(a).is_none(), "stale device is None");
    check!(!readable(a), "stale fd is not readable");
    check!(drops(a).is_none(), "stale drops is None");
    check!(read(a, &mut buf).is_err(), "stale read errors");
    check!(dup(a).is_err(), "dup of a stale handle errors");
    close(a); // double close: no-op

    serial_println!("[evdev_fd] evdev fd lifecycle self-test PASSED");
    Ok(())
}
