//! Atomic file operations for the Slate OS file explorer.
//!
//! Provides copy, move, delete, recycle, and undo operations with:
//! - Progress tracking (bytes, files, ETA)
//! - Journalling that lets an interrupted operation continue without redoing
//!   work (see the caveat below: this is not crash recovery)
//! - Conflict resolution policies
//! - Per-file error handling (skip, retry, stop)
//! - Undo via an operation journal
//! - Recycle bin management with auto-purge
//!
//! All multi-file operations are planned before execution: the source tree is
//! scanned to produce an [`OperationPlan`], which records total bytes and file
//! count. The plan is then executed step-by-step, updating an
//! [`OperationProgress`] after each file and writing completed actions to an
//! [`OperationJournal`] so that an interrupted operation can continue by
//! re-reading the journal and skipping already-finished items.
//!
//! # This is not crash recovery, and said plainly because it reads like it
//!
//! The journal records a plan **id** and which action indices finished. It
//! does not record the plan: not the sources, not the destinations, not the
//! operation. So it can tell a *running* executor which of its own steps are
//! already done, and after a crash it can tell a new process that "actions
//! 0..7 of plan 4391 completed" -- about a plan that no longer exists
//! anywhere. Surviving a restart needs the plan persisted too, which is a
//! change to this file's format rather than a reader to add. Recorded in
//! `roadmap-detailed.md` §4.1 under durable bulk operations.

// What this suppression is hiding, measured 2026-09-16 by removing it:
// ten findings, including that three `ConflictPolicy` variants (`Overwrite`,
// `OverwriteIfNewer`, `Ask`), two error policies (`StopOnFirst`, `RetryN`) and
// `ExecutorConfig` are never constructed anywhere -- while the list at the top
// of this file advertises "conflict resolution policies" and "per-file error
// handling (skip, retry, stop)". The allow stays for now because removing it
// means deciding, variant by variant, between wiring and deleting; it is no
// longer *silent*, which was the part that let the gap live here unremarked.
// See known-issues TD-C-THE-FILE-OPERATIONS-MODULE-ADVERTISES-POLICIES-NOTHING-SELECTS.
#![allow(dead_code)]

use pathtext::ShowPath;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write as IoWrite};
use std::path::{Path, PathBuf};

// The percent-encoding design-decisions §426 chose for records that must
// stay human-readable. Shared rather than copied: §426 picked ONE escape
// precisely so two formats could not drift, and this file and
// `apps/backup` held byte-identical copies of it until 2026-09-16.
// The recycle bin is its own crate, so the image viewer's Delete can use the
// same bin the file manager restores from.
pub use recyclebin::{RecycleBin, resolve_rename};
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
    /// Create a symbolic link at the destination pointing at the source.
    ///
    /// Not a copy with a different name: one action per source, never a walk
    /// into a directory. Linking a folder means *one* link to the folder, and
    /// a plan that recursed would produce a tree of links to each file inside
    /// it -- which is not what the gesture asks for and is not undoable as one
    /// thing.
    Link,
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
    /// Stop at the taken name -- emit a [`FileOpEvent::Conflict`] and wait,
    /// doing nothing more, until the caller answers with
    /// [`OperationExecutor::answer`].
    ///
    /// Until 2026-09-27 this emitted the event and then *skipped the file*
    /// ("In a real async implementation the caller would respond. For now,
    /// skip."), and a link skipped without even the event -- so a caller that
    /// chose `Ask` got `Skip` and a notice it could no longer act on.
    Ask,
}

/// A taken name the operation has stopped at, under [`ConflictPolicy::Ask`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictQuestion {
    /// The planned action it stopped at.
    pub action: u32,
    /// What is being copied, moved or linked there.
    pub src: PathBuf,
    /// The name that is taken.
    pub dest: PathBuf,
}

/// What to do with a taken name the operation has asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictAnswer {
    /// Put the new one beside it, numbered: `name (2).txt`.
    KeepBoth,
    /// Leave the one there, and do not copy this one.
    Skip,
    /// Put the new one in its place.
    Replace,
    /// Stop the whole operation here. What is done stays done -- see
    /// [`OperationExecutor::cancel`].
    Stop,
}

impl ConflictAnswer {
    /// The policy that carries the answer out. `Stop` has none: it is not a
    /// way of dealing with a file.
    #[must_use]
    pub fn policy(self) -> Option<ConflictPolicy> {
        match self {
            Self::KeepBoth => Some(ConflictPolicy::Rename),
            Self::Skip => Some(ConflictPolicy::Skip),
            Self::Replace => Some(ConflictPolicy::Overwrite),
            Self::Stop => None,
        }
    }
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
            let remaining = self.total_bytes.saturating_sub(self.copied_bytes);
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
    /// Whether the source is a symbolic link -- or, on Windows, a junction --
    /// which is copied, moved and deleted *as the link* and never followed.
    ///
    /// Until 2026-09-27 every scan followed links. A permanent delete of a
    /// folder holding a link to another folder deleted the other folder's
    /// files, through the link; a move of it copied them and then deleted
    /// them at the source; a link to a folder above it made the scan endless.
    /// None of those files was ever selected.
    pub is_link: bool,
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
    /// Every drive this plan will touch, source ends and destination ends.
    ///
    /// The scheduler's question: two operations that share a drive should not
    /// run at once. It is asked of the *plan* rather than of the paths the
    /// user selected, because the plan is what knows the whole expansion of a
    /// directory tree -- a selection of one folder can reach a mounted
    /// subvolume that the selection itself does not name.
    #[must_use]
    pub fn drives(&self) -> crate::drives::DriveSet {
        // Two per action at most -- a source and a destination. `saturating`
        // rather than `*`: this is only a hint to the allocator, and a plan
        // large enough to overflow it would be a plan of nine quintillion
        // files.
        let mut paths: Vec<&Path> = Vec::with_capacity(self.actions.len().saturating_mul(2));
        for action in &self.actions {
            paths.push(action.src.as_path());
            if let Some(dest) = action.dest.as_deref() {
                paths.push(dest);
            }
        }
        crate::drives::DriveSet::of(paths)
    }
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
            action.is_link.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Build a plan for copying `sources` into `dest_dir`.
    ///
    /// See [`plan_transfer`](Self::plan_transfer) for what happens to a
    /// source that is already there, and to a folder asked to go inside
    /// itself.
    pub fn plan_copy(
        sources: &[PathBuf],
        dest_dir: &Path,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        Self::plan_transfer(
            sources,
            dest_dir,
            FileOperation::Copy,
            conflict_policy,
            error_policy,
        )
    }

    /// Build a plan for moving `sources` into `dest_dir`.
    ///
    /// A source already in `dest_dir` is left out: see
    /// [`plan_transfer`](Self::plan_transfer).
    pub fn plan_move(
        sources: &[PathBuf],
        dest_dir: &Path,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        Self::plan_transfer(
            sources,
            dest_dir,
            FileOperation::Move,
            conflict_policy,
            error_policy,
        )
    }

    /// What copying and moving share: every source, and everything under
    /// it, with where it goes.
    ///
    /// **A source whose destination is itself is never a taken name.** A
    /// paste back into the folder it came from, or a drop onto its own
    /// folder, puts the source exactly where it already is, and settling
    /// that with the conflict policy is how data is lost: "replace it"
    /// replaces a file with itself, and a move then deletes the source --
    /// the only copy there is. So a copy of it is a duplicate, beside it and
    /// numbered (`notes (2).txt`), whatever the policy says, which is what
    /// a paste into the same folder means everywhere else; and a move of it
    /// has nothing to do and is left out of the plan. The menu's "Replace
    /// it" (2026-09-27) is what made the first reachable: until then the
    /// policy was always to keep both, whose numbered copy hid the problem.
    ///
    /// **A folder cannot go inside itself.** Copying or moving `A` into `A`,
    /// or anywhere under it, is refused: the tree read now would be written
    /// into a branch of itself, and a move would then try to delete the
    /// folder it had just been put in.
    fn plan_transfer(
        sources: &[PathBuf],
        dest_dir: &Path,
        operation: FileOperation,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> io::Result<Self> {
        let mut actions = Vec::new();
        let mut index: u32 = 0;
        let mut total_bytes: u64 = 0;

        for src in sources {
            let file_name = src.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "source has no file name")
            })?;
            if is_real_dir(src) && inside(dest_dir, src) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "\u{201c}{}\u{201d} cannot go inside itself",
                        file_name.shown()
                    ),
                ));
            }
            let mut dest = dest_dir.join(file_name);
            if same_entry(src, &dest) {
                if operation == FileOperation::Move {
                    continue;
                }
                dest = resolve_rename(&dest);
            }
            Self::scan_source(src, &dest, &mut actions, &mut index, &mut total_bytes)?;
        }

        let total_files =
            u32::try_from(actions.iter().filter(|a| !a.is_dir).count()).unwrap_or(u32::MAX);

        Ok(Self {
            operation,
            actions,
            total_bytes,
            total_files,
            conflict_policy,
            error_policy,
        })
    }

    /// Build a plan for linking `sources` into `dest_dir`.
    ///
    /// One action per source and no recursion, for the reason
    /// [`FileOperation::Link`] gives. `total_bytes` is zero: a symbolic link
    /// is a path, and a progress bar that counted the *target's* bytes would
    /// promise a transfer that is not going to happen.
    ///
    /// Nothing is checked here about whether the filesystem supports links.
    /// It is checked when the link is created, per file, so that a batch on a
    /// filesystem that refuses them reports which ones failed rather than the
    /// whole plan refusing up front -- Windows needs a privilege for symlinks,
    /// so "this machine cannot" is a per-attempt answer rather than a
    /// property a planner can read.
    /// Infallible, unlike its `plan_copy` neighbour: there is no scan to
    /// fail. A link plan reads one `is_dir` per source and asks the
    /// filesystem nothing else, so there is no error to report until the
    /// links are actually created.
    #[must_use]
    pub fn plan_link(
        sources: &[PathBuf],
        dest_dir: &Path,
        conflict_policy: ConflictPolicy,
        error_policy: ErrorPolicy,
    ) -> Self {
        let mut actions = Vec::new();
        for (index, src) in sources.iter().enumerate() {
            let Some(name) = src.file_name() else {
                continue;
            };
            // A link made in the folder its target is in goes beside it,
            // numbered, whatever the policy: "replace it" would delete the
            // file to put a link to the deleted file in its place. See
            // `plan_transfer`.
            let mut dest = dest_dir.join(name);
            if same_entry(src, &dest) {
                dest = resolve_rename(&dest);
            }
            actions.push(PlannedAction {
                dest: Some(dest),
                // Recorded from the *source*, and only so that a failure can
                // say "directory" or "file". Nothing walks it.
                is_dir: src.is_dir(),
                is_link: is_link(src),
                src: src.clone(),
                size: 0,
                index: u32::try_from(index).unwrap_or(u32::MAX),
            });
        }

        let total_files = u32::try_from(actions.len()).unwrap_or(u32::MAX);
        Self {
            operation: FileOperation::Link,
            actions,
            total_bytes: 0,
            total_files,
            conflict_policy,
            error_policy,
        }
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

    /// Recursively scan a source path and add planned copy actions, `src`
    /// going to `dest` and everything under it to the same place under
    /// `dest`.
    ///
    /// Given the destination rather than the folder it goes in, because the
    /// top of the tree is not always named as the source is: a duplicate in
    /// the source's own folder is `name (2)`, and everything inside it has to
    /// follow it there.
    fn scan_source(
        src: &Path,
        dest: &Path,
        actions: &mut Vec<PlannedAction>,
        index: &mut u32,
        total_bytes: &mut u64,
    ) -> io::Result<()> {
        let dest = dest.to_path_buf();
        // `symlink_metadata`, which does not follow: a link is one action,
        // copied as the link. See `PlannedAction::is_link`.
        let meta = fs::symlink_metadata(src)?;
        if meta.file_type().is_symlink() {
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: Some(dest),
                size: 0,
                is_dir: false,
                is_link: true,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);
            return Ok(());
        }
        if meta.is_dir() {
            // Directory creation action.
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: Some(dest.clone()),
                size: 0,
                is_dir: true,
                is_link: false,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);

            // Recurse into children.
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                Self::scan_source(
                    &entry.path(),
                    &dest.join(entry.file_name()),
                    actions,
                    index,
                    total_bytes,
                )?;
            }
        } else {
            let size = meta.len();
            *total_bytes = total_bytes.saturating_add(size);
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: Some(dest),
                size,
                is_dir: false,
                is_link: false,
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
        // `symlink_metadata`, which does not follow: a link is deleted as
        // the link, and what it names is left alone. Following it deleted the
        // files of whatever folder the link reached.
        let meta = fs::symlink_metadata(src)?;
        if meta.file_type().is_symlink() {
            actions.push(PlannedAction {
                src: src.to_path_buf(),
                dest: None,
                size: 0,
                is_dir: false,
                is_link: true,
                index: *index,
            });
            *index = index.checked_add(1).unwrap_or(*index);
            return Ok(());
        }
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
                is_link: false,
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
                is_link: false,
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
            if header
                .strip_prefix("plan ")
                .and_then(|id| id.trim().parse::<u64>().ok())
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

/// Where an operation left the thing it acted on -- what an undo needs in
/// order to put it back.
///
/// This was an `Option<PathBuf>`, and the `None` meant two opposite things.
/// A permanent delete recorded `None` for "gone, nothing to reverse", and a
/// recycle recorded `None` too, for "the bin owns it now, restore is by id".
/// [`execute_undo`] skipped both, so undoing a recycle silently reported
/// success and restored nothing. Naming the three cases separately is what
/// makes that state unrepresentable rather than merely fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoTarget {
    /// The item now sits at this path; undo moves it back to the source.
    Path(PathBuf),
    /// The item became this recycle-bin entry. Undo must go back *through the
    /// bin*, by id, so that the bin's metadata and its data stay in step --
    /// renaming the file out from under the bin would leave a listed entry
    /// whose data is gone.
    Recycled(String),
    /// The operation left nothing to reverse: a permanent delete, or an action
    /// that was skipped. Undoing it is a no-op, and saying so is the point --
    /// the caller needs to be able to tell the user nothing came back.
    Nothing,
}

