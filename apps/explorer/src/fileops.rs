//! Atomic file operations for the Slate OS file explorer.
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
    Error { path: PathBuf, error: String },
    /// The operation finished.
    Complete { summary: OperationSummary },
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
    /// A fingerprint of this plan, used to tell whether a journal found in the
    /// destination directory belongs to it.
    ///
    /// Not a cryptographic hash and does not need to be: it exists to catch a
    /// journal left behind by a *different* operation, not one forged by an
    /// attacker. It covers what makes two plans different work — the operation
    /// and every source/destination pair, in order — so a plan that resumes
    /// really is the plan that was interrupted.
    #[must_use]
    pub fn id(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.operation).hash(&mut hasher);
        self.actions.len().hash(&mut hasher);
        for action in &self.actions {
            action.index.hash(&mut hasher);
            action.src.hash(&mut hasher);
            action.dest.hash(&mut hasher);
            action.is_dir.hash(&mut hasher);
        }
        hasher.finish()
    }

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
    pub fn plan_delete(sources: &[PathBuf], error_policy: ErrorPolicy) -> io::Result<Self> {
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
/// The journal is a line-oriented text file stored at
/// `<dest_dir>/.fileop-journal`. The first line is `plan <id>`; each later line
/// is the index of a finished action, with ` skip` appended when the action
/// finished *without* transferring anything. On resume the journal is read and
/// already-finished indices are skipped.
///
/// # Why the header exists
///
/// The journal lives in the destination directory and its indices are
/// plan-relative, so without an identity check a journal left behind by one
/// operation would be read as progress for the *next* operation into the same
/// directory — and every action whose index collided would be silently treated
/// as already done, i.e. never copied. A journal that cannot be attributed to
/// the plan being run is therefore discarded rather than trusted.
///
/// # Why the skip flag exists
///
/// A Move deletes each source after its copy succeeds. An action skipped by
/// conflict policy did *not* copy anything — the file at the destination is
/// some pre-existing file, not this source — so deleting the source would
/// destroy the only copy of the user's data.
pub struct OperationJournal {
    path: PathBuf,
    /// Action index -> whether it actually transferred data.
    completed: HashMap<u32, bool>,
}

impl OperationJournal {
    /// Create or open the journal for `plan_id` at `dir/.fileop-journal`.
    ///
    /// A journal belonging to a different plan — or one with no header, which
    /// is the same thing as far as trust goes — is deleted and started over.
    pub fn open(dir: &Path, plan_id: u64) -> io::Result<Self> {
        let path = dir.join(".fileop-journal");
        let mut completed = HashMap::new();

        if path.exists() {
            let file = fs::File::open(&path)?;
            let reader = io::BufReader::new(file);
            let mut lines = reader.lines();
            let header = lines.next().transpose()?.unwrap_or_default();
            if header.strip_prefix("plan ").and_then(|id| id.trim().parse::<u64>().ok())
                == Some(plan_id)
            {
                for line in lines {
                    let line = line?;
                    let (idx, transferred) = match line.trim().strip_suffix(" skip") {
                        Some(rest) => (rest.trim(), false),
                        None => (line.trim(), true),
                    };
                    if let Ok(idx) = idx.parse::<u32>() {
                        completed.insert(idx, transferred);
                    }
                }
            } else {
                // Not ours. Removing it is safe: the worst case is redoing
                // work, whereas trusting it means skipping work that was never
                // done.
                fs::remove_file(&path)?;
            }
        }

        if !path.exists() {
            let mut file = fs::File::create(&path)?;
            writeln!(file, "plan {plan_id}")?;
            file.flush()?;
        }

        Ok(Self { path, completed })
    }

    /// Record that action `index` finished and transferred its data.
    pub fn mark_complete(&mut self, index: u32) -> io::Result<()> {
        self.record(index, true)
    }

    /// Record that action `index` finished *without* transferring anything.
    pub fn mark_skipped(&mut self, index: u32) -> io::Result<()> {
        self.record(index, false)
    }

    fn record(&mut self, index: u32, transferred: bool) -> io::Result<()> {
        self.completed.insert(index, transferred);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        if transferred {
            writeln!(file, "{index}")?;
        } else {
            writeln!(file, "{index} skip")?;
        }
        file.flush()?;
        Ok(())
    }

    /// Check whether action `index` was already completed (in a prior run).
    pub fn is_complete(&self, index: u32) -> bool {
        self.completed.contains_key(&index)
    }

    /// Whether action `index` actually copied its data to the destination.
    ///
    /// The question a Move must ask before deleting a source. `false` for an
    /// action that was skipped, that failed, or that has not run.
    pub fn transferred(&self, index: u32) -> bool {
        self.completed.get(&index) == Some(&true)
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
    pub fn push(
        &mut self,
        operation: FileOperation,
        entries: Vec<(PathBuf, Option<PathBuf>)>,
    ) -> u64 {
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
        let plan_id = self.plan.id();
        let journal = match OperationJournal::open(&dest_dir, plan_id) {
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
            .find_map(|a| {
                a.dest
                    .as_ref()
                    .and_then(|d| d.parent().map(Path::to_path_buf))
            })
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
                    self.progress.copied_bytes =
                        self.progress.copied_bytes.saturating_add(action.size);
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
                    // A journal write that fails only costs redone work on a
                    // resume, which is why it does not abort the operation.
                    let _ = journal.mark_complete(action.index);
                    if !action.is_dir {
                        self.progress.completed_files += 1;
                        self.progress.copied_bytes =
                            self.progress.copied_bytes.saturating_add(action.size);
                    }
                }
                Ok(ActionOutcome::Skipped) => {
                    // Recorded as a *skip*: nothing was copied, so a Move must
                    // not delete this source. See `OperationJournal`.
                    let _ = journal.mark_skipped(action.index);
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
                                    if matches!(outcome, ActionOutcome::Skipped) {
                                        let _ = journal.mark_skipped(action.index);
                                        self.skipped += 1;
                                    } else {
                                        let _ = journal.mark_complete(action.index);
                                    }
                                    if !action.is_dir {
                                        self.progress.completed_files += 1;
                                        self.progress.copied_bytes =
                                            self.progress.copied_bytes.saturating_add(action.size);
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
            self.events
                .push(FileOpEvent::Progress(self.progress.clone()));
        }

        // For Move: delete the sources whose data actually reached the
        // destination.
        //
        // The condition is `journal.transferred(..)`, not "the operation did
        // not fail". A Move is a copy followed by a delete, and the delete is
        // only ever safe for an action whose copy *transferred data*. Three
        // kinds of action reach this point having transferred nothing:
        //
        //   - skipped by conflict policy (`ConflictPolicy::Skip`, or
        //     `OverwriteIfNewer` where the source was not newer) — the file
        //     sitting at the destination is some pre-existing file, not this
        //     source;
        //   - failed and continued past under `ErrorPolicy::SkipAndContinue`;
        //   - failed every retry under `ErrorPolicy::RetryN`.
        //
        // This used to delete all three, which destroyed the user's only copy
        // of the data. Deleting only what was transferred degrades those cases
        // to "the file stayed where it was", which is recoverable.
        if operation == FileOperation::Move && self.progress.state != OperationState::Failed {
            // Whether anything was left behind. A source directory that is
            // still non-empty is *expected* when something under it was left
            // behind, and an anomaly worth reporting when nothing was.
            let anything_left_behind = actions
                .iter()
                .any(|action| !action.is_dir && !journal.transferred(action.index));

            for action in &actions {
                if action.is_dir {
                    // Directories are removed in reverse order (children first).
                    continue;
                }
                if !journal.transferred(action.index) {
                    continue;
                }
                if let Err(e) = fs::remove_file(&action.src) {
                    // A failed removal silently turned the Move into a Copy
                    // before this was reported: the summary said "moved" while
                    // the source was still there.
                    self.report_move_cleanup_failure(&action.src, &e);
                }
            }
            // Remove source directories in reverse order.
            for action in actions.iter().rev() {
                if !action.is_dir || !journal.transferred(action.index) {
                    continue;
                }
                if let Err(e) = fs::remove_dir(&action.src) {
                    // Leaving a directory behind because its contents were
                    // left behind is a consequence already reported against
                    // the files themselves; reporting it again would bury the
                    // real error under one line per ancestor directory.
                    if anything_left_behind && Self::dir_is_non_empty(&action.src) {
                        continue;
                    }
                    self.report_move_cleanup_failure(&action.src, &e);
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

        // Clean up the journal on success — before the summary is built, so a
        // failure to remove it is counted in it.
        //
        // A leftover journal is not cosmetic. Its action indices are relative
        // to a *plan*, so the next operation writing into this same directory
        // would read them as its own progress and treat every colliding index
        // as already done — i.e. never copy those files, and report success.
        // `OperationJournal::open` now discards a journal whose `plan` header
        // does not match, which contains the damage, but a journal we cannot
        // delete is still a symptom (a read-only or vanished destination) that
        // the user needs to see rather than have swallowed.
        if self.progress.state == OperationState::Completed {
            let journal_path = journal.path().to_path_buf();
            if let Err(e) = journal.remove() {
                let message = format!("failed to remove operation journal: {e}");
                self.events.push(FileOpEvent::Error {
                    path: journal_path.clone(),
                    error: message.clone(),
                });
                self.errors.push(FileOpError {
                    path: journal_path,
                    message,
                });
            }
        }

        let elapsed = self.started.map_or(Duration::ZERO, |s| s.elapsed());
        let succeeded = self.progress.completed_files.saturating_sub(self.skipped);

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
    }

    /// Report a source that a Move copied but could not then remove.
    ///
    /// The copy succeeded, so this does not fail the operation — the user's
    /// data is at the destination. What it must not do is stay quiet: an
    /// unreported removal failure turns a Move into a Copy while the summary
    /// still says the files were moved, and the user finds two copies later
    /// with no record of which is which.
    fn report_move_cleanup_failure(&mut self, src: &Path, error: &io::Error) {
        let message = format!("moved, but the source could not be removed: {error}");
        self.events.push(FileOpEvent::Error {
            path: src.to_path_buf(),
            error: message.clone(),
        });
        self.errors.push(FileOpError {
            path: src.to_path_buf(),
            message,
        });
    }

    /// Whether `dir` still holds at least one entry.
    ///
    /// A directory that cannot be read is treated as non-empty: the caller
    /// uses this only to decide whether a `remove_dir` failure is the expected
    /// consequence of something being left behind, and guessing "empty" there
    /// would report a spurious error.
    fn dir_is_non_empty(dir: &Path) -> bool {
        fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_some())
    }

    fn execute_copy_action(
        &mut self,
        action: &PlannedAction,
        conflict: ConflictPolicy,
    ) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "copy action has no destination",
            )
        })?;

        if action.is_dir {
            if !dest.exists() {
                fs::create_dir_all(dest)?;
            }
            self.undo_entries
                .push((action.src.clone(), Some(dest.clone())));
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
        self.undo_entries
            .push((action.src.clone(), Some(dest.clone())));
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
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "recycle action has no destination",
            )
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
        self.undo_entries
            .push((action.src.clone(), Some(dest.clone())));
        Ok(ActionOutcome::Done)
    }

    fn execute_restore_action(&mut self, action: &PlannedAction) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "restore action has no destination",
            )
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
        self.undo_entries
            .push((action.src.clone(), Some(dest.clone())));
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

        // A copy that fails part-way still leaves a partial temporary behind,
        // so it is cleaned up on the error path too.
        if let Err(e) = fs::copy(src, &tmp_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }

        // Attempt to preserve modification timestamp.
        if let Ok(src_meta) = fs::metadata(src)
            && let Ok(mtime) = src_meta.modified()
        {
            // Best-effort: not all platforms support filetime setting in
            // std, but our OS will.
            let _ = set_file_mtime(&tmp_path, mtime);
        }

        // Atomic rename into final position.
        //
        // On failure the temporary must go: it is a full-size copy of the
        // source sitting in the user's destination directory under a name they
        // never asked for, and leaving it behind meant a failed copy of a large
        // file silently consumed its own size in disk space. Its removal is
        // best-effort — if it cannot be removed the original error is still the
        // one worth reporting.
        if let Err(e) = fs::rename(&tmp_path, dest) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
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
// The `Result` is not superfluous, it is unreached: the body stands in for a
// filesystem syscall that will fail in practice (no permission, a read-only
// mount, a filesystem that stores no mtime), and the callers already handle
// that. Narrowing the return type to `()` now would mean widening it back —
// and revisiting every caller — the day the real implementation lands.
#[allow(clippy::unnecessary_wraps)]
fn set_file_mtime(path: &Path, _mtime: SystemTime) -> io::Result<()> {
    // Placeholder: on Slate OS this would call the appropriate filesystem
    // syscall to set the modification time. On the host (for testing)
    // std::fs does not provide a portable setter, so this is a no-op.
    let _ = path;
    Ok(())
}

// ============================================================================
// Recycle bin
// ============================================================================

/// Marker on the first line of a `meta.txt` whose path line is escaped.
///
/// Entries written before this existed begin with the raw path, so the marker
/// is what tells the two formats apart.
const META_VERSION: &str = "slate-recycle-v2";

/// Escape a path into a single line of printable ASCII, losslessly.
///
/// Paths on this OS may contain any byte except `/` and NUL, so they are not
/// necessarily UTF-8 and cannot be written with `Display` — that substitutes
/// U+FFFD and the original bytes are gone. `OsStr::as_encoded_bytes` gives the
/// exact bytes back; everything outside printable ASCII, plus `%` itself, is
/// percent-encoded so the metadata file stays line-oriented text.
fn encode_path(path: &Path) -> String {
    encode_bytes(path.as_os_str().as_encoded_bytes())
}

/// The lossless core of [`encode_path`], on bytes rather than a path.
///
/// Kept separate because this — not the `OsStr` conversion around it — is where
/// the round-trip property lives, and it can be tested on any host.
fn encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'%' || !(0x20..0x7f).contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char); // guarded: printable ASCII only
        }
    }
    out
}

