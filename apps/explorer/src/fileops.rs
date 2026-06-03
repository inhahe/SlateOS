//! Atomic file operations for the OurOS file explorer.
//!
//! Provides copy, move, delete, recycle, and undo operations with:
//! - Progress tracking (bytes, files, ETA)
//! - Crash-safe journaling for resume on interruption
//! - Conflict resolution policies
//! - Per-file error handling (skip, retry, stop)
//! - Undo via an operation journal
//! - Recycle bin management with auto-purge
//!
//! All multi-file operations are planned before execution: the source tree is
//! scanned to produce an [`OperationPlan`], which records total bytes and file
//! count. The plan is then executed step-by-step, updating an
//! [`OperationProgress`] after each file and writing completed actions to an
//! [`OperationJournal`] so that a crashed/interrupted operation can be resumed
//! by re-reading the journal and skipping already-finished items.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

// ============================================================================
// Core enums
// ============================================================================

/// Top-level operation type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Copy,
    Move,
    Delete,
    Recycle,
    Restore,
}

/// What to do when a destination already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Silently skip the conflicting file.
    Skip,
    /// Overwrite the destination unconditionally.
    Overwrite,
    /// Overwrite only when source is newer than destination.
    OverwriteIfNewer,
    /// Rename the destination with a numeric suffix, e.g. `file (2).txt`.
    Rename,
    /// Emit a [`FileOpEvent::Conflict`] and wait for the caller to decide.
    Ask,
}

/// What to do when a per-file error occurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorPolicy {
    /// Abort the entire operation on the first error.
    StopOnFirst,
    /// Record the error and continue with the next file.
    SkipAndContinue,
    /// Retry up to N times, then skip.
    RetryN(u32),
}

/// Current state of an in-progress operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    Scanning,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for OperationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scanning => write!(f, "Scanning"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

// ============================================================================
// Progress
// ============================================================================

/// Live progress information for a running operation.
#[derive(Clone, Debug)]
pub struct OperationProgress {
    pub total_bytes: u64,
    pub copied_bytes: u64,
    pub total_files: u32,
    pub completed_files: u32,
    pub current_file: String,
    pub elapsed_secs: f64,
    pub eta_secs: f64,
    pub bytes_per_sec: u64,
    pub state: OperationState,
}

impl OperationProgress {
    fn new(total_bytes: u64, total_files: u32) -> Self {
        Self {
            total_bytes,
            copied_bytes: 0,
            total_files,
            completed_files: 0,
            current_file: String::new(),
            elapsed_secs: 0.0,
            eta_secs: 0.0,
            bytes_per_sec: 0,
            state: OperationState::Scanning,
        }
    }

    /// Recalculate throughput and ETA from elapsed time and bytes copied.
    fn update_rates(&mut self, elapsed: Duration) {
        self.elapsed_secs = elapsed.as_secs_f64();
        if self.elapsed_secs > 0.0 {
            self.bytes_per_sec = (self.copied_bytes as f64 / self.elapsed_secs) as u64;
        }
        if self.bytes_per_sec > 0 && self.total_bytes > self.copied_bytes {
            let remaining = self.total_bytes - self.copied_bytes;
            self.eta_secs = remaining as f64 / self.bytes_per_sec as f64;
        } else {
            self.eta_secs = 0.0;
        }
    }

    /// Fraction complete in [0.0, 1.0].
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            if self.total_files == 0 {
                return 1.0;
            }
            return f64::from(self.completed_files) / f64::from(self.total_files);
        }
        self.copied_bytes as f64 / self.total_bytes as f64
    }
}

// ============================================================================
// Events
// ============================================================================

/// Events emitted by a running file operation.
#[derive(Clone, Debug)]
pub enum FileOpEvent {
    /// Periodic progress update.
    Progress(OperationProgress),
    /// A conflict needs resolution (only when policy is [`ConflictPolicy::Ask`]).
    Conflict {
        src: PathBuf,
        dest: PathBuf,
        policy: ConflictPolicy,
    },
    /// A per-file error occurred.
    Error {
        path: PathBuf,
        error: String,
    },
    /// The operation finished.
    Complete {
        summary: OperationSummary,
    },
    /// An undo operation is now available.
    UndoAvailable(u64),
}

/// Summary returned when an operation completes.
#[derive(Clone, Debug)]
pub struct OperationSummary {
    pub operation: FileOperation,
    pub total_files: u32,
    pub succeeded: u32,
    pub skipped: u32,
    pub failed: u32,
    pub total_bytes: u64,
    pub elapsed: Duration,
    pub errors: Vec<FileOpError>,
}

/// A per-file error that did not abort the operation.
#[derive(Clone, Debug)]
pub struct FileOpError {
    pub path: PathBuf,
    pub message: String,
}

// ============================================================================
// Plan — individual file actions
// ============================================================================

/// A single action inside an [`OperationPlan`].
#[derive(Clone, Debug)]
pub struct PlannedAction {
    /// Source path.
    pub src: PathBuf,
    /// Destination path (if applicable).
    pub dest: Option<PathBuf>,
    /// Size of the source file (0 for directories).
    pub size: u64,
    /// Whether this action is a directory creation rather than a file copy.
    pub is_dir: bool,
    /// Unique index inside the plan (stable across pause/resume).
    pub index: u32,
}

/// A pre-computed list of individual actions for an operation.
///
/// Created by scanning the source paths. The plan records every file and
/// directory that must be processed, along with the total byte count, so that
/// progress can be reported accurately.
#[derive(Clone, Debug)]
pub struct OperationPlan {
    pub operation: FileOperation,
    pub actions: Vec<PlannedAction>,
    pub total_bytes: u64,
    pub total_files: u32,
    pub conflict_policy: ConflictPolicy,
    pub error_policy: ErrorPolicy,
}