/// Records what an operation did so it can be undone.
#[derive(Clone, Debug)]
pub struct UndoRecord {
    pub id: u64,
    pub operation: FileOperation,
    /// (source, where-it-went) pairs that were acted on.
    pub entries: Vec<(PathBuf, UndoTarget)>,
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
    pub fn push(&mut self, operation: FileOperation, entries: Vec<(PathBuf, UndoTarget)>) -> u64 {
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
    undo_entries: Vec<(PathBuf, UndoTarget)>,
    errors: Vec<FileOpError>,
    events: Vec<FileOpEvent>,
    skipped: u32,
    started: Option<Instant>,
    /// The journal, held across steps. `None` before `begin` and after
    /// `finish`.
    journal: Option<OperationJournal>,
    /// The plan's actions, cloned once by `begin`.
    actions: Vec<PlannedAction>,
    /// The next action to carry out.
    next: usize,
    /// Set when an action ended the operation early -- cancelled, or failed
    /// under `ErrorPolicy::StopOnFirst`. Distinct from `next == len`, which is
    /// the ordinary end, because the two mean different things to a resume.
    stopped: bool,
    /// A file copy part-way through, held between steps.
    cursor: Option<CopyCursor>,
    /// The taken name the operation has stopped at, under
    /// [`ConflictPolicy::Ask`]. While it is here nothing moves; see
    /// [`waiting_on`](Self::waiting_on).
    question: Option<ConflictQuestion>,
    /// How to deal with the taken name of one action, by the action's index:
    /// the answer to its question. Held for as long as that action takes,
    /// because a file copied in pieces meets its taken name again on every
    /// step until the last piece lands.
    answered: Option<(u32, ConflictPolicy)>,
    /// "The same for the rest": the answer given for every later taken name,
    /// in place of the plan's policy.
    policy_for_the_rest: Option<ConflictPolicy>,
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
            journal: None,
            actions: Vec::new(),
            next: 0,
            stopped: false,
            cursor: None,
            question: None,
            answered: None,
            policy_for_the_rest: None,
        }
    }

    /// Run the full operation without returning until it is finished.
    ///
    /// Now a loop over [`step`](Self::step), kept because some callers
    /// genuinely want to block: the tests, and the undo path, which is a short
    /// operation with no window to keep alive. A caller that *does* have a
    /// window should drive `begin`/`step`/`finish` itself -- see `step`.
    ///
    /// Returns the events emitted during execution.
    ///
    /// A plan under [`ConflictPolicy::Ask`] cannot be run this way: there is
    /// nobody to answer, so the first taken name stops it as though it had
    /// been told to stop there.
    pub fn execute(&mut self) -> Vec<FileOpEvent> {
        if !self.begin() {
            return std::mem::take(&mut self.events);
        }
        while !self.is_done() {
            if self.question.is_some() {
                self.answer(ConflictAnswer::Stop, false);
            }
            self.step();
        }
        self.finish();
        std::mem::take(&mut self.events)
    }

    /// Ask the operation to stop after the action now in progress.
    ///
    /// Only reachable now that the operation is stepped: the check this sets
    /// has been at the top of the action loop since the loop was written, and
    /// with a synchronous `execute` there was no moment at which anyone could
    /// have set it.
    ///
    /// **What a cancelled Move leaves behind is deliberate.** `finish` still
    /// runs, and its source deletion is guarded by `journal.transferred`, so
    /// the files that reached the destination have their sources removed and
    /// the files that did not keep theirs. The operation is half done because
    /// it was stopped half way; every individual file is wholly moved or
    /// wholly not. The journal is kept -- it is only removed on `Completed` --
    /// so the operation can be resumed instead.
    ///
    /// An operation waiting on an answer is stopped too: its question is
    /// withdrawn, or `step` -- which does nothing while one is up -- would
    /// never reach the check that ends it.
    pub fn cancel(&mut self) {
        self.question = None;
        self.progress.state = OperationState::Cancelled;
    }

    /// The taken name the operation has stopped at, if it has.
    ///
    /// While there is one, [`step`](Self::step) does nothing and
    /// [`is_done`](Self::is_done) answers false: the operation is neither
    /// finished nor moving, and a caller stepping it in a loop must stop
    /// asking until it has been answered.
    #[must_use]
    pub fn waiting_on(&self) -> Option<&ConflictQuestion> {
        self.question.as_ref()
    }

    /// Answer the question the operation stopped at, and let it go on.
    ///
    /// `for_the_rest` makes the same answer for every later taken name in
    /// this operation, which then stops at none of them. Nothing happens when
    /// there is no question.
    pub fn answer(&mut self, answer: ConflictAnswer, for_the_rest: bool) {
        let Some(question) = self.question.take() else {
            return;
        };
        match answer.policy() {
            None => self.cancel(),
            Some(policy) => {
                self.answered = Some((question.action, policy));
                if for_the_rest {
                    self.policy_for_the_rest = Some(policy);
                }
                self.progress.state = OperationState::Running;
            }
        }
    }

    /// Which plan this is carrying out, for a caller that has to find the
    /// operation again -- the answer to a question comes back to the
    /// operation that asked it.
    #[must_use]
    pub fn plan_id(&self) -> u64 {
        self.plan.id()
    }

    /// Take the events emitted since this was last called.
    ///
    /// For a caller stepping the operation: `execute` hands back everything at
    /// the end, which is no use to a progress bar that wants to move while the
    /// copy is running.
    pub fn take_events(&mut self) -> Vec<FileOpEvent> {
        std::mem::take(&mut self.events)
    }

    /// Return a copy of the current progress.
    pub fn progress(&self) -> &OperationProgress {
        &self.progress
    }

    /// Build undo entries from what was done.
    pub fn into_undo_entries(self) -> (FileOperation, Vec<(PathBuf, UndoTarget)>) {
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

    /// Open the journal and mark the operation running.
    ///
    /// Public, and split from [`execute`](Self::execute), so a caller with an
    /// event loop can own the pacing. Answers whether the operation can proceed: a journal
    /// that will not open is reported and the operation is `Failed` before a
    /// single file is touched.
    pub fn begin(&mut self) -> bool {
        self.started = Some(Instant::now());
        self.progress.state = OperationState::Running;
        // Cloned once, for the reason the loop used to clone it every call:
        // `self.plan.actions` cannot be borrowed while `self.execute_*_action`
        // borrows `self` mutably.
        self.actions = self.plan.actions.clone();
        self.next = 0;
        self.stopped = false;

        // Nothing to do -- a move of files into the folder they are already
        // in plans nothing. There is no destination to keep a journal in, and
        // `journal_dir` would fall back to the process's working directory and
        // write one there.
        if self.actions.is_empty() {
            self.progress.state = OperationState::Completed;
            self.push_summary();
            return true;
        }

        let dest_dir = self.journal_dir();
        let plan_id = self.plan.id();
        match OperationJournal::open(&dest_dir, plan_id) {
            Ok(journal) => {
                self.journal = Some(journal);
                true
            }
            Err(e) => {
                self.progress.state = OperationState::Failed;
                self.events.push(FileOpEvent::Error {
                    path: dest_dir,
                    error: format!("failed to open journal: {e}"),
                });
                false
            }
        }
    }

    /// Whether there is no more work to do.
    ///
    /// True before [`begin`](Self::begin) as well, which is deliberate: a
    /// caller that loops on this without having begun does nothing rather than
    /// spinning on an executor with no journal.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.stopped || self.next >= self.actions.len()
    }

    /// Carry out one planned action.
    ///
    /// **This is what lets the window stay alive during a copy.** The engine
    /// used to run every action in one call, inside an event handler, so for
    /// the length of the operation the explorer did not repaint, did not
    /// answer a click, and could not move the progress bar it was already
    /// computing -- every `FileOpEvent::Progress` it pushed arrived after the
    /// last byte was copied. One action is the unit because it is the unit the
    /// journal already records, so it is also the unit an interrupted
    /// operation resumes from.
    ///
    /// No-op once [`is_done`](Self::is_done) answers true, and while the
    /// operation is waiting on an answer ([`waiting_on`](Self::waiting_on)).
    pub fn step(&mut self) {
        if self.question.is_some() {
            return;
        }
        // Taken and put back rather than borrowed: the body below calls
        // `self.execute_*_action`, which borrows `self` mutably, so a live
        // `&mut self.journal` across it would not compile. Restoring it is the
        // caller's job here precisely so that the many early returns inside
        // `step_action` cannot lose it.
        let Some(mut journal) = self.journal.take() else {
            return;
        };
        self.step_action(&mut journal);
        self.journal = Some(journal);
    }

    /// One action, with the journal already in hand.
    fn step_action(&mut self, journal: &mut OperationJournal) {
        let Some(action) = self.actions.get(self.next).cloned() else {
            return;
        };
        // Advanced before the work, not after: every exit below is an early
        // return, and an action that returned without advancing would be
        // retried for ever.
        self.next = self.next.saturating_add(1);
        let action = &action;
        let operation = self.plan.operation.clone();
        // The answer to this action's own question first, then "the same for
        // the rest", then what the plan was made with.
        let conflict_policy = match self.answered {
            Some((index, policy)) if index == action.index => policy,
            _ => self
                .policy_for_the_rest
                .unwrap_or(self.plan.conflict_policy),
        };
        let error_policy = self.plan.error_policy;

        if self.progress.state == OperationState::Cancelled {
            self.stopped = true;
            return;
        }

        // Skip actions already completed in a previous (interrupted) run.
        if journal.is_complete(action.index) {
            if !action.is_dir {
                self.progress.completed_files = self.progress.completed_files.saturating_add(1);
                self.progress.copied_bytes = self.progress.copied_bytes.saturating_add(action.size);
            }
            return;
        }

        self.progress.current_file = action
            .src
            .file_name()
            .map_or_else(|| action.src.shown().to_string(), |n| n.shown().to_string());

        let result = match operation {
            FileOperation::Copy | FileOperation::Move => {
                self.execute_copy_action(action, conflict_policy)
            }
            FileOperation::Delete => self.execute_delete_action(action),
            FileOperation::Recycle => self.execute_recycle_action(action),
            FileOperation::Restore => self.execute_restore_action(action),
            FileOperation::Link => self.execute_link_action(action, conflict_policy),
        };

        match result {
            // Not finished: step back to the same action, and do not journal
            // it, count it, or give it an undo entry -- none of those are true
            // of a file that is still being written. The only place `next`
            // moves backwards, and it terminates because every visit reads at
            // least one byte until the file ends.
            Ok(ActionOutcome::Partial) => {
                self.next = self.next.saturating_sub(1);
            }
            // Stopped at a taken name: back to the same action, as for
            // `Partial`, and nothing more until the question is answered --
            // not journalled, not counted, nothing to undo.
            Ok(ActionOutcome::Waiting) => {
                self.next = self.next.saturating_sub(1);
                self.progress.state = OperationState::Paused;
            }
            Ok(ActionOutcome::Done) => {
                // A journal write that fails only costs redone work on a
                // resume, which is why it does not abort the operation.
                let _ = journal.mark_complete(action.index);
                if !action.is_dir {
                    self.progress.completed_files = self.progress.completed_files.saturating_add(1);
                    self.progress.copied_bytes =
                        self.progress.copied_bytes.saturating_add(action.size);
                }
            }
            Ok(ActionOutcome::Skipped) => {
                // Recorded as a *skip*: nothing was copied, so a Move must
                // not delete this source. See `OperationJournal`.
                let _ = journal.mark_skipped(action.index);
                self.skipped = self.skipped.saturating_add(1);
                if !action.is_dir {
                    self.progress.completed_files = self.progress.completed_files.saturating_add(1);
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
                        self.stopped = true;
                        return;
                    }
                    ErrorPolicy::SkipAndContinue => {
                        self.skipped = self.skipped.saturating_add(1);
                        return;
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
                                FileOperation::Link => {
                                    self.execute_link_action(action, conflict_policy)
                                }
                            };
                            if let Ok(outcome) = retry {
                                // A retry that came back part-way is not a
                                // success to record: leave the retry loop and
                                // let the next step carry the same action on.
                                if matches!(
                                    outcome,
                                    ActionOutcome::Partial | ActionOutcome::Waiting
                                ) {
                                    if outcome == ActionOutcome::Waiting {
                                        self.progress.state = OperationState::Paused;
                                    }
                                    self.next = self.next.saturating_sub(1);
                                    retried = true;
                                    break;
                                }
                                if matches!(outcome, ActionOutcome::Skipped) {
                                    let _ = journal.mark_skipped(action.index);
                                    self.skipped = self.skipped.saturating_add(1);
                                } else {
                                    let _ = journal.mark_complete(action.index);
                                }
                                if !action.is_dir {
                                    self.progress.completed_files =
                                        self.progress.completed_files.saturating_add(1);
                                    self.progress.copied_bytes =
                                        self.progress.copied_bytes.saturating_add(action.size);
                                }
                                retried = true;
                                break;
                            }
                        }
                        if !retried {
                            self.skipped = self.skipped.saturating_add(1);
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

    /// Everything that happens after the last action.
    ///
    /// A Move's source deletion lives here and nowhere else, which is the
    /// reason this is a separate phase rather than a tail on the last step: it
    /// must run once, after every action has had its turn, and it must see the
    /// finished journal.
    pub fn finish(&mut self) {
        // A copy stopped part-way -- cancelled, or failed under `StopOnFirst`
        // -- leaves a temporary beside its destination. It is removed here
        // rather than left for the user to find: it is a part-sized file under
        // a name they never asked for, and nothing tells them it is ours.
        self.discard_cursor();
        let Some(journal) = self.journal.take() else {
            return;
        };
        let actions: Vec<PlannedAction> = self.actions.clone();
        let operation = self.plan.operation.clone();
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
                // A link that moved is removed as the link: a Windows link to
                // a folder does not go with `remove_file`.
                let removed = if action.is_link {
                    remove_link(&action.src)
                } else {
                    fs::remove_file(&action.src)
                };
                if let Err(e) = removed {
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

        self.push_summary();
    }

    /// The `Complete` event: what was done, skipped and failed.
    fn push_summary(&mut self) {
        let elapsed = self.started.map_or(Duration::ZERO, |s| s.elapsed());
        let succeeded = self.progress.completed_files.saturating_sub(self.skipped);

        self.events.push(FileOpEvent::Complete {
            summary: OperationSummary {
                operation: self.plan.operation.clone(),
                total_files: self.plan.total_files,
                succeeded,
                skipped: self.skipped,
                failed: u32::try_from(self.errors.len()).unwrap_or(u32::MAX),
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

    /// Create one symbolic link.
    ///
    /// The conflict policy is honoured exactly as a copy honours it: a link is
    /// still a file appearing where something may already be, and a user who
    /// asked to rename on conflict means it here too.
    fn execute_link_action(
        &mut self,
        action: &PlannedAction,
        conflict: ConflictPolicy,
    ) -> io::Result<ActionOutcome> {
        let dest = action.dest.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "link action has no destination",
            )
        })?;

        // `symlink_metadata`, not `exists`: a broken link already sitting at
        // the destination is a conflict, and `exists` follows it and answers
        // "no" -- which would make the create below fail with a bare
        // AlreadyExists that the conflict policy never got to rule on.
        let occupied = fs::symlink_metadata(dest).is_ok();
        let dest = if occupied && same_entry(&action.src, dest) {
            // Occupied by the file it would link to: beside it, whatever the
            // policy -- see `plan_link`.
            resolve_rename(dest)
        } else if occupied {
            match conflict {
                ConflictPolicy::Skip => return Ok(ActionOutcome::Skipped),
                // `OverwriteIfNewer` cannot be answered for a link: the
                // comparison is between the *contents'* timestamps, and a link
                // has no contents of its own. Treated as plain overwrite,
                // which is what "replace it" means when there is nothing to
                // compare -- and said here rather than left to a reader to
                // work out from the absence of an arm.
                ConflictPolicy::Overwrite | ConflictPolicy::OverwriteIfNewer => {
                    // A file or a link in the way is replaced; a folder is
                    // not, and `remove_link_or_file` refuses one -- making
                    // room for a link would delete the folder and everything
                    // in it.
                    remove_link_or_file(dest)?;
                    dest.clone()
                }
                ConflictPolicy::Rename => resolve_rename(dest),
                ConflictPolicy::Ask => return Ok(self.ask(action, dest)),
            }
        } else {
            dest.clone()
        };

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        create_symlink(&action.src, &dest, action.is_dir)?;
        self.undo_entries
            .push((action.src.clone(), UndoTarget::Path(dest)));
        Ok(ActionOutcome::Done)
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

        if action.is_link {
            return self.copy_link(action, dest, conflict);
        }

        if action.is_dir {
            // A folder that is its own destination has nothing to create --
            // and must not be given an undo entry, since undoing a copy
            // removes what the copy made, and this would name the original.
            if same_entry(&action.src, dest) {
                return Ok(ActionOutcome::Skipped);
            }
            if !dest.exists() {
                fs::create_dir_all(dest)?;
            }
            self.undo_entries
                .push((action.src.clone(), UndoTarget::Path(dest.clone())));
            return Ok(ActionOutcome::Done);
        }

        // Conflict resolution.
        if dest.exists() {
            // The planner leaves no source that is its own destination, but a
            // name can come to be one by another route -- the same folder
            // spelt two ways, or one renamed while the plan was waiting --
            // and the policy must never be asked about a file and itself:
            // see `plan_transfer`.
            if same_entry(&action.src, dest) {
                if self.plan.operation == FileOperation::Move {
                    return Ok(ActionOutcome::Skipped);
                }
                let renamed = resolve_rename(dest);
                self.atomic_copy_file(&action.src, &renamed)?;
                self.undo_entries
                    .push((action.src.clone(), UndoTarget::Path(renamed)));
                return Ok(ActionOutcome::Done);
            }
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
                    self.undo_entries
                        .push((action.src.clone(), UndoTarget::Path(renamed)));
                    return Ok(ActionOutcome::Done);
                }
                ConflictPolicy::Ask => return Ok(self.ask(action, dest)),
            }
        }

        // Chunked, so a large file does not hold the loop. The undo entry
        // is pushed only when the last byte lands: an entry for a copy that is
        // still running would offer to undo a file that is not there yet.
        let outcome = self.copy_chunk(action, dest)?;
        if outcome == ActionOutcome::Done {
            self.undo_entries
                .push((action.src.clone(), UndoTarget::Path(dest.clone())));
        }
        Ok(outcome)
    }

    /// Stop at `dest`, taken, and say so: the question for
    /// [`waiting_on`](Self::waiting_on), and the event for a caller reading
    /// the event stream.
    fn ask(&mut self, action: &PlannedAction, dest: &Path) -> ActionOutcome {
        self.events.push(FileOpEvent::Conflict {
            src: action.src.clone(),
            dest: dest.to_path_buf(),
            policy: ConflictPolicy::Ask,
        });
        self.question = Some(ConflictQuestion {
            action: action.index,
            src: action.src.clone(),
            dest: dest.to_path_buf(),
        });
        ActionOutcome::Waiting
    }

    /// Copy the link `action.src` to `dest` as a link -- the same target,
    /// never what it names. See [`PlannedAction::is_link`].
    ///
    /// A host that cannot make links (Windows without the privilege) fails
    /// the one action and says why; a move then leaves that link where it
    /// was, since only what arrived is removed at the source.
    fn copy_link(
        &mut self,
        action: &PlannedAction,
        dest: &Path,
        conflict: ConflictPolicy,
    ) -> io::Result<ActionOutcome> {
        let target = fs::read_link(&action.src)?;
        // Whether it names a folder: Windows makes the two kinds differently,
        // and one made as the wrong kind does not open.
        let target_is_dir = fs::metadata(&action.src).is_ok_and(|m| m.is_dir());
        let dest = if fs::symlink_metadata(dest).is_ok() {
            match conflict {
                ConflictPolicy::Skip => return Ok(ActionOutcome::Skipped),
                ConflictPolicy::Ask => return Ok(self.ask(action, dest)),
                ConflictPolicy::Rename => resolve_rename(dest),
                // A link has no contents whose age could be compared -- see
                // `execute_link_action` -- so "if newer" is plain replace.
                ConflictPolicy::Overwrite | ConflictPolicy::OverwriteIfNewer => {
                    remove_link_or_file(dest)?;
                    dest.to_path_buf()
                }
            }
        } else {
            dest.to_path_buf()
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        create_symlink(&target, &dest, target_is_dir)?;
        self.undo_entries
            .push((action.src.clone(), UndoTarget::Path(dest)));
        Ok(ActionOutcome::Done)
    }

    fn execute_delete_action(&mut self, action: &PlannedAction) -> io::Result<ActionOutcome> {
        if action.is_dir {
            fs::remove_dir(&action.src)?;
        } else if action.is_link {
            remove_link(&action.src)?;
        } else {
            fs::remove_file(&action.src)?;
        }
        // A permanent delete really is unrecoverable, and this is the record
        // that says so rather than the one that looks like a recycle.
        self.undo_entries
            .push((action.src.clone(), UndoTarget::Nothing));
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
            .push((action.src.clone(), UndoTarget::Path(dest.clone())));
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
            .push((action.src.clone(), UndoTarget::Path(dest.clone())));
        Ok(ActionOutcome::Done)
    }

    /// Copy `src` to a temporary name next to `dest`, then rename atomically.
    /// Copy one chunk of `action`'s file, starting it if it is not started.
    ///
    /// **This is what stops a single large file freezing the window.** The
    /// step-wise executor made a *file* the unit of interruption, which leaves
    /// one enormous file holding the loop for the length of its copy; this
    /// makes a megabyte the unit instead.
    ///
    /// Answers `Partial` while there is more to write. The caller steps back
    /// to the same action, and the cursor is what makes that terminate: every
    /// visit writes at least as much as it read, and a read of zero is the end
    /// of the file.
    fn copy_chunk(&mut self, action: &PlannedAction, dest: &Path) -> io::Result<ActionOutcome> {
        use std::io::{Read as _, Write as _};

        // A cursor belonging to some other action is not ours to continue.
        // Dropping it costs one file's restart; using it would write this
        // file's bytes into that file.
        if self
            .cursor
            .as_ref()
            .is_some_and(|c| c.action != action.index)
        {
            self.discard_cursor();
        }
        if self.cursor.is_none() {
            self.cursor = Some(self.begin_copy(action.index, &action.src, dest)?);
        }
        let Some(cursor) = self.cursor.as_mut() else {
            return Err(io::Error::other("copy cursor vanished"));
        };

        // Filled, not read once. `Read::read` may return fewer bytes than
        // asked for at any time and for any reason, so a short *read* says
        // nothing; a short *fill* -- reading until the buffer is full or the
        // source gives nothing -- is the end of the file. Without this every
        // file would need one extra step to discover its own end, which for a
        // folder of small files is twice the steps for no bytes.
        let mut buf = vec![0_u8; COPY_CHUNK];
        let mut filled = 0;
        while filled < COPY_CHUNK {
            let Some(rest) = buf.get_mut(filled..) else {
                break;
            };
            let read = cursor.src.read(rest)?;
            if read == 0 {
                break;
            }
            filled = filled.saturating_add(read);
        }

        let Some(chunk) = buf.get(..filled) else {
            return Err(io::Error::other("filled more of the buffer than it has"));
        };
        cursor.tmp.write_all(chunk)?;
        if filled == COPY_CHUNK {
            // The buffer filled, so there may be more. One more turn.
            return Ok(ActionOutcome::Partial);
        }

        // Short of a full buffer: that was the end of the file. Commit it.
        let Some(cursor) = self.cursor.take() else {
            return Err(io::Error::other("copy cursor vanished"));
        };
        Self::commit_copy(cursor)?;
        Ok(ActionOutcome::Done)
    }

    /// Open the source and a fresh temporary beside the destination.
    fn begin_copy(&self, action: u32, src: &Path, dest: &Path) -> io::Result<CopyCursor> {
        let parent = dest.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let tmp_path = parent.join(Self::temp_name(dest));
        Ok(CopyCursor {
            action,
            src: fs::File::open(src)?,
            tmp: fs::File::create(&tmp_path)?,
            tmp_path,
            dest: dest.to_path_buf(),
            src_path: src.to_path_buf(),
        })
    }

    /// Finish a copy: timestamp, then the rename that makes it visible.
    fn commit_copy(cursor: CopyCursor) -> io::Result<()> {
        let CopyCursor {
            tmp,
            tmp_path,
            dest,
            src_path,
            ..
        } = cursor;
        // Dropped before the rename: a file still open for writing cannot be
        // renamed on some platforms, and the data has to be on the disk before
        // the name says it is there.
        drop(tmp);

        if let Ok(src_meta) = fs::metadata(&src_path)
            && let Ok(mtime) = src_meta.modified()
        {
            let _ = set_file_mtime(&tmp_path, mtime);
        }

        // On failure the temporary must go: it is a full-size copy sitting in
        // the user's destination directory under a name they never asked for,
        // and leaving it behind meant a failed copy of a large file silently
        // consumed its own size in disk space.
        if let Err(e) = fs::rename(&tmp_path, &dest) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
        Ok(())
    }

    /// Throw away a copy in progress, and the bytes it had written.
    ///
    /// The temporary is removed rather than left: it is exactly the "full-size
    /// copy under a name the user never asked for" that `commit_copy` cleans
    /// up on its own failure path.
    fn discard_cursor(&mut self) {
        if let Some(cursor) = self.cursor.take() {
            drop(cursor.tmp);
            let _ = fs::remove_file(&cursor.tmp_path);
        }
    }

    /// The temporary name a copy to `dest` writes through.
    /// The scratch name a copy writes to before renaming it into place.
    ///
    /// Built from the destination's name as **bytes**. It went through
    /// `to_string_lossy` until 2026-09-16, which meant two files whose names
    /// differ only in bytes that are not valid UTF-8 produced the *same*
    /// scratch name -- both collapsing to U+FFFD -- so two copies into one
    /// directory could write over each other's temporary file and one would
    /// land holding the other's contents. That is the failure
    /// `TD-C-A-SCRATCH-BACKUP-KEYED-BY-BASENAME-OVERWROTE-THE-FILE-IT-WAS-PROTECTING`
    /// records, reached by a different road.
    ///
    /// An `OsString` keeps every byte, so distinct names stay distinct.
    fn temp_name(dest: &Path) -> std::ffi::OsString {
        let mut name = std::ffi::OsString::from(".");
        name.push(dest.file_name().unwrap_or_else(|| "file".as_ref()));
        name.push(".fileop-tmp");
        name
    }

    fn atomic_copy_file(&self, src: &Path, dest: &Path) -> io::Result<()> {
        let parent = dest.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;

        // The same scratch name the other copy path uses. This was a second
        // copy of the format string, which is how one of them could have been
        // fixed without the other.
        let tmp_path = parent.join(Self::temp_name(dest));

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionOutcome {
    Done,
    Skipped,
    /// A file copy that has written some of its bytes and will need another
    /// turn. The action is *not* finished: it is not journalled, it is not
    /// counted, and the executor steps back to it.
    Partial,
    /// Stopped at a taken name under [`ConflictPolicy::Ask`]: nothing was
    /// done, and the executor steps back to the action once it is answered.
    Waiting,
}

/// How much of one file a single step copies.
///
/// A byte budget rather than a time one, because you cannot know how long a
/// write will take until it returns -- a deadline checked before the write is
/// a deadline the write can overrun by any amount. A megabyte is a few
/// milliseconds on a slow USB stick and well under one on anything internal,
/// which is the frame budget this is for.
const COPY_CHUNK: usize = 1024 * 1024;

/// A file copy that has not finished, carried from one step to the next.
///
/// The handles stay open across steps: reopening and seeking each time would
/// be correct and would also mean a 4 GiB file did four thousand opens. The
/// partial data lives in the same temporary file `atomic_copy_file` has always
/// used, so an interrupted copy leaves a `.name.fileop-tmp` beside the
/// destination and never a half-written file under the name the user expects.
struct CopyCursor {
    /// Which planned action this belongs to.
    ///
    /// Checked before the cursor is used. An executor that somehow arrived at
    /// a different action must not pour this file's remaining bytes into it --
    /// the cursor is dropped instead, which costs a restart of one file.
    action: u32,
    src: fs::File,
    tmp: fs::File,
    tmp_path: PathBuf,
    /// Where the finished file goes.
    dest: PathBuf,
    /// The source, kept for its modification time at the end.
    src_path: PathBuf,
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
// Convenience: execute an undo
// ============================================================================

/// Undo a previously completed operation, returning how many items were put
/// back.
///
/// - Copy undo: delete the copied files.
/// - Move undo: move files back to their original locations.
/// - Recycle undo: restore through `bin`, by entry id.
/// - Permanent-delete undo: nothing, and the count says so.
///
/// # Why it returns a count
///
/// Some records are legitimately un-undoable -- a permanent delete leaves
/// [`UndoTarget::Nothing`], and there is no file to bring back. Returning
/// `Ok(())` for that case told the caller the same thing as a successful
/// restore, so a UI could only report "Undone". The count lets it tell the
/// truth: zero means nothing came back, and the user needs to know that.
///
/// # Errors
///
/// Propagates the underlying filesystem error. Also fails if a record holds a
/// [`UndoTarget::Recycled`] and `bin` is `None`, or if a recycled target turns
/// up under an operation that cannot produce one -- both mean the record and
/// the operation disagree, and guessing which is right would be how a wrong
/// file gets moved.
pub fn execute_undo(record: &UndoRecord, bin: Option<&RecycleBin>) -> io::Result<usize> {
    let mismatch = |op: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("undo record for a {op} names a recycle-bin entry"),
        )
    };
    let mut restored = 0usize;

    match record.operation {
        FileOperation::Copy => {
            // Reverse order: a copied directory can only be removed once the
            // files copied into it are gone.
            for (_src, target) in record.entries.iter().rev() {
                match target {
                    UndoTarget::Path(d) => {
                        if d.is_dir() {
                            // A directory that is not empty is one the user has
                            // since put something in; leaving it is safer than
                            // recursing, and `remove_dir` refuses on its own.
                            if fs::remove_dir(d).is_ok() {
                                restored = restored.saturating_add(1);
                            }
                        } else if d.exists() {
                            fs::remove_file(d)?;
                            restored = restored.saturating_add(1);
                        }
                    }
                    UndoTarget::Recycled(_) => return Err(mismatch("copy")),
                    UndoTarget::Nothing => {}
                }
            }
        }
        FileOperation::Link => {
            // Removing the *link*, never what it points at. `is_dir()` follows
            // a symlink, so the Copy arm above -- which is otherwise the same
            // shape -- would call `remove_dir` on a link to a directory and,
            // on Unix, fail; worse, any code reaching for `remove_dir_all`
            // there would delete the user's folder to undo a shortcut.
            // `remove_link_or_file` asks `symlink_metadata`, which does not
            // follow.
            for (_src, target) in record.entries.iter().rev() {
                match target {
                    UndoTarget::Path(link) => {
                        if fs::symlink_metadata(link).is_ok() && remove_link_or_file(link).is_ok() {
                            restored = restored.saturating_add(1);
                        }
                    }
                    UndoTarget::Recycled(_) => return Err(mismatch("link")),
                    UndoTarget::Nothing => {}
                }
            }
        }
        FileOperation::Move => {
            for (src, target) in &record.entries {
                match target {
                    UndoTarget::Path(d) => {
                        if d.exists() {
                            move_back(src, d)?;
                            restored = restored.saturating_add(1);
                        }
                    }
                    UndoTarget::Recycled(_) => return Err(mismatch("move")),
                    UndoTarget::Nothing => {}
                }
            }
        }
        FileOperation::Delete | FileOperation::Recycle => {
            for (src, target) in &record.entries {
                match target {
                    UndoTarget::Path(d) => {
                        if d.exists() {
                            move_back(src, d)?;
                            restored = restored.saturating_add(1);
                        }
                    }
                    UndoTarget::Recycled(id) => {
                        let Some(bin) = bin else {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "undoing a recycle needs the bin it went into",
                            ));
                        };
                        bin.restore(id)?;
                        restored = restored.saturating_add(1);
                    }
                    // A permanent delete. Nothing to bring back, and the
                    // caller must not be told otherwise.
                    UndoTarget::Nothing => {}
                }
            }
        }
        FileOperation::Restore => {
            // Undo restore = recycle again: move from original back to the bin.
            for (src, target) in &record.entries {
                match target {
                    UndoTarget::Path(d) => {
                        if src.exists() {
                            move_back(d, src)?;
                            restored = restored.saturating_add(1);
                        }
                    }
                    UndoTarget::Recycled(_) => return Err(mismatch("restore")),
                    UndoTarget::Nothing => {}
                }
            }
        }
    }
    Ok(restored)
}

/// Create a symbolic link at `link` pointing at `target`.
///
/// Windows needs to know at creation time whether the target is a directory
/// and needs a privilege (Developer Mode, or SeCreateSymbolicLinkPrivilege) to
/// do it at all; Unix needs neither. The failure that matters is therefore a
/// *runtime* one on Windows, which is why nothing tries to answer "does this
/// filesystem support links" before the attempt.
#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path, target_is_dir: bool) -> io::Result<()> {
    if target_is_dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(windows))]
