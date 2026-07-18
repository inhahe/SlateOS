//! Filesystem change notification system (inotify equivalent).
//!
//! Provides asynchronous notifications when files or directories are
//! created, deleted, modified, or renamed.  Programs register "watches"
//! on paths and then poll for events.
//!
//! ## Architecture
//!
//! ```text
//! userspace program
//!    ↓ create_watch("/docs", CREATE | DELETE | MODIFY)
//! notify module (this)
//!    ← emit() called by VFS after each operation
//!    → events queued for matching watches
//!    ↓ read_events(watch_id)
//! userspace program
//! ```
//!
//! ## Design decisions
//!
//! - **Asynchronous, not synchronous**: events are queued and delivered
//!   on poll.  A slow consumer cannot stall filesystem operations.
//!   (Per design spec: "Make hooks asynchronous (notification queue,
//!   not synchronous callback)")
//! - **Path-based watches**: a watch monitors a directory path.  Events
//!   are generated for files/subdirectories within that directory.
//! - **Event coalescing**: if the same event (same type + path) is
//!   already pending, it's not duplicated.  This prevents flooding from
//!   rapid repeated writes.
//! - **Bounded queues**: each watch has a maximum event queue depth.
//!   When full, oldest events are dropped and an overflow flag is set.
//! - **Non-recursive by default**: a watch on `/docs` reports events
//!   for `/docs/file.txt` but not `/docs/sub/file.txt`.  Recursive
//!   watching is opt-in.
//!
//! ## Performance
//!
//! The `emit()` function is called on every VFS operation (hot path).
//! It must be fast when there are no watches (common case).
//! Implementation: lock the watch table, iterate watches, compare
//! paths.  With a small number of watches (<100), linear scan is fine.
//! If watch counts grow, a trie-based path index would be needed.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};

// ---------------------------------------------------------------------------
// Event types and masks
// ---------------------------------------------------------------------------

/// Bitmask of event types a watch is interested in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsEventMask(pub u32);

impl FsEventMask {
    /// A file or directory was created.
    pub const CREATE: Self = Self(1 << 0);
    /// A file or directory was deleted (including trash).
    pub const DELETE: Self = Self(1 << 1);
    /// A file's contents were modified.
    pub const MODIFY: Self = Self(1 << 2);
    /// A file or directory was renamed or moved.
    pub const RENAME: Self = Self(1 << 3);
    /// File metadata changed (permissions, attributes, etc.).
    pub const METADATA: Self = Self(1 << 4);
    /// A file was read/accessed (high-frequency, off by default).
    pub const ACCESS: Self = Self(1 << 5);
    /// A file was opened (high-frequency, off by default).
    pub const OPEN: Self = Self(1 << 6);
    /// A file opened for writing was closed (off by default).
    pub const CLOSE_WRITE: Self = Self(1 << 7);
    /// A file not opened for writing was closed (off by default).
    pub const CLOSE_NOWRITE: Self = Self(1 << 8);

    /// All change events (create/delete/modify/rename/metadata).  Excludes the
    /// high-frequency, opt-in access/open/close notifications (`ACCESS`,
    /// `OPEN`, `CLOSE_WRITE`, `CLOSE_NOWRITE`) so the common "watch a dir for
    /// changes" idiom never pays for them.
    pub const ALL_CHANGES: Self = Self(0x1F);

    /// Check if a specific event type is enabled.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Type of filesystem change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEventType {
    /// A file or directory was created.
    Created = 0,
    /// A file or directory was deleted.
    Deleted = 1,
    /// A file's contents were modified.
    Modified = 2,
    /// A file or directory was renamed.
    Renamed = 3,
    /// Metadata changed (not yet implemented).
    MetadataChanged = 4,
    /// A file was accessed/read (optional, high-frequency).
    Accessed = 5,
    /// A file was opened (optional, high-frequency).
    Opened = 6,
    /// A file opened for writing was closed (optional).
    ClosedWrite = 7,
    /// A file not opened for writing was closed (optional).
    ClosedNoWrite = 8,
    /// Events were lost due to queue overflow.
    Overflow = 255,
}

impl FsEventType {
    /// Convert to the corresponding event mask bit.
    pub const fn to_mask(self) -> FsEventMask {
        match self {
            Self::Created => FsEventMask::CREATE,
            Self::Deleted => FsEventMask::DELETE,
            Self::Modified => FsEventMask::MODIFY,
            Self::Renamed => FsEventMask::RENAME,
            Self::MetadataChanged => FsEventMask::METADATA,
            Self::Accessed => FsEventMask::ACCESS,
            Self::Opened => FsEventMask::OPEN,
            Self::ClosedWrite => FsEventMask::CLOSE_WRITE,
            Self::ClosedNoWrite => FsEventMask::CLOSE_NOWRITE,
            Self::Overflow => FsEventMask(0),
        }
    }
}

/// A filesystem change event.
#[derive(Debug, Clone)]
pub struct FsEvent {
    /// The watch that generated this event.
    pub watch_id: u64,
    /// Type of change.
    pub event_type: FsEventType,
    /// Affected path (relative to the watched directory, or absolute).
    pub path: String,
    /// For rename events: the new path.
    pub new_path: Option<String>,
    /// Whether the subject of this event is a directory (as opposed to a
    /// regular file).  The Linux-ABI inotify adapter ORs `IN_ISDIR` into the
    /// reported event mask when this is set.  Defaults to `false` for the
    /// common file-event path; directory-aware emitters (mkdir/rmdir, the
    /// directory-handle close) set it true.
    pub is_dir: bool,
}

