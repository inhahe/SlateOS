//! The filesystem walk behind the analyzer — the part that reads a real disk.
//!
//! Until this module existed, `scan_directory` took a `FileNode` tree someone
//! else had built and did nothing but sum it. Its own doc comment said so:
//! *"provides the algorithm with stub filesystem calls that accept pre-built
//! `FileNode` trees for testing."* Every rectangle in the treemap, every row in
//! the list and every bar in the extension chart therefore described a disk
//! that did not exist. A disk analyzer whose numbers are invented is worse than
//! one that refuses to start, because the numbers look exactly like measurements
//! and the first thing anyone does with them is delete a file.
//!
//! ## Four things this walk does that a naive one would not
//!
//! **It measures links as links.** [`fs::symlink_metadata`], never
//! [`fs::metadata`]: the difference is between recording a symlink's own few
//! bytes and recording whatever it points at. It also never *descends* through
//! one. That is not only about the loop a link to an ancestor would create — a
//! walk that follows links double-counts, so the same megabyte appears under two
//! paths and the reported total exceeds the size of the disk. For a tool whose
//! entire job is "where did the space go", a total that cannot be trusted is the
//! whole product broken. `du` makes the same choice by default, for the same
//! reason.
//!
//! **It counts what it could not read, and says so.** A directory refused for
//! want of permission is *not* an empty directory, and charging it zero bytes is
//! the failure mode that sends a user hunting through the wrong folder for the
//! space a scan says is not there. [`Outcome::unreadable`] names the first
//! [`MAX_NAMED_UNREADABLE`] of them and [`Outcome::unreadable_count`] counts all
//! of them, so the window can say "12 GB, 3 folders unreadable" rather than
//! silently under-reporting.
//!
//! **It is bounded.** A scan of `/` on a full disk is millions of entries, and a
//! hostile or merely broken tree can be deeper than the stack. [`Limits`] caps
//! both, [`Outcome::truncated`] records that a cap was hit, and the window says
//! so — a truncated scan that claims to be complete is the invented-numbers
//! failure in a second costume.
//!
//! **It runs on its own thread and can be called off.** [`Job::spawn`] puts the
//! walk behind a channel and a [`Shared`] progress block; the window polls it
//! from `Event::Tick`. Done in-line, a scan of a large disk would freeze the
//! window for minutes with no way to stop it, and the `scanning` flag and
//! [`crate::ScanProgress`] that the UI has always carried would have nothing to
//! report. [`Job`]'s `Drop` cancels and joins, so the thread cannot outlive the
//! window it is reporting to.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{DirTree, FileNode};

// ============================================================================
// Limits
// ============================================================================

/// The deepest this walk will ever descend, whatever [`Limits::max_depth`] says.
///
/// `max_depth = 0` means "no limit *I* am imposing", not "recurse without
/// bound": the walk is recursive, and an unbounded recursion over a tree an
/// attacker controls is a stack overflow, which is a crash rather than a scan.
/// Real filesystems are two orders of magnitude shallower than this; a tree that
/// reaches it is pathological, and [`Outcome::truncated`] says the answer is
/// partial.
pub const MAX_DEPTH: u32 = 512;

/// Entries a scan visits before it gives up and reports a partial answer.
///
/// Sized so that a scan of an ordinary desktop disk finishes and a runaway one
/// does not run for hours: at roughly 200 bytes of `FileNode` per entry this
/// caps the tree at a few hundred megabytes of memory.
pub const DEFAULT_MAX_ENTRIES: u64 = 2_000_000;

/// How many unreadable paths are kept by name.
///
/// The count is exact; the *names* are capped, because "you have no permission
/// for these 40,000 directories" is not a message anyone reads. The window shows
/// a handful and the total.
pub const MAX_NAMED_UNREADABLE: usize = 64;

/// Bounds on one walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Deepest level below the root to descend into; 0 means [`MAX_DEPTH`].
    pub max_depth: u32,
    /// Ceiling on entries visited.
    pub max_entries: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 0,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }
}

impl Limits {
    /// The depth ceiling actually applied, with `0` resolved and [`MAX_DEPTH`]
    /// enforced over anything larger.
    #[must_use]
    pub fn depth_ceiling(self) -> u32 {
        if self.max_depth == 0 {
            MAX_DEPTH
        } else {
            self.max_depth.min(MAX_DEPTH)
        }
    }
}

// ============================================================================
// Live progress
// ============================================================================

