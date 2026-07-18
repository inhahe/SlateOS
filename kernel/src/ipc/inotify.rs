//! inotify — Linux-compatible filesystem-watch instance objects.
//!
//! An inotify instance is a kernel object that lets a process **watch
//! files and directories for changes via a file descriptor**.  A Linux
//! process creates one with `inotify_init(2)` / `inotify_init1(2)`,
//! registers per-path watches with `inotify_add_watch(2)` (each returns a
//! *watch descriptor*, `wd`), and then `read(2)`s a stream of variable-length
//! `struct inotify_event` records describing the changes.  The fd is also
//! pollable: it becomes readable exactly when at least one event is queued.
//!
//! ## Relationship to the native notify subsystem
//!
//! This module is a thin **Linux-ABI adapter** over the kernel's native
//! filesystem change-notification subsystem ([`crate::fs::notify`]).  The
//! native layer already implements path-based watches, an asynchronous
//! bounded event queue per watch, coalescing, and overflow tracking.  Each
//! inotify watch owns one native watch; this module:
//!
//! - translates the Linux inotify interest mask (`IN_*`) into the native
//!   [`FsEventMask`](crate::fs::notify::FsEventMask) at `add_watch` time, and
//! - translates native [`FsEvent`](crate::fs::notify::FsEvent)s back into
//!   `inotify_event` records (`wd` / `mask` / `cookie` / `name`) at read time.
//!
//! Keeping the C-struct *serialization* in the syscall layer
//! (`crate::syscall::linux`) — and the watch-table bookkeeping here — mirrors
//! how the rest of the anon_inode fd family (eventfd / epoll / signalfd /
//! timerfd) is split.
//!
//! ## Watch descriptors and path identity
//!
//! Linux keys watches by the underlying inode, so a second `add_watch` on the
//! same object returns the *same* `wd` (merging or replacing the mask).  We do
//! not have a stable inode identity available at this layer, so we key watches
//! by their **normalized path string** instead.  Re-adding a watch for a path
//! that is already watched returns the existing `wd` and updates its mask
//! (honoring `IN_MASK_ADD`, which ORs rather than replaces).  This is
//! behaviorally identical for the common case (one path → one watch) and only
//! differs from Linux if two distinct paths resolve to the same inode (hard
//! links / bind mounts), which our path-based native layer does not deduplicate
//! either.
//!
//! ## Refcounting and `fork`
//!
//! Like the rest of the family, an inotify instance is reference counted:
//! `create()` starts the count at 1, `dup()` bumps it (used when `fork`
//! duplicates an inheriting fd), and `close()` drops one reference — only the
//! final `close()` (count → 0) tears the instance down, releasing every native
//! watch it still owns.
//!
//! ## Lock ordering
//!
//! `INOTIFY_TABLE` is held across calls into [`crate::fs::notify`] (whose
//! `WATCHES` lock is itself a leaf).  The ordering is therefore
//! `INOTIFY_TABLE` → `notify::WATCHES`, and no path takes them in the reverse
//! order, so there is no cycle.

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::notify::{self, FsEventMask, FsEventType};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Linux inotify mask bits (include/uapi/linux/inotify.h)
// ---------------------------------------------------------------------------

/// File was accessed (read).
pub const IN_ACCESS: u32 = 0x0000_0001;
/// File was modified.
pub const IN_MODIFY: u32 = 0x0000_0002;
/// Metadata changed (permissions, timestamps, ...).
pub const IN_ATTRIB: u32 = 0x0000_0004;
// The close/open/isdir/excl bits below cannot be produced by the native
// fs::notify layer (it has no open/close hooks and FsEvent carries no
// dir flag), and IN_ONESHOT/IN_DONT_FOLLOW/IN_EXCL_UNLINK control bits are
// accepted-but-ignored.  They are defined here to document the complete
// inotify flag ABI and so callers can name them; allow(dead_code) keeps
// the build warning-clean until a future native hook wires them.
/// Writable file closed.
#[allow(dead_code)]
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// Unwritable file closed.
#[allow(dead_code)]
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// File was opened.
pub const IN_OPEN: u32 = 0x0000_0020;
/// File moved out of a watched directory.
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// File moved into a watched directory.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// Subfile was created.
pub const IN_CREATE: u32 = 0x0000_0100;
/// Subfile was deleted.
pub const IN_DELETE: u32 = 0x0000_0200;
/// Watched file/directory itself was deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// Watched file/directory itself was moved.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;

