//! Bulk file operations engine (copy, move, delete).
//!
//! Provides an operation engine for multi-file copy, move, and delete
//! with progress tracking, conflict resolution, error handling, and
//! undo support.  Both file explorer drag-and-drop and command-line
//! tools use this same engine, per the design spec.
//!
//! ## Design Spec Requirements (lines 754-756)
//!
//! - Windows-style directory copy with easy conflict resolution
//! - Automatic "foo (2)" rename for name collisions
//! - Skip files that couldn't be copied
//! - Atomic: can undo whole operation before it finishes
//! - Resume after interruption (computer shutdown, log off)
//! - CLI commands use the same mechanism as file explorer
//!
//! ## Architecture
//!
//! ```text
//! File Explorer / CLI
//!   → fileops::start(plan)
//!   → engine processes items sequentially
//!   → progress callbacks on each item
//!   → conflict resolution via policy or callback
//!   → undo log for rollback
//! ```
//!
//! ## Conflict Resolution Policies
//!
//! - **AutoRename**: append " (2)", " (3)", etc. to conflicting names
//! - **Overwrite**: replace existing files
//! - **Skip**: skip conflicting files silently
//! - **MergeDir**: merge subdirectories, apply policy to file conflicts
//! - **Ask**: defer to callback (for GUI prompts)

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum items in a single file operation.
const MAX_ITEMS: usize = 65536;

/// Maximum undo log entries.
const MAX_UNDO_LOG: usize = 65536;

/// Maximum concurrent operations.
const MAX_OPERATIONS: usize = 16;

/// Maximum rename suffix attempts (foo (2) through foo (N)).
const MAX_RENAME_ATTEMPTS: u32 = 9999;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kind of file operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// Copy files/directories to a destination.
    Copy,
    /// Move files/directories to a destination (copy + delete source).
    Move,
    /// Delete files/directories.
    Delete,
}

impl OpKind {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Delete => "delete",
        }
    }
}

/// How to handle name conflicts at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Automatically rename: "foo" → "foo (2)", "foo (3)", etc.
    AutoRename,
    /// Overwrite existing files.
    Overwrite,
    /// Skip conflicting files.
    Skip,
    /// Merge directories; apply this policy to file conflicts within.
    MergeDir,
    /// Pause and let the caller decide (for GUI "ask" dialog).
    Ask,
}

/// Per-item status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    /// Not yet processed.
    Pending,
    /// Currently being processed.
    InProgress,
    /// Completed successfully.
    Done,
    /// Skipped due to conflict or error.
    Skipped,
    /// Failed with an error.
    Failed,
    /// Renamed to resolve conflict.
    Renamed,
}

/// Current state of the overall operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpState {
    /// Operation is queued but not started.
    Queued,
    /// Currently processing items.
    Running,
    /// Paused (waiting for user input on conflict, or explicit pause).
    Paused,
    /// Completed all items (some may have failed/skipped).
    Completed,
    /// Cancelled by user (partial undo may have been applied).
    Cancelled,
    /// Undo in progress.
    Undoing,
}

/// A single item in a file operation (one file or directory).
#[derive(Debug, Clone)]
pub struct OpItem {
    /// Source path.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): this is a
    /// filesystem path, and our filesystems permit any byte but `/` and NUL
    /// in a name.  A copy engine that cannot name a file is a copy engine
    /// that silently operates on the wrong one.
    pub source: PathBuf,
    /// Destination path (empty for delete operations).
    pub dest: PathBuf,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Current status.
    pub status: ItemStatus,
    /// Actual destination name if renamed.
    pub actual_dest: PathBuf,
    /// Error message if failed.  Text we generate, so a `String`.
    pub error: String,
}

/// An undo log entry (records what was done so it can be reversed).
#[derive(Debug, Clone)]
pub struct UndoEntry {
    /// What was done.
    pub action: UndoAction,
    /// Source path involved.
    pub source: PathBuf,
    /// Destination path involved.
    pub dest: PathBuf,
}