/// A reading of a walk in flight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Directories opened so far.
    pub dirs: u64,
    /// Non-directory entries recorded so far.
    pub files: u64,
    /// Bytes accounted for so far.
    pub bytes: u64,
    /// The directory currently being read.
    pub current: PathBuf,
}

/// The block a running walk writes to and the window reads from.
///
/// Counters are atomics rather than living behind the one mutex because they are
/// written per *entry* — millions of times in a large scan — while `current` is
/// written once per directory. One lock for both would put a mutex acquisition
/// on the hottest line of the walk to keep a string that is only ever read sixty
/// times a second.
#[derive(Debug, Default)]
pub struct Shared {
    dirs: AtomicU64,
    files: AtomicU64,
    bytes: AtomicU64,
    cancel: AtomicBool,
    current: Mutex<PathBuf>,
}

impl Shared {
    /// Read the counters. Not a consistent snapshot across all four fields, and
    /// deliberately so: this drives a progress line, and a progress line that
    /// took a lock would slow the thing it is measuring.
    #[must_use]
    pub fn snapshot(&self) -> Progress {
        Progress {
            dirs: self.dirs.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            current: self.current_path(),
        }
    }

    /// Ask the walk to stop at the next entry.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether [`Shared::cancel`] has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn current_path(&self) -> PathBuf {
        // A poisoned lock means the walk panicked while holding it. The path
        // inside is then merely stale, not dangerous, and refusing to show a
        // progress line because of it would turn a bug in the walk into a
        // second, unrelated-looking bug in the window.
        match self.current.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn set_current(&self, path: &Path) {
        match self.current.lock() {
            Ok(mut guard) => path.clone_into(&mut guard),
            Err(poisoned) => path.clone_into(&mut poisoned.into_inner()),
        }
    }
}

// ============================================================================
// Outcome
// ============================================================================

/// Everything one walk found, including what it could not.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The tree, with directory sizes already summed.
    pub tree: DirTree,
    /// The first [`MAX_NAMED_UNREADABLE`] paths that could not be read.
    pub unreadable: Vec<PathBuf>,
    /// How many paths could not be read, named or not.
    pub unreadable_count: u64,
    /// Whether a cap in [`Limits`] cut the walk short.
    pub truncated: bool,
    /// Whether [`Shared::cancel`] cut the walk short.
    pub cancelled: bool,
    /// Set when the *root* itself could not be read, in which case the tree is
    /// empty and nothing else here means anything.
    pub root_error: Option<String>,
}

impl Outcome {
    /// Whether the numbers in [`Outcome::tree`] are the whole truth.
    ///
    /// The window must not present a partial total as a complete one, and there
    /// are four separate ways for it to be partial; asking each caller to
    /// remember all four is how one of them gets forgotten.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.root_error.is_none()
            && !self.truncated
            && !self.cancelled
            && self.unreadable_count == 0
    }
}

// ============================================================================
// The walk
// ============================================================================

/// Walk `root`, returning what is there.
///
/// Runs on the calling thread; see [`Job::spawn`] for the version a window can
/// use without freezing.
#[must_use]
pub fn walk(root: &Path, limits: Limits, shared: &Shared) -> Outcome {
    let started = Instant::now();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let mut walker = Walker {
        limits,
        shared,
        budget: limits.max_entries,
        unreadable: Vec::new(),
        unreadable_count: 0,
        truncated: false,
    };

    let meta = match fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(err) => return walker.failed_root(root, &err, timestamp, started),
    };

    let name = display_name(root);
    let mut node = if meta.is_dir() {
        let mut dir = FileNode::new_dir(&name, root);
        walker.fill_dir(&mut dir, 0);
        dir
    } else {
        // Pointing the analyzer at a single file is not an error — it is what
        // "open with" does when the user picks the wrong entry — so it draws a
        // one-rectangle treemap rather than an error box.
        walker.leaf(&name, root, &meta)
    };

    let tree = crate::summarize_tree(&mut node);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Outcome {
        tree: DirTree {
            scan_timestamp: timestamp,
            scan_duration_ms: duration_ms,
            ..tree
        },
        unreadable: walker.unreadable,
        unreadable_count: walker.unreadable_count,
        truncated: walker.truncated,
        cancelled: shared.is_cancelled(),
        root_error: None,
    }
}

/// The display name for a path, which is the last component, or the whole path
/// when there is no last component (`/`, or a bare drive letter).
fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