/// Event queue overflowed (reported with `wd == -1`).
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// Watch was removed (explicitly via `inotify_rm_watch`, or automatically).
pub const IN_IGNORED: u32 = 0x0000_8000;
/// Subject of the event is a directory.
#[allow(dead_code)]
pub const IN_ISDIR: u32 = 0x4000_0000;

/// Both move directions (convenience).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// Both close kinds (convenience).
#[allow(dead_code)]
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;

/// `add_watch` control bit: OR the new mask into an existing watch rather
/// than replacing it.
pub const IN_MASK_ADD: u32 = 0x2000_0000;
/// `add_watch` control bit: fail with `EEXIST` if the watch already exists.
pub const IN_MASK_CREATE: u32 = 0x1000_0000;
/// `add_watch` control bit: only watch the path if it is a directory.
pub const IN_ONLYDIR: u32 = 0x0100_0000;
/// `add_watch` control bit: deliver one event then auto-remove the watch.
#[allow(dead_code)]
pub const IN_ONESHOT: u32 = 0x8000_0000;
/// `add_watch` control bit: do not follow a terminal symlink.
#[allow(dead_code)]
pub const IN_DONT_FOLLOW: u32 = 0x0200_0000;
/// `add_watch` control bit: stop watching once all hard links are gone.
#[allow(dead_code)]
pub const IN_EXCL_UNLINK: u32 = 0x0400_0000;

/// The set of *event* bits (low 16 minus the overflow/ignored flags) that a
/// caller may legally request and that we can report from native events.
const REPORTABLE_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_CREATE
    | IN_DELETE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

// ---------------------------------------------------------------------------
// Mask translation
// ---------------------------------------------------------------------------

/// Translate a Linux inotify interest mask into the native event mask.
///
/// `IN_ISDIR` is an *output-only* flag in the inotify ABI — it is OR'd into a
/// reported event's mask when the subject is a directory, never something a
/// caller sets in an add-watch interest mask.  It therefore maps to no native
/// interest bit here (an empty mask); the native `FsEvent::is_dir` flag drives
/// the OR-in at read time (see `refill`).  `IN_OPEN` / `IN_CLOSE_WRITE` /
/// `IN_CLOSE_NOWRITE` are observable via the file-handle open/close hooks.
#[must_use]
pub fn to_native_mask(in_mask: u32) -> FsEventMask {
    let mut bits = 0u32;
    if in_mask & IN_ACCESS != 0 {
        bits |= FsEventMask::ACCESS.0;
    }
    if in_mask & IN_MODIFY != 0 {
        bits |= FsEventMask::MODIFY.0;
    }
    if in_mask & IN_ATTRIB != 0 {
        bits |= FsEventMask::METADATA.0;
    }
    if in_mask & IN_OPEN != 0 {
        bits |= FsEventMask::OPEN.0;
    }
    if in_mask & IN_CLOSE_WRITE != 0 {
        bits |= FsEventMask::CLOSE_WRITE.0;
    }
    if in_mask & IN_CLOSE_NOWRITE != 0 {
        bits |= FsEventMask::CLOSE_NOWRITE.0;
    }
    if in_mask & (IN_CREATE) != 0 {
        bits |= FsEventMask::CREATE.0;
    }
    if in_mask & (IN_DELETE | IN_DELETE_SELF) != 0 {
        bits |= FsEventMask::DELETE.0;
    }
    if in_mask & (IN_MOVED_FROM | IN_MOVED_TO | IN_MOVE_SELF) != 0 {
        bits |= FsEventMask::RENAME.0;
    }
    FsEventMask(bits)
}

/// Translate a native event type into the inotify mask bit it surfaces as.
///
/// Returns 0 for event types that have no inotify representation.
#[must_use]
const fn native_type_to_in_bit(t: FsEventType) -> u32 {
    match t {
        FsEventType::Created => IN_CREATE,
        FsEventType::Deleted => IN_DELETE,
        FsEventType::Modified => IN_MODIFY,
        // Renamed is special-cased in `refill` (it can produce a MOVED_FROM
        // and/or MOVED_TO pair); this default is only a fallback.
        FsEventType::Renamed => IN_MOVED_TO,
        FsEventType::MetadataChanged => IN_ATTRIB,
        FsEventType::Accessed => IN_ACCESS,
        FsEventType::Opened => IN_OPEN,
        FsEventType::ClosedWrite => IN_CLOSE_WRITE,
        FsEventType::ClosedNoWrite => IN_CLOSE_NOWRITE,
        FsEventType::Overflow => IN_Q_OVERFLOW,
    }
}