impl OperationPlan {
    /// Build a plan for copying `sources` into `dest_dir`.
    pub fn plan_copy(
        sources: &[PathBuf],
        dest_dir: &Path,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        let mut actions = Vec::new();
        let mut index: u32 = 0;
        let mut total_bytes: u64 = 0;

        for src in sources {
            Self::scan_source(src, dest_dir, &mut actions, &mut index, &mut total_bytes)?;
        }

        let total_files = actions.iter().filter(|a| !a.is_dir).count() as u32;

        Ok(Self {
            operation: FileOperation::Copy,
            actions,
            total_bytes,
            total_files,
            conflict_policy,
            error_policy,
        })
    }

    /// Build a plan for moving `sources` into `dest_dir`.
    pub fn plan_move(
        sources: &[PathBuf],
        dest_dir: &Path,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        let mut plan = Self::plan_copy(sources, dest_dir, conflict_policy, error_policy)?;
        plan.operation = FileOperation::Move;
        Ok(plan)
    }

    /// Build a plan for deleting `sources` permanently.
    pub fn plan_delete(
        sources: &[PathBuf],
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        let mut actions = Vec::new();
        let mut index: u32 = 0;
        let mut total_bytes: u64 = 0;

        for src in sources {
            Self::scan_delete(src, &mut actions, &mut index, &mut total_bytes)?;
        }

        let total_files = actions.iter().filter(|a| !a.is_dir).count() as u32;

        Ok(Self {
            operation: FileOperation::Delete,
            actions,
            total_bytes,
            total_files,
            conflict_policy: ConflictPolicy::Skip, // unused for delete
            error_policy,
        })
    }

    /// Recursively scan a source path and add planned copy actions.
    fn scan_source(
        src: &Path,
        dest_base: &Path,
        actions: &mut Vec<PlannedAction>,
        index: &mut u32,
        total_bytes: &mut u64,
    ) -> io::Result<()> {
        let file_name = src.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "source has no file name")
        })?;
        let dest = dest_base.join(file_name);

        let meta = fs::metadata(src)?;
        if meta.is_dir() {
            // Directory creation action.
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: Some(dest.clone()),
                size: 0,
                is_dir: true,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);

            // Recurse into children.
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                Self::scan_source(&entry.path(), &dest, actions, index, total_bytes)?;
            }
        } else {
            let size = meta.len();
            *total_bytes = total_bytes.saturating_add(size);
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: Some(dest),
                size,
                is_dir: false,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);
        }

        Ok(())
    }

    /// Recursively scan a source path and add planned delete actions.
    ///
    /// Directories are scanned depth-first so that children appear before their
    /// parent in the action list; this allows deletion in forward order.
    fn scan_delete(
        src: &Path,
        actions: &mut Vec<PlannedAction>,
        index: &mut u32,
        total_bytes: &mut u64,
    ) -> io::Result<()> {
        let meta = fs::metadata(src)?;
        if meta.is_dir() {
            // Children first.
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                Self::scan_delete(&entry.path(), actions, index, total_bytes)?;
            }
            // Then the directory itself.
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: None,
                size: 0,
                is_dir: true,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);
        } else {
            let size = meta.len();
            *total_bytes = total_bytes.saturating_add(size);
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: None,
                size,
                is_dir: false,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);
        }
        Ok(())
    }
}

// ============================================================================
// Journal — crash-safe progress tracking
// ============================================================================

/// Crash-safe journal that records completed actions so an interrupted
/// operation can be resumed without re-doing work.
///
/// The journal is a simple line-oriented text file stored at
/// `<dest_dir>/.fileop-journal`. Each line records the index of a completed
/// action. On resume the journal is read and already-completed indices are
/// skipped.
pub struct OperationJournal {
    path: PathBuf,
    completed: HashMap<u32, bool>,
}

impl OperationJournal {
    /// Create or open a journal at `dir/.fileop-journal`.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(".fileop-journal");
        let mut completed = HashMap::new();

        if path.exists() {
            let file = fs::File::open(&path)?;
            let reader = io::BufReader::new(file);
            for line in reader.lines() {
                let line = line?;
                if let Ok(idx) = line.trim().parse::<u32>() {
                    completed.insert(idx, true);
                }
            }
        }

        Ok(Self { path, completed })
    }

    /// Record that action `index` is complete.
    pub fn mark_complete(&mut self, index: u32) -> io::Result<()> {
        self.completed.insert(index, true);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{index}")?;
        file.flush()?;
        Ok(())
    }

    /// Check whether action `index` was already completed (in a prior run).
    pub fn is_complete(&self, index: u32) -> bool {
        self.completed.contains_key(&index)
    }

    /// Remove the journal file (called on successful completion).
    pub fn remove(self) -> io::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    /// Number of completed actions recorded.
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// The path of the journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ============================================================================
// Undo journal
// ============================================================================

/// Records what an operation did so it can be undone.
#[derive(Clone, Debug)]
pub struct UndoRecord {
    pub id: u64,
    pub operation: FileOperation,
    /// (source, destination) pairs that were acted on.
    pub entries: Vec<(PathBuf, Option<PathBuf>)>,
    pub timestamp: SystemTime,
}