/// Reverse of [`encode_path`].
fn decode_path(encoded: &str) -> PathBuf {
    PathBuf::from(os_string_from_bytes(decode_bytes(encoded)))
}

/// Reverse of [`encode_bytes`].
///
/// A `%` not followed by two hex digits is passed through literally rather than
/// dropped: the metadata file may have been hand-edited, and losing a byte
/// silently is worse than keeping one that was never an escape.
fn decode_bytes(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let Some(hex) = encoded.get(i.saturating_add(1)..i.saturating_add(3))
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i = i.saturating_add(3);
            continue;
        }
        out.push(b);
        i = i.saturating_add(1);
    }
    out
}

/// Build an `OsString` from the raw bytes of a path.
///
/// This is where the byte world meets the platform's path type, so it is split
/// per platform rather than papered over with
/// `OsStr::from_encoded_bytes_unchecked`: that function's contract is that the
/// bytes are valid for the platform's `OsStr` encoding, which is true for
/// arbitrary bytes on Unix but *not* on Windows, where `OsStr` is WTF-8. Since
/// our target is `target-family = ["unix"]`, the safe, total conversion below
/// is the one that actually runs; Windows appears only as a test host.
#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes)
}

/// Test-host fallback. Windows `OsString` cannot hold a byte string that is not
/// WTF-8, so bytes that are not valid UTF-8 cannot survive here. They are not
/// silently mangled: [`decode_bytes`] is still exact, and the tests assert the
/// round-trip at that level, which is the level `meta.txt` is written at.
#[cfg(not(unix))]
fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    match String::from_utf8(bytes) {
        Ok(s) => std::ffi::OsString::from(s),
        // Reachable only on a non-Unix host reading a bin written on the
        // target. Nothing better is representable; `decode_bytes` is the API to
        // use if the exact bytes are needed.
        Err(e) => std::ffi::OsString::from(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

/// Move `src` to `dest`, falling back to copy-then-remove across devices.
///
/// `fs::rename` cannot cross a mount point — it fails with `EXDEV`. The recycle
/// bin lives under the user's home directory, so recycling anything from a
/// separate data partition hit exactly that and simply reported an error.
/// (`same_device` exists for this check but is a first-component heuristic;
/// attempting the rename and reacting to its failure is both cheaper in the
/// common case and correct in the cases the heuristic gets wrong.)
fn move_path(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => return Ok(()),
        Err(e) => {
            // A missing source, or a destination whose parent does not exist,
            // will not be fixed by copying either — report it as-is.
            if e.kind() == io::ErrorKind::NotFound {
                return Err(e);
            }
        }
    }

    if src.is_dir() {
        copy_tree(src, dest)?;
        fs::remove_dir_all(src)
    } else {
        fs::copy(src, dest)?;
        fs::remove_file(src)
    }
}

/// Recursively copy a directory tree.
fn copy_tree(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let child_dest = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &child_dest)?;
        } else {
            fs::copy(entry.path(), &child_dest)?;
        }
    }
    Ok(())
}

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