// ---------------------------------------------------------------------------
// Output event record (pre-serialization)
// ---------------------------------------------------------------------------

/// One inotify event in pre-serialized form.
///
/// The syscall layer turns this into the on-the-wire `struct inotify_event`
/// (16-byte header + null-padded name).  `name` holds the bare basename bytes
/// with **no** trailing null and **no** padding; the serializer adds those.
#[derive(Debug, Clone)]
pub struct InotifyEventOut {
    /// Watch descriptor that produced the event (`-1` for queue overflow).
    pub wd: i32,
    /// Event mask (`IN_*` bits).
    pub mask: u32,
    /// Rename-association cookie (0 when not part of a move pair).
    pub cookie: u32,
    /// Basename bytes relative to the watched directory (may be empty).
    pub name: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for an inotify instance (the handle IS the ID).
type InotifyId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_INOTIFY_ID: AtomicU64 = AtomicU64::new(1);

/// Monotonic move-cookie generator.  Starts at 1 so 0 means "no cookie".
static NEXT_COOKIE: AtomicU64 = AtomicU64::new(1);

fn alloc_inotify_id() -> InotifyId {
    NEXT_INOTIFY_ID.fetch_add(1, Ordering::Relaxed)
}

fn alloc_cookie() -> u32 {
    // Wrap within u32; 0 is reserved for "no cookie" so skip it on wrap.
    let raw = NEXT_COOKIE.fetch_add(1, Ordering::Relaxed);
    #[allow(clippy::cast_possible_truncation)]
    let c = raw as u32;
    if c == 0 { 1 } else { c }
}

/// A handle to an inotify instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::Inotify` variant); the syscall layer reconstructs it with
/// [`InotifyHandle::from_raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InotifyHandle(u64);

impl InotifyHandle {
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

    const fn id(self) -> InotifyId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// One registered watch within an inotify instance.
struct Watch {
    /// Backing native watch id, or 0 if the requested mask mapped to no
    /// observable native events (the watch is still tracked so its `wd`
    /// stays valid and `rm_watch` works).
    native_id: u64,
    /// The Linux interest mask as last set (used to honor `IN_MASK_ADD` and
    /// to filter which native events to report, e.g. directionally for moves).
    in_mask: u32,
    /// Normalized watched path (always ends in `/`, matching the native
    /// layer's stored form), used to compute event basenames.
    path_norm: String,
}

/// A kernel inotify instance: a watch table, a pending-event buffer, and a
/// reference count.
struct Inotify {
    /// `wd → Watch`.
    watches: BTreeMap<i32, Watch>,
    /// Next watch descriptor to hand out.  inotify wds are positive and
    /// monotonically increasing per instance (Linux uses an idr; we use a
    /// simple counter, which never reuses a wd within an instance's life —
    /// matching the practical guarantee programs rely on).
    next_wd: i32,
    /// Events drained from native watches but not yet copied to userspace
    /// (a `read` with a small buffer leaves the remainder here).
    pending: VecDeque<InotifyEventOut>,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
}

impl Inotify {
    const fn new() -> Self {
        Self {
            watches: BTreeMap::new(),
            next_wd: 1,
            pending: VecDeque::new(),
            refcount: 1,
        }
    }