/// Keeps a stack of undoable operations.
pub struct UndoStack {
    records: Vec<UndoRecord>,
    next_id: u64,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            next_id: 1,
        }
    }

    /// Push a new undo record and return its id.
    pub fn push(&mut self, operation: FileOperation, entries: Vec<(PathBuf, Option<PathBuf>)>) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.records.push(UndoRecord {
            id,
            operation,
            entries,
            timestamp: SystemTime::now(),
        });
        id
    }

    /// Pop the most recent record for undo.
    pub fn pop(&mut self) -> Option<UndoRecord> {
        self.records.pop()
    }

    /// Peek at the most recent record without removing it.
    pub fn peek(&self) -> Option<&UndoRecord> {
        self.records.last()
    }

    /// True when there is nothing to undo.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of undo records.
    pub fn len(&self) -> usize {
        self.records.len()
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Conflict resolution helpers
// ============================================================================

/// Generate a non-conflicting destination name.
///
/// Given `/dest/file.txt`, tries `/dest/file (2).txt`, `/dest/file (3).txt`, etc.
pub fn resolve_rename(dest: &Path) -> PathBuf {
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = dest.parent().unwrap_or(Path::new(""));

    for n in 2u32..10_000 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    parent.join(format!("{stem} (renamed){ext}"))
}

/// Determine whether two paths are on the same filesystem / device.
///
/// This is a best-effort heuristic. On the real OS we would compare device IDs
/// from `stat`. Here we compare the root/prefix component as a proxy.
pub fn same_device(a: &Path, b: &Path) -> bool {
    // Compare the first component (mount point heuristic).
    let root_a = a.components().next();
    let root_b = b.components().next();
    root_a == root_b
}

/// Determine whether `src` is newer than `dest` based on modification time.
fn source_is_newer(src: &Path, dest: &Path) -> bool {
    let src_time = fs::metadata(src).ok().and_then(|m| m.modified().ok());
    let dest_time = fs::metadata(dest).ok().and_then(|m| m.modified().ok());
    match (src_time, dest_time) {
        (Some(s), Some(d)) => s > d,
        _ => true, // if we can't determine, treat source as newer
    }
}

// ============================================================================
// Executor — runs a plan
// ============================================================================

/// Configuration for running an operation plan.
pub struct ExecutorConfig {
    pub conflict_policy: ConflictPolicy,
    pub error_policy: ErrorPolicy,
}

/// Execute an [`OperationPlan`], returning progress, a summary, and undo info.
///
/// Writes completed actions to a journal in the destination directory so the
/// operation can be resumed if interrupted.
pub struct OperationExecutor {
    plan: OperationPlan,
    progress: OperationProgress,
    undo_entries: Vec<(PathBuf, Option<PathBuf>)>,
    errors: Vec<FileOpError>,
    events: Vec<FileOpEvent>,
    skipped: u32,
    started: Option<Instant>,
}

impl OperationExecutor {
    pub fn new(plan: OperationPlan) -> Self {
        let progress = OperationProgress::new(plan.total_bytes, plan.total_files);
        Self {
            plan,
            progress,
            undo_entries: Vec::new(),
            errors: Vec::new(),
            events: Vec::new(),
            skipped: 0,
            started: None,
        }
    }

    /// Run the full operation synchronously, collecting events.
    ///
    /// Returns the events emitted during execution.
    pub fn execute(&mut self) -> Vec<FileOpEvent> {
        self.started = Some(Instant::now());
        self.progress.state = OperationState::Running;

        let dest_dir = self.journal_dir();
        let journal = match OperationJournal::open(&dest_dir) {
            Ok(j) => j,
            Err(e) => {
                self.progress.state = OperationState::Failed;
                self.events.push(FileOpEvent::Error {
                    path: dest_dir,
                    error: format!("failed to open journal: {e}"),
                });
                return std::mem::take(&mut self.events);
            }
        };

        self.run_actions(journal);
        std::mem::take(&mut self.events)
    }

    /// Return a copy of the current progress.
    pub fn progress(&self) -> &OperationProgress {
        &self.progress
    }

    /// Build undo entries from what was done.
    pub fn into_undo_entries(self) -> (FileOperation, Vec<(PathBuf, Option<PathBuf>)>) {
        (self.plan.operation, self.undo_entries)
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    fn journal_dir(&self) -> PathBuf {
        // Use the first action's destination parent, or fall back to cwd.
        self.plan
            .actions
            .iter()
            .find_map(|a| a.dest.as_ref().and_then(|d| d.parent().map(Path::to_path_buf)))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn run_actions(&mut self, mut journal: OperationJournal) {
        // Clone values we need to iterate over since we cannot borrow self
        // immutably (via plan.actions) and mutably (via self.handle_*) at
        // the same time.
        let actions: Vec<PlannedAction> = self.plan.actions.clone();
        let operation = self.plan.operation.clone();
        let conflict_policy = self.plan.conflict_policy;
        let error_policy = self.plan.error_policy;

        for action in &actions {
            if self.progress.state == OperationState::Cancelled {
                break;
            }

            // Skip actions already completed in a previous (interrupted) run.
            if journal.is_complete(action.index) {
                if !action.is_dir {
                    self.progress.completed_files += 1;
                    self.progress.copied_bytes = self.progress.copied_bytes.saturating_add(action.size);
                }
                continue;
            }

            self.progress.current_file = action.src.to_string_lossy().to_string();

            let result = match operation {
                FileOperation::Copy | FileOperation::Move => {
                    self.execute_copy_action(action, conflict_policy)
                }
                FileOperation::Delete => self.execute_delete_action(action),
                FileOperation::Recycle => self.execute_recycle_action(action),
                FileOperation::Restore => self.execute_restore_action(action),
            };

            match result {
                Ok(ActionOutcome::Done) => {
                    let _ = journal.mark_complete(action.index);
                    if !action.is_dir {
                        self.progress.completed_files += 1;
                        self.progress.copied_bytes =
                            self.progress.copied_bytes.saturating_add(action.size);
                    }
                }
                Ok(ActionOutcome::Skipped) => {
                    let _ = journal.mark_complete(action.index);
                    self.skipped += 1;
                    if !action.is_dir {
                        self.progress.completed_files += 1;
                        // Count skipped bytes in progress so ETA stays accurate.
                        self.progress.copied_bytes =
                            self.progress.copied_bytes.saturating_add(action.size);
                    }
                }
                Err(e) => {
                    let err = FileOpError {
                        path: action.src.clone(),
                        message: e.to_string(),
                    };
                    self.events.push(FileOpEvent::Error {
                        path: action.src.clone(),
                        error: e.to_string(),
                    });
                    self.errors.push(err);

                    match error_policy {
                        ErrorPolicy::StopOnFirst => {
                            self.progress.state = OperationState::Failed;
                            break;
                        }
                        ErrorPolicy::SkipAndContinue => {
                            self.skipped += 1;
                            continue;
                        }
                        ErrorPolicy::RetryN(max) => {
                            let mut retried = false;
                            for _ in 0..max {
                                let retry = match operation {
                                    FileOperation::Copy | FileOperation::Move => {
                                        self.execute_copy_action(action, conflict_policy)
                                    }
                                    FileOperation::Delete => self.execute_delete_action(action),
                                    FileOperation::Recycle => self.execute_recycle_action(action),
                                    FileOperation::Restore => self.execute_restore_action(action),
                                };
                                if let Ok(outcome) = retry {
                                    let _ = journal.mark_complete(action.index);
                                    if matches!(outcome, ActionOutcome::Skipped) {
                                        self.skipped += 1;
                                    }
                                    if !action.is_dir {
                                        self.progress.completed_files += 1;
                                        self.progress.copied_bytes = self
                                            .progress
                                            .copied_bytes
                                            .saturating_add(action.size);
                                    }
                                    retried = true;
                                    break;
                                }
                            }
                            if !retried {
                                self.skipped += 1;
                            }
                        }
                    }
                }
            }

            // Emit progress periodically.
            if let Some(start) = self.started {
                self.progress.update_rates(start.elapsed());
            }
            self.events.push(FileOpEvent::Progress(self.progress.clone()));
        }

        // For Move: after all copies succeed, delete sources.
        if operation == FileOperation::Move && self.progress.state != OperationState::Failed {
            for action in &actions {
                if action.is_dir {
                    // Directories are removed in reverse order (children first).
                    continue;
                }
                let _ = fs::remove_file(&action.src);
            }
            // Remove source directories in reverse order.
            for action in actions.iter().rev() {
                if action.is_dir {
                    let _ = fs::remove_dir(&action.src);
                }
            }
        }

        // Finish up.
        if self.progress.state == OperationState::Running {
            self.progress.state = OperationState::Completed;
        }
        if let Some(start) = self.started {
            self.progress.update_rates(start.elapsed());
        }

        let elapsed = self.started.map_or(Duration::ZERO, |s| s.elapsed());
        let succeeded = self
            .progress
            .completed_files
            .saturating_sub(self.skipped);

        self.events.push(FileOpEvent::Complete {
            summary: OperationSummary {
                operation: operation.clone(),
                total_files: self.plan.total_files,
                succeeded,
                skipped: self.skipped,
                failed: self.errors.len() as u32,
                total_bytes: self.plan.total_bytes,
                elapsed,
                errors: self.errors.clone(),
            },
        });

        // Clean up journal on success.
        if self.progress.state == OperationState::Completed {
            let _ = journal.remove();
        }
    }

    fn execute_copy_action(
        &mut self,
        action: &PlannedAction,
        conflict: ConflictPolicy,
    ) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "copy action has no destination")
        })?;

        if action.is_dir {
            if !dest.exists() {
                fs::create_dir_all(dest)?;
            }
            self.undo_entries.push((action.src.clone(), Some(dest.clone())));
            return Ok(ActionOutcome::Done);
        }

        // Conflict resolution.
        if dest.exists() {
            match conflict {
                ConflictPolicy::Skip => return Ok(ActionOutcome::Skipped),
                ConflictPolicy::Overwrite => { /* continue to overwrite */ }
                ConflictPolicy::OverwriteIfNewer => {
                    if !source_is_newer(&action.src, dest) {
                        return Ok(ActionOutcome::Skipped);
                    }
                }
                ConflictPolicy::Rename => {
                    let renamed = resolve_rename(dest);
                    self.atomic_copy_file(&action.src, &renamed)?;
                    self.undo_entries.push((action.src.clone(), Some(renamed)));
                    return Ok(ActionOutcome::Done);
                }
                ConflictPolicy::Ask => {
                    self.events.push(FileOpEvent::Conflict {
                        src: action.src.clone(),
                        dest: dest.clone(),
                        policy: conflict,
                    });
                    // In a real async implementation the caller would respond.
                    // For now, skip.
                    return Ok(ActionOutcome::Skipped);
                }
            }
        }

        self.atomic_copy_file(&action.src, dest)?;
        self.undo_entries.push((action.src.clone(), Some(dest.clone())));
        Ok(ActionOutcome::Done)
    }

    fn execute_delete_action(&mut self, action: &PlannedAction) -> io::Result<ActionOutcome> {
        if action.is_dir {
            fs::remove_dir(&action.src)?;
        } else {
            fs::remove_file(&action.src)?;
        }
        self.undo_entries.push((action.src.clone(), None));
        Ok(ActionOutcome::Done)
    }

    fn execute_recycle_action(&mut self, action: &PlannedAction) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "recycle action has no destination")
        })?;
        if action.is_dir {
            if !dest.exists() {
                fs::create_dir_all(dest)?;
            }
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&action.src, dest)?;
        }
        self.undo_entries.push((action.src.clone(), Some(dest.clone())));
        Ok(ActionOutcome::Done)
    }

    fn execute_restore_action(&mut self, action: &PlannedAction) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "restore action has no destination")
        })?;
        if action.is_dir {
            if !dest.exists() {
                fs::create_dir_all(dest)?;
            }
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&action.src, dest)?;
        }
        self.undo_entries.push((action.src.clone(), Some(dest.clone())));
        Ok(ActionOutcome::Done)
    }

    /// Copy `src` to a temporary name next to `dest`, then rename atomically.
    fn atomic_copy_file(&self, src: &Path, dest: &Path) -> io::Result<()> {
        let parent = dest.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;

        // Temporary name: <dest>.fileop-tmp
        let tmp_name = format!(
            ".{}.fileop-tmp",
            dest.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string())
        );
        let tmp_path = parent.join(tmp_name);

        fs::copy(src, &tmp_path)?;

        // Attempt to preserve modification timestamp.
        if let Ok(src_meta) = fs::metadata(src)
            && let Ok(mtime) = src_meta.modified() {
                // Best-effort: not all platforms support filetime setting in
                // std, but our OS will.
                let _ = set_file_mtime(&tmp_path, mtime);
            }

        // Atomic rename into final position.
        fs::rename(&tmp_path, dest)?;
        Ok(())
    }
}