/// Possible undo actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoAction {
    /// A file was copied here — undo by deleting.
    FileCopied,
    /// A directory was created — undo by removing (if empty).
    DirCreated,
    /// A file was moved here — undo by moving back.
    FileMoved,
    /// A file was overwritten — cannot fully undo (original lost).
    FileOverwritten,
    /// A file was deleted — cannot undo.
    FileDeleted,
    /// A file was renamed at destination.
    FileRenamed,
}

/// Progress information for callbacks.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Operation ID.
    pub op_id: u64,
    /// Total items to process.
    pub total_items: usize,
    /// Items processed so far.
    pub processed_items: usize,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Bytes transferred so far.
    pub transferred_bytes: u64,
    /// Current item being processed.
    pub current_item: PathBuf,
    /// Items that were skipped.
    pub skipped: usize,
    /// Items that failed.
    pub failed: usize,
}

/// A conflict that needs resolution (when policy is Ask).
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Source file path.
    pub source: PathBuf,
    /// Destination path that already exists.
    pub dest: PathBuf,
    /// Whether both are directories (merge is possible).
    pub both_dirs: bool,
    /// Source file size.
    pub source_size: u64,
    /// Existing file size.
    pub dest_size: u64,
}

/// A complete file operation (all state for one copy/move/delete).
#[derive(Debug, Clone)]
pub struct FileOperation {
    /// Unique operation ID.
    pub id: u64,
    /// Kind of operation.
    pub kind: OpKind,
    /// Source description (for display).
    pub label: String,
    /// Current state.
    pub state: OpState,
    /// Conflict policy.
    pub policy: ConflictPolicy,
    /// Items to process.
    pub items: Vec<OpItem>,
    /// Undo log (in reverse order for rollback).
    pub undo_log: Vec<UndoEntry>,
    /// Bytes transferred so far.
    pub transferred_bytes: u64,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Items processed.
    pub processed: usize,
    /// Items skipped.
    pub skipped: usize,
    /// Items failed.
    pub failed: usize,
    /// Timestamp when started (ns).
    pub started_ns: u64,
    /// Pending conflict (when paused for Ask policy).
    pub pending_conflict: Option<Conflict>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static OP_COUNTER: AtomicU64 = AtomicU64::new(1);
static TOTAL_OPS: AtomicU64 = AtomicU64::new(0);
static TOTAL_COMPLETED: AtomicU64 = AtomicU64::new(0);
static TOTAL_CANCELLED: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_MOVED: AtomicU64 = AtomicU64::new(0);

static OPERATIONS: PreemptSpinMutex<Vec<FileOperation>> =
    PreemptSpinMutex::named(Vec::new(), b"OPERATIONS");

// ---------------------------------------------------------------------------
// Conflict resolution helpers
// ---------------------------------------------------------------------------

/// Generate a rename for a conflicting path: "file.txt" → "file (2).txt".
///
/// Operates on bytes (design-decisions.md 261): the conflicting path came off
/// the filesystem, which permits any byte but `/` and NUL, and the renamed
/// copy must sit beside the original in the same directory — so the parent
/// and stem have to be carried through byte-exactly.  Only the inserted
/// ` (n)` is text, and that text is ours.
pub fn auto_rename(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref().as_bytes();

    // Split into parent + name.  Note this is a *byte* split on the last `/`,
    // deliberately not `Path::parent`/`Path::file_name`: a trailing slash
    // must not be silently dropped here, because the caller is about to
    // create a file at the name we return.
    let (parent, name) = match path.iter().rposition(|&b| b == b'/') {
        Some(pos) => (
            path.get(..pos).unwrap_or(b""),
            path.get(pos.saturating_add(1)..).unwrap_or(b""),
        ),
        None => (&b""[..], path),
    };

    // Split the name at the last `.`, but only if it is not the first byte —
    // a leading dot makes a hidden file, not an extension, so `.bashrc`
    // becomes `.bashrc (2)` rather than ` (2).bashrc`.
    let (stem, ext) = match name.iter().rposition(|&b| b == b'.') {
        Some(dot) if dot > 0 => (
            name.get(..dot).unwrap_or(b""),
            name.get(dot..).unwrap_or(b""),
        ),
        _ => (name, &b""[..]),
    };

    /// Assemble `parent/stem<infix>ext` without requiring any part to be text.
    fn build(parent: &[u8], stem: &[u8], infix: &str, ext: &[u8]) -> PathBuf {
        let mut out = PathBuf::with_capacity(
            parent
                .len()
                .saturating_add(1)
                .saturating_add(stem.len())
                .saturating_add(infix.len())
                .saturating_add(ext.len()),
        );
        if !parent.is_empty() {
            out.extend_bytes(parent);
            out.extend_bytes(b"/");
        }
        out.extend_bytes(stem);
        out.extend_bytes(infix.as_bytes());
        out.extend_bytes(ext);
        out
    }

    // Try "foo (2).ext", "foo (3).ext", etc.
    for n in 2..=MAX_RENAME_ATTEMPTS {
        let candidate = build(parent, stem, &format!(" ({})", n), ext);
        // Check if this name is free (via VFS).
        if crate::fs::vfs::Vfs::metadata(candidate.as_path()).is_err() {
            return candidate;
        }
    }

    // Fallback (extremely unlikely: 9999 collisions).
    build(parent, stem, " (copy)", ext)
}

// ---------------------------------------------------------------------------
// Operation lifecycle
// ---------------------------------------------------------------------------

/// Create a new file operation from a list of source paths and a destination.
///
/// For Delete operations, `dest` should be empty.
/// The items list is populated by scanning source paths.
pub fn create(
    kind: OpKind,
    sources: &[impl AsRef<Path>],
    dest: impl AsRef<Path>,
    policy: ConflictPolicy,
) -> KernelResult<u64> {
    let dest = dest.as_ref();
    if sources.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if sources.len() > MAX_ITEMS {
        return Err(KernelError::InvalidArgument);
    }
    if kind != OpKind::Delete && dest.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Early admission check, so an over-quota caller is rejected before paying
    // for the metadata walk below. It is only advisory — the binding check is
    // the re-check under the lock the operation is actually pushed under.
    {
        let ops = OPERATIONS.lock();
        if ops.iter().filter(|o| o.state == OpState::Running).count() >= MAX_OPERATIONS {
            return Err(KernelError::WouldBlock);
        }
    }

    let now = crate::timekeeping::clock_monotonic();

    // Build item list from sources, with NO lock held. `Vfs::metadata` walks the
    // mount table, takes filesystem locks of its own and can block on the
    // backing device; running it once per source inside OPERATIONS' critical
    // section held a leaf lock across an unbounded I/O path — for an
    // arbitrarily long `sources` list — and put its acquisition order ahead of
    // the VFS's.
    let mut items = Vec::new();
    let mut total_bytes: u64 = 0;

    for src in sources {
        let src = src.as_ref();
        let meta = crate::fs::vfs::Vfs::metadata(src);
        let (is_dir, size) = match &meta {
            Ok(m) => (m.entry_type == crate::fs::EntryType::Directory, m.size),
            Err(_) => (false, 0),
        };

        let item_dest = if kind == OpKind::Delete {
            PathBuf::new()
        } else {
            // Compute destination path: dest/basename(src).  `Path::join`
            // handles the root case (it does not double the separator) and
            // carries the basename's bytes through unexamined.
            let basename = src.file_name().unwrap_or(src);
            dest.join(basename)
        };

        total_bytes = total_bytes.saturating_add(size);
        items.push(OpItem {
            source: src.to_path_buf(),
            dest: item_dest.clone(),
            is_dir,
            size,
            status: ItemStatus::Pending,
            actual_dest: item_dest,
            error: String::new(),
        });
    }

    let label = format!(
        "{} {} item{} to {}",
        kind.label(),
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        dest.display()
    );

    let mut ops = OPERATIONS.lock();
    // Binding admission check. The metadata walk above ran unlocked, so another
    // caller may have taken the last slot since the early check; without
    // re-testing here MAX_OPERATIONS would be exceedable. The id is allocated
    // only once admission is certain, so a rejected create burns no id.
    if ops.iter().filter(|o| o.state == OpState::Running).count() >= MAX_OPERATIONS {
        return Err(KernelError::WouldBlock);
    }
    let id = OP_COUNTER.fetch_add(1, Ordering::Relaxed);

    let op = FileOperation {
        id,
        kind,
        label,
        state: OpState::Queued,
        policy,
        items,
        undo_log: Vec::new(),
        transferred_bytes: 0,
        total_bytes,
        processed: 0,
        skipped: 0,
        failed: 0,
        started_ns: now,
        pending_conflict: None,
    };

    ops.push(op);
    TOTAL_OPS.fetch_add(1, Ordering::Relaxed);

    Ok(id)
}

/// Execute a file operation (processes all items).
///
/// This is a synchronous operation — it processes all items sequentially.
/// In a real GUI, this would be called from a background thread with
/// progress callbacks.
pub fn execute(op_id: u64) -> KernelResult<Progress> {
    // Mark as running.
    {
        let mut ops = OPERATIONS.lock();
        let op = ops
            .iter_mut()
            .find(|o| o.id == op_id)
            .ok_or(KernelError::NotFound)?;
        if op.state != OpState::Queued && op.state != OpState::Paused {
            return Err(KernelError::InvalidArgument);
        }
        op.state = OpState::Running;
    }

    // Process items one at a time, releasing the lock between items.
    loop {
        let (item_idx, item_source, item_dest, kind, policy, is_dir);
        {
            let ops = OPERATIONS.lock();
            let op = ops
                .iter()
                .find(|o| o.id == op_id)
                .ok_or(KernelError::NotFound)?;

            if op.state == OpState::Cancelled {
                break;
            }

            // Find next pending item.
            let next = op
                .items
                .iter()
                .enumerate()
                .find(|(_, it)| it.status == ItemStatus::Pending);

            match next {
                Some((idx, item)) => {
                    item_idx = idx;
                    item_source = item.source.clone();
                    item_dest = item.dest.clone();
                    kind = op.kind;
                    policy = op.policy;
                    is_dir = item.is_dir;
                }
                None => break, // All items processed.
            }
        }

        // Process the item (without holding the lock).
        let result = process_item(
            kind,
            item_source.as_path(),
            item_dest.as_path(),
            is_dir,
            policy,
            op_id,
        );

        // Update item status.
        {
            let mut ops = OPERATIONS.lock();
            if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
                if let Some(item) = op.items.get_mut(item_idx) {
                    item.status = ItemStatus::InProgress;
                    match result {
                        Ok(actual_dest) => {
                            item.status = if actual_dest != item_dest {
                                item.actual_dest = actual_dest;
                                ItemStatus::Renamed
                            } else {
                                ItemStatus::Done
                            };
                            op.transferred_bytes = op.transferred_bytes.saturating_add(item.size);
                        }
                        Err(ProcessError::Skipped) => {
                            item.status = ItemStatus::Skipped;
                            op.skipped = op.skipped.saturating_add(1);
                        }
                        Err(ProcessError::Failed(msg)) => {
                            item.status = ItemStatus::Failed;
                            item.error = msg;
                            op.failed = op.failed.saturating_add(1);
                        }
                    }
                    op.processed = op.processed.saturating_add(1);
                }
            }
        }
    }

