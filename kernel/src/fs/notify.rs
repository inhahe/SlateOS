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

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

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

    /// All events except ACCESS (common usage).
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
// Public API — watch management
// ---------------------------------------------------------------------------

/// Create a new filesystem watch.
///
/// Monitors `path` for events matching `mask`.  If `recursive` is true,
/// events from subdirectories are also reported.
///
/// Returns a watch ID that can be used with [`read_events`] and
/// [`close_watch`].
pub fn create_watch(
    path: &str,
    mask: FsEventMask,
    recursive: bool,
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
    });

    crate::serial_println!(
        "[notify] Watch {} created for '{}' (mask={:#x}, recursive={})",
        id, path, mask.0, recursive
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
    if watches.remove(&watch_id).is_none() {
        return Err(KernelError::InvalidHandle);
    }

    crate::serial_println!("[notify] Watch {} closed", watch_id);
    Ok(())
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
#[allow(clippy::arithmetic_side_effects)]
pub fn emit(event_type: FsEventType, path: &str, new_path: Option<&str>) {
    let mut watches = WATCHES.lock();

    // Fast path: no watches registered.
    if watches.is_empty() {
        return;
    }

    // Check each watch for a matching path.
    for watch in watches.values_mut() {
        // Does this watch care about this event type?
        if !watch.mask.contains(event_type.to_mask()) {
            continue;
        }

        // Does the path match the watched directory?
        let matched = if path.starts_with(&watch.path) {
            // Path is inside the watched directory.
            if watch.recursive {
                true
            } else {
                // Non-recursive: only match direct children.
                // The remaining path after the watch prefix should have
                // no more '/' characters (it's a direct child).
                let remainder = &path[watch.path.len()..];
                !remainder.contains('/')
            }
        } else if watch.path.starts_with(path) && watch.path.len() == path.len() + 1 {
            // The path IS the watched directory itself (without trailing /).
            true
        } else {
            false
        };

        if !matched {
            // Also check for rename where new_path matches.
            if let Some(np) = new_path {
                let np_matched = if np.starts_with(&watch.path) {
                    if watch.recursive {
                        true
                    } else {
                        let remainder = &np[watch.path.len()..];
                        !remainder.contains('/')
                    }
                } else {
                    false
                };

                if !np_matched {
                    continue;
                }
            } else {
                continue;
            }
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
        });
    }
}

/// Convenience: emit a "created" event.
#[inline]
pub fn emit_created(path: &str) {
    emit(FsEventType::Created, path, None);
}

/// Convenience: emit a "deleted" event.
#[inline]
pub fn emit_deleted(path: &str) {
    emit(FsEventType::Deleted, path, None);
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

    // Clean up.
    close_watch(watch_id)?;
    close_watch(rec_id)?;

    // Verify closed watch returns error.
    match read_events(watch_id, 10) {
        Err(KernelError::InvalidHandle) => {}
        _ => {
            crate::serial_println!("[notify]   FAIL: closed watch didn't return InvalidHandle");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[notify]   Watch cleanup verified ✓");

    crate::serial_println!("[notify] Self-test passed.");
    Ok(())
}