/// Internal result of processing a single action.
enum ActionOutcome {
    Done,
    Skipped,
}

/// Best-effort modification time preservation.
///
/// The real OS will expose this via a proper syscall. The std implementation
/// may or may not support it, so we silently ignore errors.
fn set_file_mtime(path: &Path, _mtime: SystemTime) -> io::Result<()> {
    // Placeholder: on OurOS this would call the appropriate filesystem
    // syscall to set the modification time. On the host (for testing)
    // std::fs does not provide a portable setter, so this is a no-op.
    let _ = path;
    Ok(())
}

// ============================================================================
// Recycle bin
// ============================================================================

/// Metadata for a recycled item.
#[derive(Clone, Debug)]
pub struct RecycleEntry {
    /// Unique identifier for this entry.
    pub id: String,
    /// Original absolute path before recycling.
    pub original_path: PathBuf,
    /// When the item was recycled.
    pub recycled_at: SystemTime,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Whether this is a directory.
    pub is_dir: bool,
}

/// Manages the recycle bin at `~/.recycle/`.
///
/// Layout on disk:
/// ```text
/// ~/.recycle/
///     <hash>/
///         meta.txt        # original_path, recycled_at
///         data/           # the actual file or directory contents
/// ```
pub struct RecycleBin {
    root: PathBuf,
    /// Items older than this are eligible for auto-purge.
    max_age: Duration,
}

