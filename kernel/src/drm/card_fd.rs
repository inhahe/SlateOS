//! DRM card file-descriptor instance objects — the per-open kernel state
//! behind a `/dev/dri/card0` or `/dev/dri/renderD128` file descriptor.
//!
//! When a Linux graphics client (Mesa, libdrm, the X.Org modesetting
//! driver, a Wayland compositor, SDL/KMSDRM) opens `/dev/dri/card0` it gets
//! a file descriptor that it then drives entirely through `ioctl(2)`:
//! `DRM_IOCTL_VERSION` / `GET_CAP` to learn the driver's identity and
//! capabilities, `SET_CLIENT_CAP` to opt into atomic/universal-planes
//! behaviour, and the KMS ioctls (`MODE_GETRESOURCES`, …) to enumerate and
//! program the display.  Each open is an independent *client* with its own
//! capability state.
//!
//! This module owns the **instance object** that a `HandleKind::DrmCard`
//! [`crate::proc::linux_fd::FdEntry`] points at: which DRM device the fd
//! refers to, whether it is a render node (no modeset/KMS authority), and
//! the per-client capability opt-ins.  It mirrors the refcounted-instance
//! pattern used by [`crate::ipc::alsa_pcm`] / [`crate::ipc::timerfd`]:
//! [`create`] starts the count at 1, [`dup`] bumps it (so `fork`/`dup` let
//! parent and child — or two fds — share one client object, matching how
//! Linux shares the `struct drm_file` across a dup'd fd), and only the
//! final [`close`] (count → 0) removes the object.
//!
//! ## What lives here vs. in [`crate::drm::uapi`]
//!
//! [`crate::drm::uapi`] holds the *ABI*: the ioctl numbers and the
//! `#[repr(C)]` payload structs.  [`crate::drm`] holds the *device model*
//! (connectors, CRTCs, planes, GEM buffers).  This module holds the *live
//! per-fd state* of one open client; the ioctl-dispatch glue that reads a
//! request struct, validates it, and drives the device lands in the syscall
//! layer (a later commit).  This commit is just the instance object plus its
//! fd-family wiring.
//!
//! Some accessors here are consumed by the ioctl-dispatch glue that lands in
//! a later commit, so the whole `drm` subsystem allows `dead_code` until that
//! wiring is in place.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::uapi;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a DRM card client instance (the handle IS the ID).
type DrmCardId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_DRM_CARD_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_drm_card_id() -> DrmCardId {
    NEXT_DRM_CARD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an open DRM card client instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::DrmCard` variant); the syscall layer reconstructs it with
/// [`DrmCardHandle::from_raw`] on each ioctl / poll / close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrmCardHandle(u64);

impl DrmCardHandle {
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

    fn id(self) -> DrmCardId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A kernel DRM card client instance — the per-open state of one
/// `/dev/dri/*` fd.
struct DrmClient {
    /// Index of the DRM device (in [`crate::drm`]'s registry) this fd refers
    /// to.  All `/dev/dri/cardN` and `renderD128+N` nodes for the same GPU
    /// map to the same device index.
    device: usize,
    /// True if opened via a render node (`renderD128`): such fds may not
    /// perform modeset/KMS ioctls (no display authority), only GEM/render
    /// operations.  A `cardN` fd has full authority.
    render_node: bool,
    /// Bitmask of accepted `DRM_CLIENT_CAP_*` opt-ins, indexed by the cap
    /// tag value (bit `cap` set ⇒ that cap is enabled for this client).
    client_caps: u32,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
    /// FIFO of pending KMS events (serialized `drm_event_vblank` records),
    /// drained in order by `read(2)`.  A completed page-flip with
    /// `DRM_MODE_PAGE_FLIP_EVENT` pushes one record here; `poll(2)` reports
    /// the fd readable while it is non-empty.  Shared across `dup`'d fds,
    /// matching how Linux queues events on the `struct drm_file`.
    events: VecDeque<Vec<u8>>,
}

impl DrmClient {
    const fn new(device: usize, render_node: bool) -> Self {
        Self {
            device,
            render_node,
            client_caps: 0,
            refcount: 1,
            events: VecDeque::new(),
        }
    }
}

/// Upper bound on queued-but-undrained events per client.
///
/// A client that requests flip events but never `read(2)`s them would
/// otherwise grow this queue without bound.  Linux caps the per-file event
/// space similarly (`drm_event` accounting against a fixed budget); we cap the
/// record count.  Past the cap the oldest event is dropped to make room — a
/// slow reader loses the stalest completion rather than pinning kernel memory.
const MAX_PENDING_EVENTS: usize = 128;

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live DRM card client instances, keyed by ID.
///
/// Leaf lock — no other lock is taken while it is held.
static DRM_CARD_TABLE: Mutex<BTreeMap<DrmCardId, DrmClient>> = Mutex::new(BTreeMap::new());

/// Convert a `DRM_CLIENT_CAP_*` tag value to its bit in `client_caps`.
///
/// Returns `None` if the tag does not fit in the 32-bit mask (all real DRM
/// client-cap tags are small, so this only guards against a malformed value).
fn cap_bit(cap: u64) -> Option<u32> {
    let shift = u32::try_from(cap).ok()?;
    1u32.checked_shl(shift)
}

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new DRM card client instance for `device`.
///
/// `render_node` selects render-node semantics (`renderD128`, no KMS
/// authority).  The returned handle owns one reference; the caller must
/// [`close`] it (directly or via process-exit cleanup) exactly once for that
/// reference.
#[must_use]
pub fn create(device: usize, render_node: bool) -> DrmCardHandle {
    let id = alloc_drm_card_id();
    DRM_CARD_TABLE.lock().insert(id, DrmClient::new(device, render_node));
    DrmCardHandle(id)
}

/// Add one reference to a client instance, returning the same handle.
///
/// Used when `fork`/`dup` duplicates the inheriting fd: both fds then share
/// the *same* client object (shared capability state), and neither one's
/// [`close`] invalidates the other's.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: DrmCardHandle) -> KernelResult<DrmCardHandle> {
    let mut table = DRM_CARD_TABLE.lock();
    let client = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    client.refcount = client
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to a client instance.
///
/// Only the final [`close`] (refcount → 0) removes the instance.  A
/// double-close is harmless: the saturating decrement floors at 0 and an
/// unknown handle is simply ignored.
pub fn close(handle: DrmCardHandle) {
    let mut table = DRM_CARD_TABLE.lock();
    if let Some(client) = table.get_mut(&handle.id()) {
        client.refcount = client.refcount.saturating_sub(1);
        if client.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Does this handle refer to a live client instance?
#[must_use]
pub fn exists(handle: DrmCardHandle) -> bool {
    DRM_CARD_TABLE.lock().contains_key(&handle.id())
}

/// The DRM device index this fd refers to, or `None` if stale.
#[must_use]
pub fn device(handle: DrmCardHandle) -> Option<usize> {
    DRM_CARD_TABLE.lock().get(&handle.id()).map(|c| c.device)
}

/// Whether this fd is a render node (`renderD128`), or `None` if stale.
#[must_use]
pub fn is_render_node(handle: DrmCardHandle) -> Option<bool> {
    DRM_CARD_TABLE.lock().get(&handle.id()).map(|c| c.render_node)
}

// ---------------------------------------------------------------------------
// Client capabilities (driven by DRM_IOCTL_SET_CLIENT_CAP)
// ---------------------------------------------------------------------------

/// Set or clear a `DRM_CLIENT_CAP_*` opt-in for this client.
///
/// Mirrors the Linux DRM core: an unsupported capability, or a value other
/// than 0/1, returns `-EINVAL`.  `value != 0` enables the cap, `0` disables
/// it.  The setting is shared across all fds that share this instance (dup),
/// matching the per-`struct drm_file` semantics in Linux.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if the instance is stale.
/// - [`KernelError::InvalidArgument`] if `cap` is not one this driver
///   supports (see [`uapi::client_cap_supported`]) or `value` is not 0/1.
pub fn set_client_cap(handle: DrmCardHandle, cap: u64, value: u64) -> KernelResult<()> {
    if !uapi::client_cap_supported(cap) || value > 1 {
        // Confirm existence so a stale fd still reports InvalidHandle rather
        // than masking it as an argument error.
        if !exists(handle) {
            return Err(KernelError::InvalidHandle);
        }
        return Err(KernelError::InvalidArgument);
    }
    let bit = cap_bit(cap).ok_or(KernelError::InvalidArgument)?;
    let mut table = DRM_CARD_TABLE.lock();
    let client = table.get_mut(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    if value == 1 {
        client.client_caps |= bit;
    } else {
        client.client_caps &= !bit;
    }
    Ok(())
}

/// Whether a given `DRM_CLIENT_CAP_*` is enabled for this client.
///
/// Returns `None` for a stale handle, `Some(false)` for an unset/unsupported
/// cap.
#[must_use]
pub fn client_cap(handle: DrmCardHandle, cap: u64) -> Option<bool> {
    let bit = cap_bit(cap)?;
    DRM_CARD_TABLE
        .lock()
        .get(&handle.id())
        .map(|c| c.client_caps & bit != 0)
}

// ---------------------------------------------------------------------------
// KMS event queue (read(2) / poll(2) on a /dev/dri fd)
// ---------------------------------------------------------------------------

/// Queue a serialized KMS event (`bytes`, a `drm_event_vblank` wire record)
/// for delivery via `read(2)`.
///
/// Used by the `PAGE_FLIP` handler when the client set
/// `DRM_MODE_PAGE_FLIP_EVENT`.  If the per-client queue is at
/// [`MAX_PENDING_EVENTS`] the oldest record is dropped first (a slow reader
/// loses the stalest completion rather than pinning unbounded kernel memory).
///
/// A stale handle is silently ignored — the event simply has no live fd to be
/// delivered on, exactly as if it had been read and discarded.
pub fn queue_event(handle: DrmCardHandle, bytes: &[u8]) {
    let mut table = DRM_CARD_TABLE.lock();
    if let Some(client) = table.get_mut(&handle.id()) {
        while client.events.len() >= MAX_PENDING_EVENTS {
            client.events.pop_front();
        }
        client.events.push_back(bytes.to_vec());
    }
}

/// Whether at least one event is queued for delivery.
///
/// Returns `false` for a stale handle (nothing to read).  Used by the poll
/// path to set `POLLIN`.
#[must_use]
pub fn has_events(handle: DrmCardHandle) -> bool {
    DRM_CARD_TABLE
        .lock()
        .get(&handle.id())
        .is_some_and(|c| !c.events.is_empty())
}

/// Byte length of the next queued event without removing it, or `None` if the
/// queue is empty (or the handle is stale).
///
/// The `read(2)` path uses this to decide whether the next whole event fits in
/// the caller's remaining buffer before committing to dequeue it (DRM reads
/// deliver only whole event records).
#[must_use]
pub fn next_event_len(handle: DrmCardHandle) -> Option<usize> {
    DRM_CARD_TABLE
        .lock()
        .get(&handle.id())
        .and_then(|c| c.events.front().map(Vec::len))
}

/// Remove and return the next queued event, or `None` if the queue is empty
/// (or the handle is stale).
#[must_use]
pub fn pop_event(handle: DrmCardHandle) -> Option<Vec<u8>> {
    DRM_CARD_TABLE
        .lock()
        .get_mut(&handle.id())
        .and_then(|c| c.events.pop_front())
}

/// Atomically drain whole queued events into one contiguous buffer, stopping
/// before any event that would push the total over `max_bytes`.
///
/// The whole drain happens under a single lock acquisition, so it is race-free
/// against a concurrent reader on a `dup`'d fd: each event is delivered to
/// exactly one reader.  Only complete event records are dequeued (DRM never
/// delivers a partial event), and the returned buffer is always `≤ max_bytes`.
/// A stale handle yields an empty buffer.
#[must_use]
pub fn drain_into_kernel(handle: DrmCardHandle, max_bytes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut table = DRM_CARD_TABLE.lock();
    if let Some(client) = table.get_mut(&handle.id()) {
        while let Some(front) = client.events.front() {
            // Stop if this whole event would not fit in the remaining budget.
            match out.len().checked_add(front.len()) {
                Some(total) if total <= max_bytes => {}
                _ => break,
            }
            if let Some(ev) = client.events.pop_front() {
                out.extend_from_slice(&ev);
            } else {
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the DRM card client instance lifecycle.
///
/// Exercises create → dup → close (twice) refcounting, the device/render-node
/// accessors, and the client-cap set/clear/validate paths, leaving no
/// instances behind.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] on the first failed invariant.
pub fn self_test() -> KernelResult<()> {
    macro_rules! check {
        ($cond:expr, $msg:expr) => {
            if !($cond) {
                serial_println!("[drm_card] SELF-TEST FAILED: {}", $msg);
                return Err(KernelError::InternalError);
            }
        };
    }

    // Fresh card fd: device 0, not a render node, alive.
    let h = create(0, false);
    check!(exists(h), "new instance must exist");
    check!(device(h) == Some(0), "device index recorded");
    check!(is_render_node(h) == Some(false), "card node, not render node");

    // Client caps start clear; the supported ones set/clear, unsupported and
    // out-of-range values are rejected.
    check!(
        client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC) == Some(false),
        "atomic cap starts clear"
    );
    set_client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC, 1)?;
    check!(
        client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC) == Some(true),
        "atomic cap set"
    );
    set_client_cap(h, uapi::DRM_CLIENT_CAP_UNIVERSAL_PLANES, 1)?;
    check!(
        client_cap(h, uapi::DRM_CLIENT_CAP_UNIVERSAL_PLANES) == Some(true),
        "universal-planes cap set"
    );
    // Atomic stays set independently.
    check!(
        client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC) == Some(true),
        "atomic cap independent of universal-planes"
    );
    set_client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC, 0)?;
    check!(
        client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC) == Some(false),
        "atomic cap cleared"
    );
    check!(
        set_client_cap(h, uapi::DRM_CLIENT_CAP_STEREO_3D, 1)
            == Err(KernelError::InvalidArgument),
        "unsupported cap rejected"
    );
    check!(
        set_client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC, 2)
            == Err(KernelError::InvalidArgument),
        "out-of-range cap value rejected"
    );

    // dup bumps the refcount: it then takes two closes to free, and the cap
    // state is shared.
    let h2 = dup(h)?;
    check!(h2 == h, "dup returns the same handle");
    check!(
        client_cap(h2, uapi::DRM_CLIENT_CAP_UNIVERSAL_PLANES) == Some(true),
        "cap state shared across dup"
    );
    close(h);
    check!(exists(h), "still alive after one of two closes");
    close(h);
    check!(!exists(h), "freed after the second close");

    // Render node round-trips.
    let r = create(0, true);
    check!(is_render_node(r) == Some(true), "render node flag");
    close(r);
    check!(!exists(r), "render-node instance freed");

    // Event queue: empty → no events; push two → FIFO order + length probe;
    // pop drains; shared across dup; drops on final close.
    let e = create(0, false);
    check!(!has_events(e), "fresh fd has no events");
    check!(next_event_len(e).is_none(), "empty queue has no next length");
    check!(pop_event(e).is_none(), "popping an empty queue is None");
    queue_event(e, &[1, 2, 3]);
    queue_event(e, &[4, 5, 6, 7]);
    check!(has_events(e), "events pending after queue");
    check!(next_event_len(e) == Some(3), "FIFO: first event length");
    let e2 = dup(e)?;
    check!(pop_event(e2) == Some(alloc::vec![1, 2, 3]), "shared queue across dup");
    check!(next_event_len(e) == Some(4), "second event now at front");
    check!(pop_event(e) == Some(alloc::vec![4, 5, 6, 7]), "FIFO: second event");
    check!(!has_events(e), "queue drained");
    close(e);
    close(e2);
    check!(!exists(e), "event-queue fd freed after final close");
    // Stale handle: event ops are inert, never a panic.
    queue_event(e, &[9]);
    check!(!has_events(e), "stale fd reports no events");
    check!(pop_event(e).is_none(), "stale fd pop is None");

    // Stale-handle accessors are all None/err, never a panic.
    check!(device(h).is_none(), "stale device is None");
    check!(is_render_node(h).is_none(), "stale render-node is None");
    check!(client_cap(h, uapi::DRM_CLIENT_CAP_ATOMIC).is_none(), "stale cap is None");
    check!(dup(h).is_err(), "dup of a stale handle errors");

    serial_println!("[drm_card] DRM card client lifecycle self-test PASSED");
    Ok(())
}