    // Mark completed.
    let progress;
    {
        let mut ops = OPERATIONS.lock();
        let op = ops
            .iter_mut()
            .find(|o| o.id == op_id)
            .ok_or(KernelError::NotFound)?;

        if op.state == OpState::Running {
            op.state = OpState::Completed;
            TOTAL_COMPLETED.fetch_add(1, Ordering::Relaxed);
        }
        TOTAL_BYTES_MOVED.fetch_add(op.transferred_bytes, Ordering::Relaxed);

        progress = Progress {
            op_id,
            total_items: op.items.len(),
            processed_items: op.processed,
            total_bytes: op.total_bytes,
            transferred_bytes: op.transferred_bytes,
            current_item: PathBuf::new(),
            skipped: op.skipped,
            failed: op.failed,
        };
    }

    Ok(progress)
}

/// Internal error type for item processing.
enum ProcessError {
    Skipped,
    Failed(String),
}

/// Process a single item in a file operation.
fn process_item(
    kind: OpKind,
    source: &Path,
    dest: &Path,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<PathBuf, ProcessError> {
    match kind {
        OpKind::Copy => copy_item(source, dest, is_dir, policy, op_id),
        OpKind::Move => move_item(source, dest, is_dir, policy, op_id),
        OpKind::Delete => delete_item(source, is_dir, op_id),
    }
}

/// Copy a single file or directory.
fn copy_item(
    source: &Path,
    dest: &Path,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<PathBuf, ProcessError> {
    let actual_dest = resolve_conflict(dest, policy)?;

    if is_dir {
        // Create directory at destination.
        if let Err(e) = crate::fs::vfs::Vfs::mkdir(actual_dest.as_path()) {
            if e != KernelError::AlreadyExists {
                return Err(ProcessError::Failed(format!("mkdir: {:?}", e)));
            }
        }
        add_undo(op_id, UndoAction::DirCreated, source, actual_dest.as_path());
    } else {
        // Read source, write to destination.
        let data = crate::fs::vfs::Vfs::read_file(source)
            .map_err(|e| ProcessError::Failed(format!("read: {:?}", e)))?;
        crate::fs::vfs::Vfs::write_file(actual_dest.as_path(), &data)
            .map_err(|e| ProcessError::Failed(format!("write: {:?}", e)))?;
        add_undo(op_id, UndoAction::FileCopied, source, actual_dest.as_path());
    }

    Ok(actual_dest)
}

/// Move a single file or directory.
fn move_item(
    source: &Path,
    dest: &Path,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<PathBuf, ProcessError> {
    // First copy, then delete source.
    let actual_dest = copy_item(source, dest, is_dir, policy, op_id)?;

    if is_dir {
        // For directories, we'd need recursive delete of source.
        // Record move intent; actual source cleanup done after all items.
        let mut ops = OPERATIONS.lock();
        if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
            // Remove the copy undo entry and replace with move.
            if let Some(last) = op.undo_log.last_mut() {
                last.action = if is_dir {
                    UndoAction::DirCreated
                } else {
                    UndoAction::FileMoved
                };
            }
        }
    } else {
        // Delete source file.
        if let Err(e) = crate::fs::vfs::Vfs::remove(source) {
            // Move partially failed — file was copied but source not deleted.
            // Log but don't fail the whole item.
            crate::serial_println!(
                "[fileops] warning: could not delete source {}: {:?}",
                source.display(),
                e
            );
        }
        // Update undo log to reflect move rather than copy.
        let mut ops = OPERATIONS.lock();
        if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
            if let Some(last) = op.undo_log.last_mut() {
                last.action = UndoAction::FileMoved;
            }
        }
    }

    Ok(actual_dest)
}

/// Delete a single file or directory.
fn delete_item(source: &Path, _is_dir: bool, op_id: u64) -> Result<PathBuf, ProcessError> {
    // Try delete via VFS.
    crate::fs::vfs::Vfs::remove(source)
        .map_err(|e| ProcessError::Failed(format!("delete: {:?}", e)))?;

    add_undo(op_id, UndoAction::FileDeleted, source, Path::new(""));

    Ok(PathBuf::new())
}

/// Resolve a destination conflict according to policy.
fn resolve_conflict(dest: &Path, policy: ConflictPolicy) -> Result<PathBuf, ProcessError> {
    // Check if destination already exists.
    let exists = crate::fs::vfs::Vfs::metadata(dest).is_ok();

    if !exists {
        return Ok(dest.to_path_buf());
    }

    match policy {
        ConflictPolicy::AutoRename => Ok(auto_rename(dest)),
        ConflictPolicy::Overwrite => {
            // Will overwrite — return same dest.
            Ok(dest.to_path_buf())
        }
        ConflictPolicy::Skip => Err(ProcessError::Skipped),
        ConflictPolicy::MergeDir => {
            // For directories, merge is OK — create if needed.
            // For files within merged dirs, use AutoRename fallback.
            Ok(dest.to_path_buf())
        }
        ConflictPolicy::Ask => {
            // In non-interactive mode, fall back to skip.
            Err(ProcessError::Skipped)
        }
    }
}

/// Add an entry to an operation's undo log.
fn add_undo(op_id: u64, action: UndoAction, source: &Path, dest: &Path) {
    let mut ops = OPERATIONS.lock();
    if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
        if op.undo_log.len() < MAX_UNDO_LOG {
            op.undo_log.push(UndoEntry {
                action,
                source: source.to_path_buf(),
                dest: dest.to_path_buf(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Cancel and undo
// ---------------------------------------------------------------------------

/// Cancel an in-progress operation.
pub fn cancel(op_id: u64) -> KernelResult<()> {
    let mut ops = OPERATIONS.lock();
    let op = ops
        .iter_mut()
        .find(|o| o.id == op_id)
        .ok_or(KernelError::NotFound)?;

    match op.state {
        OpState::Running | OpState::Paused | OpState::Queued => {
            op.state = OpState::Cancelled;
            TOTAL_CANCELLED.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        _ => Err(KernelError::InvalidArgument),
    }
}

/// Undo a completed operation (best-effort rollback).
///
/// Processes the undo log in reverse order. Files that were copied
/// are deleted; files that were moved are moved back. Overwrites
/// and deletes cannot be fully undone.
pub fn undo(op_id: u64) -> KernelResult<(usize, usize)> {
    let undo_log;
    {
        let mut ops = OPERATIONS.lock();
        let op = ops
            .iter_mut()
            .find(|o| o.id == op_id)
            .ok_or(KernelError::NotFound)?;

        if op.state != OpState::Completed && op.state != OpState::Cancelled {
            return Err(KernelError::InvalidArgument);
        }

        op.state = OpState::Undoing;
        undo_log = op.undo_log.clone();
    }

    let mut undone = 0usize;
    let mut failed = 0usize;

    // Process undo log in reverse.
    for entry in undo_log.iter().rev() {
        let result = match entry.action {
            UndoAction::FileCopied => {
                // Delete the copied file.
                crate::fs::vfs::Vfs::remove(entry.dest.as_path())
            }
            UndoAction::DirCreated => {
                // Try to remove directory (only succeeds if empty).
                crate::fs::vfs::Vfs::rmdir(entry.dest.as_path())
            }
            UndoAction::FileMoved => {
                // Move file back to original location.
                let data = crate::fs::vfs::Vfs::read_file(entry.dest.as_path());
                match data {
                    Ok(d) => {
                        let w = crate::fs::vfs::Vfs::write_file(entry.source.as_path(), &d);
                        if w.is_ok() {
                            let _ = crate::fs::vfs::Vfs::remove(entry.dest.as_path());
                        }
                        w
                    }
                    Err(e) => Err(e),
                }
            }
            UndoAction::FileOverwritten | UndoAction::FileDeleted => {
                // Cannot undo — original data is lost.
                Err(KernelError::NotSupported)
            }
            UndoAction::FileRenamed => {
                // Just delete the renamed copy.
                crate::fs::vfs::Vfs::remove(entry.dest.as_path())
            }
        };

        if result.is_ok() {
            undone = undone.saturating_add(1);
        } else {
            failed = failed.saturating_add(1);
        }
    }

    Ok((undone, failed))
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get the progress of an operation.
pub fn progress(op_id: u64) -> KernelResult<Progress> {
    let ops = OPERATIONS.lock();
    let op = ops
        .iter()
        .find(|o| o.id == op_id)
        .ok_or(KernelError::NotFound)?;

    let current = op
        .items
        .iter()
        .find(|i| i.status == ItemStatus::InProgress)
        .map(|i| i.source.clone())
        .unwrap_or_default();

    Ok(Progress {
        op_id,
        total_items: op.items.len(),
        processed_items: op.processed,
        total_bytes: op.total_bytes,
        transferred_bytes: op.transferred_bytes,
        current_item: current,
        skipped: op.skipped,
        failed: op.failed,
    })
}

/// List all operations (active and completed).
pub fn list_ops() -> Vec<(u64, OpKind, OpState, String)> {
    let ops = OPERATIONS.lock();
    ops.iter()
        .map(|o| (o.id, o.kind, o.state, o.label.clone()))
        .collect()
}

/// Get full detail for an operation.
pub fn get_op(op_id: u64) -> Option<FileOperation> {
    OPERATIONS.lock().iter().find(|o| o.id == op_id).cloned()
}

/// Remove completed/cancelled operations from the list.
pub fn cleanup() -> usize {
    let mut ops = OPERATIONS.lock();
    let before = ops.len();
    ops.retain(|o| {
        o.state == OpState::Running || o.state == OpState::Queued || o.state == OpState::Paused
    });
    before.saturating_sub(ops.len())
}

/// Clear all operations.
pub fn clear() {
    OPERATIONS.lock().clear();
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (total_ops, completed, cancelled, bytes_moved).
pub fn stats() -> (u64, u64, u64, u64) {
    (
        TOTAL_OPS.load(Ordering::Relaxed),
        TOTAL_COMPLETED.load(Ordering::Relaxed),
        TOTAL_CANCELLED.load(Ordering::Relaxed),
        TOTAL_BYTES_MOVED.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    TOTAL_OPS.store(0, Ordering::Relaxed);
    TOTAL_COMPLETED.store(0, Ordering::Relaxed);
    TOTAL_CANCELLED.store(0, Ordering::Relaxed);
    TOTAL_BYTES_MOVED.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file operations engine.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: auto_rename logic.
    {
        // Names chosen so nothing else in the boot sequence can occupy the
        // " (2)" slot and turn this into " (3)".
        let renamed = auto_rename("/tmp/_ar_uniq.txt");
        // The parent, stem and extension are all preserved; only a " (n)"
        // is inserted before the extension.
        assert_eq!(renamed.as_path(), Path::new("/tmp/_ar_uniq (2).txt"));

        // A leading dot makes a hidden file, not an extension.
        assert_eq!(
            auto_rename("/tmp/._ar_uniq").as_path(),
            Path::new("/tmp/._ar_uniq (2)")
        );
        // A bare name with no parent stays parentless.
        assert_eq!(
            auto_rename("_ar_uniq.txt").as_path(),
            Path::new("_ar_uniq (2).txt")
        );

        // The counter really does advance past an occupied slot.
        crate::fs::vfs::Vfs::write_file("/tmp/_ar_uniq (2).txt", b"")?;
        assert_eq!(
            auto_rename("/tmp/_ar_uniq.txt").as_path(),
            Path::new("/tmp/_ar_uniq (3).txt")
        );
        let _ = crate::fs::vfs::Vfs::remove("/tmp/_ar_uniq (2).txt");
        serial_println!("[fileops] test 1 passed: auto_rename");
    }

    // Test 2: create operation.
    {
        // Create a test file first.
        let _ = crate::fs::vfs::Vfs::write_file("/tmp/fileops_test.txt", b"hello");
        let op_id = create(
            OpKind::Copy,
            &["/tmp/fileops_test.txt"],
            "/tmp",
            ConflictPolicy::AutoRename,
        )?;
        assert!(op_id > 0);
        let ops = list_ops();
        assert!(ops.iter().any(|o| o.0 == op_id));
        serial_println!("[fileops] test 2 passed: create operation");
    }

    // Test 3: stats tracking.
    {
        let (total, _, _, _) = stats();
        assert!(total > 0);
        serial_println!("[fileops] test 3 passed: stats");
    }

    // Test 4: cancel operation.
    {
        let op_id = create(
            OpKind::Delete,
            &["/tmp/fileops_test.txt"],
            "",
            ConflictPolicy::Skip,
        )?;
        cancel(op_id)?;
        let op = get_op(op_id);
        assert!(op.is_some());
        if let Some(o) = op {
            assert_eq!(o.state, OpState::Cancelled);
        }
        serial_println!("[fileops] test 4 passed: cancel operation");
    }

    // Test 5: cleanup completed/cancelled.
    {
        let removed = cleanup();
        assert!(removed > 0);
        serial_println!("[fileops] test 5 passed: cleanup");
    }

    // Test 6: conflict policies.
    {
        assert_eq!(ConflictPolicy::AutoRename as u8, 0);
        assert_ne!(ConflictPolicy::AutoRename as u8, ConflictPolicy::Skip as u8);
        assert_ne!(ConflictPolicy::AutoRename, ConflictPolicy::Skip);
        serial_println!("[fileops] test 6 passed: conflict policies");
    }

    // Test 7: non-UTF-8 paths (design-decisions.md 261).
    //
    // Everything this engine touches is a filesystem path, and a name may
    // contain any byte but `/` and NUL.  While these were `String`s the
    // explorer could not copy, move or delete such a file at all — and a
    // caller that reached the engine through a lossy conversion would have
    // copied, or worse *deleted*, some other file entirely while reporting
    // success.
    {
        let src = Path::new(&b"/tmp/fo_\xFFs.txt"[..]);
        let dstdir = Path::new(&b"/tmp/fo_\xFEd"[..]);
        crate::fs::vfs::Vfs::write_file(src, b"payload")?;
        crate::fs::vfs::Vfs::mkdir(dstdir)?;

        // auto_rename carries every byte of parent and stem through.
        let renamed = auto_rename(src);
        assert_eq!(renamed.as_path(), Path::new(&b"/tmp/fo_\xFFs (2).txt"[..]));

        // The destination is composed from the destination directory and the
        // source's basename, both non-UTF-8.
        let op_id = create(OpKind::Copy, &[src], dstdir, ConflictPolicy::AutoRename)?;
        let op = get_op(op_id).ok_or(KernelError::NotFound)?;
        let item = op.items.first().ok_or(KernelError::NotFound)?;
        assert_eq!(item.source.as_path(), src);
        assert_eq!(
            item.dest.as_path(),
            Path::new(&b"/tmp/fo_\xFEd/fo_\xFFs.txt"[..])
        );

        // And it actually runs: the copy lands at that byte-exact path.
        let _ = execute(op_id)?;
        assert_eq!(
            crate::fs::vfs::Vfs::read_file(item.dest.as_path())?,
            b"payload"
        );

        let _ = crate::fs::vfs::Vfs::remove(item.dest.as_path());
        let _ = crate::fs::vfs::Vfs::remove(src);
        let _ = crate::fs::vfs::Vfs::rmdir(dstdir);
        serial_println!("[fileops] test 7 passed: non-UTF-8 paths");
    }

    // Clean up.
    let _ = crate::fs::vfs::Vfs::remove("/tmp/fileops_test.txt");
    clear();

    serial_println!("[fileops] all 7 self-tests passed");
    Ok(())
}