impl RecycleBin {
    /// Create a new `RecycleBin` rooted at `root`.
    ///
    /// `max_age` is the auto-purge threshold (default 30 days).
    pub fn new(root: PathBuf, max_age: Duration) -> Self {
        Self { root, max_age }
    }

    /// Create a `RecycleBin` at the default location (`~/.recycle/`)
    /// with 30-day auto-purge.
    pub fn default_location() -> Self {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        Self::new(home.join(".recycle"), Duration::from_secs(30 * 24 * 60 * 60))
    }

    /// Move `path` into the recycle bin and return the entry id.
    pub fn recycle(&self, path: &Path) -> io::Result<String> {
        let id = self.make_id(path);
        let entry_dir = self.root.join(&id);
        let data_path = entry_dir.join("data");

        fs::create_dir_all(&entry_dir)?;

        // Write metadata.
        let meta_path = entry_dir.join("meta.txt");
        let mut meta_file = fs::File::create(&meta_path)?;
        writeln!(meta_file, "{}", path.display())?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        writeln!(meta_file, "{now}")?;
        meta_file.flush()?;

        // Move the actual data.
        fs::rename(path, &data_path)?;

        Ok(id)
    }

    /// Restore a recycled item back to its original location.
    pub fn restore(&self, entry_id: &str) -> io::Result<PathBuf> {
        let entry = self.read_entry(entry_id)?;
        let data_path = self.root.join(entry_id).join("data");

        // Ensure parent directory exists.
        if let Some(parent) = entry.original_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::rename(&data_path, &entry.original_path)?;

        // Clean up the entry directory.
        let entry_dir = self.root.join(entry_id);
        let _ = fs::remove_file(entry_dir.join("meta.txt"));
        let _ = fs::remove_dir(&entry_dir);

        Ok(entry.original_path)
    }

    /// List all items in the recycle bin.
    pub fn list(&self) -> io::Result<Vec<RecycleEntry>> {
        let mut entries = Vec::new();

        if !self.root.exists() {
            return Ok(entries);
        }

        for dir_entry in fs::read_dir(&self.root)? {
            let dir_entry = dir_entry?;
            if !dir_entry.path().is_dir() {
                continue;
            }
            let id = dir_entry.file_name().to_string_lossy().to_string();
            match self.read_entry(&id) {
                Ok(entry) => entries.push(entry),
                Err(_) => continue,
            }
        }

        // Most recently recycled first.
        entries.sort_by_key(|e| std::cmp::Reverse(e.recycled_at));
        Ok(entries)
    }