    /// Compute the event basename for `abs_path` relative to a watch whose
    /// normalized (trailing-slash) directory path is `path_norm`.
    ///
    /// Returns an empty vec for "self" events (the watched path itself),
    /// matching inotify's convention of `len == 0` for events on the watch
    /// target rather than a child.
    fn basename_for(path_norm: &str, abs_path: &str) -> Vec<u8> {
        // Self event: abs_path equals the watch path with the slash stripped.
        if let Some(bare) = path_norm.strip_suffix('/') {
            if abs_path == bare {
                return Vec::new();
            }
        }
        if let Some(rem) = abs_path.strip_prefix(path_norm) {
            if rem.is_empty() {
                return Vec::new();
            }
            // inotify names are always a single path component (the immediate
            // child of the watched directory), never a multi-level path.  Our
            // native watches are non-recursive, so `rem` is normally already a
            // single component; guard against a deeper path slipping through by
            // taking only the first component so the emitted name can never
            // contain an embedded '/' (which would corrupt the record stream).
            let first = rem.split('/').next().unwrap_or(rem);
            if first.is_empty() {
                return Vec::new();
            }
            return first.as_bytes().to_vec();
        }
        // Path did not fall under the watch (shouldn't happen — native
        // already filtered) — fall back to the whole path's final component.
        let tail = abs_path.rsplit('/').next().unwrap_or(abs_path);
        tail.as_bytes().to_vec()
    }