struct Walker<'a> {
    limits: Limits,
    shared: &'a Shared,
    budget: u64,
    unreadable: Vec<PathBuf>,
    unreadable_count: u64,
    truncated: bool,
}

impl Walker<'_> {
    /// Record a path we could not read.
    fn note_unreadable(&mut self, path: &Path) {
        self.unreadable_count = self.unreadable_count.saturating_add(1);
        if self.unreadable.len() < MAX_NAMED_UNREADABLE {
            self.unreadable.push(path.to_path_buf());
        }
    }

    /// A non-directory entry, with the progress counters advanced for it.
    fn leaf(&mut self, name: &str, path: &Path, meta: &fs::Metadata) -> FileNode {
        let size = meta.len();
        self.shared.files.fetch_add(1, Ordering::Relaxed);
        self.shared.bytes.fetch_add(size, Ordering::Relaxed);
        let file_type = meta.file_type();
        if file_type.is_symlink() {
            FileNode::new_symlink(name, path, size)
        } else if file_type.is_file() {
            FileNode::new_file(name, path, size)
        } else {
            // A device node, a socket, a fifo. It occupies a directory entry and
            // no data blocks, so it is listed at whatever length the filesystem
            // reports rather than being dropped: a file the user can see in a
            // file manager and cannot see here reads as a bug in this program.
            FileNode::new_other(name, path, size)
        }
    }

    /// Read `node`'s directory and fill in its children.
    fn fill_dir(&mut self, node: &mut FileNode, depth: u32) {
        if self.truncated || self.shared.is_cancelled() {
            return;
        }
        if depth >= self.limits.depth_ceiling() {
            // Not an error and not "unreadable": the contents exist, we chose
            // not to look. `truncated` is what says the total is short.
            self.truncated = true;
            return;
        }

        self.shared.set_current(&node.path);
        self.shared.dirs.fetch_add(1, Ordering::Relaxed);

        let entries = match fs::read_dir(&node.path) {
            Ok(entries) => entries,
            Err(_) => {
                self.note_unreadable(&node.path);
                return;
            }
        };

        // Entry-level failures are attributed to the containing directory, once,
        // however many of them there were: a directory that fails to enumerate
        // usually fails for every entry, and 4000 identical lines naming the
        // same folder would push the folders that failed *individually* out of
        // the capped list.
        let mut entry_error = false;

        for entry in entries {
            if self.shared.is_cancelled() {
                return;
            }
            if self.budget == 0 {
                self.truncated = true;
                break;
            }
            self.budget = self.budget.saturating_sub(1);

            let Ok(entry) = entry else {
                entry_error = true;
                continue;
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();

            // `symlink_metadata`, not `metadata`: see the module docs. A link in
            // `/tmp` pointing at `/` would otherwise have this walk measure the
            // entire disk and file it under one entry.
            let Ok(meta) = fs::symlink_metadata(&path) else {
                self.note_unreadable(&path);
                continue;
            };

            let child = if meta.is_dir() {
                let mut dir = FileNode::new_dir(&name, &path);
                self.fill_dir(&mut dir, depth.saturating_add(1));
                dir
            } else {
                self.leaf(&name, &path, &meta)
            };
            node.add_child(child);
        }

        if entry_error {
            self.note_unreadable(&node.path);
        }
    }

    /// The outcome for a root that could not be read at all.
    fn failed_root(
        &mut self,
        root: &Path,
        err: &io::Error,
        timestamp: u64,
        started: Instant,
    ) -> Outcome {
        self.note_unreadable(root);
        let mut node = FileNode::new_dir(&display_name(root), root);
        let tree = crate::summarize_tree(&mut node);
        Outcome {
            tree: DirTree {
                scan_timestamp: timestamp,
                scan_duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                ..tree
            },
            unreadable: std::mem::take(&mut self.unreadable),
            unreadable_count: self.unreadable_count,
            truncated: false,
            cancelled: false,
            root_error: Some(format!("{}: {err}", root.display())),
        }
    }
}

// ============================================================================
// The walk, on its own thread
// ============================================================================

/// A walk running on a background thread.
///
/// The window holds one of these while `scanning` is true, reads
/// [`Job::progress`] to draw the status line and calls [`Job::poll`] from
/// `Event::Tick` until it hands back an [`Outcome`].
pub struct Job {
    shared: Arc<Shared>,
    rx: Receiver<Outcome>,
    handle: Option<JoinHandle<()>>,
    finished: bool,
}