    /// Permanently delete all items in the recycle bin.
    pub fn empty(&self) -> io::Result<u32> {
        let entries = self.list()?;
        let mut count = 0u32;
        for entry in &entries {
            let entry_dir = self.root.join(&entry.id);
            if fs::remove_dir_all(&entry_dir).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Permanently delete items older than `max_age`.
    pub fn purge_old(&self) -> io::Result<u32> {
        let entries = self.list()?;
        let now = SystemTime::now();
        let mut count = 0u32;

        for entry in &entries {
            let age = now
                .duration_since(entry.recycled_at)
                .unwrap_or(Duration::ZERO);
            if age > self.max_age {
                let entry_dir = self.root.join(&entry.id);
                if fs::remove_dir_all(&entry_dir).is_ok() {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Set the auto-purge age threshold.
    pub fn set_max_age(&mut self, age: Duration) {
        self.max_age = age;
    }

    /// Current auto-purge age threshold.
    pub fn max_age(&self) -> Duration {
        self.max_age
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    /// Generate a unique entry id from the file path and current time.
    fn make_id(&self, path: &Path) -> String {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        // Simple hash to keep directory names manageable.
        let hash = ts ^ (name.len() as u128).wrapping_mul(0x517cc1b727220a95);
        format!("{name}_{hash:016x}")
    }

    /// Read the metadata for a recycled entry.
    fn read_entry(&self, id: &str) -> io::Result<RecycleEntry> {
        let entry_dir = self.root.join(id);
        let meta_path = entry_dir.join("meta.txt");
        let content = fs::read_to_string(&meta_path)?;
        let mut lines = content.lines();

        let original_path = PathBuf::from(
            lines
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path in meta"))?,
        );
        let ts_secs: u64 = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing timestamp in meta"))?
            .trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad timestamp"))?;

        let recycled_at = SystemTime::UNIX_EPOCH + Duration::from_secs(ts_secs);

        let data_path = entry_dir.join("data");
        let (size, is_dir) = if data_path.exists() {
            let meta = fs::metadata(&data_path)?;
            (meta.len(), meta.is_dir())
        } else {
            (0, false)
        };

        Ok(RecycleEntry {
            id: id.to_string(),
            original_path,
            recycled_at,
            size,
            is_dir,
        })
    }
}

// ============================================================================
// Convenience: execute an undo
// ============================================================================

/// Undo a previously completed operation.
///
/// - Copy undo: delete the copied files.
/// - Move undo: move files back to their original locations.
/// - Delete/Recycle undo: restore from recycle bin (if entries are present).
pub fn execute_undo(record: &UndoRecord) -> io::Result<()> {
    match record.operation {
        FileOperation::Copy => {
            // Delete all destination files that were created.
            for (_src, dest) in record.entries.iter().rev() {
                if let Some(d) = dest {
                    if d.is_dir() {
                        let _ = fs::remove_dir(d);
                    } else if d.exists() {
                        fs::remove_file(d)?;
                    }
                }
            }
        }
        FileOperation::Move => {
            // Move files back from destination to source.
            for (src, dest) in &record.entries {
                if let Some(d) = dest
                    && d.exists() {
                        if let Some(parent) = src.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::rename(d, src)?;
                    }
            }
        }
        FileOperation::Delete | FileOperation::Recycle => {
            // Restore: entries are (original_path, recycle_dest).
            for (src, dest) in &record.entries {
                if let Some(d) = dest
                    && d.exists() {
                        if let Some(parent) = src.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::rename(d, src)?;
                    }
            }
        }
        FileOperation::Restore => {
            // Undo restore = recycle again: move from original back to bin.
            for (src, dest) in &record.entries {
                if let Some(d) = dest
                    && src.exists() {
                        if let Some(parent) = d.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::rename(src, d)?;
                    }
            }
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as IoWrite;
    use std::path::PathBuf;

    /// Create a temporary directory with a unique name under the system temp dir.
    fn temp_dir(label: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("fileops_test_{label}_{ts}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write a file with the given content.
    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    /// Read file to string.
    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    // ----------------------------------------------------------------
    // Plan generation tests
    // ----------------------------------------------------------------

    #[test]
    fn plan_copy_single_file() {
        let src_dir = temp_dir("plan_copy_single_src");
        let dst_dir = temp_dir("plan_copy_single_dst");

        write_file(&src_dir.join("hello.txt"), "hello world");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("hello.txt")],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        assert_eq!(plan.total_files, 1);
        assert_eq!(plan.total_bytes, 11); // "hello world" = 11 bytes
        assert_eq!(plan.actions.len(), 1);
        assert!(!plan.actions[0].is_dir);
        assert_eq!(plan.actions[0].size, 11);

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn plan_copy_directory_tree() {
        let src_dir = temp_dir("plan_copy_tree_src");
        let dst_dir = temp_dir("plan_copy_tree_dst");

        // src/
        //   a.txt  (5 bytes)
        //   sub/
        //     b.txt (3 bytes)
        write_file(&src_dir.join("tree").join("a.txt"), "aaaaa");
        write_file(&src_dir.join("tree").join("sub").join("b.txt"), "bbb");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("tree")],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();

        assert_eq!(plan.total_files, 2);
        assert_eq!(plan.total_bytes, 8);
        // Should have: dir(tree), file(a.txt), dir(sub), file(b.txt)
        let dir_count = plan.actions.iter().filter(|a| a.is_dir).count();
        let file_count = plan.actions.iter().filter(|a| !a.is_dir).count();
        assert_eq!(dir_count, 2);
        assert_eq!(file_count, 2);

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn plan_delete() {
        let src_dir = temp_dir("plan_delete_src");

        write_file(&src_dir.join("data").join("x.txt"), "xxxx");
        write_file(&src_dir.join("data").join("y.txt"), "yy");

        let plan = OperationPlan::plan_delete(
            &[src_dir.join("data")],
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        assert_eq!(plan.total_files, 2);
        assert_eq!(plan.total_bytes, 6);
        // Directories should come after their children (depth-first).
        let last = plan.actions.last().unwrap();
        assert!(last.is_dir);
        assert_eq!(last.src, src_dir.join("data"));

        let _ = fs::remove_dir_all(&src_dir);
    }

    // ----------------------------------------------------------------
    // Conflict resolution tests
    // ----------------------------------------------------------------

    #[test]
    fn resolve_rename_basic() {
        let dir = temp_dir("resolve_rename");
        let original = dir.join("file.txt");
        write_file(&original, "original");

        let renamed = resolve_rename(&original);
        assert_eq!(renamed, dir.join("file (2).txt"));

        // Create file (2) and check that (3) is chosen next.
        write_file(&renamed, "copy2");
        let renamed2 = resolve_rename(&original);
        assert_eq!(renamed2, dir.join("file (3).txt"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_rename_no_extension() {
        let dir = temp_dir("resolve_rename_noext");
        let original = dir.join("Makefile");
        write_file(&original, "data");

        let renamed = resolve_rename(&original);
        assert_eq!(renamed, dir.join("Makefile (2)"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_device_detection() {
        // Paths sharing the same root component should be same-device.
        assert!(same_device(
            Path::new("/home/user/a"),
            Path::new("/home/user/b")
        ));
        // Different roots.
        // Note: on Unix "/" is always the root, so this tests the prefix logic.
        // On our OS different mount points would have different first components.
    }

    // ----------------------------------------------------------------
    // Journal tests
    // ----------------------------------------------------------------

    #[test]
    fn journal_write_and_read() {
        let dir = temp_dir("journal_rw");

        {
            let mut j = OperationJournal::open(&dir).unwrap();
            assert_eq!(j.completed_count(), 0);
            j.mark_complete(0).unwrap();
            j.mark_complete(3).unwrap();
            j.mark_complete(7).unwrap();
        }

        // Re-open and verify.
        let j2 = OperationJournal::open(&dir).unwrap();
        assert_eq!(j2.completed_count(), 3);
        assert!(j2.is_complete(0));
        assert!(j2.is_complete(3));
        assert!(j2.is_complete(7));
        assert!(!j2.is_complete(1));
        assert!(!j2.is_complete(999));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_resume_skips_completed() {
        let src_dir = temp_dir("journal_resume_src");
        let dst_dir = temp_dir("journal_resume_dst");

        write_file(&src_dir.join("a.txt"), "aaa");
        write_file(&src_dir.join("b.txt"), "bbb");

        // Pre-write a journal marking action 0 (the first file) as done.
        {
            let mut j = OperationJournal::open(&dst_dir).unwrap();
            j.mark_complete(0).unwrap();
        }

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("a.txt"), src_dir.join("b.txt")],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();

        // Should complete without error.
        let complete = events.iter().find(|e| matches!(e, FileOpEvent::Complete { .. }));
        assert!(complete.is_some());

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn journal_remove_on_completion() {
        let dir = temp_dir("journal_remove");

        let mut j = OperationJournal::open(&dir).unwrap();
        j.mark_complete(0).unwrap();
        let jpath = j.path().to_path_buf();
        assert!(jpath.exists());

        j.remove().unwrap();
        assert!(!jpath.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------------
    // Progress calculation tests
    // ----------------------------------------------------------------

    #[test]
    fn progress_fraction_empty() {
        let p = OperationProgress::new(0, 0);
        assert!((p.fraction() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_fraction_by_bytes() {
        let mut p = OperationProgress::new(1000, 10);
        p.copied_bytes = 500;
        assert!((p.fraction() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_fraction_by_files_when_zero_bytes() {
        let mut p = OperationProgress::new(0, 4);
        p.completed_files = 2;
        assert!((p.fraction() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_update_rates() {
        let mut p = OperationProgress::new(2000, 10);
        p.copied_bytes = 1000;
        p.update_rates(Duration::from_secs(2));

        assert_eq!(p.bytes_per_sec, 500);
        assert!((p.eta_secs - 2.0).abs() < 0.01);
        assert!((p.elapsed_secs - 2.0).abs() < f64::EPSILON);
    }

    // ----------------------------------------------------------------
    // Recycle bin tests
    // ----------------------------------------------------------------

    #[test]
    fn recycle_and_restore() {
        let dir = temp_dir("recycle_restore");
        let bin_root = dir.join("bin");
        let file_path = dir.join("important.txt");
        write_file(&file_path, "important data");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));

        // Recycle.
        let id = bin.recycle(&file_path).unwrap();
        assert!(!file_path.exists());

        // List.
        let entries = bin.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original_path, file_path);

        // Restore.
        let restored = bin.restore(&id).unwrap();
        assert_eq!(restored, file_path);
        assert!(file_path.exists());
        assert_eq!(read_file(&file_path), "important data");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recycle_bin_empty() {
        let dir = temp_dir("recycle_empty");
        let bin_root = dir.join("bin");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));

        write_file(&dir.join("a.txt"), "aaa");
        write_file(&dir.join("b.txt"), "bbb");

        bin.recycle(&dir.join("a.txt")).unwrap();
        bin.recycle(&dir.join("b.txt")).unwrap();

        assert_eq!(bin.list().unwrap().len(), 2);

        let removed = bin.empty().unwrap();
        assert_eq!(removed, 2);
        assert_eq!(bin.list().unwrap().len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recycle_bin_list_empty() {
        let dir = temp_dir("recycle_list_empty");
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_secs(86400));
        let entries = bin.list().unwrap();
        assert!(entries.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recycle_bin_purge_old() {
        let dir = temp_dir("recycle_purge");
        let bin_root = dir.join("bin");

        // Max age of 0 seconds means everything is "old".
        let bin = RecycleBin::new(bin_root, Duration::from_secs(0));

        write_file(&dir.join("old.txt"), "old");
        bin.recycle(&dir.join("old.txt")).unwrap();

        let purged = bin.purge_old().unwrap();
        assert_eq!(purged, 1);
        assert_eq!(bin.list().unwrap().len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------------
    // Undo tests
    // ----------------------------------------------------------------

    #[test]
    fn undo_stack_push_pop() {
        let mut stack = UndoStack::new();
        assert!(stack.is_empty());

        let id1 = stack.push(FileOperation::Copy, vec![]);
        let id2 = stack.push(FileOperation::Move, vec![]);
        assert_eq!(stack.len(), 2);
        assert!(id2 > id1);

        let rec = stack.pop().unwrap();
        assert_eq!(rec.operation, FileOperation::Move);
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn undo_copy_deletes_dest() {
        let dir = temp_dir("undo_copy");
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        write_file(&src, "data");
        write_file(&dst, "data");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Copy,
            entries: vec![(src.clone(), Some(dst.clone()))],
            timestamp: SystemTime::now(),
        };

        execute_undo(&record).unwrap();
        assert!(!dst.exists());
        // Source should still exist (copy undo only removes the destination).
        assert!(src.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_move_restores_src() {
        let dir = temp_dir("undo_move");
        let src = dir.join("original.txt");
        let dst = dir.join("moved.txt");
        write_file(&dst, "moved data");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Move,
            entries: vec![(src.clone(), Some(dst.clone()))],
            timestamp: SystemTime::now(),
        };

        execute_undo(&record).unwrap();
        assert!(src.exists());
        assert!(!dst.exists());
        assert_eq!(read_file(&src), "moved data");

        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------------
    // Full execution tests
    // ----------------------------------------------------------------

    #[test]
    fn execute_copy_single_file() {
        let src_dir = temp_dir("exec_copy_src");
        let dst_dir = temp_dir("exec_copy_dst");
        write_file(&src_dir.join("test.txt"), "test content");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("test.txt")],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();

        // File should exist at destination.
        assert!(dst_dir.join("test.txt").exists());
        assert_eq!(read_file(&dst_dir.join("test.txt")), "test content");
        // Source should still exist.
        assert!(src_dir.join("test.txt").exists());

        // Should have a Complete event.
        let complete = events.iter().find_map(|e| {
            if let FileOpEvent::Complete { summary } = e {
                Some(summary)
            } else {
                None
            }
        });
        assert!(complete.is_some());
        let summary = complete.unwrap();
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.failed, 0);

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_copy_with_skip_conflict() {
        let src_dir = temp_dir("exec_copy_skip_src");
        let dst_dir = temp_dir("exec_copy_skip_dst");

        write_file(&src_dir.join("conflict.txt"), "new content");
        write_file(&dst_dir.join("conflict.txt"), "old content");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("conflict.txt")],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        // Destination should retain old content.
        assert_eq!(read_file(&dst_dir.join("conflict.txt")), "old content");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_copy_with_overwrite_conflict() {
        let src_dir = temp_dir("exec_copy_ow_src");
        let dst_dir = temp_dir("exec_copy_ow_dst");

        write_file(&src_dir.join("file.txt"), "new");
        write_file(&dst_dir.join("file.txt"), "old");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("file.txt")],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert_eq!(read_file(&dst_dir.join("file.txt")), "new");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_copy_with_rename_conflict() {
        let src_dir = temp_dir("exec_copy_rn_src");
        let dst_dir = temp_dir("exec_copy_rn_dst");

        write_file(&src_dir.join("file.txt"), "new");
        write_file(&dst_dir.join("file.txt"), "existing");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("file.txt")],
            &dst_dir,
            ConflictPolicy::Rename,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        // Both should exist.
        assert_eq!(read_file(&dst_dir.join("file.txt")), "existing");
        assert!(dst_dir.join("file (2).txt").exists());
        assert_eq!(read_file(&dst_dir.join("file (2).txt")), "new");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_copy_directory() {
        let src_dir = temp_dir("exec_copy_dir_src");
        let dst_dir = temp_dir("exec_copy_dir_dst");

        write_file(&src_dir.join("mydir").join("a.txt"), "aaa");
        write_file(&src_dir.join("mydir").join("sub").join("b.txt"), "bb");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("mydir")],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(dst_dir.join("mydir").join("a.txt").exists());
        assert!(dst_dir.join("mydir").join("sub").join("b.txt").exists());
        assert_eq!(read_file(&dst_dir.join("mydir").join("a.txt")), "aaa");
        assert_eq!(read_file(&dst_dir.join("mydir").join("sub").join("b.txt")), "bb");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_move_removes_source() {
        let src_dir = temp_dir("exec_move_src");
        let dst_dir = temp_dir("exec_move_dst");

        write_file(&src_dir.join("moveme.txt"), "move data");

        let plan = OperationPlan::plan_move(
            &[src_dir.join("moveme.txt")],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(dst_dir.join("moveme.txt").exists());
        assert!(!src_dir.join("moveme.txt").exists());

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn execute_delete() {
        let dir = temp_dir("exec_delete");
        write_file(&dir.join("delme").join("x.txt"), "xxx");
        write_file(&dir.join("delme").join("y.txt"), "yy");

        let plan = OperationPlan::plan_delete(
            &[dir.join("delme")],
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(!dir.join("delme").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