    /// Drain all native watches into `self.pending`, translating each native
    /// event into one or two inotify records.
    fn refill(&mut self) {
        for (&wd, watch) in &self.watches {
            if watch.native_id == 0 {
                continue;
            }
            let events = match notify::read_events(watch.native_id, usize::MAX) {
                Ok(ev) => ev,
                Err(_) => continue,
            };
            for ev in events {
                // inotify ORs IN_ISDIR into the event mask whenever the
                // subject is a directory (mkdir/rmdir, directory-handle close,
                // a renamed subdirectory, ...).  The native FsEvent carries
                // this as a dedicated flag so we never have to re-stat.
                let isdir_bit = if ev.is_dir { IN_ISDIR } else { 0 };
                match ev.event_type {
                    FsEventType::Overflow => {
                        // IN_Q_OVERFLOW is a synthetic wd=-1 event and never
                        // carries IN_ISDIR.
                        self.pending.push_back(InotifyEventOut {
                            wd: -1,
                            mask: IN_Q_OVERFLOW,
                            cookie: 0,
                            name: Vec::new(),
                        });
                    }
                    FsEventType::Renamed => {
                        // A native rename carries both the old and the new
                        // path.  Emit a MOVED_FROM for the old name and a
                        // MOVED_TO for the new name when each falls under the
                        // watch, sharing one cookie so a consumer can pair
                        // them.  inotify only includes the requested
                        // directions, so respect the watch's interest mask.
                        let cookie = alloc_cookie();
                        let old_name = Self::basename_for(&watch.path_norm, &ev.path);
                        if watch.in_mask & IN_MOVED_FROM != 0 {
                            self.pending.push_back(InotifyEventOut {
                                wd,
                                mask: IN_MOVED_FROM | isdir_bit,
                                cookie,
                                name: old_name,
                            });
                        }
                        if let Some(np) = ev.new_path.as_deref() {
                            if watch.in_mask & IN_MOVED_TO != 0 {
                                let new_name = Self::basename_for(&watch.path_norm, np);
                                self.pending.push_back(InotifyEventOut {
                                    wd,
                                    mask: IN_MOVED_TO | isdir_bit,
                                    cookie,
                                    name: new_name,
                                });
                            }
                        }
                    }
                    other => {
                        let bit = native_type_to_in_bit(other);
                        // Only surface bits the watch actually asked for.
                        if watch.in_mask & bit == 0 {
                            continue;
                        }
                        let name = Self::basename_for(&watch.path_norm, &ev.path);
                        self.pending.push_back(InotifyEventOut {
                            wd,
                            mask: bit | isdir_bit,
                            cookie: 0,
                            name,
                        });
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live inotify instances, keyed by ID.
static INOTIFY_TABLE: Mutex<BTreeMap<InotifyId, Inotify>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new, empty inotify instance.
///
/// The returned handle owns one reference; the caller must `close()` it
/// (directly or via process-exit cleanup) exactly once.
#[must_use]
pub fn create() -> InotifyHandle {
    let id = alloc_inotify_id();
    INOTIFY_TABLE.lock().insert(id, Inotify::new());
    InotifyHandle(id)
}

/// Add one reference to an instance, returning the same handle.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists or the
/// reference count would overflow `u32::MAX`.
pub fn dup(handle: InotifyHandle) -> KernelResult<InotifyHandle> {
    let mut table = INOTIFY_TABLE.lock();
    let ino = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    ino.refcount = ino.refcount.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to an instance.
///
/// Only the final `close()` (refcount → 0) removes the instance, releasing
/// every native watch it still owns.  A double-close is harmless.
pub fn close(handle: InotifyHandle) {
    let mut table = INOTIFY_TABLE.lock();
    let teardown = if let Some(ino) = table.get_mut(&handle.id()) {
        ino.refcount = ino.refcount.saturating_sub(1);
        ino.refcount == 0
    } else {
        false
    };
    if teardown {
        if let Some(ino) = table.remove(&handle.id()) {
            // Release backing native watches outside nothing-special — we
            // still hold INOTIFY_TABLE, but notify::close_watch only locks
            // its own WATCHES (the established lock order).
            for w in ino.watches.values() {
                if w.native_id != 0 {
                    let _ = notify::close_watch(w.native_id);
                }
            }
        }
    }
}

/// Does this handle refer to a live inotify instance?
#[must_use]
pub fn exists(handle: InotifyHandle) -> bool {
    INOTIFY_TABLE.lock().contains_key(&handle.id())
}

// ---------------------------------------------------------------------------
// Watch API
// ---------------------------------------------------------------------------

/// Register (or update) a watch on `path` with the given inotify mask.
///
/// `path` must already be normalized to the form the native layer expects
/// (an absolute path; a trailing slash is added internally for matching).
///
/// Returns the watch descriptor.  If a watch for the same path already
/// exists, its mask is updated (ORed when `IN_MASK_ADD` is set, replaced
/// otherwise) and the existing `wd` is returned — unless `IN_MASK_CREATE`
/// was requested, in which case [`KernelError::AlreadyExists`] is returned.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is dead.
/// - [`KernelError::AlreadyExists`] if `IN_MASK_CREATE` is set and the path is
///   already watched.
/// - [`KernelError::OutOfMemory`] if the native watch table is full.
pub fn add_watch(handle: InotifyHandle, path: &str, in_mask: u32) -> KernelResult<i32> {
    // Normalize to trailing-slash form for identity + basename math.
    let mut norm = String::from(path);
    if !norm.ends_with('/') {
        norm.push('/');
    }

    let mut table = INOTIFY_TABLE.lock();
    let ino = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;

    // Existing watch for this path?
    let existing = ino
        .watches
        .iter()
        .find(|(_, w)| w.path_norm == norm)
        .map(|(&wd, _)| wd);

    if let Some(wd) = existing {
        if in_mask & IN_MASK_CREATE != 0 {
            return Err(KernelError::AlreadyExists);
        }
        // Compute the merged/replaced mask.
        let cur = ino.watches.get(&wd).map_or(0, |w| w.in_mask);
        let new_mask = if in_mask & IN_MASK_ADD != 0 {
            cur | (in_mask & REPORTABLE_EVENTS)
        } else {
            in_mask & REPORTABLE_EVENTS
        };
        rebind_native(ino, handle.id(), wd, &norm, new_mask)?;
        return Ok(wd);
    }

    // Brand-new watch.
    let wd = ino.next_wd;
    // Allocate next wd defensively (saturate rather than wrap to a negative).
    ino.next_wd = ino.next_wd.saturating_add(1);

    let effective = in_mask & REPORTABLE_EVENTS;
    let native_id = create_native(&norm, effective, handle.id())?;
    ino.watches.insert(wd, Watch {
        native_id,
        in_mask: in_mask & REPORTABLE_EVENTS,
        path_norm: norm,
    });
    Ok(wd)
}

/// Create a native watch for `norm`/`effective`, returning 0 when the mask
/// has no observable native events (a tracked-but-inert watch).
///
/// `owner_token` is the owning inotify instance id, registered on the native
/// watch so a blocked `read()` on this instance is woken when the watch fires.
fn create_native(norm: &str, effective: u32, owner_token: u64) -> KernelResult<u64> {
    let native_mask = to_native_mask(effective);
    if native_mask.0 == 0 {
        return Ok(0);
    }
    notify::create_watch_owned(norm, native_mask, false, owner_token).map_err(|e| match e {
        KernelError::OutOfMemory => KernelError::OutOfMemory,
        _ => KernelError::InvalidArgument,
    })
}

/// Update an existing watch's native backing to reflect `new_mask`.
///
/// The native layer has no in-place mask update, so we tear down the old
/// native watch (discarding any queued-but-undrained events for it — the same
/// thing Linux's IN_MASK_ADD semantics tolerate) and create a fresh one.
fn rebind_native(
    ino: &mut Inotify,
    owner_token: u64,
    wd: i32,
    norm: &str,
    new_mask: u32,
) -> KernelResult<()> {
    // Create the replacement first so a failure leaves the old watch intact.
    let new_native = create_native(norm, new_mask, owner_token)?;
    if let Some(w) = ino.watches.get_mut(&wd) {
        if w.native_id != 0 {
            let _ = notify::close_watch(w.native_id);
        }
        w.native_id = new_native;
        w.in_mask = new_mask;
    } else if new_native != 0 {
        // Watch vanished between lookup and here (can't happen under the lock,
        // but be defensive): don't leak the freshly created native watch.
        let _ = notify::close_watch(new_native);
    }
    Ok(())
}

/// Remove a watch by descriptor.
///
/// On success the caller should deliver an `IN_IGNORED` event for `wd` to the
/// reader (Linux does); whether to do so is left to the syscall layer.
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is dead.
/// - [`KernelError::InvalidArgument`] if `wd` is not a live watch (maps to
///   `EINVAL`, matching Linux's `inotify_rm_watch` on a bad wd).
pub fn rm_watch(handle: InotifyHandle, wd: i32) -> KernelResult<()> {
    let mut table = INOTIFY_TABLE.lock();
    let ino = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    match ino.watches.remove(&wd) {
        Some(w) => {
            if w.native_id != 0 {
                let _ = notify::close_watch(w.native_id);
            }
            // Synthesize the IN_IGNORED notification inotify delivers when a
            // watch is removed, so a reader learns the wd is now invalid.
            ino.pending.push_back(InotifyEventOut {
                wd,
                mask: IN_IGNORED,
                cookie: 0,
                name: Vec::new(),
            });
            Ok(())
        }
        None => Err(KernelError::InvalidArgument),
    }
}

// ---------------------------------------------------------------------------
// Read / poll API
// ---------------------------------------------------------------------------

/// Serialized size of one `inotify_event` for a given basename length.
///
/// Layout: 16-byte fixed header + `len` name bytes, where `len` is the name
/// length **including a trailing null, rounded up to a multiple of 16**
/// (`sizeof(struct inotify_event)`).  A zero-length name contributes no name
/// bytes (`len == 0`), matching the kernel.
#[must_use]
pub fn record_name_len(name_len: usize) -> usize {
    if name_len == 0 {
        0
    } else {
        // roundup(name_len + 1, 16)
        let needed = name_len.saturating_add(1);
        needed.saturating_add(15) & !15usize
    }
}

/// Total wire size of one event record (header + padded name).
#[must_use]
pub fn record_size(name_len: usize) -> usize {
    16usize.saturating_add(record_name_len(name_len))
}

/// Is the instance readable (has at least one event available)?
///
/// Performs a refill (draining native watch queues into the instance's
/// pending buffer) and reports whether anything is queued.  Used by
/// `poll`/`select`/`epoll`.
#[must_use]
pub fn is_readable(handle: InotifyHandle) -> bool {
    let mut table = INOTIFY_TABLE.lock();
    let Some(ino) = table.get_mut(&handle.id()) else {
        return false;
    };
    ino.refill();
    !ino.pending.is_empty()
}

/// Drain as many whole events as fit into `budget` bytes, removing them from
/// the instance.
///
/// Returns `Ok(events)` with the events to serialize (possibly empty if no
/// events are pending).  Returns `Err(KernelError::InvalidArgument)` —
/// mapped to `EINVAL` by the caller — if at least one event is pending but
/// the buffer is too small to hold even the first one (Linux's behavior).
///
/// # Errors
///
/// - [`KernelError::InvalidHandle`] if `handle` is dead.
/// - [`KernelError::InvalidArgument`] if `budget` cannot hold the first event.
pub fn read_into(handle: InotifyHandle, budget: usize) -> KernelResult<Vec<InotifyEventOut>> {
    let mut table = INOTIFY_TABLE.lock();
    let ino = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    ino.refill();

    // Buffer too small for the first event → EINVAL (do not consume it).
    // Also handles the empty-queue case: no front ⇒ nothing to return.
    let Some(first) = ino.pending.front() else {
        return Ok(Vec::new());
    };
    let first_size = record_size(first.name.len());
    if first_size > budget {
        return Err(KernelError::InvalidArgument);
    }

    let mut out = Vec::new();
    let mut used = 0usize;
    while let Some(front) = ino.pending.front() {
        let sz = record_size(front.name.len());
        if used.saturating_add(sz) > budget {
            break;
        }
        used = used.saturating_add(sz);
        // Safe: we just checked front() is Some.
        if let Some(ev) = ino.pending.pop_front() {
            out.push(ev);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the inotify instance object.
///
/// Exercises create/add_watch/event-translation/read/rm_watch/dup/close and
/// the buffer-too-small and IN_MASK_ADD paths against the live native notify
/// subsystem.  Returns `Err` (after a `[inotify] FAIL: …` line) instead of
/// panicking, consistent with the rest of the boot self-tests.
#[allow(clippy::arithmetic_side_effects)]
// Indexing into the just-read events Vec is intentional: the self-test
// asserts exact lengths first, so an out-of-bounds index is a genuine
// test failure we want to surface loudly.
#[allow(clippy::indexing_slicing)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[inotify] Running inotify instance self-test...");

    // 1. Mask translation sanity.  IN_OPEN / IN_CLOSE_* are now observable
    //    (file-handle open/close hooks); IN_ISDIR is output-only so it maps
    //    to no native interest bit (it is OR'd in at read time per is_dir).
    if to_native_mask(IN_CREATE).0 != FsEventMask::CREATE.0
        || to_native_mask(IN_MODIFY).0 != FsEventMask::MODIFY.0
        || to_native_mask(IN_OPEN).0 != FsEventMask::OPEN.0
        || to_native_mask(IN_CLOSE_WRITE).0 != FsEventMask::CLOSE_WRITE.0
        || to_native_mask(IN_CLOSE_NOWRITE).0 != FsEventMask::CLOSE_NOWRITE.0
        || to_native_mask(IN_ISDIR).0 != 0
    {
        serial_println!("[inotify]   FAIL: mask translation wrong");
        return Err(KernelError::InternalError);
    }

    // 2. record_size: header-only for empty name; rounded for non-empty.
    if record_size(0) != 16 || record_size(8) != 16 + 16 || record_size(16) != 16 + 32 {
        serial_println!(
            "[inotify]   FAIL: record_size wrong ({},{},{})",
            record_size(0), record_size(8), record_size(16)
        );
        return Err(KernelError::InternalError);
    }

    // 3. Create instance + a watch on a unique directory.
    let ino = create();
    if !exists(ino) {
        serial_println!("[inotify]   FAIL: fresh instance does not exist");
        return Err(KernelError::InternalError);
    }
    let dir = "/INOTIFY_SELFTEST";
    let wd = add_watch(ino, dir, IN_CREATE | IN_MODIFY | IN_DELETE | IN_MOVE)?;
    if wd <= 0 {
        serial_println!("[inotify]   FAIL: add_watch returned non-positive wd {}", wd);
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 4. Emit native events and read them back as inotify records.
    notify::emit_created("/INOTIFY_SELFTEST/a.txt");
    notify::emit_modified("/INOTIFY_SELFTEST/a.txt");
    let events = read_into(ino, 4096)?;
    if events.len() != 2 {
        serial_println!("[inotify]   FAIL: expected 2 events, got {}", events.len());
        close(ino);
        return Err(KernelError::InternalError);
    }
    if events[0].mask != IN_CREATE || events[0].name != b"a.txt" {
        serial_println!(
            "[inotify]   FAIL: event[0] mask={:#x} name={:?}",
            events[0].mask, core::str::from_utf8(&events[0].name)
        );
        close(ino);
        return Err(KernelError::InternalError);
    }
    if events[1].mask != IN_MODIFY {
        serial_println!("[inotify]   FAIL: event[1] mask={:#x}", events[1].mask);
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 4b. IN_ISDIR: a directory-subject event (mkdir) ORs IN_ISDIR into the
    //     reported mask, while a file-subject event of the same type does not.
    notify::emit_created_dir("/INOTIFY_SELFTEST/subdir");
    let events = read_into(ino, 4096)?;
    if events.len() != 1
        || events[0].mask != (IN_CREATE | IN_ISDIR)
        || events[0].name != b"subdir"
    {
        serial_println!(
            "[inotify]   FAIL: dir-create event mask={:#x} name={:?} (want IN_CREATE|IN_ISDIR 'subdir')",
            events.first().map_or(0, |e| e.mask),
            events.first().map(|e| core::str::from_utf8(&e.name)),
        );
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 5. Move pair: a rename under the watch yields MOVED_FROM + MOVED_TO with
    //    a shared, nonzero cookie.
    notify::emit_renamed("/INOTIFY_SELFTEST/a.txt", "/INOTIFY_SELFTEST/b.txt");
    let events = read_into(ino, 4096)?;
    if events.len() != 2
        || events[0].mask != IN_MOVED_FROM
        || events[1].mask != IN_MOVED_TO
        || events[0].cookie == 0
        || events[0].cookie != events[1].cookie
    {
        serial_println!(
            "[inotify]   FAIL: move pair wrong (n={}, m0={:#x}, m1={:#x}, c0={}, c1={})",
            events.len(),
            events.first().map_or(0, |e| e.mask),
            events.get(1).map_or(0, |e| e.mask),
            events.first().map_or(0, |e| e.cookie),
            events.get(1).map_or(0, |e| e.cookie),
        );
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 6. Buffer-too-small: one event pending, tiny budget → EINVAL, event kept.
    notify::emit_created("/INOTIFY_SELFTEST/longname.txt");
    match read_into(ino, 4) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            serial_println!("[inotify]   FAIL: small-buffer not EINVAL: {:?}", other);
            close(ino);
            return Err(KernelError::InternalError);
        }
    }
    // The event must still be there.
    let events = read_into(ino, 4096)?;
    if events.len() != 1 || events[0].mask != IN_CREATE {
        serial_println!("[inotify]   FAIL: kept event lost after small-buffer read");
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 7. rm_watch synthesizes IN_IGNORED and invalidates the wd.
    rm_watch(ino, wd)?;
    let events = read_into(ino, 4096)?;
    if events.len() != 1 || events[0].mask != IN_IGNORED || events[0].wd != wd {
        serial_println!("[inotify]   FAIL: rm_watch did not yield IN_IGNORED");
        close(ino);
        return Err(KernelError::InternalError);
    }
    if rm_watch(ino, wd).err() != Some(KernelError::InvalidArgument) {
        serial_println!("[inotify]   FAIL: double rm_watch not EINVAL");
        close(ino);
        return Err(KernelError::InternalError);
    }

    // 8. dup/close refcount lifetime.
    let ino2 = dup(ino)?;
    if ino2 != ino {
        serial_println!("[inotify]   FAIL: dup returned a different handle");
        close(ino);
        close(ino);
        return Err(KernelError::InternalError);
    }
    close(ino); // 2 -> 1, survives.
    if !exists(ino) {
        serial_println!("[inotify]   FAIL: freed after first of two closes");
        return Err(KernelError::InternalError);
    }
    close(ino); // 1 -> 0, freed (releases any remaining native watches).
    if exists(ino) {
        serial_println!("[inotify]   FAIL: still exists after final close");
        return Err(KernelError::InternalError);
    }

    // 9. Stale-handle operations fail cleanly.
    if add_watch(ino, "/x", IN_CREATE).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[inotify]   FAIL: add_watch on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if is_readable(ino) {
        serial_println!("[inotify]   FAIL: stale handle reported readable");
        return Err(KernelError::InternalError);
    }
    close(ino); // harmless no-op.

    serial_println!("[inotify]   inotify instance object (create/watch/read/rm/dup/close): OK");

    // Exercise the fs::notify blocking-read waiter registry here too: this
    // self-test runs unconditionally at boot, whereas notify::self_test is
    // gated behind a mounted FAT filesystem, so this is the path that actually
    // verifies the inotify-blocking-read wake registry on a typical boot.
    crate::fs::notify::waiter_registry_self_test()?;

    // Likewise drive the FS-independent opt-in interest-gate checks (ACCESS /
    // OPEN / CLOSE_* synthetic emit + mask filtering) here so they run on a
    // typical (diskless) boot rather than only under a mounted FAT root.
    crate::fs::notify::interest_gate_self_test()?;

    Ok(())
}