impl Job {
    /// Start walking `root` on a new thread.
    ///
    /// # Errors
    ///
    /// Returns the error from [`thread::Builder::spawn`] if the thread cannot be
    /// created.
    pub fn spawn(root: PathBuf, limits: Limits) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("diskanalyzer-scan".to_string())
            .spawn(move || {
                let outcome = walk(&root, limits, &thread_shared);
                // The receiver is gone when the window closed before the scan
                // ended. That is the normal way a scan is abandoned, not a
                // failure, so there is nothing to report and nobody to report to.
                drop(tx.send(outcome));
            })?;
        Ok(Self {
            shared,
            rx,
            handle: Some(handle),
            finished: false,
        })
    }

    /// A reading of how far the walk has got.
    #[must_use]
    pub fn progress(&self) -> Progress {
        self.shared.snapshot()
    }

    /// Ask the walk to stop. It stops at the next entry; the partial outcome
    /// still arrives through [`Job::poll`], with `cancelled` set.
    pub fn cancel(&self) {
        self.shared.cancel();
    }

    /// The outcome, once there is one. `None` while the walk is still running.
    ///
    /// After it has returned `Some` once it returns `None` forever; a caller
    /// that keeps polling a finished job gets no second copy of the answer.
    pub fn poll(&mut self) -> Option<Outcome> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(outcome) => {
                self.finished = true;
                Some(outcome)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                // The thread ended without sending, which can only be a panic
                // inside the walk. Reporting *nothing* would leave the window
                // saying "Scanning…" until it is closed, so the failure is
                // turned into an outcome that says what happened.
                self.finished = true;
                Some(Outcome {
                    tree: DirTree {
                        root: FileNode::new_dir("", ""),
                        total_size: 0,
                        file_count: 0,
                        dir_count: 0,
                        scan_timestamp: 0,
                        scan_duration_ms: 0,
                    },
                    unreadable: Vec::new(),
                    unreadable_count: 0,
                    truncated: false,
                    cancelled: false,
                    root_error: Some("the scan stopped unexpectedly".to_string()),
                })
            }
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Cancel *then* join, rather than detaching: a walk left running writes
        // to an `Arc` the window has dropped and holds a directory handle open
        // for however long it takes to finish scanning a disk nobody is looking
        // at any more. The walk checks the flag once per entry, so this returns
        // promptly however large the tree is.
        self.shared.cancel();
        if let Some(handle) = self.handle.take() {
            // A panicked walk has already been turned into an outcome by `poll`;
            // there is nothing further to do with the panic here.
            drop(handle.join());
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::FileKind;
    use scratchdir::ScratchDir;

    /// Write `bytes` bytes to `dir/name`.
    fn write_file(dir: &Path, name: &str, bytes: usize) {
        fs::write(dir.join(name), vec![b'x'; bytes]).unwrap();
    }

    fn child<'a>(node: &'a FileNode, name: &str) -> &'a FileNode {
        node.children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no child named {name} among {:?}", node.children.len()))
    }

    #[test]
    fn a_walk_reports_the_bytes_that_are_really_there() {
        let scratch = ScratchDir::new("diskanalyzer_sizes");
        let root = scratch.dir();
        write_file(root, "a.txt", 100);
        write_file(root, "b.bin", 250);
        fs::create_dir(root.join("sub")).unwrap();
        write_file(&root.join("sub"), "c.txt", 7);

        let shared = Shared::default();
        let outcome = walk(root, Limits::default(), &shared);

        assert!(outcome.root_error.is_none());
        assert!(outcome.is_complete(), "a readable scratch tree scans whole");
        assert_eq!(outcome.tree.total_size, 357);
        assert_eq!(outcome.tree.file_count, 3);
        // The root and `sub`.
        assert_eq!(outcome.tree.dir_count, 2);
    }

    #[test]
    fn a_directorys_size_is_the_sum_of_what_is_under_it() {
        let scratch = ScratchDir::new("diskanalyzer_rollup");
        let root = scratch.dir();
        fs::create_dir(root.join("sub")).unwrap();
        write_file(&root.join("sub"), "one", 40);
        write_file(&root.join("sub"), "two", 60);
        write_file(root, "loose", 5);

        let outcome = walk(root, Limits::default(), &Shared::default());
        assert_eq!(child(&outcome.tree.root, "sub").size_bytes, 100);
        assert_eq!(outcome.tree.total_size, 105);
    }

    #[test]
    fn an_empty_directory_scans_to_an_empty_tree_not_an_error() {
        let scratch = ScratchDir::new("diskanalyzer_empty");
        let outcome = walk(scratch.dir(), Limits::default(), &Shared::default());
        assert!(outcome.root_error.is_none());
        assert_eq!(outcome.tree.total_size, 0);
        assert!(outcome.tree.root.children.is_empty());
        assert!(outcome.is_complete());
    }

    #[test]
    fn a_root_that_does_not_exist_is_reported_rather_than_shown_as_empty() {
        let scratch = ScratchDir::new("diskanalyzer_missing");
        let missing = scratch.dir().join("no-such-directory");

        let outcome = walk(&missing, Limits::default(), &Shared::default());

        assert!(
            outcome.root_error.is_some(),
            "a missing root must say so; an empty treemap looks like an empty disk"
        );
        assert_eq!(outcome.unreadable_count, 1);
        assert!(!outcome.is_complete());
    }

    #[test]
    fn pointing_the_scan_at_one_file_measures_that_file() {
        let scratch = ScratchDir::new("diskanalyzer_one_file");
        write_file(scratch.dir(), "solo.dat", 512);
        let target = scratch.dir().join("solo.dat");

        let outcome = walk(&target, Limits::default(), &Shared::default());

        assert!(outcome.root_error.is_none());
        assert_eq!(outcome.tree.total_size, 512);
        assert_eq!(outcome.tree.root.kind, FileKind::RegularFile);
        assert_eq!(outcome.tree.root.name, "solo.dat");
    }

    #[test]
    fn a_depth_limit_stops_the_walk_and_admits_the_total_is_short() {
        let scratch = ScratchDir::new("diskanalyzer_depth");
        let root = scratch.dir();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        write_file(root, "top", 1);
        write_file(&root.join("a"), "one", 10);
        write_file(&root.join("a/b"), "two", 100);
        write_file(&root.join("a/b/c"), "three", 1000);

        let limits = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        let outcome = walk(root, limits, &Shared::default());

        // Depth 0 is the root's own entries, depth 1 is `a`'s. `a/b` is depth 2
        // and is not opened.
        assert_eq!(outcome.tree.total_size, 11);
        assert!(
            outcome.truncated,
            "a total cut short by a depth limit must not be presented as complete"
        );
        assert!(!outcome.is_complete());
    }

    #[test]
    fn an_entry_budget_stops_the_walk_and_admits_the_total_is_short() {
        let scratch = ScratchDir::new("diskanalyzer_budget");
        let root = scratch.dir();
        for i in 0..20 {
            write_file(root, &format!("f{i}"), 10);
        }

        let limits = Limits {
            max_entries: 5,
            ..Limits::default()
        };
        let outcome = walk(root, limits, &Shared::default());

        assert_eq!(outcome.tree.root.children.len(), 5);
        assert!(outcome.truncated);
        assert!(!outcome.is_complete());
    }

    #[test]
    fn depth_zero_means_the_built_in_ceiling_not_no_ceiling() {
        assert_eq!(Limits::default().depth_ceiling(), MAX_DEPTH);
        assert_eq!(
            Limits {
                max_depth: 9,
                ..Limits::default()
            }
            .depth_ceiling(),
            9
        );
        assert_eq!(
            Limits {
                max_depth: u32::MAX,
                ..Limits::default()
            }
            .depth_ceiling(),
            MAX_DEPTH,
            "a caller asking for unbounded recursion is asking for a stack overflow"
        );
    }

    #[test]
    fn progress_counts_up_while_the_walk_runs_and_matches_it_at_the_end() {
        let scratch = ScratchDir::new("diskanalyzer_progress");
        let root = scratch.dir();
        write_file(root, "a", 30);
        write_file(root, "b", 70);

        let shared = Shared::default();
        assert_eq!(shared.snapshot(), Progress::default());

        let outcome = walk(root, Limits::default(), &shared);
        let progress = shared.snapshot();

        assert_eq!(progress.files, 2);
        assert_eq!(progress.bytes, outcome.tree.total_size);
        assert_eq!(progress.dirs, 1);
        assert_eq!(progress.current, root);
    }

    #[test]
    fn a_cancelled_walk_stops_and_says_it_was_cancelled() {
        let scratch = ScratchDir::new("diskanalyzer_cancel");
        let root = scratch.dir();
        for i in 0..50 {
            write_file(root, &format!("f{i}"), 1);
        }

        let shared = Shared::default();
        shared.cancel();
        let outcome = walk(root, Limits::default(), &shared);

        assert!(outcome.cancelled);
        assert!(!outcome.is_complete());
        assert!(
            outcome.tree.root.children.is_empty(),
            "a walk cancelled before it started reads nothing"
        );
    }

    #[test]
    fn a_background_job_delivers_the_same_answer_as_a_direct_walk() {
        let scratch = ScratchDir::new("diskanalyzer_job");
        let root = scratch.dir();
        write_file(root, "a", 11);
        write_file(root, "b", 22);

        let direct = walk(root, Limits::default(), &Shared::default());

        let mut job = Job::spawn(root.to_path_buf(), Limits::default()).unwrap();
        let outcome = loop {
            if let Some(outcome) = job.poll() {
                break outcome;
            }
            thread::yield_now();
        };

        assert_eq!(outcome.tree.total_size, direct.tree.total_size);
        assert_eq!(outcome.tree.file_count, direct.tree.file_count);
        assert!(
            job.poll().is_none(),
            "a finished job must not hand out its answer twice"
        );
    }

    #[test]
    fn dropping_a_job_does_not_leave_the_walk_running() {
        let scratch = ScratchDir::new("diskanalyzer_job_drop");
        let root = scratch.dir();
        for i in 0..200 {
            write_file(root, &format!("f{i}"), 1);
        }

        let job = Job::spawn(root.to_path_buf(), Limits::default()).unwrap();
        // `Drop` cancels and joins. If it detached instead, this would return
        // while the thread still held the directory open, and the scratch
        // directory's own removal could race it.
        drop(job);
    }

    #[test]
    fn names_that_are_not_utf8_survive_as_paths() {
        // The tree's rule is that paths are bytes. The *name* is a lossy string
        // because it is drawn on screen, but the path must still open the file
        // the entry came from -- which is what `PathBuf` guarantees and
        // `String` does not.
        let scratch = ScratchDir::new("diskanalyzer_paths");
        let root = scratch.dir();
        write_file(root, "plain.txt", 3);

        let outcome = walk(root, Limits::default(), &Shared::default());
        let entry = child(&outcome.tree.root, "plain.txt");
        assert_eq!(entry.path, root.join("plain.txt"));
        assert!(fs::symlink_metadata(&entry.path).is_ok());
    }

    #[test]
    fn an_unreadable_path_is_counted_rather_than_charged_zero_bytes() {
        // Constructed without needing a permission model: a directory that is
        // removed between being listed and being read produces exactly the
        // failure a permission denial does, and is the reason the walk must not
        // treat "cannot read" as "empty".
        let mut walker = Walker {
            limits: Limits::default(),
            shared: &Shared::default(),
            budget: 10,
            unreadable: Vec::new(),
            unreadable_count: 0,
            truncated: false,
        };
        walker.note_unreadable(Path::new("/locked"));
        walker.note_unreadable(Path::new("/also-locked"));

        assert_eq!(walker.unreadable_count, 2);
        assert_eq!(
            walker.unreadable,
            [PathBuf::from("/locked"), PathBuf::from("/also-locked")]
        );
    }

    #[test]
    fn the_named_unreadable_list_is_capped_but_the_count_is_not() {
        let shared = Shared::default();
        let mut walker = Walker {
            limits: Limits::default(),
            shared: &shared,
            budget: 0,
            unreadable: Vec::new(),
            unreadable_count: 0,
            truncated: false,
        };
        for i in 0..(MAX_NAMED_UNREADABLE + 40) {
            walker.note_unreadable(Path::new(&format!("/d{i}")));
        }
        assert_eq!(walker.unreadable.len(), MAX_NAMED_UNREADABLE);
        assert_eq!(
            walker.unreadable_count,
            (MAX_NAMED_UNREADABLE + 40) as u64,
            "the total must be exact even when the names are elided"
        );
    }

    #[test]
    fn a_complete_scan_is_only_complete_when_nothing_went_wrong() {
        let scratch = ScratchDir::new("diskanalyzer_complete");
        write_file(scratch.dir(), "a", 1);
        let base = walk(scratch.dir(), Limits::default(), &Shared::default());
        assert!(base.is_complete());

        let with_unreadable = Outcome {
            unreadable_count: 1,
            ..base.clone()
        };
        assert!(!with_unreadable.is_complete());
    }
}