// ---------------------------------------------------------------------------
// Watch state
// ---------------------------------------------------------------------------

/// A filesystem watch that monitors a path for changes.
struct FsWatch {
    /// Unique watch identifier.
    id: u64,
    /// Watched directory path (normalized, with trailing `/`).
    path: String,
    /// Which events to report.
    mask: FsEventMask,
    /// Watch subdirectories recursively?
    recursive: bool,
    /// Pending event queue.
    events: VecDeque<FsEvent>,
    /// Maximum event queue depth.
    max_events: usize,
    /// Whether events have been dropped due to overflow.
    overflowed: bool,
    /// Opaque owner token used to wake a *blocked* reader when this watch
    /// queues an event (0 = no blocking owner — the poll/`read_events`-only
    /// consumers).  The Linux-ABI inotify adapter sets this to the owning
    /// instance id so any watch of that instance — including ones added after
    /// the reader parked — routes the wake to the same blocked task.
    owner_token: u64,
}

/// Maximum number of events per watch queue.
const DEFAULT_MAX_EVENTS: usize = 256;

/// Maximum number of concurrent watches.
const MAX_WATCHES: usize = 256;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static NEXT_WATCH_ID: AtomicU64 = AtomicU64::new(1);

static WATCHES: Mutex<BTreeMap<u64, FsWatch>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Per-event-bit interest counts (lock-free hot-path gate)
// ---------------------------------------------------------------------------
//
// `emit()` is called after VFS operations on the hot path (every write, and —
// once gated — every read).  Taking the `WATCHES` lock on every such call just
// to discover that no watch cares about this event type is wasteful, and the
// read path is high-frequency enough that the original code deliberately did
// NOT emit an `ACCESS` event at all (see the note on `Vfs::read_at`).
//
// Instead we keep one reference count per `FsEventMask` bit: the number of live
// watches whose mask includes that bit.  Maintained on every watch create
// (increment) and close (decrement), it lets any caller ask
// `interest_includes(mask)` with a handful of relaxed atomic loads and no lock.
// The read path uses it to skip the `ACCESS` emit entirely in the common case
// (no watch asked for `ACCESS`), so the inotify `IN_ACCESS` feature costs
// nothing when unused; `emit()` itself uses it as a lock-free early-out.

/// Number of distinct event-type bits in [`FsEventMask`] — CREATE, DELETE,
/// MODIFY, RENAME, METADATA, ACCESS, OPEN, CLOSE_WRITE, CLOSE_NOWRITE
/// (bits 0..=8).
const NUM_EVENT_BITS: usize = 9;

/// Per-event-bit reference counts: `INTEREST_COUNTS[b]` is the number of live
/// watches whose mask includes bit `b`.  See the module-internal note above.
static INTEREST_COUNTS: [AtomicU32; NUM_EVENT_BITS] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

/// Adjust the per-bit interest counts for `mask`: increment each set bit on a
/// watch create (`add == true`), decrement on a close.  The decrement is
/// saturating so a stray double-close can never underflow a counter.
fn adjust_interest(mask: FsEventMask, add: bool) {
    for (bit, counter) in INTEREST_COUNTS.iter().enumerate() {
        if mask.0 & (1u32 << bit) == 0 {
            continue;
        }
        if add {
            counter.fetch_add(1, Ordering::Relaxed);
        } else {
            // The closure always returns `Some`, so `fetch_update` never fails;
            // the saturating subtraction is the whole point, so discarding the
            // (always-`Ok`) result is safe.
            let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
        }
    }
}

/// Is any live watch interested in *any* of the event bits in `mask`?
///
/// A lock-free check (one relaxed load per set bit) used by VFS hot paths to
/// avoid calling [`emit`] — and taking the `WATCHES` lock — when nothing would
/// match.  The file read path in particular gates its high-frequency `ACCESS`
/// event on this.
#[must_use]
pub fn interest_includes(mask: FsEventMask) -> bool {
    INTEREST_COUNTS
        .iter()
        .enumerate()
        .any(|(bit, counter)| mask.0 & (1u32 << bit) != 0 && counter.load(Ordering::Relaxed) > 0)
}

// ---------------------------------------------------------------------------
// Blocking-read wait queue
// ---------------------------------------------------------------------------
//
// A consumer (e.g. the Linux-ABI inotify adapter) that wants to *block* until
// one of its watches produces an event registers its task here, keyed by an
// opaque **owner token** (the consumer's instance id), then re-checks for
// pending events before parking (register-then-recheck — see the inotify read
// path).  `emit()` wakes every task registered against the owner token of a
// watch it just queued an event for, after dropping the `WATCHES` lock
// (leaf-lock discipline), so the wake never runs with `WATCHES` held.
//
// Keying by owner token (rather than per-watch id) means a reader registers
// ONCE for its whole instance: a watch added *after* the reader parked, or an
// instance that had no watches at park time, still routes the wake correctly,
// matching Linux's per-`inotify_group` wait queue.
//
// The registry is kept SEPARATE from `FsWatch` so the hot `emit()` scan does
// not touch it for watches with no blocked readers, and so the wake set can be
// collected under `WATCHES` and woken after the lock is released.