fn create_symlink(target: &Path, link: &Path, _target_is_dir: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Whether `a` and `b` are one directory entry: the same name in the same
/// folder, however the folder is spelt -- `..`, a link on the way, another
/// case on the Windows development host.
///
/// The entry, and not the file it holds: two links to one file are two
/// entries, and so are two hard links, and replacing one with a copy of the
/// other loses nothing, since the copy is written beside the name and then
/// moved over it. Only an entry and itself are in danger -- a file "replaced"
/// by itself, and then deleted as the source of a move. "Cannot tell" is
/// `false`, which is what "not there yet" should be.
fn same_entry(a: &Path, b: &Path) -> bool {
    let (Some(a_name), Some(b_name)) = (a.file_name(), b.file_name()) else {
        return false;
    };
    let (Some(a_dir), Some(b_dir)) = (a.parent(), b.parent()) else {
        return false;
    };
    let names_match = a_name == b_name || (cfg!(windows) && a_name.eq_ignore_ascii_case(b_name));
    names_match
        && fs::symlink_metadata(a).is_ok()
        && fs::symlink_metadata(b).is_ok()
        && matches!(
            (fs::canonicalize(a_dir), fs::canonicalize(b_dir)),
            (Ok(x), Ok(y)) if x == y
        )
}

/// Whether `path` is `dir` or anywhere under it, compared as resolved paths.
/// "Cannot tell" is `false`.
fn inside(path: &Path, dir: &Path) -> bool {
    match (fs::canonicalize(path), fs::canonicalize(dir)) {
        (Ok(p), Ok(d)) => p.starts_with(&d),
        _ => false,
    }
}

/// Whether `path` is a link (a symbolic link, or a junction on Windows).
fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Whether `path` is a folder itself, and not a link to one.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

/// Remove the link at `path` -- the link, never what it names.
///
/// Refused when `path` is no longer a link: something else has been put there
/// since the plan was made, and removing it would remove what nobody chose.
fn remove_link(path: &Path) -> io::Result<()> {
    if !is_link(path) {
        return Err(io::Error::other("it is no longer a link"));
    }
    // A Windows link to a folder, and a junction, go with `remove_dir`; every
    // Unix link with `remove_file`. Neither follows it.
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

/// Remove a file or a link that is in the way -- never a folder.
///
/// A folder in the way is refused rather than removed: this makes room for
/// one file or one link, and "replace it" never meant deleting a folder and
/// everything in it to do so. It removed one whole until 2026-09-27. A link
/// is removed as the link, never followed: `remove_dir_all` on a link to a
/// folder would delete the folder the old link pointed at.
fn remove_link_or_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return remove_link(path);
    }
    if meta.is_dir() {
        return Err(io::Error::other(
            "a folder of that name is there, and a file or a link does not replace a folder",
        ));
    }
    fs::remove_file(path)
}

