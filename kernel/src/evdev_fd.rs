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
    /// This fd has been revoked with `EVIOCREVOKE`; every further operation on
    /// it fails.  The instance stays in the table so the fd number remains
    /// valid until the client closes it — revoking is not closing.
    revoked: bool,
}

/// Which instance, if any, holds an exclusive grab (`EVIOCGRAB`) on each
/// device.  Indexed by [`InputDevice::minor`]; 0 means ungrabbed.
///
/// A grab is a real security primitive, not a formality: a screen locker grabs
/// the keyboard precisely so that no other client can read the password being
/// typed into it.  Honouring the ioctl but still delivering events to everyone
/// would be worse than refusing it outright, because the locker would believe
/// it was protected.
static GRAB_OWNER: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// The grab slot for a device, or `None` if it has no slot (unreachable: every
/// [`InputDevice`] minor is in range).
fn grab_slot(device: InputDevice) -> Option<&'static AtomicU64> {
    GRAB_OWNER.get(device.minor() as usize)
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
            revoked: false,
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
            // Release any exclusive grab *before* the instance disappears.
            // A grab that outlived its owner would silence the device for
            // every other client with no way left to lift it.
            if let Some(slot) = grab_slot(inst.client.device()) {
                // The exchange fails exactly when this instance was not the
                // grab owner — the common case, and one needing no action.
                let _ = slot.compare_exchange(handle.id(), 0, Ordering::AcqRel, Ordering::Relaxed);
            }
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
    let mut table = EVDEV_TABLE.lock();
    let Some(inst) = table.get_mut(&handle.id()) else {
        return false;
    };
    if inst.revoked {
        return false;
    }
    if grabbed_out(inst, handle.id()) {
        return false;
    }
    inst.client.readable()
}

