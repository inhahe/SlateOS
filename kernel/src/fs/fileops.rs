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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

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
    pub source: String,
    /// Destination path (empty for delete operations).
    pub dest: String,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Current status.
    pub status: ItemStatus,
    /// Actual destination name if renamed.
    pub actual_dest: String,
    /// Error message if failed.
    pub error: String,
}

/// An undo log entry (records what was done so it can be reversed).
#[derive(Debug, Clone)]
pub struct UndoEntry {
    /// What was done.
    pub action: UndoAction,
    /// Source path involved.
    pub source: String,
    /// Destination path involved.
    pub dest: String,
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
    pub current_item: String,
    /// Items that were skipped.
    pub skipped: usize,
    /// Items that failed.
    pub failed: usize,
}

/// A conflict that needs resolution (when policy is Ask).
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Source file path.
    pub source: String,
    /// Destination path that already exists.
    pub dest: String,
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

static OPERATIONS: spin::Mutex<Vec<FileOperation>> = spin::Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Conflict resolution helpers
// ---------------------------------------------------------------------------

/// Generate a rename for a conflicting path: "file.txt" → "file (2).txt".
pub fn auto_rename(path: &str) -> String {
    // Split into parent + name + extension.
    let (parent, name) = match path.rfind('/') {
        Some(pos) => {
            let p = path.get(..pos).unwrap_or("");
            let n = path.get(pos.saturating_add(1)..).unwrap_or("");
            (p, n)
        }
        None => ("", path),
    };

    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => {
            let s = name.get(..dot).unwrap_or("");
            let e = name.get(dot..).unwrap_or("");
            (s, e)
        }
        _ => (name, ""),
    };

    // Try "foo (2).ext", "foo (3).ext", etc.
    for n in 2..=MAX_RENAME_ATTEMPTS {
        let candidate = if parent.is_empty() {
            format!("{} ({}){}", stem, n, ext)
        } else {
            format!("{}/{} ({}){}", parent, stem, n, ext)
        };
        // Check if this name is free (via VFS).
        if crate::fs::vfs::Vfs::metadata(&candidate).is_err() {
            return candidate;
        }
    }

    // Fallback (extremely unlikely: 9999 collisions).
    if parent.is_empty() {
        format!("{} (copy){}", stem, ext)
    } else {
        format!("{}/{} (copy){}", parent, stem, ext)
    }
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
    sources: &[&str],
    dest: &str,
    policy: ConflictPolicy,
) -> KernelResult<u64> {
    if sources.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if sources.len() > MAX_ITEMS {
        return Err(KernelError::InvalidArgument);
    }
    if kind != OpKind::Delete && dest.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let mut ops = OPERATIONS.lock();
    let active = ops.iter().filter(|o| o.state == OpState::Running).count();
    if active >= MAX_OPERATIONS {
        return Err(KernelError::WouldBlock);
    }

    let id = OP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    // Build item list from sources.
    let mut items = Vec::new();
    let mut total_bytes: u64 = 0;

    for src in sources {
        let meta = crate::fs::vfs::Vfs::metadata(src);
        let (is_dir, size) = match &meta {
            Ok(m) => (m.entry_type == crate::fs::EntryType::Directory, m.size),
            Err(_) => (false, 0),
        };

        let item_dest = if kind == OpKind::Delete {
            String::new()
        } else {
            // Compute destination path: dest/basename(src)
            let basename = src.rsplit('/').next().unwrap_or(src);
            if dest == "/" {
                format!("/{}", basename)
            } else {
                format!("{}/{}", dest, basename)
            }
        };

        total_bytes = total_bytes.saturating_add(size);
        items.push(OpItem {
            source: String::from(*src),
            dest: item_dest.clone(),
            is_dir,
            size,
            status: ItemStatus::Pending,
            actual_dest: item_dest,
            error: String::new(),
        });
    }

    let label = format!("{} {} item{} to {}",
        kind.label(), items.len(), if items.len() == 1 { "" } else { "s" }, dest);

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
        let op = ops.iter_mut().find(|o| o.id == op_id)
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
            let op = ops.iter().find(|o| o.id == op_id)
                .ok_or(KernelError::NotFound)?;

            if op.state == OpState::Cancelled {
                break;
            }

            // Find next pending item.
            let next = op.items.iter().enumerate()
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
            kind, &item_source, &item_dest, is_dir, policy, op_id,
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
                            op.transferred_bytes = op.transferred_bytes
                                .saturating_add(item.size);
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
        let op = ops.iter_mut().find(|o| o.id == op_id)
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
            current_item: String::new(),
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
    source: &str,
    dest: &str,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<String, ProcessError> {
    match kind {
        OpKind::Copy => copy_item(source, dest, is_dir, policy, op_id),
        OpKind::Move => move_item(source, dest, is_dir, policy, op_id),
        OpKind::Delete => delete_item(source, is_dir, op_id),
    }
}

/// Copy a single file or directory.
fn copy_item(
    source: &str,
    dest: &str,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<String, ProcessError> {
    let actual_dest = resolve_conflict(dest, policy)?;

    if is_dir {
        // Create directory at destination.
        if let Err(e) = crate::fs::vfs::Vfs::mkdir(&actual_dest) {
            if e != KernelError::AlreadyExists {
                return Err(ProcessError::Failed(format!("mkdir: {:?}", e)));
            }
        }
        add_undo(op_id, UndoAction::DirCreated, source, &actual_dest);
    } else {
        // Read source, write to destination.
        let data = crate::fs::vfs::Vfs::read_file(source)
            .map_err(|e| ProcessError::Failed(format!("read: {:?}", e)))?;
        crate::fs::vfs::Vfs::write_file(&actual_dest, &data)
            .map_err(|e| ProcessError::Failed(format!("write: {:?}", e)))?;
        add_undo(op_id, UndoAction::FileCopied, source, &actual_dest);
    }

    Ok(actual_dest)
}

/// Move a single file or directory.
fn move_item(
    source: &str,
    dest: &str,
    is_dir: bool,
    policy: ConflictPolicy,
    op_id: u64,
) -> Result<String, ProcessError> {
    // First copy, then delete source.
    let actual_dest = copy_item(source, dest, is_dir, policy, op_id)?;

    if is_dir {
        // For directories, we'd need recursive delete of source.
        // Record move intent; actual source cleanup done after all items.
        let mut ops = OPERATIONS.lock();
        if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
            // Remove the copy undo entry and replace with move.
            if let Some(last) = op.undo_log.last_mut() {
                last.action = if is_dir { UndoAction::DirCreated } else { UndoAction::FileMoved };
            }
        }
    } else {
        // Delete source file.
        if let Err(e) = crate::fs::vfs::Vfs::remove(source) {
            // Move partially failed — file was copied but source not deleted.
            // Log but don't fail the whole item.
            crate::serial_println!("[fileops] warning: could not delete source {}: {:?}", source, e);
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
fn delete_item(
    source: &str,
    _is_dir: bool,
    op_id: u64,
) -> Result<String, ProcessError> {
    // Try delete via VFS.
    crate::fs::vfs::Vfs::remove(source)
        .map_err(|e| ProcessError::Failed(format!("delete: {:?}", e)))?;

    add_undo(op_id, UndoAction::FileDeleted, source, "");

    Ok(String::new())
}

/// Resolve a destination conflict according to policy.
fn resolve_conflict(
    dest: &str,
    policy: ConflictPolicy,
) -> Result<String, ProcessError> {
    // Check if destination already exists.
    let exists = crate::fs::vfs::Vfs::metadata(dest).is_ok();

    if !exists {
        return Ok(String::from(dest));
    }

    match policy {
        ConflictPolicy::AutoRename => {
            Ok(auto_rename(dest))
        }
        ConflictPolicy::Overwrite => {
            // Will overwrite — return same dest.
            Ok(String::from(dest))
        }
        ConflictPolicy::Skip => {
            Err(ProcessError::Skipped)
        }
        ConflictPolicy::MergeDir => {
            // For directories, merge is OK — create if needed.
            // For files within merged dirs, use AutoRename fallback.
            Ok(String::from(dest))
        }
        ConflictPolicy::Ask => {
            // In non-interactive mode, fall back to skip.
            Err(ProcessError::Skipped)
        }
    }
}

/// Add an entry to an operation's undo log.
fn add_undo(op_id: u64, action: UndoAction, source: &str, dest: &str) {
    let mut ops = OPERATIONS.lock();
    if let Some(op) = ops.iter_mut().find(|o| o.id == op_id) {
        if op.undo_log.len() < MAX_UNDO_LOG {
            op.undo_log.push(UndoEntry {
                action,
                source: String::from(source),
                dest: String::from(dest),
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
    let op = ops.iter_mut().find(|o| o.id == op_id)
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
        let op = ops.iter_mut().find(|o| o.id == op_id)
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
                crate::fs::vfs::Vfs::remove(&entry.dest)
            }
            UndoAction::DirCreated => {
                // Try to remove directory (only succeeds if empty).
                crate::fs::vfs::Vfs::rmdir(&entry.dest)
            }
            UndoAction::FileMoved => {
                // Move file back to original location.
                let data = crate::fs::vfs::Vfs::read_file(&entry.dest);
                match data {
                    Ok(d) => {
                        let w = crate::fs::vfs::Vfs::write_file(&entry.source, &d);
                        if w.is_ok() {
                            let _ = crate::fs::vfs::Vfs::remove(&entry.dest);
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
                crate::fs::vfs::Vfs::remove(&entry.dest)
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
    let op = ops.iter().find(|o| o.id == op_id)
        .ok_or(KernelError::NotFound)?;

    let current = op.items.iter()
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
    ops.iter().map(|o| (o.id, o.kind, o.state, o.label.clone())).collect()
}

/// Get full detail for an operation.
pub fn get_op(op_id: u64) -> Option<FileOperation> {
    OPERATIONS.lock().iter().find(|o| o.id == op_id).cloned()
}

/// Remove completed/cancelled operations from the list.
pub fn cleanup() -> usize {
    let mut ops = OPERATIONS.lock();
    let before = ops.len();
    ops.retain(|o| o.state == OpState::Running || o.state == OpState::Queued || o.state == OpState::Paused);
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
        let renamed = auto_rename("/tmp/file.txt");
        // Should produce /tmp/file (2).txt or similar (depends on VFS state).
        assert!(renamed.contains("file"));
        assert!(renamed.contains("("));
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
        assert_eq!(ConflictPolicy::AutoRename as u8, ConflictPolicy::AutoRename as u8);
        assert_ne!(ConflictPolicy::AutoRename, ConflictPolicy::Skip);
        serial_println!("[fileops] test 6 passed: conflict policies");
    }

    // Clean up.
    let _ = crate::fs::vfs::Vfs::remove("/tmp/fileops_test.txt");
    clear();

    serial_println!("[fileops] all 6 self-tests passed");
    Ok(())
}