/// Move `from` back to `to`, creating the parent it used to live in.
///
/// Shared by every arm above so that "put it back" means one thing; the four
/// hand-written copies of this loop body were identical apart from which way
/// round the pair was read.
fn move_back(to: &Path, from: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(from, to)
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
    use recyclebin::move_path;
    use scratchdir::ScratchDir;
    use std::fs;
    use std::io::Write as IoWrite;
    use std::path::PathBuf;

    /// A private temporary directory for one test, removed when the returned
    /// guard drops.
    ///
    /// This used to tag the directory name with the system clock in
    /// nanoseconds. That is not unique: `cargo test` runs a binary's tests as
    /// threads of a single process, and the clock a thread reads is only
    /// refreshed on a timer interrupt, so every test that starts within the
    /// same tick gets the *same* tag and they scribble on each other's files.
    /// `ScratchDir` names itself from the process id and a per-process atomic
    /// counter, which is unique by construction rather than by luck.
    ///
    /// The caller must bind the guard to a named local — `let scratch = ...` —
    /// and not to `_`, which would drop it (and delete the directory) before
    /// the test's first line.
    fn temp_dir(label: &str) -> ScratchDir {
        crate::guarded_scratch(&format!("fileops_test_{label}"))
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
        let src_scratch = temp_dir("plan_copy_single_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("plan_copy_single_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn plan_copy_directory_tree() {
        let src_scratch = temp_dir("plan_copy_tree_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("plan_copy_tree_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn plan_delete() {
        let src_scratch = temp_dir("plan_delete_src");
        let src_dir = src_scratch.dir().to_path_buf();

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
    }

    // ----------------------------------------------------------------
    // Conflict resolution tests
    // ----------------------------------------------------------------

    #[test]
    fn resolve_rename_basic() {
        let scratch = temp_dir("resolve_rename");
        let dir = scratch.dir().to_path_buf();
        let original = dir.join("file.txt");
        write_file(&original, "original");

        let renamed = resolve_rename(&original);
        assert_eq!(renamed, dir.join("file (2).txt"));

        // Create file (2) and check that (3) is chosen next.
        write_file(&renamed, "copy2");
        let renamed2 = resolve_rename(&original);
        assert_eq!(renamed2, dir.join("file (3).txt"));
    }

    #[test]
    fn resolve_rename_no_extension() {
        let scratch = temp_dir("resolve_rename_noext");
        let dir = scratch.dir().to_path_buf();
        let original = dir.join("Makefile");
        write_file(&original, "data");

        let renamed = resolve_rename(&original);
        assert_eq!(renamed, dir.join("Makefile (2)"));
    }

    /// `prefix` followed by one unit that makes the name not text: a lone
    /// `0xE9` byte (Latin-1 `é`, not UTF-8 on its own) where names are bytes,
    /// an unpaired surrogate on the Windows host this suite also runs on.
    #[cfg(unix)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.push(0xE9);
        std::ffi::OsString::from_vec(bytes)
    }

    #[cfg(windows)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0xD800);
        std::ffi::OsString::from_wide(&units)
    }

    #[test]
    fn a_name_that_is_not_text_keeps_every_byte_through_a_conflict_rename() {
        // The copy of a name that is not text used to be created under
        // U+FFFD, because the new name was built from a lossy decode of the
        // old one. Both the paste and the recycle-bin restore go through here.
        let scratch = temp_dir("resolve_rename_not_text");
        let dir = scratch.dir().to_path_buf();
        let mut name = not_text("caf");
        name.push(".txt");
        let original = dir.join(&name);
        assert!(
            original.to_str().is_none(),
            "the fixture's name is text after all, so this test proves nothing"
        );

        let mut want = not_text("caf");
        want.push(" (2).txt");
        assert_eq!(
            resolve_rename(&original),
            dir.join(&want),
            "the copy's name lost the unit that made it this file's"
        );

        // And the counter still advances past a taken name that is not text.
        fs::write(dir.join(&want), "copy 2").unwrap();
        let mut third = not_text("caf");
        third.push(" (3).txt");
        assert_eq!(resolve_rename(&original), dir.join(&third));
    }

    #[test]
    fn a_move_within_one_directory_does_not_need_a_device_check() {
        // What used to be `same_device_detection`, which asserted that two
        // paths under `/home/user` share a device "because they share a root
        // component" -- and noted, in place of testing it, that "on our OS
        // different mount points would have different first components". They
        // would not: a mount point is a directory, not a prefix. The real
        // question moved to `crate::drives`, which resolves it against the
        // platform and says `None` when it cannot.
        //
        // What is left here is the claim `move_path` actually rests on, which
        // is not about devices at all: a rename inside one directory works,
        // and the fallback is never reached.
        let scratch = temp_dir("move_same_dir");
        let dir = scratch.dir().to_path_buf();
        write_file(&dir.join("before.txt"), "contents");

        move_path(&dir.join("before.txt"), &dir.join("after.txt")).expect("rename");

        assert!(!dir.join("before.txt").exists());
        assert_eq!(read_file(&dir.join("after.txt")), "contents");
    }

    // ----------------------------------------------------------------
    // Journal tests
    // ----------------------------------------------------------------

    #[test]
    fn journal_write_and_read() {
        let scratch = temp_dir("journal_rw");
        let dir = scratch.dir().to_path_buf();

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
    }

    /// A finished action records *whether it transferred data*, because that
    /// is the question a Move has to ask before deleting a source.
    #[test]
    fn journal_distinguishes_a_skip_from_a_copy() {
        let scratch = temp_dir("journal_skip_flag");
        let dir = scratch.dir().to_path_buf();

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
    }

    /// A journal left behind by a *different* plan must be discarded, not read
    /// as this plan's progress. Its indices are plan-relative, so honouring it
    /// would mark unrelated actions as already done and silently never copy
    /// them.
    #[test]
    fn a_journal_from_another_plan_is_discarded() {
        let scratch = temp_dir("journal_stale");
        let dir = scratch.dir().to_path_buf();

        {
            let mut j = OperationJournal::open(&dir, 1).unwrap();
            j.mark_complete(0).unwrap();
            j.mark_complete(1).unwrap();
        }

        let j2 = OperationJournal::open(&dir, 2).unwrap();
        assert_eq!(j2.completed_count(), 0);
        assert!(!j2.is_complete(0));
    }

    /// The end-to-end form of the above: a stale journal in the destination
    /// directory must not cause files to be silently left uncopied.
    #[test]
    fn a_stale_journal_does_not_swallow_a_copy() {
        let src_scratch = temp_dir("journal_stale_e2e_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("journal_stale_e2e_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn journal_resume_skips_completed() {
        let src_scratch = temp_dir("journal_resume_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("journal_resume_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn journal_remove_on_completion() {
        let scratch = temp_dir("journal_remove");
        let dir = scratch.dir().to_path_buf();

        let mut j = OperationJournal::open(&dir, 99).unwrap();
        j.mark_complete(0).unwrap();
        let jpath = j.path().to_path_buf();
        assert!(jpath.exists());

        j.remove().unwrap();
        assert!(!jpath.exists());
    }

    // ----------------------------------------------------------------
    // Move: a source is deleted only when its data reached the destination
    // ----------------------------------------------------------------

    /// A Move whose copy was skipped by conflict policy must leave the source
    /// alone. Deleting it would destroy the only copy of that data — the file
    /// at the destination is a *different*, pre-existing file.
    #[test]
    fn a_skipped_move_does_not_delete_the_source() {
        let src_scratch = temp_dir("move_skip_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("move_skip_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    /// The same guarantee when the copy *failed* rather than being skipped:
    /// `SkipAndContinue` keeps the operation running, but a source whose copy
    /// failed has not been transferred anywhere.
    #[test]
    fn a_failed_move_does_not_delete_the_source() {
        let src_scratch = temp_dir("move_fail_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("move_fail_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
        assert!(
            leftovers.is_empty(),
            "temporaries left behind: {leftovers:?}"
        );
    }

    /// A Move that copies successfully still deletes its source — the fix must
    /// not have turned every Move into a Copy.
    #[test]
    fn a_successful_move_still_deletes_the_source() {
        let src_scratch = temp_dir("move_ok_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("move_ok_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    /// Moving a directory whose contents were partly skipped must leave the
    /// skipped file *and* the directory holding it, and must not report the
    /// non-empty directory as a separate failure.
    #[test]
    fn a_partly_skipped_directory_move_keeps_what_it_did_not_copy() {
        let root_scratch = temp_dir("move_dir_partial_src");
        let root = root_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("move_dir_partial_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    // Recycle failure handling
    // ----------------------------------------------------------------

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
        let scratch = temp_dir("undo_copy");
        let dir = scratch.dir().to_path_buf();
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        write_file(&src, "data");
        write_file(&dst, "data");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Copy,
            entries: vec![(src.clone(), UndoTarget::Path(dst.clone()))],
            timestamp: SystemTime::now(),
        };

        execute_undo(&record, None).unwrap();
        assert!(!dst.exists());
        // Source should still exist (copy undo only removes the destination).
        assert!(src.exists());
    }

    #[test]
    fn undo_move_restores_src() {
        let scratch = temp_dir("undo_move");
        let dir = scratch.dir().to_path_buf();
        let src = dir.join("original.txt");
        let dst = dir.join("moved.txt");
        write_file(&dst, "moved data");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Move,
            entries: vec![(src.clone(), UndoTarget::Path(dst.clone()))],
            timestamp: SystemTime::now(),
        };

        execute_undo(&record, None).unwrap();
        assert!(src.exists());
        assert!(!dst.exists());
        assert_eq!(read_file(&src), "moved data");
    }

    /// A permanent delete is honestly un-undoable, and the count is how the
    /// caller finds that out. Before `UndoTarget`, this case and a recycle
    /// were both `None` and both returned `Ok(())`, so a UI could only say
    /// "Undone" to each -- true of one and a lie about the other.
    #[test]
    fn undoing_a_permanent_delete_puts_nothing_back_and_reports_zero() {
        let scratch = temp_dir("undo_permanent");
        let dir = scratch.dir().to_path_buf();
        let gone = dir.join("gone.txt");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Delete,
            entries: vec![(gone.clone(), UndoTarget::Nothing)],
            timestamp: SystemTime::now(),
        };

        let restored = execute_undo(&record, None).expect("a no-op undo is not an error");
        assert_eq!(restored, 0, "nothing was restored, and it must say so");
        assert!(
            !gone.exists(),
            "a permanently deleted file cannot come back"
        );
    }

    /// Undoing a recycle without the bin is refused rather than skipped.
    ///
    /// The skip is the original defect in miniature: there is a file to
    /// restore and no way to reach it, which is a caller error, not a
    /// successful undo of nothing.
    #[test]
    fn undoing_a_recycle_without_the_bin_is_an_error() {
        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Recycle,
            entries: vec![(
                PathBuf::from("/notes.txt"),
                UndoTarget::Recycled("e1".into()),
            )],
            timestamp: SystemTime::now(),
        };

        let err = execute_undo(&record, None).expect_err("no bin means it cannot be done");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    // ----------------------------------------------------------------
    // Full execution tests
    // ----------------------------------------------------------------

    #[test]
    fn execute_copy_single_file() {
        let src_scratch = temp_dir("exec_copy_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_copy_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
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
    }

    // ---- stepping ---------------------------------------------------
    //
    // What these are for: the engine used to run every action in one call, so
    // the explorer froze for the length of a copy and the progress bar it was
    // already computing could never move. The tests below are about the thing
    // that fixed it -- that the work comes in pieces a caller can interleave
    // with drawing -- and each one of them fails against a single-call engine.

    /// Three files, three steps, and the caller is in charge of every one.
    #[test]
    fn a_plan_is_carried_out_one_action_at_a_time() {
        let src_scratch = temp_dir("step_count_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("step_count_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        for name in ["a.txt", "b.txt", "c.txt"] {
            write_file(&src_dir.join(name), name);
        }

        let plan = OperationPlan::plan_copy(
            &[
                src_dir.join("a.txt"),
                src_dir.join("b.txt"),
                src_dir.join("c.txt"),
            ],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let total = plan.actions.len();
        assert_eq!(total, 3, "three files should plan three actions");

        let mut executor = OperationExecutor::new(plan);
        assert!(
            executor.is_done(),
            "an executor that has not begun has nothing to step"
        );
        assert!(executor.begin(), "the journal did not open");

        let mut steps = 0;
        while !executor.is_done() {
            executor.step();
            steps += 1;
            assert!(steps <= total, "stepping did not terminate");
        }
        executor.finish();

        assert_eq!(steps, total, "one step per planned action");
        for name in ["a.txt", "b.txt", "c.txt"] {
            assert!(dst_dir.join(name).exists(), "{name} was not copied");
        }
    }

    /// **The point of the whole change: progress moves while the copy runs.**
    ///
    /// Asserted between steps rather than at the end. The old engine computed
    /// exactly these numbers and handed them over only once the last byte was
    /// copied, which is the difference between a progress bar and a picture of
    /// one.
    #[test]
    fn progress_advances_between_steps_rather_than_at_the_end() {
        let src_scratch = temp_dir("step_progress_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("step_progress_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        let names = ["one.txt", "two.txt", "three.txt", "four.txt"];
        for name in names {
            write_file(&src_dir.join(name), "0123456789");
        }

        let sources: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
        let plan = OperationPlan::plan_copy(
            &sources,
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        assert_eq!(executor.progress().completed_files, 0);

        let mut seen = Vec::new();
        while !executor.is_done() {
            executor.step();
            seen.push(executor.progress().completed_files);
            // And the events are available *now*, not at the end.
            let events = executor.take_events();
            assert!(
                events.iter().any(|e| matches!(e, FileOpEvent::Progress(_))),
                "a step emitted no progress the caller could draw"
            );
        }
        executor.finish();

        assert_eq!(
            seen,
            vec![1, 2, 3, 4],
            "progress did not climb with the steps"
        );
    }

    /// Cancelling stops the operation where it stands.
    #[test]
    fn cancelling_stops_the_operation_and_leaves_the_rest_alone() {
        let src_scratch = temp_dir("step_cancel_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("step_cancel_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        let names = ["a.txt", "b.txt", "c.txt", "d.txt"];
        for name in names {
            write_file(&src_dir.join(name), name);
        }

        let sources: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
        let plan = OperationPlan::plan_copy(
            &sources,
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        executor.step();
        executor.cancel();
        // The next step is the one that notices, and it must not copy.
        executor.step();
        assert!(executor.is_done(), "a cancelled operation kept going");
        executor.finish();

        assert_eq!(executor.progress().state, OperationState::Cancelled);
        assert!(dst_dir.join("a.txt").exists(), "the first file was undone");
        let copied = names.iter().filter(|n| dst_dir.join(n).exists()).count();
        assert!(copied < names.len(), "cancelling copied everything anyway");
    }

    /// A cancelled operation keeps its journal, so it can be resumed.
    ///
    /// The journal is removed only on `Completed`. That is what makes cancel
    /// different from failure and from finishing: the record of which actions
    /// are done outlives the run that stopped.
    #[test]
    fn a_cancelled_operation_keeps_the_record_of_what_it_did() {
        let src_scratch = temp_dir("step_resume_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("step_resume_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        let names = ["a.txt", "b.txt", "c.txt"];
        for name in names {
            write_file(&src_dir.join(name), name);
        }
        let sources: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
        let make = || {
            OperationPlan::plan_copy(
                &sources,
                &dst_dir,
                ConflictPolicy::Skip,
                ErrorPolicy::StopOnFirst,
            )
            .unwrap()
        };

        let mut executor = OperationExecutor::new(make());
        assert!(executor.begin());
        executor.step();
        executor.cancel();
        executor.step();
        executor.finish();

        // Resuming the same plan finishes the job.
        let mut resumed = OperationExecutor::new(make());
        let events = resumed.execute();
        for name in names {
            assert!(
                dst_dir.join(name).exists(),
                "{name} is missing after resume"
            );
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, FileOpEvent::Complete { .. })),
            "the resumed operation never completed"
        );
    }

    /// A file big enough to matter is copied in pieces.
    ///
    /// **This is the whole point of the chunk.** A step used to be a whole
    /// file, so one four-gigabyte image held the loop -- and the window --
    /// for the length of its copy. The bytes below are two and a half chunks,
    /// which is the smallest size that proves a chunk is not the file.
    #[test]
    fn a_file_larger_than_one_chunk_is_copied_in_more_than_one_step() {
        let src_scratch = temp_dir("chunk_big_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("chunk_big_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

        // Not all one byte: a run of zeroes would survive a copy that dropped
        // a chunk and wrote the next one in its place.
        let size = COPY_CHUNK * 5 / 2;
        let content: Vec<u8> = (0..size)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        let src = src_dir.join("big.bin");
        fs::write(&src, &content).unwrap();

        let plan = OperationPlan::plan_copy(
            std::slice::from_ref(&src),
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 1, "one file is one action");

        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        let mut steps = 0;
        while !executor.is_done() {
            executor.step();
            steps += 1;
            assert!(
                steps <= 16,
                "a two-and-a-half chunk file took {steps} steps"
            );
        }
        executor.finish();

        assert!(steps > 1, "the whole file went in one step: {steps}");
        assert_eq!(
            fs::read(dst_dir.join("big.bin")).unwrap(),
            content,
            "the copy does not match the original"
        );
    }

    /// **A copy stopped part-way leaves nothing under the name the user
    /// expects.**
    ///
    /// The partial bytes go to a temporary, and the rename that gives them
    /// the real name happens only when the last one lands. So a cancelled
    /// copy is not a truncated file wearing the right name -- which would be
    /// indistinguishable from a small file, and is how a backup becomes worse
    /// than no backup.
    #[test]
    fn a_copy_stopped_part_way_writes_no_file_at_the_destination() {
        let src_scratch = temp_dir("chunk_cancel_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("chunk_cancel_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        let src = src_dir.join("big.bin");
        fs::write(&src, vec![7_u8; COPY_CHUNK * 3]).unwrap();

        let plan = OperationPlan::plan_copy(
            &[src],
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        executor.step();
        assert!(!executor.is_done(), "three chunks went in one step");

        executor.cancel();
        executor.step();
        executor.finish();

        assert!(
            !dst_dir.join("big.bin").exists(),
            "a cancelled copy left a truncated file under the real name"
        );
        let leftovers: Vec<String> = fs::read_dir(&dst_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("fileop-tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "a cancelled copy left its scratch file behind: {leftovers:?}"
        );
    }

    /// **A cancelled Move is half a move, not half a file.**
    ///
    /// `finish` still runs, and its source deletion is guarded by what the
    /// journal says actually transferred -- so a file that reached the
    /// destination has its source removed and a file that did not keeps it.
    /// Nothing is left half-written and nothing is lost, which is the property
    /// that matters when the user presses Cancel on a move.
    #[test]
    fn a_cancelled_move_leaves_every_file_wholly_moved_or_wholly_not() {
        let src_scratch = temp_dir("step_cancel_move_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("step_cancel_move_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();
        let names = ["a.txt", "b.txt", "c.txt", "d.txt"];
        for name in names {
            write_file(&src_dir.join(name), name);
        }
        let sources: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
        let plan = OperationPlan::plan_move(
            &sources,
            &dst_dir,
            ConflictPolicy::Skip,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();

        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        executor.step();
        executor.cancel();
        executor.step();
        executor.finish();

        for name in names {
            let at_source = src_dir.join(name).exists();
            let at_dest = dst_dir.join(name).exists();
            assert!(
                at_source != at_dest,
                "{name} is in both places or in neither: source {at_source}, dest {at_dest}"
            );
            if at_dest {
                assert_eq!(read_file(&dst_dir.join(name)), name, "{name} is truncated");
            }
        }
    }

    #[test]
    fn execute_copy_with_skip_conflict() {
        let src_scratch = temp_dir("exec_copy_skip_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_copy_skip_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn execute_copy_with_overwrite_conflict() {
        let src_scratch = temp_dir("exec_copy_ow_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_copy_ow_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn execute_copy_with_rename_conflict() {
        let src_scratch = temp_dir("exec_copy_rn_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_copy_rn_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn execute_copy_directory() {
        let src_scratch = temp_dir("exec_copy_dir_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_copy_dir_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn execute_move_removes_source() {
        let src_scratch = temp_dir("exec_move_src");
        let src_dir = src_scratch.dir().to_path_buf();
        let dst_scratch = temp_dir("exec_move_dst");
        let dst_dir = dst_scratch.dir().to_path_buf();

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
    }

    #[test]
    fn execute_delete() {
        let scratch = temp_dir("exec_delete");
        let dir = scratch.dir().to_path_buf();
        write_file(&dir.join("delme").join("x.txt"), "xxx");
        write_file(&dir.join("delme").join("y.txt"), "yy");

        let plan =
            OperationPlan::plan_delete(&[dir.join("delme")], ErrorPolicy::StopOnFirst).unwrap();

        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        assert!(!dir.join("delme").exists());
    }

    // ------------------------------------------------------------------
    // A damaged recycle entry
    //
    // It used to be skipped by `list`, which meant the file stayed on disk,
    // kept occupying space, and could not be seen or emptied by the only
    // means a user has. These cover it being visible, deletable, and not
    // restorable or silently aged out.
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Linking
    //
    // The planning half is platform-independent and tested outright. The
    // creating half needs a filesystem that will make a symbolic link, which
    // Windows refuses without a privilege -- those tests announce themselves
    // and return rather than passing quietly, so a run on a host that *can*
    // make links is visibly a stronger run than one that cannot.
    // ------------------------------------------------------------------

    /// Whether this machine can create a symbolic link at all.
    ///
    /// Probed by trying, because that is the only reliable answer: Windows
    /// grants the privilege per user and per session, so no property of the
    /// path or the build can be consulted instead.
    fn symlinks_available(dir: &Path) -> bool {
        let target = dir.join("probe-target");
        if fs::write(&target, b"x").is_err() {
            return false;
        }
        let link = dir.join("probe-link");
        let ok = create_symlink(&target, &link, false).is_ok();
        let _ = fs::remove_file(&link);
        let _ = fs::remove_file(&target);
        ok
    }

    #[test]
    fn a_link_plan_makes_one_action_per_source_and_never_walks_a_directory() {
        let scratch = temp_dir("link_plan");
        let dir = scratch.dir().to_path_buf();
        let folder = dir.join("folder");
        fs::create_dir(&folder).expect("mkdir");
        write_file(&folder.join("inside.txt"), "a");
        write_file(&folder.join("also.txt"), "b");
        let loose = dir.join("loose.txt");
        write_file(&loose, "c");

        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("mkdir");
        let plan = OperationPlan::plan_link(
            &[folder.clone(), loose.clone()],
            &dest,
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        );

        assert_eq!(
            plan.actions.len(),
            2,
            "a link plan walked into the directory: {:?}",
            plan.actions.iter().map(|a| &a.src).collect::<Vec<_>>()
        );
        assert_eq!(plan.operation, FileOperation::Link);
    }

    /// A link plan reports no bytes.
    ///
    /// Counting the target's size would put a progress bar on a transfer that
    /// is not going to happen.
    #[test]
    fn a_link_plan_promises_no_transfer() {
        let scratch = temp_dir("link_bytes");
        let dir = scratch.dir().to_path_buf();
        let big = dir.join("big.bin");
        write_file(&big, &"x".repeat(4096));
        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("mkdir");

        let plan = OperationPlan::plan_link(
            &[big],
            &dest,
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        );
        assert_eq!(plan.total_bytes, 0);
        assert_eq!(plan.total_files, 1);
    }

    /// Replacing a plain file with a link does not follow anything.
    #[test]
    fn removing_a_plain_file_in_the_way_of_a_link_removes_that_file() {
        let scratch = temp_dir("link_replace_file");
        let dir = scratch.dir().to_path_buf();
        let victim = dir.join("in-the-way.txt");
        write_file(&victim, "old");

        remove_link_or_file(&victim).expect("remove");
        assert!(!victim.exists());
    }

    /// A folder in the way is not removed to make room for a file or a link.
    ///
    /// It was, whole, until 2026-09-27: "replace it" on a link dropped onto
    /// a folder's name deleted the folder and everything in it.
    #[test]
    fn a_folder_in_the_way_is_never_removed_to_make_room() {
        let scratch = temp_dir("link_replace_dir");
        let dir = scratch.dir().to_path_buf();
        let victim = dir.join("in-the-way");
        fs::create_dir(&victim).expect("mkdir");
        write_file(&victim.join("child.txt"), "x");

        let err = remove_link_or_file(&victim).expect_err("a folder was removed");
        assert!(err.to_string().contains("folder"), "{err}");
        assert_eq!(read_file(&victim.join("child.txt")), "x");
    }

    /// Undoing a link removes the link and leaves the target alone.
    ///
    /// The failure this guards is the worst one available here: `is_dir()`
    /// follows a symlink, so an undo written like the copy undo would reach
    /// through a link to a folder and delete the user's folder to take back a
    /// shortcut.
    #[test]
    fn undoing_a_link_removes_the_link_and_not_its_target() {
        let scratch = temp_dir("link_undo");
        let dir = scratch.dir().to_path_buf();
        if !symlinks_available(&dir) {
            eprintln!(
                "SKIPPED undoing_a_link_removes_the_link_and_not_its_target: \
                 this host will not create symbolic links"
            );
            return;
        }

        let target_dir = dir.join("real-folder");
        fs::create_dir(&target_dir).expect("mkdir");
        write_file(&target_dir.join("keep.txt"), "precious");
        let link = dir.join("shortcut");
        create_symlink(&target_dir, &link, true).expect("link");

        let record = UndoRecord {
            id: 1,
            operation: FileOperation::Link,
            entries: vec![(target_dir.clone(), UndoTarget::Path(link.clone()))],
            timestamp: SystemTime::now(),
        };
        let removed = execute_undo(&record, None).expect("undo");

        assert_eq!(removed, 1);
        assert!(
            fs::symlink_metadata(&link).is_err(),
            "the link survived the undo"
        );
        assert!(
            target_dir.join("keep.txt").exists(),
            "the undo followed the link and deleted the folder it pointed at"
        );
    }

    /// A link is actually created, and resolves to the target's contents.
    #[test]
    fn a_link_action_creates_a_link_that_resolves() {
        let scratch = temp_dir("link_create");
        let dir = scratch.dir().to_path_buf();
        if !symlinks_available(&dir) {
            eprintln!(
                "SKIPPED a_link_action_creates_a_link_that_resolves: this host \
                 will not create symbolic links"
            );
            return;
        }

        let src = dir.join("note.txt");
        write_file(&src, "hello");
        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("mkdir");

        let plan = OperationPlan::plan_link(
            &[src],
            &dest,
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        );
        let mut executor = OperationExecutor::new(plan);
        executor.execute();

        let made = dest.join("note.txt");
        let meta = fs::symlink_metadata(&made).expect("the link exists");
        assert!(meta.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&made).expect("resolves"), "hello");
    }

    /// Two names that differ only in undecodable bytes get different scratch
    /// names.
    ///
    /// The collision this guards is silent and destructive: with a lossy
    /// scratch name both files copy through `.<U+FFFD>.fileop-tmp`, so two
    /// copies into one directory can overwrite each other's temporary and one
    /// arrives holding the other's contents. Windows-only because that is
    /// where such a name can be built in a test; the defect is not.
    #[cfg(windows)]
    #[test]
    fn scratch_names_keep_bytes_that_are_not_utf8_apart() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let a = PathBuf::from(OsString::from_wide(&[0x0041_u16, 0xD800]));
        let b = PathBuf::from(OsString::from_wide(&[0x0041_u16, 0xD801]));
        assert_ne!(a, b, "the fixture is not two different names");
        assert!(
            a.to_str().is_none() && b.to_str().is_none(),
            "fixture is UTF-8"
        );

        assert_ne!(
            OperationExecutor::temp_name(&a),
            OperationExecutor::temp_name(&b),
            "two distinct names share one scratch name, so a copy can land holding the wrong file"
        );
    }

    // ---- asking about a taken name (2026-09-27) ----------------------------
    //
    // `Ask` used to emit its event and skip the file, and a link skipped
    // without the event, so nobody asked could ever answer.

    /// Two files to copy onto a folder that already has the first.
    fn a_copy_onto_a_taken_name(
        label: &str,
        policy: ConflictPolicy,
    ) -> (ScratchDir, PathBuf, OperationExecutor) {
        let scratch = temp_dir(label);
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("src").join("a.txt"), "new a");
        write_file(&root.join("src").join("b.txt"), "new b");
        write_file(&root.join("dst").join("a.txt"), "old a");
        let plan = OperationPlan::plan_copy(
            &[
                root.join("src").join("a.txt"),
                root.join("src").join("b.txt"),
            ],
            &root.join("dst"),
            policy,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        (scratch, root, executor)
    }

    /// Step until the operation finishes or stops to ask, bounded.
    fn run_until_it_asks(executor: &mut OperationExecutor) {
        for _ in 0..1000 {
            if executor.is_done() || executor.waiting_on().is_some() {
                return;
            }
            executor.step();
        }
        panic!("the operation neither finished nor asked");
    }

    #[test]
    fn ask_stops_at_the_taken_name_and_waits() {
        let (_scratch, root, mut executor) =
            a_copy_onto_a_taken_name("ask_waits", ConflictPolicy::Ask);
        run_until_it_asks(&mut executor);
        let question = executor.waiting_on().cloned().expect("it did not ask");
        assert_eq!(question.dest, root.join("dst").join("a.txt"));
        assert_eq!(question.src, root.join("src").join("a.txt"));
        assert!(
            !executor.is_done(),
            "an operation waiting on an answer is not done"
        );
        assert_eq!(executor.progress().state, OperationState::Paused);
        // Stepping a waiting operation does nothing at all.
        for _ in 0..10 {
            executor.step();
        }
        assert!(executor.waiting_on().is_some());
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "old a");
        assert!(
            !root.join("dst").join("b.txt").exists(),
            "it went on past the question"
        );
        assert!(
            executor
                .take_events()
                .iter()
                .any(|e| matches!(e, FileOpEvent::Conflict { .. })),
            "the question was not in the event stream"
        );
    }

    #[test]
    fn each_answer_does_what_it_says_to_the_file_it_was_asked_about() {
        for (answer, check) in [
            (ConflictAnswer::Replace, "replace"),
            (ConflictAnswer::Skip, "skip"),
            (ConflictAnswer::KeepBoth, "keep both"),
        ] {
            let (_scratch, root, mut executor) = a_copy_onto_a_taken_name(
                &format!("ask_{check}").replace(' ', "_"),
                ConflictPolicy::Ask,
            );
            run_until_it_asks(&mut executor);
            executor.answer(answer, false);
            assert_eq!(executor.progress().state, OperationState::Running);
            run_until_it_asks(&mut executor);
            assert!(
                executor.is_done(),
                "{check}: it asked again about a free name"
            );
            executor.finish();
            let dst = root.join("dst");
            match answer {
                ConflictAnswer::Replace => {
                    assert_eq!(read_file(&dst.join("a.txt")), "new a", "{check}");
                }
                ConflictAnswer::Skip => {
                    assert_eq!(read_file(&dst.join("a.txt")), "old a", "{check}");
                }
                ConflictAnswer::KeepBoth => {
                    assert_eq!(read_file(&dst.join("a.txt")), "old a", "{check}");
                    assert_eq!(read_file(&dst.join("a (2).txt")), "new a", "{check}");
                }
                ConflictAnswer::Stop => unreachable!(),
            }
            assert_eq!(
                read_file(&dst.join("b.txt")),
                "new b",
                "{check}: the free name was not copied"
            );
        }
    }

    #[test]
    fn stop_ends_the_operation_where_it_is() {
        let (_scratch, root, mut executor) =
            a_copy_onto_a_taken_name("ask_stop", ConflictPolicy::Ask);
        run_until_it_asks(&mut executor);
        executor.answer(ConflictAnswer::Stop, false);
        run_until_it_asks(&mut executor);
        assert!(executor.is_done());
        executor.finish();
        assert_eq!(executor.progress().state, OperationState::Cancelled);
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "old a");
        assert!(
            !root.join("dst").join("b.txt").exists(),
            "it went on after Stop"
        );
    }

    #[test]
    fn the_same_for_the_rest_asks_no_more() {
        let scratch = temp_dir("ask_rest");
        let root = scratch.dir().to_path_buf();
        for name in ["a.txt", "b.txt", "c.txt"] {
            write_file(&root.join("src").join(name), &format!("new {name}"));
            write_file(&root.join("dst").join(name), &format!("old {name}"));
        }
        let plan = OperationPlan::plan_copy(
            &[
                root.join("src").join("a.txt"),
                root.join("src").join("b.txt"),
                root.join("src").join("c.txt"),
            ],
            &root.join("dst"),
            ConflictPolicy::Ask,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        run_until_it_asks(&mut executor);
        executor.answer(ConflictAnswer::Replace, true);
        run_until_it_asks(&mut executor);
        assert!(
            executor.is_done(),
            "it asked again after \"the same for the rest\""
        );
        executor.finish();
        for name in ["a.txt", "b.txt", "c.txt"] {
            assert_eq!(
                read_file(&root.join("dst").join(name)),
                format!("new {name}")
            );
        }
    }

    #[test]
    fn an_answer_for_one_file_is_not_an_answer_for_the_next() {
        let scratch = temp_dir("ask_each");
        let root = scratch.dir().to_path_buf();
        for name in ["a.txt", "b.txt"] {
            write_file(&root.join("src").join(name), &format!("new {name}"));
            write_file(&root.join("dst").join(name), &format!("old {name}"));
        }
        let plan = OperationPlan::plan_copy(
            &[
                root.join("src").join("a.txt"),
                root.join("src").join("b.txt"),
            ],
            &root.join("dst"),
            ConflictPolicy::Ask,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        run_until_it_asks(&mut executor);
        executor.answer(ConflictAnswer::Replace, false);
        run_until_it_asks(&mut executor);
        let second = executor
            .waiting_on()
            .cloned()
            .expect("the second taken name was not asked about");
        assert_eq!(second.dest, root.join("dst").join("b.txt"));
        executor.answer(ConflictAnswer::Skip, false);
        run_until_it_asks(&mut executor);
        executor.finish();
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "new a.txt");
        assert_eq!(read_file(&root.join("dst").join("b.txt")), "old b.txt");
    }

    #[test]
    fn a_file_copied_in_pieces_keeps_its_answer_to_the_last_piece() {
        let scratch = temp_dir("ask_chunks");
        let root = scratch.dir().to_path_buf();
        let big = "x".repeat(COPY_CHUNK * 2 + 17);
        write_file(&root.join("src").join("big.bin"), &big);
        write_file(&root.join("dst").join("big.bin"), "old");
        let plan = OperationPlan::plan_copy(
            &[root.join("src").join("big.bin")],
            &root.join("dst"),
            ConflictPolicy::Ask,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        run_until_it_asks(&mut executor);
        executor.answer(ConflictAnswer::Replace, false);
        run_until_it_asks(&mut executor);
        assert!(
            executor.is_done(),
            "a later piece of the same file asked again"
        );
        executor.finish();
        assert_eq!(read_file(&root.join("dst").join("big.bin")), big);
    }

    #[test]
    fn a_move_answered_skip_keeps_its_source() {
        let scratch = temp_dir("ask_move");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("src").join("a.txt"), "new a");
        write_file(&root.join("dst").join("a.txt"), "old a");
        let plan = OperationPlan::plan_move(
            &[root.join("src").join("a.txt")],
            &root.join("dst"),
            ConflictPolicy::Ask,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        assert!(executor.begin());
        run_until_it_asks(&mut executor);
        assert!(
            executor.waiting_on().is_some(),
            "a move onto a taken name did not ask"
        );
        executor.answer(ConflictAnswer::Skip, false);
        run_until_it_asks(&mut executor);
        executor.finish();
        assert_eq!(
            read_file(&root.join("src").join("a.txt")),
            "new a",
            "a skipped move lost its source"
        );
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "old a");
    }

    #[test]
    fn execute_cannot_ask_so_the_first_taken_name_stops_it() {
        let scratch = temp_dir("ask_blocking");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("src").join("a.txt"), "new a");
        write_file(&root.join("src").join("b.txt"), "new b");
        write_file(&root.join("dst").join("a.txt"), "old a");
        let plan = OperationPlan::plan_copy(
            &[
                root.join("src").join("a.txt"),
                root.join("src").join("b.txt"),
            ],
            &root.join("dst"),
            ConflictPolicy::Ask,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        assert_eq!(executor.progress().state, OperationState::Cancelled);
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "old a");
    }

    #[test]
    fn cancelling_an_operation_that_is_asking_ends_it() {
        let (_scratch, root, mut executor) =
            a_copy_onto_a_taken_name("ask_cancel", ConflictPolicy::Ask);
        run_until_it_asks(&mut executor);
        executor.cancel();
        assert!(
            executor.waiting_on().is_none(),
            "the question outlived the cancel"
        );
        run_until_it_asks(&mut executor);
        assert!(
            executor.is_done(),
            "a cancelled operation that was asking never ended"
        );
        executor.finish();
        assert_eq!(read_file(&root.join("dst").join("a.txt")), "old a");
        assert!(!root.join("dst").join("b.txt").exists());
    }

    // ---- a source that is its own destination (2026-09-27) ----------------
    //
    // With "Replace it" on the explorer's menu, a cut pasted back into its own
    // folder copied the file onto itself and then deleted the source: the
    // only copy there was.

    const EVERY_POLICY: [ConflictPolicy; 5] = [
        ConflictPolicy::Rename,
        ConflictPolicy::Skip,
        ConflictPolicy::Overwrite,
        ConflictPolicy::OverwriteIfNewer,
        ConflictPolicy::Ask,
    ];

    #[test]
    fn a_file_moved_into_its_own_folder_stays_whatever_the_policy() {
        for policy in EVERY_POLICY {
            let scratch = temp_dir(&format!("self_move_{policy:?}"));
            let root = scratch.dir().to_path_buf();
            write_file(&root.join("note.txt"), "hello");
            let plan = OperationPlan::plan_move(
                &[root.join("note.txt")],
                &root,
                policy,
                ErrorPolicy::StopOnFirst,
            )
            .unwrap();
            assert!(
                plan.actions.is_empty(),
                "{policy:?}: a move to where it is planned work"
            );
            let mut executor = OperationExecutor::new(plan);
            let _events = executor.execute();
            assert_eq!(
                read_file(&root.join("note.txt")),
                "hello",
                "{policy:?}: the file was lost"
            );
            assert!(!root.join("note (2).txt").exists(), "{policy:?}");
            assert_eq!(
                executor.progress().state,
                OperationState::Completed,
                "{policy:?}"
            );
        }
    }

    #[test]
    fn a_file_copied_into_its_own_folder_is_a_duplicate_whatever_the_policy() {
        for policy in EVERY_POLICY {
            let scratch = temp_dir(&format!("self_copy_{policy:?}"));
            let root = scratch.dir().to_path_buf();
            write_file(&root.join("note.txt"), "hello");
            let plan = OperationPlan::plan_copy(
                &[root.join("note.txt")],
                &root,
                policy,
                ErrorPolicy::StopOnFirst,
            )
            .unwrap();
            let mut executor = OperationExecutor::new(plan);
            let _events = executor.execute();
            assert_eq!(read_file(&root.join("note.txt")), "hello", "{policy:?}");
            assert_eq!(
                read_file(&root.join("note (2).txt")),
                "hello",
                "{policy:?}: no duplicate"
            );
        }
    }

    #[test]
    fn a_folder_copied_into_its_own_folder_is_duplicated_whole() {
        let scratch = temp_dir("self_copy_dir");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("album").join("a.txt"), "a");
        write_file(&root.join("album").join("inner").join("b.txt"), "b");
        let plan = OperationPlan::plan_copy(
            &[root.join("album")],
            &root,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        assert_eq!(read_file(&root.join("album (2)").join("a.txt")), "a");
        assert_eq!(
            read_file(&root.join("album (2)").join("inner").join("b.txt")),
            "b"
        );
        assert!(
            !root.join("album").join("a (2).txt").exists(),
            "the files were duplicated inside the original instead"
        );
        assert_eq!(read_file(&root.join("album").join("a.txt")), "a");
    }

    #[test]
    fn a_folder_moved_into_the_folder_it_is_in_is_left_alone() {
        let scratch = temp_dir("self_move_dir");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("album").join("a.txt"), "a");
        let plan = OperationPlan::plan_move(
            &[root.join("album")],
            &root,
            ConflictPolicy::Overwrite,
            ErrorPolicy::StopOnFirst,
        )
        .unwrap();
        assert!(plan.actions.is_empty());
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        assert_eq!(read_file(&root.join("album").join("a.txt")), "a");
    }

    #[test]
    fn a_folder_cannot_go_inside_itself() {
        let scratch = temp_dir("self_inside");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("album").join("inner").join("b.txt"), "b");
        for dest in [root.join("album"), root.join("album").join("inner")] {
            for plan in [
                OperationPlan::plan_copy(
                    &[root.join("album")],
                    &dest,
                    ConflictPolicy::Rename,
                    ErrorPolicy::StopOnFirst,
                ),
                OperationPlan::plan_move(
                    &[root.join("album")],
                    &dest,
                    ConflictPolicy::Rename,
                    ErrorPolicy::StopOnFirst,
                ),
            ] {
                let err = plan.expect_err("a folder was planned into itself");
                assert!(err.to_string().contains("inside itself"), "{err}");
            }
        }
        // A sibling whose name merely starts the same is not inside it.
        fs::create_dir_all(root.join("album2")).unwrap();
        assert!(
            OperationPlan::plan_copy(
                &[root.join("album")],
                &root.join("album2"),
                ConflictPolicy::Rename,
                ErrorPolicy::StopOnFirst,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_link_made_in_its_own_folder_never_replaces_the_file() {
        let scratch = temp_dir("self_link");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("note.txt"), "hello");
        let plan = OperationPlan::plan_link(
            &[root.join("note.txt")],
            &root,
            ConflictPolicy::Overwrite,
            ErrorPolicy::SkipAndContinue,
        );
        assert_eq!(
            plan.actions.first().and_then(|a| a.dest.clone()),
            Some(root.join("note (2).txt"))
        );
        let mut executor = OperationExecutor::new(plan);
        // A host that cannot make links fails the one link; either way the
        // file it points at is still there.
        let _events = executor.execute();
        assert_eq!(read_file(&root.join("note.txt")), "hello");
    }

    // ---- links are never followed (2026-09-27) -----------------------------
    //
    // Every scan followed links: deleting a folder holding a link to another
    // folder deleted that folder's files, moving it copied them and deleted
    // them at the source, and a link to a folder above made the scan endless.
    // Links here are junctions on Windows (no privilege needed) and symbolic
    // links elsewhere.

    /// A link at `link` to the folder `target`, or `None` if this host can
    /// make neither kind.
    fn folder_link(target: &Path, link: &Path) -> Option<()> {
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return Some(());
            }
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .ok()?;
            (made.status.success() && is_link(link)).then_some(())
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(target, link).ok()
        }
    }

    /// `outside/keep.txt`, and `doomed/` holding a file and a link to
    /// `outside`.
    fn a_folder_with_a_link_out(root: &Path) -> Option<()> {
        write_file(&root.join("outside").join("keep.txt"), "not selected");
        write_file(&root.join("doomed").join("mine.txt"), "selected");
        folder_link(&root.join("outside"), &root.join("doomed").join("link"))
    }

    #[test]
    fn deleting_a_folder_never_deletes_what_a_link_inside_it_reaches() {
        let scratch = temp_dir("link_delete");
        let root = scratch.dir().to_path_buf();
        if a_folder_with_a_link_out(&root).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        let plan =
            OperationPlan::plan_delete(&[root.join("doomed")], ErrorPolicy::StopOnFirst).unwrap();
        assert!(
            plan.actions.iter().all(|a| !a
                .src
                .starts_with(root.join("doomed").join("link").join("keep.txt"))),
            "the plan reaches through the link"
        );
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        assert_eq!(
            read_file(&root.join("outside").join("keep.txt")),
            "not selected"
        );
        assert!(
            !root.join("doomed").exists(),
            "the folder itself was not deleted"
        );
    }

    #[test]
    fn moving_a_folder_never_takes_what_a_link_inside_it_reaches() {
        let scratch = temp_dir("link_move");
        let root = scratch.dir().to_path_buf();
        if a_folder_with_a_link_out(&root).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        fs::create_dir_all(root.join("dst")).unwrap();
        let plan = OperationPlan::plan_move(
            &[root.join("doomed")],
            &root.join("dst"),
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();
        let link_actions: Vec<_> = plan.actions.iter().filter(|a| a.is_link).collect();
        assert_eq!(
            link_actions.len(),
            1,
            "the link is one action: {:?}",
            plan.actions
        );
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        assert_eq!(
            read_file(&root.join("outside").join("keep.txt")),
            "not selected"
        );
        assert_eq!(
            read_file(&root.join("dst").join("doomed").join("mine.txt")),
            "selected"
        );
        // Either the link arrived as a link, or this host could not make one
        // and the link stayed where it was -- never a copy of what it reaches.
        let arrived = root.join("dst").join("doomed").join("link");
        assert!(
            is_link(&arrived) || is_link(&root.join("doomed").join("link")),
            "the link was neither moved nor left"
        );
        assert!(
            !arrived.join("keep.txt").exists() || is_link(&arrived),
            "the link's target was copied"
        );
    }

    #[test]
    fn a_link_to_a_folder_above_does_not_make_the_scan_endless() {
        let scratch = temp_dir("link_cycle");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("album").join("a.txt"), "a");
        if folder_link(&root.join("album"), &root.join("album").join("again")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        fs::create_dir_all(root.join("dst")).unwrap();
        let plan = OperationPlan::plan_copy(
            &[root.join("album")],
            &root.join("dst"),
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 3, "{:?}", plan.actions);
        let deletion =
            OperationPlan::plan_delete(&[root.join("album")], ErrorPolicy::StopOnFirst).unwrap();
        assert_eq!(deletion.actions.len(), 3, "{:?}", deletion.actions);
    }

    #[test]
    fn a_link_is_copied_as_the_link_where_the_host_can_make_one() {
        let scratch = temp_dir("link_copy");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("outside").join("keep.txt"), "x");
        if folder_link(&root.join("outside"), &root.join("shortcut")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        fs::create_dir_all(root.join("dst")).unwrap();
        let plan = OperationPlan::plan_copy(
            &[root.join("shortcut")],
            &root.join("dst"),
            ConflictPolicy::Rename,
            ErrorPolicy::SkipAndContinue,
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].is_link);
        let mut executor = OperationExecutor::new(plan);
        let _events = executor.execute();
        let copy = root.join("dst").join("shortcut");
        // A link, or nothing at all where no link can be made here -- never a
        // folder holding a copy of what it reaches.
        assert!(
            is_link(&copy) || fs::symlink_metadata(&copy).is_err(),
            "copied through the link"
        );
    }

    #[test]
    fn a_link_that_is_no_longer_one_is_not_removed() {
        let scratch = temp_dir("link_gone");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("was-a-link"), "a file now");
        let err = remove_link(&root.join("was-a-link")).expect_err("a file was removed as a link");
        assert!(err.to_string().contains("no longer a link"), "{err}");
        assert!(root.join("was-a-link").exists());
    }

    #[test]
    fn the_same_entry_is_the_same_name_in_the_same_folder() {
        let scratch = temp_dir("same_entry");
        let root = scratch.dir().to_path_buf();
        write_file(&root.join("a").join("note.txt"), "x");
        write_file(&root.join("b").join("note.txt"), "x");
        assert!(same_entry(
            &root.join("a").join("note.txt"),
            &root.join("a").join("..").join("a").join("note.txt")
        ));
        assert!(!same_entry(
            &root.join("a").join("note.txt"),
            &root.join("b").join("note.txt")
        ));
        assert!(!same_entry(
            &root.join("a").join("note.txt"),
            &root.join("a").join("absent.txt")
        ));
    }
}