/// Is this instance locked out by *another* fd's exclusive grab?
///
/// Also does the locking-out: an instance that cannot receive the events now
/// in the ring must not receive them later either, so its cursor is skipped
/// forward here.  Otherwise a screen locker's grab would merely *delay* the
/// password reaching every other client rather than withhold it.
fn grabbed_out(inst: &mut Instance, id: EvdevId) -> bool {
    let Some(slot) = grab_slot(inst.client.device()) else {
        return false;
    };
    let owner = slot.load(Ordering::Acquire);
    if owner == 0 || owner == id {
        return false;
    }
    inst.client.discard_pending();
    true
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
/// - [`KernelError::NoSuchDevice`] if the fd was revoked with `EVIOCREVOKE`.
/// - [`KernelError::InvalidArgument`] if `buf` cannot hold one whole event.
pub fn read(handle: EvdevHandle, buf: &mut [u8]) -> KernelResult<usize> {
    let mut table = EVDEV_TABLE.lock();
    let inst = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if inst.revoked {
        return Err(KernelError::NoSuchDevice);
    }
    if grabbed_out(inst, handle.id()) {
        // A whole-buffer check still applies: a client with a bad buffer
        // should learn that regardless of who holds the grab.
        if buf.len() < crate::evdev::INPUT_EVENT_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        return Ok(0);
    }
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
// Control — grab, revoke, clock selection (the `EVIOC*` ioctls)
// ---------------------------------------------------------------------------

/// Take an exclusive grab of this fd's device (`EVIOCGRAB` with a non-zero
/// argument).
///
/// While a grab is held, every *other* open of the same device reads nothing
/// and polls not-readable, and the events produced during the grab are never
/// delivered to them — see [`grabbed_out`].  Re-grabbing with the same fd
/// succeeds and changes nothing, matching Linux.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::NoSuchDevice`] if the fd was revoked.
/// - [`KernelError::DeviceBusy`] if another fd already holds the grab (`EBUSY`).
pub fn grab(handle: EvdevHandle) -> KernelResult<()> {
    let table = EVDEV_TABLE.lock();
    let inst = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    if inst.revoked {
        return Err(KernelError::NoSuchDevice);
    }
    let slot = grab_slot(inst.client.device()).ok_or(KernelError::InvalidArgument)?;
    match slot.compare_exchange(0, handle.id(), Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(()),
        // Already ours: idempotent, as on Linux.
        Err(owner) if owner == handle.id() => Ok(()),
        Err(_) => Err(KernelError::DeviceBusy),
    }
}

/// Release this fd's exclusive grab (`EVIOCGRAB` with a zero argument).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if this fd does not hold the grab, which
///   is Linux's `EINVAL` for ungrabbing something you never grabbed.
pub fn ungrab(handle: EvdevHandle) -> KernelResult<()> {
    let table = EVDEV_TABLE.lock();
    let inst = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    let slot = grab_slot(inst.client.device()).ok_or(KernelError::InvalidArgument)?;
    match slot.compare_exchange(handle.id(), 0, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(()),
        Err(_) => Err(KernelError::InvalidArgument),
    }
}

/// Does this fd currently hold the exclusive grab on its device?
#[must_use]
pub fn holds_grab(handle: EvdevHandle) -> bool {
    let table = EVDEV_TABLE.lock();
    table.get(&handle.id()).is_some_and(|inst| {
        grab_slot(inst.client.device())
            .is_some_and(|slot| slot.load(Ordering::Acquire) == handle.id())
    })
}

/// Permanently revoke this fd (`EVIOCREVOKE`).
///
/// Every subsequent operation on it fails; only `close` still works.  This is
/// how a session manager takes input away from a process it is switching away
/// from without needing that process to cooperate — the alternative, killing
/// it, loses its state.
///
/// Irreversible by design: a revoke that could be undone would be a lock with
/// a key left in it.  Any grab this fd held is released, since the fd can no
/// longer do anything with it.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance is stale.
pub fn revoke(handle: EvdevHandle) -> KernelResult<()> {
    let mut table = EVDEV_TABLE.lock();
    let inst = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    inst.revoked = true;
    if let Some(slot) = grab_slot(inst.client.device()) {
        // Fails when this fd was not the owner; nothing to release then.
        let _ = slot.compare_exchange(handle.id(), 0, Ordering::AcqRel, Ordering::Relaxed);
    }
    Ok(())
}

/// Has this fd been revoked?
#[must_use]
pub fn is_revoked(handle: EvdevHandle) -> bool {
    EVDEV_TABLE
        .lock()
        .get(&handle.id())
        .is_some_and(|i| i.revoked)
}

/// Select the clock this fd's event timestamps are reported in
/// (`EVIOCSCLOCKID`).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::NoSuchDevice`] if the fd was revoked.
/// - [`KernelError::InvalidArgument`] for an unsupported clock.
pub fn set_clockid(handle: EvdevHandle, clockid: u32) -> KernelResult<()> {
    let mut table = EVDEV_TABLE.lock();
    let inst = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    if inst.revoked {
        return Err(KernelError::NoSuchDevice);
    }
    inst.client.set_clockid(clockid)
}

/// The clock this fd's timestamps are reported in, or `None` if stale.
#[must_use]
pub fn clockid(handle: EvdevHandle) -> Option<u32> {
    EVDEV_TABLE
        .lock()
        .get(&handle.id())
        .map(|i| i.client.clockid())
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

    // --- exclusive grab -----------------------------------------------------
    check!(!holds_grab(a) && !holds_grab(b), "nobody grabs by default");
    grab(a)?;
    check!(holds_grab(a), "A now holds the grab");
    check!(
        grab(a).is_ok(),
        "re-grabbing with the same fd is idempotent"
    );
    check!(
        grab(b) == Err(KernelError::DeviceBusy),
        "a second grabber is EBUSY"
    );
    check!(
        ungrab(b) == Err(KernelError::InvalidArgument),
        "ungrabbing a grab you never took is EINVAL"
    );

    // Events typed during the grab reach the owner and nobody else — and are
    // not merely delayed for the others, which is the whole point.
    crate::evdev::push_key(InputDevice::Keyboard, 30, 0x1E, true);
    check!(readable(a), "the grab owner still reads");
    check!(!readable(b), "a grabbed-out fd is not readable");
    check!(read(b, &mut buf)? == 0, "and reads nothing");
    check!(
        read(a, &mut buf)? == 3 * crate::evdev::INPUT_EVENT_SIZE,
        "the owner gets the whole keystroke"
    );
    ungrab(a)?;
    check!(!holds_grab(a), "the grab is released");
    // B learns its view is stale rather than receiving the withheld keystroke.
    let n = read(b, &mut buf)?;
    check!(n == crate::evdev::INPUT_EVENT_SIZE, "B gets one record");
    check!(
        buf.get(18..20) == Some(&crate::evdev::SYN_DROPPED.to_le_bytes()[..]),
        "...and it is SYN_DROPPED, not the withheld keypress"
    );

    // A grab dies with its owner: otherwise the device would be silenced for
    // everyone with no fd left that could lift it.
    grab(a)?;
    close(a);
    check!(!exists(a), "grab owner closed");
    crate::evdev::push_key(InputDevice::Keyboard, 31, 0x1F, true);
    check!(readable(b), "closing the grab owner frees the device");
    while read(b, &mut buf)? > 0 {}

    // --- clock selection ----------------------------------------------------
    check!(
        clockid(b) == Some(crate::evdev::CLOCK_REALTIME),
        "realtime is the default clock"
    );
    set_clockid(b, crate::evdev::CLOCK_MONOTONIC)?;
    check!(
        clockid(b) == Some(crate::evdev::CLOCK_MONOTONIC),
        "EVIOCSCLOCKID(MONOTONIC) sticks"
    );
    check!(
        set_clockid(b, crate::evdev::CLOCK_BOOTTIME).is_ok(),
        "boottime is accepted"
    );
    check!(
        set_clockid(b, 99) == Err(KernelError::InvalidArgument),
        "an unknown clock is EINVAL"
    );

    // --- revoke -------------------------------------------------------------
    check!(!is_revoked(b), "not revoked to begin with");
    revoke(b)?;
    check!(is_revoked(b), "revoke sticks");
    check!(!readable(b), "a revoked fd is never readable");
    check!(
        read(b, &mut buf) == Err(KernelError::NoSuchDevice),
        "a revoked fd reads ENODEV"
    );
    check!(
        grab(b) == Err(KernelError::NoSuchDevice),
        "a revoked fd cannot grab"
    );
    check!(exists(b), "revoking is not closing: the instance survives");

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