/// `owner_token → tasks blocked waiting for an event on that owner's watches`.
static NOTIFY_WAITERS: Mutex<BTreeMap<u64, Vec<TaskId>>> = Mutex::new(BTreeMap::new());

/// Register `task` as blocked waiting for an event on any watch owned by
/// `owner_token`.
///
/// Idempotent: a task already registered for this owner is not duplicated.
pub fn register_notify_waiter(owner_token: u64, task: TaskId) {
    let mut waiters = NOTIFY_WAITERS.lock();
    let list = waiters.entry(owner_token).or_default();
    if !list.contains(&task) {
        list.push(task);
    }
}

/// Remove `task`'s registration for `owner_token` (a no-op if not present).
pub fn deregister_notify_waiter(owner_token: u64, task: TaskId) {
    let mut waiters = NOTIFY_WAITERS.lock();
    if let Some(list) = waiters.get_mut(&owner_token) {
        list.retain(|&t| t != task);
        if list.is_empty() {
            waiters.remove(&owner_token);
        }
    }
}

/// Remove and return every task waiting on `owner_token` (pure registry
/// mutation, no scheduler interaction — split out so it is unit-testable).
fn take_notify_waiters(owner_token: u64) -> Vec<TaskId> {
    NOTIFY_WAITERS.lock().remove(&owner_token).unwrap_or_default()
}