/// Recycled items are eligible for auto-purge after 30 days.
///
/// Spelled in seconds rather than `Duration::from_days`, which is still
/// nightly-gated (rust-lang/rust#120301) and would pin this crate to a
/// nightly toolchain for nothing but a nicer literal.
const DEFAULT_RECYCLE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How many candidate names a new recycle bin entry will try before failing.
///
/// Collisions come from same-named files recycled inside one clock tick, so
/// the realistic worst case is a handful. The bound is generous enough never
/// to be reached by that, and exists only so a pathological bin cannot spin
/// forever.
const MAX_ENTRY_ID_ATTEMPTS: usize = 4096;

/// Nanoseconds since the Unix epoch, or 0 if the clock is before it.
///
/// A clock that cannot be read yields a usable-but-colliding id rather than
/// failing the delete; [`RecycleBin::create_entry_dir`] resolves the collision,
/// so the degraded case costs a suffix, not the user's file.
fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
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
        Self::new(home.join(".recycle"), DEFAULT_RECYCLE_MAX_AGE)
    }

    /// Move `path` into the recycle bin and return the entry id.
    ///
    /// The original path is recorded losslessly (see [`encode_path`]) so that a
    /// file whose name is not valid UTF-8 can still be restored to where it
    /// came from. Recording it with `Display` would have written U+FFFD in
    /// place of every undecodable byte, and restore would then have recreated
    /// the file under a different name.
    pub fn recycle(&self, path: &Path) -> io::Result<String> {
        self.recycle_at(path, now_nanos())
    }

    /// [`Self::recycle`] with the clock supplied by the caller.
    ///
    /// The seam exists so that the collision handling in
    /// [`Self::create_entry_dir`] can be tested deterministically. Through
    /// `recycle` the timestamps of two successive calls always differ, because
    /// each call does enough filesystem work to advance even a coarse clock —
    /// but that is an accident of timing, not a guarantee, and it is not one
    /// the correctness of the bin should rest on.
    fn recycle_at(&self, path: &Path, ts: u128) -> io::Result<String> {
        let (id, entry_dir) = self.create_entry_dir(path, ts)?;
        let data_path = entry_dir.join("data");

        // Write metadata *before* moving the data: if the move fails, the
        // orphaned metadata is harmless (`read_entry` reports size 0), whereas
        // moved data with no metadata would be unrestorable.
        let meta_path = entry_dir.join("meta.txt");
        let mut meta_file = fs::File::create(&meta_path)?;
        writeln!(meta_file, "{META_VERSION}")?;
        writeln!(meta_file, "{}", encode_path(path))?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        writeln!(meta_file, "{now}")?;
        meta_file.flush()?;

        // Move the actual data.
        if let Err(e) = move_path(path, &data_path) {
            // Do not leave metadata pointing at data that is not there.
            let _ = fs::remove_file(&meta_path);
            let _ = fs::remove_dir(&entry_dir);
            return Err(e);
        }

        Ok(id)
    }

    /// Restore a recycled item, returning where it actually landed.
    ///
    /// **The return value is the path to show the user**, not
    /// `entry.original_path`: if something else now occupies the original path,
    /// the item is restored beside it under a `name (2)` variant rather than on
    /// top of it. Restoring used to call [`move_path`] straight at the original
    /// path, and `fs::rename` replaces its destination without a word — so
    /// deleting `report.docx`, writing a new `report.docx`, then restoring the
    /// old one from the bin destroyed the new one. It did not go to the bin
    /// either; there was nothing left to recover.
    pub fn restore(&self, entry_id: &str) -> io::Result<PathBuf> {
        let entry = self.read_entry(entry_id)?;
        let data_path = self.root.join(entry_id).join("data");

        // Ensure parent directory exists.
        if let Some(parent) = entry.original_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // The gap between this check and the move is a race that `std` alone
        // cannot close — there is no "rename only if the destination is free".
        // The same tradeoff is documented on the engine's own conflict handling
        // above; narrowing it needs a platform primitive we do not have here.
        let dest = if entry.original_path.exists() {
            resolve_rename(&entry.original_path)
        } else {
            entry.original_path.clone()
        };

        move_path(&data_path, &dest)?;

        // Clean up the entry directory. Reported rather than ignored: a
        // `meta.txt` that survives its `data` leaves an entry that `list` still
        // shows and `restore` can no longer satisfy, and the user's only clue
        // would be the failure of a restore they try much later.
        let entry_dir = self.root.join(entry_id);
        for path in [entry_dir.join("meta.txt"), entry_dir.clone()] {
            let removed = if path == entry_dir {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(e) = removed
                && e.kind() != io::ErrorKind::NotFound
            {
                eprintln!(
                    "warning: restored {} but could not clear its recycle bin entry {}: {}",
                    dest.display(),
                    path.display(),
                    e
                );
            }
        }

        Ok(dest)
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
            // An entry whose metadata will not parse is skipped rather than
            // failing the whole listing: one corrupt `meta.txt` must not make
            // every other recycled file unrestorable. The cost is that the
            // damaged entry is invisible in the UI — tracked in
            // `known-issues.md` as `TD-EXPLORER-UNREADABLE-RECYCLE-ENTRY`.
            if let Ok(entry) = self.read_entry(&id) {
                entries.push(entry);
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
    /// A *candidate* directory name for a new entry.
    ///
    /// This is a hint, not a unique key, and must not be treated as one. Its
    /// only entropy is `ts`; two files that share a `file_name` and are
    /// recycled within one tick of the system clock produce the same string.
    /// That is not far-fetched — `SystemTime::now()` advances in 100 ns steps
    /// on Windows, and deleting `projA/README.md` and `projB/README.md`
    /// together is an ordinary multi-select. Uniqueness is enforced by
    /// [`Self::create_entry_dir`], which asks the filesystem.
    fn make_id(&self, path: &Path, ts: u128) -> String {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        // Simple hash to keep directory names manageable.
        let hash = ts ^ (name.len() as u128).wrapping_mul(0x517cc1b727220a95);
        format!("{name}_{hash:016x}")
    }

    /// Claim a fresh entry directory, returning its id and path.
    ///
    /// # Why the filesystem decides
    ///
    /// The previous code took [`Self::make_id`]'s output on trust and called
    /// `create_dir_all`, which succeeds when the directory already exists. Two
    /// entries landing on the same id therefore *shared* one directory: the
    /// second `recycle` overwrote the first's `meta.txt` and then renamed its
    /// `data` on top of the first's. One of the two deleted files ceased to
    /// exist, silently — no error, and nothing in the bin to restore it from.
    ///
    /// `fs::create_dir` fails with `AlreadyExists` instead of succeeding, so
    /// an entry directory is only ever used by the caller that created it.
    /// Uniqueness is then a property the filesystem guarantees rather than one
    /// the clock happens to provide, which also makes it hold across two
    /// explorer processes sharing a bin.
    fn create_entry_dir(&self, path: &Path, ts: u128) -> io::Result<(String, PathBuf)> {
        fs::create_dir_all(&self.root)?;
        let base = self.make_id(path, ts);

        for attempt in 0..MAX_ENTRY_ID_ATTEMPTS {
            // The first candidate is the unsuffixed name, so the common case
            // of no collision leaves the on-disk layout exactly as before.
            let id = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{attempt}")
            };
            let dir = self.root.join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }

        // Bounded rather than looping forever: a bin whose root has become
        // unwritable in a way that reports `AlreadyExists` would otherwise
        // hang the explorer. Failing the delete leaves the file where it is,
        // which is the recoverable outcome.
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("could not find a free recycle bin entry name for {base}"),
        ))
    }

    /// Read the metadata for a recycled entry.
    fn read_entry(&self, id: &str) -> io::Result<RecycleEntry> {
        let entry_dir = self.root.join(id);
        let meta_path = entry_dir.join("meta.txt");
        let content = fs::read_to_string(&meta_path)?;
        let mut lines = content.lines();

        let first = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty meta"))?;
        // A bin written before the path was escaped starts straight in with the
        // path. Reading those is still worth doing: the alternative is silently
        // orphaning whatever a user had already deleted.
        let original_path = if first == META_VERSION {
            let encoded = lines.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing path in meta")
            })?;
            decode_path(encoded)
        } else {
            PathBuf::from(first)
        };
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
                    && d.exists()
                {
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
                    && d.exists()
                {
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
                    && src.exists()
                {
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
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

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

        let plan =
            OperationPlan::plan_delete(&[src_dir.join("data")], ErrorPolicy::StopOnFirst).unwrap();

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
            let mut j = OperationJournal::open(&dir, 42).unwrap();
            assert_eq!(j.completed_count(), 0);
            j.mark_complete(0).unwrap();
            j.mark_complete(3).unwrap();
            j.mark_complete(7).unwrap();
        }

        // Re-open and verify.
        let j2 = OperationJournal::open(&dir, 42).unwrap();
        assert_eq!(j2.completed_count(), 3);
        assert!(j2.is_complete(0));
        assert!(j2.is_complete(3));
        assert!(j2.is_complete(7));
        assert!(!j2.is_complete(1));
        assert!(!j2.is_complete(999));

        let _ = fs::remove_dir_all(&dir);
    }

    /// A finished action records *whether it transferred data*, because that
    /// is the question a Move has to ask before deleting a source.
    #[test]
    fn journal_distinguishes_a_skip_from_a_copy() {
        let dir = temp_dir("journal_skip_flag");

        {
            let mut j = OperationJournal::open(&dir, 7).unwrap();
            j.mark_complete(0).unwrap();
            j.mark_skipped(1).unwrap();
        }

        let j2 = OperationJournal::open(&dir, 7).unwrap();
        // Both count as "done" — neither should be re-run on a resume.
        assert!(j2.is_complete(0));
        assert!(j2.is_complete(1));
        // Only one of them put the data at the destination.
        assert!(j2.transferred(0));
        assert!(!j2.transferred(1));
        // An action that never ran transferred nothing either.
        assert!(!j2.transferred(2));

        let _ = fs::remove_dir_all(&dir);
    }

    /// A journal left behind by a *different* plan must be discarded, not read
    /// as this plan's progress. Its indices are plan-relative, so honouring it
    /// would mark unrelated actions as already done and silently never copy
    /// them.
    #[test]
    fn a_journal_from_another_plan_is_discarded() {
        let dir = temp_dir("journal_stale");

        {
            let mut j = OperationJournal::open(&dir, 1).unwrap();
            j.mark_complete(0).unwrap();
            j.mark_complete(1).unwrap();
        }

        let j2 = OperationJournal::open(&dir, 2).unwrap();
        assert_eq!(j2.completed_count(), 0);
        assert!(!j2.is_complete(0));

        let _ = fs::remove_dir_all(&dir);
    }

    /// The end-to-end form of the above: a stale journal in the destination
    /// directory must not cause files to be silently left uncopied.
    #[test]
    fn a_stale_journal_does_not_swallow_a_copy() {
        let src_dir = temp_dir("journal_stale_e2e_src");
        let dst_dir = temp_dir("journal_stale_e2e_dst");

        write_file(&src_dir.join("a.txt"), "aaa");
        write_file(&src_dir.join("b.txt"), "bbb");

        // A journal from some earlier, unrelated operation into this directory.
        {
            let mut j = OperationJournal::open(&dst_dir, 0xDEAD_BEEF).unwrap();
            j.mark_complete(0).unwrap();
            j.mark_complete(1).unwrap();
            j.mark_complete(2).unwrap();
        }

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("a.txt"), src_dir.join("b.txt")],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(dst_dir.join("a.txt").exists(), "a.txt was never copied");
        assert!(dst_dir.join("b.txt").exists(), "b.txt was never copied");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn journal_resume_skips_completed() {
        let src_dir = temp_dir("journal_resume_src");
        let dst_dir = temp_dir("journal_resume_dst");

        write_file(&src_dir.join("a.txt"), "aaa");
        write_file(&src_dir.join("b.txt"), "bbb");

        let plan = OperationPlan::plan_copy(
            &[src_dir.join("a.txt"), src_dir.join("b.txt")],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        // Pre-write a journal for *this* plan marking action 0 (a.txt) as done,
        // as an interrupted earlier run would have left behind.
        {
            let mut j = OperationJournal::open(&dst_dir, plan.id()).unwrap();
            j.mark_complete(0).unwrap();
        }

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();

        // Should complete without error.
        let complete = events
            .iter()
            .find(|e| matches!(e, FileOpEvent::Complete { .. }));
        assert!(complete.is_some());

        // The resumed run re-did only the unfinished action.
        assert!(
            !dst_dir.join("a.txt").exists(),
            "action 0 was journalled as done and should not have been redone"
        );
        assert!(dst_dir.join("b.txt").exists());

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn journal_remove_on_completion() {
        let dir = temp_dir("journal_remove");

        let mut j = OperationJournal::open(&dir, 99).unwrap();
        j.mark_complete(0).unwrap();
        let jpath = j.path().to_path_buf();
        assert!(jpath.exists());

        j.remove().unwrap();
        assert!(!jpath.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------------
    // Move: a source is deleted only when its data reached the destination
    // ----------------------------------------------------------------

    /// A Move whose copy was skipped by conflict policy must leave the source
    /// alone. Deleting it would destroy the only copy of that data — the file
    /// at the destination is a *different*, pre-existing file.
    #[test]
    fn a_skipped_move_does_not_delete_the_source() {
        let src_dir = temp_dir("move_skip_src");
        let dst_dir = temp_dir("move_skip_dst");

        let src = src_dir.join("a.txt");
        write_file(&src, "the original");
        // A different file already occupies the destination name.
        write_file(&dst_dir.join("a.txt"), "something else");

        let plan = OperationPlan::plan_move(
            std::slice::from_ref(&src),
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(
            src.exists(),
            "the source was deleted even though nothing was copied"
        );
        assert_eq!(fs::read_to_string(&src).unwrap(), "the original");
        // And the destination still holds what was already there.
        assert_eq!(
            fs::read_to_string(dst_dir.join("a.txt")).unwrap(),
            "something else"
        );

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    /// The same guarantee when the copy *failed* rather than being skipped:
    /// `SkipAndContinue` keeps the operation running, but a source whose copy
    /// failed has not been transferred anywhere.
    #[test]
    fn a_failed_move_does_not_delete_the_source() {
        let src_dir = temp_dir("move_fail_src");
        let dst_dir = temp_dir("move_fail_dst");

        let good = src_dir.join("good.txt");
        let bad = src_dir.join("bad.txt");
        write_file(&good, "kept");
        write_file(&bad, "must survive");

        // Make the copy of `bad.txt` fail while leaving its source intact: put
        // a *directory* at the destination path. The final rename of the
        // temporary onto it fails on every platform, and — unlike removing the
        // source or fiddling with permissions — it is portable and leaves the
        // source exactly where the deletion loop would find it.
        fs::create_dir_all(dst_dir.join("bad.txt")).unwrap();

        let plan = OperationPlan::plan_move(
            &[good.clone(), bad.clone()],
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();

        // The action that succeeded was moved...
        assert!(!good.exists(), "the successful move left its source behind");
        assert!(dst_dir.join("good.txt").exists());
        // ...and the one that failed kept its source. This is the data-loss
        // regression: the deletion phase used to run over *every* planned
        // action regardless of whether its copy had transferred anything.
        assert!(
            bad.exists(),
            "the source of a failed copy was deleted — its only copy is gone"
        );
        assert_eq!(fs::read_to_string(&bad).unwrap(), "must survive");

        // The failure is reported, not swallowed.
        let summary = events
            .iter()
            .find_map(|e| match e {
                FileOpEvent::Complete { summary } => Some(summary),
                _ => None,
            })
            .expect("no completion summary");
        assert!(summary.failed >= 1, "the failed copy was not reported");

        // And the temporary the failed copy created was cleaned up rather than
        // left in the user's destination directory.
        let leftovers: Vec<_> = fs::read_dir(&dst_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".fileop-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporaries left behind: {leftovers:?}");

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    /// A Move that copies successfully still deletes its source — the fix must
    /// not have turned every Move into a Copy.
    #[test]
    fn a_successful_move_still_deletes_the_source() {
        let src_dir = temp_dir("move_ok_src");
        let dst_dir = temp_dir("move_ok_dst");

        let src = src_dir.join("a.txt");
        write_file(&src, "moving day");

        let plan = OperationPlan::plan_move(
            std::slice::from_ref(&src),
            &dst_dir,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(!src.exists(), "the source survived a successful move");
        assert_eq!(
            fs::read_to_string(dst_dir.join("a.txt")).unwrap(),
            "moving day"
        );

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    /// Moving a directory whose contents were partly skipped must leave the
    /// skipped file *and* the directory holding it, and must not report the
    /// non-empty directory as a separate failure.
    #[test]
    fn a_partly_skipped_directory_move_keeps_what_it_did_not_copy() {
        let root = temp_dir("move_dir_partial_src");
        let dst_dir = temp_dir("move_dir_partial_dst");

        let src_dir = root.join("folder");
        fs::create_dir_all(&src_dir).unwrap();
        write_file(&src_dir.join("copied.txt"), "new");
        write_file(&src_dir.join("skipped.txt"), "the original");

        // Pre-place a conflicting file so `skipped.txt` is skipped.
        let dst_folder = dst_dir.join("folder");
        fs::create_dir_all(&dst_folder).unwrap();
        write_file(&dst_folder.join("skipped.txt"), "something else");

        let plan = OperationPlan::plan_move(
            std::slice::from_ref(&src_dir),
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        let events = executor.execute();

        assert!(
            src_dir.join("skipped.txt").exists(),
            "the skipped file's source was deleted"
        );
        assert!(
            src_dir.exists(),
            "the source directory holding a skipped file was removed"
        );
        assert!(!src_dir.join("copied.txt").exists());

        // The still-populated source directory is a consequence of the skip,
        // already reported against the file, so it must not add an error.
        let summary = events.iter().find_map(|e| match e {
            FileOpEvent::Complete { summary } => Some(summary),
            _ => None,
        });
        let summary = summary.expect("no completion summary");
        assert_eq!(
            summary.failed, 0,
            "a directory left non-empty by a skip was reported as a failure: {:?}",
            summary.errors
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&dst_dir);
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

    /// Delete a file, make a new one with the same name, then restore the old
    /// one from the bin. The new file must survive.
    ///
    /// It did not. `restore` moved the recycled data straight onto
    /// `entry.original_path`, and `fs::rename` replaces its destination
    /// silently — so the newer file was destroyed by an action the user
    /// understands as *recovering* a file, and it did not go to the bin either.
    #[test]
    fn restoring_over_a_newer_file_of_the_same_name_keeps_both() {
        let dir = temp_dir("restore_conflict");
        let bin_root = dir.join("bin");
        let file_path = dir.join("report.docx");
        write_file(&file_path, "the old draft");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));
        let id = bin.recycle(&file_path).expect("recycle");
        assert!(!file_path.exists());

        // The user moves on and writes a new file under the same name.
        write_file(&file_path, "the new draft");

        let restored = bin.restore(&id).expect("restore");

        assert_eq!(
            read_file(&file_path),
            "the new draft",
            "restoring must never overwrite a file the user made since"
        );
        assert_ne!(
            restored, file_path,
            "the restored copy has to land somewhere else, and say where"
        );
        assert_eq!(
            read_file(&restored),
            "the old draft",
            "and the recycled contents must be what landed there"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// The unoccupied case must be untouched: a restore with nothing in the
    /// way still goes back to exactly where it came from, under its own name.
    #[test]
    fn restoring_to_a_free_path_uses_the_original_name() {
        let dir = temp_dir("restore_free");
        let bin_root = dir.join("bin");
        let file_path = dir.join("notes.txt");
        write_file(&file_path, "notes");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));
        let id = bin.recycle(&file_path).expect("recycle");
        let restored = bin.restore(&id).expect("restore");

        assert_eq!(restored, file_path, "no conflict, no renaming");
        assert_eq!(read_file(&file_path), "notes");

        let _ = fs::remove_dir_all(&dir);
    }

    /// A directory in the way is the same hazard with more to lose: the old
    /// code's `move_path` fell back to `copy_tree`, which merges into an
    /// existing directory and overwrites the same-named files inside it.
    #[test]
    fn restoring_a_directory_does_not_merge_into_one_that_is_in_the_way() {
        let dir = temp_dir("restore_dir_conflict");
        let bin_root = dir.join("bin");
        let src_dir = dir.join("project");
        fs::create_dir_all(&src_dir).expect("project");
        write_file(&src_dir.join("main.rs"), "the old source");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));
        let id = bin.recycle(&src_dir).expect("recycle");

        // A new, unrelated `project/` with a file of the same name.
        fs::create_dir_all(&src_dir).expect("new project");
        write_file(&src_dir.join("main.rs"), "the new source");

        let restored = bin.restore(&id).expect("restore");

        assert_eq!(
            read_file(&src_dir.join("main.rs")),
            "the new source",
            "the directory in the way must not be merged into"
        );
        assert_eq!(read_file(&restored.join("main.rs")), "the old source");

        let _ = fs::remove_dir_all(&dir);
    }

    /// Two files that share a name but live in different directories are an
    /// entirely ordinary multi-select delete — `projA/README.md` and
    /// `projB/README.md`. They must both survive it.
    ///
    /// They did not. The entry id was `{file_name}_{hash}` where the hash's
    /// only entropy was `SystemTime::now()`, and on Windows that clock
    /// advances in 100 ns ticks — measured here, ~70% of back-to-back readings
    /// are *identical*. Two same-named files recycled in the same tick
    /// therefore got the same id, and the second `recycle` overwrote the
    /// first's metadata and then renamed its data on top of the first's data.
    /// One of the two files was gone, with no error reported anywhere.
    #[test]
    fn two_same_named_files_recycled_together_both_survive() {
        let dir = temp_dir("recycle_collide");
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_secs(86400));

        // Both recycled at the *same* instant. Going through `recycle` would
        // not reproduce this: each call does enough filesystem work to advance
        // the clock, which is why the bug survived so long. The seam removes
        // that accident so the collision handling is what is under test.
        let a = dir.join("projA/README.md");
        let b = dir.join("projB/README.md");
        write_file(&a, "contents of A");
        write_file(&b, "contents of B");
        let id_a = bin.recycle_at(&a, 1_700_000_000_000_000_000).expect("recycle a");
        let id_b = bin.recycle_at(&b, 1_700_000_000_000_000_000).expect("recycle b");

        assert_ne!(id_a, id_b, "two different files must not share a bin entry");

        let entries = bin.list().expect("list");
        assert_eq!(entries.len(), 2, "both files must be listed in the bin");

        // Both must restore, to their own original paths, with their own data.
        let restored_a = bin.restore(&id_a).expect("restore a");
        let restored_b = bin.restore(&id_b).expect("restore b");
        assert_eq!(restored_a, a);
        assert_eq!(restored_b, b);
        assert_eq!(read_file(&a), "contents of A");
        assert_eq!(read_file(&b), "contents of B");

        let _ = fs::remove_dir_all(&dir);
    }

    /// The same collision, but at the scale a "delete this whole folder"
    /// produces: many identically-named files, all recycled at one instant.
    /// Uniqueness has to hold for all of them, not just for a pair.
    #[test]
    fn a_burst_of_same_named_files_keeps_every_one() {
        let dir = temp_dir("recycle_burst");
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_secs(86400));

        let mut ids = Vec::new();
        for i in 0..64 {
            let path = dir.join(format!("d{i}/notes.txt"));
            write_file(&path, &format!("file {i}"));
            ids.push((i, bin.recycle_at(&path, 4242).expect("recycle")));
        }

        let unique: std::collections::BTreeSet<&String> = ids.iter().map(|(_, id)| id).collect();
        assert_eq!(unique.len(), ids.len(), "every entry needs its own id");
        assert_eq!(bin.list().expect("list").len(), ids.len());

        for (i, id) in &ids {
            let restored = bin.restore(id).expect("restore");
            assert_eq!(restored, dir.join(format!("d{i}/notes.txt")));
            assert_eq!(read_file(&restored), format!("file {i}"));
        }

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
    // Recycle metadata — path escaping
    // ----------------------------------------------------------------

    #[test]
    fn a_path_needing_escapes_round_trips_through_the_metadata() {
        // A percent sign (the escape character itself), a space, a non-ASCII
        // name, and a control character — the four things a naive text format
        // gets wrong.
        for original in [
            "/home/u/100% done.txt",
            "/home/u/写真/2024.jpg",
            "/home/u/a\tb",
            "/home/u/plain.txt",
        ] {
            let path = PathBuf::from(original);
            let encoded = encode_path(&path);
            assert!(
                encoded.bytes().all(|b| (0x20..0x7f).contains(&b)),
                "the encoding must stay on one line of printable ASCII: {encoded:?}"
            );
            assert_eq!(
                decode_path(&encoded),
                path,
                "round trip failed for {original:?} (encoded as {encoded:?})"
            );
        }
    }

    #[test]
    fn a_path_that_is_not_utf8_survives_the_metadata() {
        // Paths on this OS allow every byte but `/` and NUL, so the metadata
        // must carry bytes, not characters. Writing the path with `Display`
        // replaced undecodable bytes with U+FFFD and the original name was
        // then unrecoverable.
        //
        // Asserted at the byte level, which is the level `meta.txt` is written
        // at: `OsString` on the Windows test host cannot hold a non-WTF-8 byte
        // string at all, so going through `PathBuf` here would be testing the
        // host's limitation rather than our encoding.
        let encoded = "/home/u/caf%E9.txt";
        let decoded = decode_bytes(encoded);
        assert_eq!(
            decoded, b"/home/u/caf\xE9.txt",
            "a lone 0xE9 must come back as 0xE9, not as U+FFFD"
        );
        assert_eq!(
            encode_bytes(&decoded),
            encoded,
            "and must re-encode to the same text"
        );
    }

    /// Every byte value must survive, not just the one a bug happened to hit.
    #[test]
    fn every_byte_value_round_trips_through_the_encoding() {
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(decode_bytes(&encode_bytes(&all)), all);
    }

    #[test]
    fn a_percent_that_is_not_an_escape_is_kept_verbatim() {
        // A hand-edited file may contain a bare `%`. Dropping it would silently
        // rename the entry; the decoder passes it through instead.
        assert_eq!(decode_bytes("100%"), b"100%");
        assert_eq!(decode_bytes("a%zz"), b"a%zz");
    }

    #[test]
    fn a_recycled_non_ascii_name_restores_to_its_original_path() {
        let dir = temp_dir("recycle_nonascii");
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_secs(86400));

        let file_path = dir.join("写真 100%.txt");
        write_file(&file_path, "keep");

        let id = bin.recycle(&file_path).expect("recycle");
        assert!(!file_path.exists());

        let restored = bin.restore(&id).expect("restore");
        assert_eq!(
            restored, file_path,
            "restore must put the file back under its original name"
        );
        assert_eq!(read_file(&file_path), "keep");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bin_written_in_the_old_format_is_still_readable() {
        let dir = temp_dir("recycle_legacy");
        let bin_root = dir.join("bin");
        let entry_dir = bin_root.join("legacy_0000000000000001");
        fs::create_dir_all(&entry_dir).expect("entry dir");
        // The pre-versioning layout: raw path on line 1, timestamp on line 2.
        write_file(
            &entry_dir.join("meta.txt"),
            "/home/u/legacy.txt\n1700000000\n",
        );
        write_file(&entry_dir.join("data"), "old contents");

        let bin = RecycleBin::new(bin_root, Duration::from_secs(86400));
        let listed = bin.list().expect("list");
        assert_eq!(
            listed.len(),
            1,
            "an already-deleted file must not be orphaned"
        );
        assert_eq!(
            listed.first().expect("one").original_path,
            PathBuf::from("/home/u/legacy.txt")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------------
    // Recycle failure handling
    // ----------------------------------------------------------------

    #[test]
    fn a_recycle_that_could_not_move_the_data_leaves_no_entry_behind() {
        let dir = temp_dir("recycle_orphan");
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_secs(86400));

        let err = bin
            .recycle(&dir.join("never_existed.txt"))
            .expect_err("recycling a missing file must fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);

        assert!(
            bin.list().expect("list").is_empty(),
            "metadata written before a failed move must be cleaned up, or the \
             bin lists an entry whose data is not there"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_path_relocates_a_whole_directory_tree() {
        let dir = temp_dir("move_tree");
        let src = dir.join("src");
        fs::create_dir_all(src.join("nested")).expect("nested");
        write_file(&src.join("top.txt"), "top");
        write_file(&src.join("nested/deep.txt"), "deep");

        let dest = dir.join("dest");
        move_path(&src, &dest).expect("move");

        assert!(!src.exists(), "the source must be gone");
        assert_eq!(read_file(&dest.join("top.txt")), "top");
        assert_eq!(read_file(&dest.join("nested/deep.txt")), "deep");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_tree_reproduces_every_level() {
        let dir = temp_dir("copy_tree");
        let src = dir.join("src");
        fs::create_dir_all(src.join("a/b")).expect("dirs");
        write_file(&src.join("a/b/leaf.txt"), "leaf");

        let dest = dir.join("dest");
        copy_tree(&src, &dest).expect("copy");

        assert_eq!(read_file(&dest.join("a/b/leaf.txt")), "leaf");
        assert!(src.exists(), "a copy must leave the source in place");

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
        assert_eq!(
            read_file(&dst_dir.join("mydir").join("sub").join("b.txt")),
            "bb"
        );

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

        let plan =
            OperationPlan::plan_delete(&[dir.join("delme")], ErrorPolicy::StopOnFirst).unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(!dir.join("delme").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