/// Wake every task blocked on any of `owner_tokens`.
///
/// Uses the `try_wake`/`defer_wake` idiom so it is safe to call from any
/// context and never re-enters a held lock.  Call this only after releasing
/// `WATCHES`.
pub fn wake_notify_waiters(owner_tokens: &[u64]) {
    for &token in owner_tokens {
        if token == 0 {
            continue;
        }
        for task in take_notify_waiters(token) {
            if !sched::try_wake(task) {
                sched::defer_wake(task);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — watch management
// ---------------------------------------------------------------------------

/// Create a new filesystem watch.
///
/// Monitors `path` for events matching `mask`.  If `recursive` is true,
/// events from subdirectories are also reported.
///
/// Returns a watch ID that can be used with [`read_events`] and
/// [`close_watch`].  The watch has no blocking-read owner (poll/`read_events`
/// consumers only); use [`create_watch_owned`] to attach a wake owner token.
pub fn create_watch(
    path: &str,
    mask: FsEventMask,
    recursive: bool,
) -> KernelResult<u64> {
    create_watch_owned(path, mask, recursive, 0)
}

/// Create a watch with an opaque `owner_token` for blocking-read wakeups.
///
/// Identical to [`create_watch`] except that, when this watch queues an event,
/// [`emit`] wakes any task registered (via [`register_notify_waiter`]) against
/// `owner_token`.  A token of 0 means "no blocking owner".  Used by the
/// Linux-ABI inotify adapter, which passes its instance id so a blocked
/// `read()` is woken when any of the instance's watches fire.
pub fn create_watch_owned(
    path: &str,
    mask: FsEventMask,
    recursive: bool,
    owner_token: u64,
) -> KernelResult<u64> {
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if mask.0 == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let mut watches = WATCHES.lock();

    if watches.len() >= MAX_WATCHES {
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_WATCH_ID.fetch_add(1, Ordering::Relaxed);

    // Normalize the path: ensure it ends with '/' for directory matching.
    let mut normalized = String::from(path);
    if !normalized.ends_with('/') {
        normalized.push('/');
    }

    watches.insert(id, FsWatch {
        id,
        path: normalized,
        mask,
        recursive,
        events: VecDeque::with_capacity(16),
        max_events: DEFAULT_MAX_EVENTS,
        overflowed: false,
        owner_token,
    });

    // Track per-bit interest so the hot-path `interest_includes` gate stays
    // accurate (counted only once the watch is actually in the table).
    adjust_interest(mask, true);

    crate::serial_println!(
        "[notify] Watch {} created for '{}' (mask={:#x}, recursive={}, owner={:#x})",
        id, path, mask.0, recursive, owner_token
    );

    Ok(id)
}

/// Read pending events from a watch.
///
/// Returns up to `max` events.  Events are removed from the queue
/// after reading.  If the queue overflowed since the last read, an
/// `Overflow` event is prepended.
///
/// Returns an empty vector if no events are pending.
pub fn read_events(watch_id: u64, max: usize) -> KernelResult<Vec<FsEvent>> {
    let mut watches = WATCHES.lock();
    let watch = watches.get_mut(&watch_id)
        .ok_or(KernelError::InvalidHandle)?;

    let mut result = Vec::with_capacity(max.min(watch.events.len().wrapping_add(1)));

    // If overflowed, report it first.
    if watch.overflowed {
        result.push(FsEvent {
            watch_id,
            event_type: FsEventType::Overflow,
            path: String::new(),
            new_path: None,
            is_dir: false,
        });
        watch.overflowed = false;
    }

    // Drain up to `max` events.
    let drain_count = max.saturating_sub(result.len()).min(watch.events.len());
    for _ in 0..drain_count {
        if let Some(event) = watch.events.pop_front() {
            result.push(event);
        }
    }

    Ok(result)
}

/// Return the number of pending events for a watch.
pub fn pending_count(watch_id: u64) -> KernelResult<usize> {
    let watches = WATCHES.lock();
    let watch = watches.get(&watch_id)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(watch.events.len())
}

/// Close (remove) a filesystem watch.
///
/// All pending events are discarded.
pub fn close_watch(watch_id: u64) -> KernelResult<()> {
    let mut watches = WATCHES.lock();
    let Some(removed) = watches.remove(&watch_id) else {
        return Err(KernelError::InvalidHandle);
    };
    let owner_token = removed.owner_token;
    // Release this watch's contribution to the per-bit interest counts.
    adjust_interest(removed.mask, false);
    drop(watches);

    // Wake any readers parked on this watch's owner so they re-evaluate (e.g.
    // pick up a synthesized IN_IGNORED, or fall through to their remaining
    // watches) rather than sleeping against a now-removed watch.
    if owner_token != 0 {
        wake_notify_waiters(&[owner_token]);
    }

    crate::serial_println!("[notify] Watch {} closed", watch_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Path matching
// ---------------------------------------------------------------------------

/// Determine whether `candidate` (an absolute path from a VFS
/// operation) falls under a watch whose normalized directory path is
/// `watch_path` (always stored with a trailing `/`, see
/// [`create_watch`]).
///
/// Two cases match:
///
/// 1. **The watched path itself** — `candidate` equals `watch_path`
///    with its trailing slash stripped.  This surfaces "self" events
///    such as the watched directory (or a watched file) being deleted
///    or renamed.
/// 2. **A path inside the watched directory** — for a non-recursive
///    watch only direct children match; for a recursive watch any
///    descendant matches.
///
/// # Why a dedicated helper
///
/// The previous inline matcher tested
/// `candidate.as_bytes().get(watch_path.len()) == Some(&b'/')` to
/// confirm a separator boundary.  That was a bug: because `watch_path`
/// already ends in `/`, the byte at `watch_path.len()` is the first
/// character of the *child name*, never a separator — so no child
/// event ever matched and the notification system delivered only
/// self-events.  A `strip_prefix` against the slash-terminated
/// `watch_path` inherently guarantees the boundary, so no extra check
/// is needed.
fn path_matches(watch_path: &str, recursive: bool, candidate: &str) -> bool {
    // Case 1: the watched path itself (watch_path minus trailing '/').
    if let Some(bare) = watch_path.strip_suffix('/') {
        if candidate == bare {
            return true;
        }
    }
    // Case 2: inside the watched directory.  The trailing '/' on
    // `watch_path` makes a successful prefix match a guaranteed
    // separator boundary.
    if let Some(remainder) = candidate.strip_prefix(watch_path) {
        if remainder.is_empty() {
            // candidate == watch_path including its trailing slash;
            // treat as the directory itself.
            return true;
        }
        if recursive {
            return true;
        }
        // Non-recursive: only direct children (no further separator).
        return !remainder.contains('/');
    }
    false
}

// ---------------------------------------------------------------------------
// Event emission — called by VFS
// ---------------------------------------------------------------------------

/// Emit a filesystem change event.
///
/// Called by the VFS layer after a successful filesystem operation.
/// Checks all active watches and queues the event for matching ones.
///
/// This is on the hot path — must be fast when no watches exist.
///
/// The subject is reported as a regular file (`is_dir = false`).  Callers
/// operating on directories use [`emit_dir`] (or the `*_dir` convenience
/// wrappers) so the inotify adapter can OR in `IN_ISDIR`.
#[inline]
pub fn emit(event_type: FsEventType, path: &str, new_path: Option<&str>) {
    emit_inner(event_type, path, new_path, false);
}

/// Emit a filesystem change event whose subject is a **directory**.
///
/// Identical to [`emit`] but tags the queued [`FsEvent`] with `is_dir = true`
/// so the Linux-ABI inotify adapter ORs `IN_ISDIR` into the reported mask.
#[inline]
pub fn emit_dir(event_type: FsEventType, path: &str, new_path: Option<&str>) {
    emit_inner(event_type, path, new_path, true);
}

/// Core event-emission path shared by [`emit`] and [`emit_dir`].
///
/// This is on the hot path — must be fast when no watches exist.
#[allow(clippy::arithmetic_side_effects)]
fn emit_inner(event_type: FsEventType, path: &str, new_path: Option<&str>, is_dir: bool) {
    // Lock-free fast path: if no live watch is interested in this event type,
    // there is nothing to queue — skip without ever taking the `WATCHES` lock.
    // (Internal `Overflow` events have an empty mask and are never emitted this
    // way; they are surfaced via the per-watch `overflowed` flag.)
    if !interest_includes(event_type.to_mask()) {
        return;
    }

    let mut watches = WATCHES.lock();

    // Defensive: a watch could have closed between the interest check and the
    // lock; bail if the table is now empty.
    if watches.is_empty() {
        return;
    }

    // Owner tokens of watches that actually gained a new event this call —
    // their blocked readers are woken after the WATCHES lock is released
    // (leaf-lock order).
    let mut woke_tokens: Vec<u64> = Vec::new();

    // Check each watch for a matching path.
    for watch in watches.values_mut() {
        // Does this watch care about this event type?
        if !watch.mask.contains(event_type.to_mask()) {
            continue;
        }

        // Does the affected path (or, for renames, the destination
        // path) fall under this watch?
        let matched = path_matches(&watch.path, watch.recursive, path)
            || new_path
                .is_some_and(|np| path_matches(&watch.path, watch.recursive, np));

        if !matched {
            continue;
        }

        // Event coalescing: if an identical event (same type + path) is
        // already pending, skip the duplicate to avoid flooding a consumer
        // with repeated writes to the same file.
        let already_queued = watch.events.iter().any(|e| {
            e.event_type == event_type
                && e.path == path
                && e.new_path.as_deref() == new_path
        });
        if already_queued {
            continue;
        }

        // Queue the event.
        if watch.events.len() >= watch.max_events {
            // Queue full — drop oldest event and set overflow flag.
            watch.events.pop_front();
            watch.overflowed = true;
        }

        watch.events.push_back(FsEvent {
            watch_id: watch.id,
            event_type,
            path: String::from(path),
            new_path: new_path.map(String::from),
            is_dir,
        });
        if watch.owner_token != 0 {
            woke_tokens.push(watch.owner_token);
        }
    }

    // Release WATCHES before waking so the scheduler is never entered with the
    // notify lock held (and so a woken reader can immediately re-take it).
    drop(watches);
    if !woke_tokens.is_empty() {
        wake_notify_waiters(&woke_tokens);
    }
}

/// Convenience: emit a "created" event.
#[inline]
pub fn emit_created(path: &str) {
    emit(FsEventType::Created, path, None);
}

/// Convenience: emit a "created" event whose subject is a directory
/// (e.g. `mkdir`).  Surfaces as inotify `IN_CREATE | IN_ISDIR`.
#[inline]
pub fn emit_created_dir(path: &str) {
    emit_dir(FsEventType::Created, path, None);
}

/// Convenience: emit a "deleted" event.
#[inline]
pub fn emit_deleted(path: &str) {
    emit(FsEventType::Deleted, path, None);
}

/// Convenience: emit a "deleted" event whose subject is a directory
/// (e.g. `rmdir`).  Surfaces as inotify `IN_DELETE | IN_ISDIR`.
#[inline]
pub fn emit_deleted_dir(path: &str) {
    emit_dir(FsEventType::Deleted, path, None);
}

/// Convenience: emit a "modified" event.
#[inline]
pub fn emit_modified(path: &str) {
    emit(FsEventType::Modified, path, None);
}

/// Convenience: emit a "renamed" event.
#[inline]
pub fn emit_renamed(old_path: &str, new_path: &str) {
    emit(FsEventType::Renamed, old_path, Some(new_path));
}

/// Convenience: emit a "metadata changed" event.
///
/// Used for permission changes, ownership changes, attribute changes,
/// and xattr modifications — operations that affect file metadata but
/// not file content.
#[inline]
pub fn emit_metadata(path: &str) {
    emit(FsEventType::MetadataChanged, path, None);
}

/// Convenience: emit an "accessed" (read) event.
///
/// High-frequency and opt-in: emitted from the VFS read path only when a live
/// watch requests `ACCESS` (see the `INTEREST_COUNTS` gate). Surfaces as
/// inotify `IN_ACCESS`.
#[inline]
pub fn emit_accessed(path: &str) {
    emit(FsEventType::Accessed, path, None);
}

/// Convenience: emit an "opened" event.
///
/// Emitted from the file-handle open path. High-frequency and opt-in (gated by
/// the `OPEN` interest count). Surfaces as inotify `IN_OPEN`.
#[inline]
pub fn emit_opened(path: &str) {
    emit(FsEventType::Opened, path, None);
}

/// Convenience: emit a "closed" event.
///
/// `was_writable` selects between `ClosedWrite` (the handle was opened for
/// writing → inotify `IN_CLOSE_WRITE`) and `ClosedNoWrite` (read-only →
/// `IN_CLOSE_NOWRITE`). `is_dir` tags the event so the inotify adapter ORs in
/// `IN_ISDIR` for directory-handle closes. Emitted from the file-handle
/// final-close path; opt-in, gated by the matching interest count.
#[inline]
pub fn emit_closed(path: &str, was_writable: bool, is_dir: bool) {
    let ty = if was_writable {
        FsEventType::ClosedWrite
    } else {
        FsEventType::ClosedNoWrite
    };
    emit_inner(ty, path, None, is_dir);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the filesystem change notification system.
///
/// Creates a watch, emits events, reads them back, and verifies
/// correctness.  Tests overflow behavior and watch cleanup.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[notify] Running self-test...");

    // Regression guard for the slash-boundary matching bug: a watch on
    // a directory must match its direct children and itself, but not
    // grandchildren (unless recursive) or unrelated siblings.
    if !path_matches("/docs/", false, "/docs/file.txt")
        || !path_matches("/docs/", false, "/docs")
        || path_matches("/docs/", false, "/docs/sub/file.txt")
        || !path_matches("/docs/", true, "/docs/sub/file.txt")
        || path_matches("/docs/", false, "/docsx")
        || path_matches("/docs/", false, "/other")
        || !path_matches("/", false, "/file.txt")
        || path_matches("/", false, "/sub/file.txt")
    {
        crate::serial_println!("[notify]   FAIL: path_matches boundary logic wrong");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   path_matches boundary logic verified ✓");

    // Create a watch on the root directory.
    let watch_id = create_watch("/", FsEventMask::ALL_CHANGES, false)?;

    // No events yet.
    let events = read_events(watch_id, 10)?;
    if !events.is_empty() {
        crate::serial_println!("[notify]   FAIL: expected 0 events, got {}", events.len());
        close_watch(watch_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Empty queue verified ✓");

    // Emit some events.
    emit_created("/TEST.TXT");
    emit_modified("/TEST.TXT");
    emit_deleted("/TEST.TXT");

    // Read them back.
    let events = read_events(watch_id, 10)?;
    if events.len() != 3 {
        crate::serial_println!(
            "[notify]   FAIL: expected 3 events, got {}",
            events.len()
        );
        close_watch(watch_id)?;
        return Err(KernelError::InternalError);
    }

    // Verify event types.
    if events[0].event_type != FsEventType::Created
        || events[1].event_type != FsEventType::Modified
        || events[2].event_type != FsEventType::Deleted
    {
        crate::serial_println!("[notify]   FAIL: event types mismatch");
        close_watch(watch_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   3 events received in correct order ✓");

    // Verify paths.
    if events[0].path != "/TEST.TXT" {
        crate::serial_println!("[notify]   FAIL: event path is '{}'", events[0].path);
        close_watch(watch_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Event paths correct ✓");

    // Test non-recursive: events in subdirectories should NOT match.
    emit_created("/SUB/DEEP.TXT");
    let events = read_events(watch_id, 10)?;
    if !events.is_empty() {
        crate::serial_println!(
            "[notify]   FAIL: non-recursive watch got subdirectory event ({} events)",
            events.len()
        );
        close_watch(watch_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Non-recursive filtering verified ✓");

    // Test recursive watch.
    let rec_id = create_watch("/", FsEventMask::ALL_CHANGES, true)?;
    emit_created("/SUB/DEEP.TXT");
    let events = read_events(rec_id, 10)?;
    if events.len() != 1 {
        crate::serial_println!(
            "[notify]   FAIL: recursive watch expected 1 event, got {}",
            events.len()
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Recursive watch verified ✓");

    // Test rename event.
    emit_renamed("/OLD.TXT", "/NEW.TXT");
    let events = read_events(watch_id, 10)?;
    if events.len() != 1 || events[0].event_type != FsEventType::Renamed {
        crate::serial_println!("[notify]   FAIL: rename event not received");
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    if events[0].new_path.as_deref() != Some("/NEW.TXT") {
        crate::serial_println!(
            "[notify]   FAIL: rename new_path is {:?}",
            events[0].new_path
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Rename event with new_path verified ✓");

    // Test metadata changed event.
    emit_metadata("/TEST.TXT");
    let events = read_events(watch_id, 10)?;
    if events.len() != 1 || events[0].event_type != FsEventType::MetadataChanged {
        crate::serial_println!(
            "[notify]   FAIL: metadata event not received (got {} events)",
            events.len()
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Metadata changed event verified ✓");

    // Test event mask filtering: watch with only CREATE mask should
    // ignore METADATA events.
    let create_only_id = create_watch("/", FsEventMask::CREATE, false)?;
    emit_metadata("/TEST.TXT");
    let events = read_events(create_only_id, 10)?;
    if !events.is_empty() {
        crate::serial_println!(
            "[notify]   FAIL: CREATE-only watch got metadata event ({} events)",
            events.len()
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        close_watch(create_only_id)?;
        return Err(KernelError::InternalError);
    }
    close_watch(create_only_id)?;
    crate::serial_println!("[notify]   Event mask filtering verified ✓");

    // Test event coalescing: duplicate events should be suppressed.
    emit_modified("/COALESCE.TXT");
    emit_modified("/COALESCE.TXT");
    emit_modified("/COALESCE.TXT");
    let events = read_events(watch_id, 10)?;
    if events.len() != 1 {
        crate::serial_println!(
            "[notify]   FAIL: coalescing expected 1 event, got {}",
            events.len()
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Event coalescing verified ✓");

    // Different event types on the same path should NOT coalesce.
    emit_created("/MULTI.TXT");
    emit_modified("/MULTI.TXT");
    emit_deleted("/MULTI.TXT");
    let events = read_events(watch_id, 10)?;
    if events.len() != 3 {
        crate::serial_println!(
            "[notify]   FAIL: different types expected 3, got {}",
            events.len()
        );
        close_watch(watch_id)?;
        close_watch(rec_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Different types not coalesced ✓");

    // Test overflow: create a small-capacity watch and flood it.
    close_watch(watch_id)?;
    close_watch(rec_id)?;

    let overflow_id = create_watch("/", FsEventMask::ALL_CHANGES, false)?;
    // Push DEFAULT_MAX_EVENTS + extra unique events to trigger overflow.
    for i in 0..DEFAULT_MAX_EVENTS + 10 {
        let p = alloc::format!("/OVF_{}", i);
        emit_created(&p);
    }
    let events = read_events(overflow_id, DEFAULT_MAX_EVENTS + 20)?;
    // First event should be Overflow indicator.
    if events.is_empty() || events[0].event_type != FsEventType::Overflow {
        crate::serial_println!(
            "[notify]   FAIL: overflow event not first (got {} events, first type={:?})",
            events.len(),
            events.first().map(|e| e.event_type),
        );
        close_watch(overflow_id)?;
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[notify]   Overflow detection verified ({} events after overflow) ✓",
        events.len()
    );
    close_watch(overflow_id)?;

    // Verify closed watch returns error.
    match read_events(overflow_id, 10) {
        Err(KernelError::InvalidHandle) => {}
        _ => {
            crate::serial_println!("[notify]   FAIL: closed watch didn't return InvalidHandle");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[notify]   Watch cleanup verified ✓");

    // --- End-to-end VFS/handle hooks (TD17) --------------------------------
    //
    // The FS-independent interest-gate / synthetic-emit / mask-filtering checks
    // for the opt-in events live in `interest_gate_self_test()` (run
    // unconditionally at boot via `ipc::inotify::self_test`). Here we verify the
    // actual VFS and file-handle wiring emits those events, which needs a
    // mounted filesystem.

    // ACCESS: a real `Vfs::read_file` must surface an Accessed event when an
    // ACCESS watch is live. Write a probe, read it, assert the event surfaces.
    let access_id = create_watch("/", FsEventMask::ACCESS, false)?;
    let probe_path = "/ACCESS_PROBE.TXT";
    super::vfs::Vfs::write_file(probe_path, b"hi")?;
    let _ = super::vfs::Vfs::read_file(probe_path)?;
    let e2e = read_events(access_id, 10)?;
    let saw_probe = e2e
        .iter()
        .any(|e| e.event_type == FsEventType::Accessed && e.path == probe_path);
    if !saw_probe {
        crate::serial_println!(
            "[notify]   FAIL: Vfs::read_file did not surface Accessed for {} ({} events)",
            probe_path,
            e2e.len()
        );
        close_watch(access_id)?;
        let _ = super::vfs::Vfs::remove(probe_path);
        return Err(KernelError::InternalError);
    }
    close_watch(access_id)?;
    let _ = super::vfs::Vfs::remove(probe_path);
    crate::serial_println!("[notify]   End-to-end Vfs::read_file ACCESS hook verified ✓");

    // --- IN_OPEN / IN_CLOSE_* coverage (TD17) ------------------------------
    //
    // File-handle open/close surface Opened / ClosedWrite / ClosedNoWrite,
    // gated (like ACCESS) on the interest counts so the open/close path pays
    // nothing unless a watch asks for them. Drive a real open/close through the
    // handle layer and assert the events surface with the right write-mode
    // discrimination. (CREATE/MODIFY from write_file are filtered out by the
    // open/close-only watch mask, so they add no noise.)
    let oc_mask = FsEventMask(
        FsEventMask::OPEN.0 | FsEventMask::CLOSE_WRITE.0 | FsEventMask::CLOSE_NOWRITE.0,
    );
    let oc_id = create_watch("/", oc_mask, false)?;
    let oc_probe = "/OPENCLOSE_PROBE.TXT";
    super::vfs::Vfs::write_file(oc_probe, b"x")?;

    // Read-only open then close → Opened + ClosedNoWrite (never ClosedWrite).
    let h_ro = super::handle::open(oc_probe, super::handle::OpenFlags::READ)?;
    super::handle::close(h_ro)?;
    let ro_events = read_events(oc_id, 10)?;
    let saw_open = ro_events
        .iter()
        .any(|e| e.event_type == FsEventType::Opened && e.path == oc_probe);
    let saw_close_nw = ro_events
        .iter()
        .any(|e| e.event_type == FsEventType::ClosedNoWrite && e.path == oc_probe);
    let bad_close_w = ro_events
        .iter()
        .any(|e| e.event_type == FsEventType::ClosedWrite);
    if !saw_open || !saw_close_nw || bad_close_w {
        crate::serial_println!(
            "[notify]   FAIL: read-only open/close (open={}, close_nw={}, bad_close_w={})",
            saw_open,
            saw_close_nw,
            bad_close_w
        );
        close_watch(oc_id)?;
        let _ = super::vfs::Vfs::remove(oc_probe);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   read-only open→Opened, close→ClosedNoWrite ✓");

    // Writable open then close → Opened + ClosedWrite.
    let h_rw = super::handle::open(
        oc_probe,
        super::handle::OpenFlags::READ.union(super::handle::OpenFlags::WRITE),
    )?;
    super::handle::close(h_rw)?;
    let rw_events = read_events(oc_id, 10)?;
    let saw_open_rw = rw_events
        .iter()
        .any(|e| e.event_type == FsEventType::Opened && e.path == oc_probe);
    let saw_close_w = rw_events
        .iter()
        .any(|e| e.event_type == FsEventType::ClosedWrite && e.path == oc_probe);
    if !saw_open_rw || !saw_close_w {
        crate::serial_println!(
            "[notify]   FAIL: writable open/close (open={}, close_w={})",
            saw_open_rw,
            saw_close_w
        );
        close_watch(oc_id)?;
        let _ = super::vfs::Vfs::remove(oc_probe);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   writable open→Opened, close→ClosedWrite ✓");

    close_watch(oc_id)?;
    let _ = super::vfs::Vfs::remove(oc_probe);

    waiter_registry_self_test()?;

    crate::serial_println!("[notify] Self-test passed.");
    Ok(())
}

/// FS-independent coverage for the opt-in event-bit interest gate (TD17).
///
/// Exercises the per-event-bit [`INTEREST_COUNTS`] gate, synthetic emit/read,
/// and mask filtering for the four high-frequency opt-in events (`ACCESS`,
/// `OPEN`, `CLOSE_WRITE`, `CLOSE_NOWRITE`) without touching a real filesystem,
/// so it runs **unconditionally** at boot (driven from
/// [`crate::ipc::inotify::self_test`]) — unlike the FAT-gated [`self_test`],
/// whose end-to-end hooks need a mounted filesystem. All assertions are
/// delta-correct against a captured baseline, so a concurrent unrelated watch
/// of the same bit cannot make them flake.
pub fn interest_gate_self_test() -> KernelResult<()> {
    // (interest bit, the event type that sets it).
    let cases: [(FsEventMask, FsEventType); 4] = [
        (FsEventMask::ACCESS, FsEventType::Accessed),
        (FsEventMask::OPEN, FsEventType::Opened),
        (FsEventMask::CLOSE_WRITE, FsEventType::ClosedWrite),
        (FsEventMask::CLOSE_NOWRITE, FsEventType::ClosedNoWrite),
    ];
    for (bit, ty) in cases {
        let baseline = interest_includes(bit);

        // Create → gate must report interest.
        let wid = create_watch("/", bit, false)?;
        if !interest_includes(bit) {
            crate::serial_println!("[notify]   FAIL: interest gate not set for {:?}", ty);
            close_watch(wid)?;
            return Err(KernelError::InternalError);
        }

        // Synthetic emit must reach the watch as exactly one event of that type.
        emit(ty, "/GATE_PROBE", None);
        let events = read_events(wid, 10)?;
        if events.len() != 1 || events.first().map(|e| e.event_type) != Some(ty) {
            crate::serial_println!(
                "[notify]   FAIL: synthetic {:?} not received ({} events)",
                ty,
                events.len()
            );
            close_watch(wid)?;
            return Err(KernelError::InternalError);
        }

        // A CREATE-only watch must NOT see it (mask filtering).
        let other = create_watch("/", FsEventMask::CREATE, false)?;
        emit(ty, "/GATE_PROBE", None);
        let leaked = read_events(other, 10)?;
        if !leaked.is_empty() {
            crate::serial_println!("[notify]   FAIL: CREATE-only watch leaked {:?}", ty);
            close_watch(wid)?;
            close_watch(other)?;
            return Err(KernelError::InternalError);
        }
        let _ = read_events(wid, 10)?; // drain the matching watch's copy.
        close_watch(other)?;

        // Close → gate must return to baseline.
        close_watch(wid)?;
        if interest_includes(bit) != baseline {
            crate::serial_println!("[notify]   FAIL: interest gate not cleared for {:?}", ty);
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[notify]   opt-in interest gate (ACCESS/OPEN/CLOSE_*) verified ✓");
    Ok(())
}

/// Pure partition-logic test for the blocking-read waiter registry
/// (register / take / deregister), exercised without touching the scheduler.
///
/// Split out so it can also be driven from an *unconditional* boot self-test
/// (the main [`self_test`] is gated behind a mounted FAT filesystem); the
/// owner-token wake path otherwise only runs when a real task blocks.
pub fn waiter_registry_self_test() -> KernelResult<()> {
    // Use owner tokens well outside any real instance id range so a concurrent
    // live waiter can never collide with the synthetic ones used here.
    let (w1, w2) = (0xF0F1_0001u64, 0xF0F1_0002u64);
    let (t_a, t_b): (TaskId, TaskId) = (0xA1, 0xB2);
    register_notify_waiter(w1, t_a);
    register_notify_waiter(w1, t_a); // idempotent
    register_notify_waiter(w1, t_b);
    register_notify_waiter(w2, t_a);
    // take w1 → both tasks, once.
    let mut taken = take_notify_waiters(w1);
    taken.sort_unstable();
    if taken != alloc::vec![t_a, t_b] {
        crate::serial_println!("[notify]   FAIL: waiter take returned {:?}", taken);
        return Err(KernelError::InternalError);
    }
    if !take_notify_waiters(w1).is_empty() {
        crate::serial_println!("[notify]   FAIL: waiters taken twice");
        return Err(KernelError::InternalError);
    }
    // deregister t_a from w2 empties it; deregister of an unknown is a no-op.
    deregister_notify_waiter(w2, t_b);
    deregister_notify_waiter(w2, t_a);
    if !take_notify_waiters(w2).is_empty() {
        crate::serial_println!("[notify]   FAIL: registry not empty after deregister");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[notify]   Blocking-read waiter registry verified ✓");
    Ok(())
}
