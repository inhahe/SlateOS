//! Recursive directory traversal engine.
//!
//! Provides efficient recursive directory walking with configurable
//! filtering, depth limits, and visitor callbacks. Used by the file
//! indexer, search, backup, dedup, and health-check subsystems.
//!
//! ## Architecture
//!
//! ```text
//! caller
//!   → fswalk::walk(root, options)
//!   → iterative BFS/DFS traversal via Vfs::readdir()
//!   → filter (depth, name, type, hidden, exclusions)
//!   → visitor callback per entry
//! ```
//!
//! ## Features
//!
//! - **Depth-first or breadth-first** traversal
//! - **Configurable depth limit** (default: 64 levels)
//! - **Path exclusion list** (skip `/proc`, `/sys`, `.git`, etc.)
//! - **Name pattern filtering** (glob match)
//! - **Type filtering** (files only, dirs only, symlinks only)
//! - **Hidden file filtering** (skip dotfiles)
//! - **Follow-symlinks option**
//! - **Error handling** — continues on permission errors, reports them
//! - **Statistics** — files/dirs visited, errors, max depth reached
//!
//! ## Design Notes
//!
//! - Stack-based iteration (no recursion) to avoid stack overflow.
//! - Maximum queue depth of 8192 pending directories to bound memory.
//! - Walk is synchronous — suitable for kernel-space file operations.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum depth.
const DEFAULT_MAX_DEPTH: usize = 64;

/// Maximum pending directories in the walk queue.
const MAX_QUEUE_SIZE: usize = 8192;

/// Maximum results to collect (safety limit).
const MAX_RESULTS: usize = 65536;

/// Default excluded prefixes.
const DEFAULT_EXCLUDES: &[&str] = &["/proc", "/sys", "/dev"];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkOrder {
    /// Depth-first: process children before siblings.
    DepthFirst,
    /// Breadth-first: process all entries at depth N before depth N+1.
    BreadthFirst,
}

/// What to include in results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkFilter {
    /// Include everything.
    All,
    /// Files only.
    FilesOnly,
    /// Directories only.
    DirsOnly,
    /// Symlinks only.
    SymlinksOnly,
}

/// Action returned by the visitor callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkAction {
    /// Continue walking.
    Continue,
    /// Skip this directory's children (prune).
    Skip,
    /// Stop the entire walk.
    Stop,
}

/// A visited entry during the walk.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// Full path of the entry.
    pub path: PathBuf,
    /// Entry type (file, directory, symlink).
    pub entry_type: EntryType,
    /// File size (0 for directories).
    pub size: u64,
    /// Depth relative to the walk root (0 = immediate children of root).
    pub depth: usize,
}

/// Options for configuring a directory walk.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Traversal order.
    pub order: WalkOrder,
    /// Maximum directory depth (0 = root only, no recursion).
    pub max_depth: usize,
    /// Type filter.
    pub filter: WalkFilter,
    /// Glob pattern for name matching (empty = all).
    ///
    /// Bytes, not text: the names it is matched against come from `readdir`
    /// and have no declared encoding, so a `String` pattern could only ever
    /// match the subset of names that happen to decode.
    pub pattern: Vec<u8>,
    /// Whether to include hidden files (names starting with '.').
    pub show_hidden: bool,
    /// Additional subtrees to exclude.
    ///
    /// Matching is by *component*, via [`crate::fs::pathutil::path_in_subtree`]:
    /// excluding `/tmp` excludes `/tmp/scratch` but not `/tmpfiles`.
    pub excludes: Vec<PathBuf>,
    /// Maximum number of results to collect (0 = unlimited up to MAX_RESULTS).
    pub limit: usize,
    /// Whether to follow symbolic links.
    pub follow_symlinks: bool,
    /// Whether to include the root directory itself.
    pub include_root: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            order: WalkOrder::DepthFirst,
            max_depth: DEFAULT_MAX_DEPTH,
            filter: WalkFilter::All,
            pattern: Vec::new(),
            show_hidden: false,
            excludes: Vec::new(),
            limit: 0,
            follow_symlinks: false,
            include_root: false,
        }
    }
}

/// Statistics from a completed walk.
#[derive(Debug, Clone)]
pub struct WalkStats {
    /// Total files visited.
    pub files: u64,
    /// Total directories visited.
    pub dirs: u64,
    /// Total symlinks visited.
    pub symlinks: u64,
    /// Errors encountered (permission denied, etc.).
    pub errors: u64,
    /// Maximum depth reached.
    pub max_depth_reached: usize,
    /// Total size of all files visited.
    pub total_size: u64,
    /// Directories skipped due to exclusion.
    pub excluded: u64,
}

impl WalkStats {
    fn new() -> Self {
        Self {
            files: 0,
            dirs: 0,
            symlinks: 0,
            errors: 0,
            max_depth_reached: 0,
            total_size: 0,
            excluded: 0,
        }
    }

    /// Total entries visited.
    pub fn total(&self) -> u64 {
        self.files + self.dirs + self.symlinks
    }
}

/// Result of a walk operation.
#[derive(Debug, Clone)]
pub struct WalkResult {
    /// Collected entries matching the filter.
    pub entries: Vec<WalkEntry>,
    /// Walk statistics.
    pub stats: WalkStats,
    /// Whether the walk was truncated (hit limit).
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Global walk count.
static WALK_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_ENTRIES: AtomicU64 = AtomicU64::new(0);
static TOTAL_ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Walk a directory tree, collecting matching entries.
///
/// Returns a `WalkResult` with entries and statistics.
pub fn walk<P: AsRef<Path> + ?Sized>(root: &P, opts: &WalkOptions) -> KernelResult<WalkResult> {
    let root = root.as_ref();
    WALK_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut result = WalkResult {
        entries: Vec::new(),
        stats: WalkStats::new(),
        truncated: false,
    };

    let effective_limit = if opts.limit > 0 {
        opts.limit.min(MAX_RESULTS)
    } else {
        MAX_RESULTS
    };

    // Verify root exists and is a directory.
    let meta = Vfs::metadata(root)?;
    if meta.entry_type != EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }

    // Include root directory itself if requested.
    if opts.include_root {
        let entry = WalkEntry {
            path: root.to_path_buf(),
            entry_type: EntryType::Directory,
            size: 0,
            depth: 0,
        };
        if matches_filter(&entry, opts) {
            result.entries.push(entry);
        }
        result.stats.dirs += 1;
    }

    // Stack/queue of (path, depth) for pending directories.
    let mut pending: Vec<(PathBuf, usize)> = Vec::new();
    pending.push((root.to_path_buf(), 0));

    while let Some((dir_path, depth)) = pop_next(&mut pending, opts.order) {
        // Check depth limit.
        if depth >= opts.max_depth {
            continue;
        }

        // Read directory contents.
        let entries = match Vfs::readdir(&dir_path) {
            Ok(e) => e,
            Err(_) => {
                result.stats.errors += 1;
                continue;
            }
        };

        for de in &entries {
            // Skip . and ..
            if de.name.as_path() == Path::new(".") || de.name.as_path() == Path::new("..") {
                continue;
            }

            // Skip hidden files unless requested.  Byte compare, not
            // `Path::starts_with`: the latter matches whole components, so it
            // would ask whether the name *is* `.`.
            if !opts.show_hidden && de.name.as_bytes().starts_with(b".") {
                continue;
            }

            // `Path::join` inserts exactly one separator, so the
            // trailing-slash special case this used to need is gone.
            let full_path = dir_path.join(&de.name);

            // Check exclusions.
            if is_excluded(&full_path, opts) {
                result.stats.excluded += 1;
                continue;
            }

            let child_depth = depth + 1;
            if child_depth > result.stats.max_depth_reached {
                result.stats.max_depth_reached = child_depth;
            }

            match de.entry_type {
                EntryType::Directory => {
                    result.stats.dirs += 1;
                    // Add to pending for recursion.
                    if pending.len() < MAX_QUEUE_SIZE {
                        pending.push((full_path.clone(), child_depth));
                    }
                }
                EntryType::File => {
                    result.stats.files += 1;
                    result.stats.total_size += de.size;
                }
                EntryType::Symlink => {
                    result.stats.symlinks += 1;
                }
                _ => {}
            }

            let walk_entry = WalkEntry {
                path: full_path,
                entry_type: de.entry_type,
                size: de.size,
                depth: child_depth,
            };

            if matches_filter(&walk_entry, opts) {
                if result.entries.len() < effective_limit {
                    result.entries.push(walk_entry);
                } else {
                    result.truncated = true;
                    // Continue walking for stats even if we stop collecting.
                }
            }
        }
    }

    TOTAL_ENTRIES.fetch_add(result.stats.total(), Ordering::Relaxed);
    TOTAL_ERRORS.fetch_add(result.stats.errors, Ordering::Relaxed);

    Ok(result)
}

/// Walk with a visitor callback for each entry.
///
/// The visitor can control traversal by returning `WalkAction`.
/// This avoids collecting all entries in memory.
pub fn walk_visit<P: AsRef<Path> + ?Sized, F>(
    root: &P,
    opts: &WalkOptions,
    mut visitor: F,
) -> KernelResult<WalkStats>
where
    F: FnMut(&WalkEntry) -> WalkAction,
{
    let root = root.as_ref();
    WALK_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut stats = WalkStats::new();

    // Verify root.
    let meta = Vfs::metadata(root)?;
    if meta.entry_type != EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }

    if opts.include_root {
        let entry = WalkEntry {
            path: root.to_path_buf(),
            entry_type: EntryType::Directory,
            size: 0,
            depth: 0,
        };
        stats.dirs += 1;
        if matches_filter(&entry, opts) {
            let action = visitor(&entry);
            if action == WalkAction::Stop {
                return Ok(stats);
            }
        }
    }

    let mut pending: Vec<(PathBuf, usize)> = Vec::new();
    pending.push((root.to_path_buf(), 0));
    let mut stopped = false;

    while let Some((dir_path, depth)) = pop_next(&mut pending, opts.order) {
        if stopped || depth >= opts.max_depth {
            continue;
        }

        let entries = match Vfs::readdir(&dir_path) {
            Ok(e) => e,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };

        for de in &entries {
            if de.name.as_path() == Path::new(".") || de.name.as_path() == Path::new("..") {
                continue;
            }
            if !opts.show_hidden && de.name.as_bytes().starts_with(b".") {
                continue;
            }

            let full_path = dir_path.join(&de.name);

            if is_excluded(&full_path, opts) {
                stats.excluded += 1;
                continue;
            }

            let child_depth = depth + 1;
            if child_depth > stats.max_depth_reached {
                stats.max_depth_reached = child_depth;
            }

            let mut should_queue = false;
            match de.entry_type {
                EntryType::Directory => {
                    stats.dirs += 1;
                    should_queue = true;
                }
                EntryType::File => {
                    stats.files += 1;
                    stats.total_size += de.size;
                }
                EntryType::Symlink => {
                    stats.symlinks += 1;
                }
                _ => {}
            }

            let walk_entry = WalkEntry {
                path: full_path.clone(),
                entry_type: de.entry_type,
                size: de.size,
                depth: child_depth,
            };

            if matches_filter(&walk_entry, opts) {
                let action = visitor(&walk_entry);
                match action {
                    WalkAction::Continue => {}
                    WalkAction::Skip => {
                        should_queue = false;
                    }
                    WalkAction::Stop => {
                        stopped = true;
                        break;
                    }
                }
            }

            if should_queue && pending.len() < MAX_QUEUE_SIZE {
                pending.push((full_path, child_depth));
            }
        }
    }

    TOTAL_ENTRIES.fetch_add(stats.total(), Ordering::Relaxed);
    TOTAL_ERRORS.fetch_add(stats.errors, Ordering::Relaxed);

    Ok(stats)
}

/// Quick count of files and directories under a path (no collecting).
pub fn count<P: AsRef<Path> + ?Sized>(root: &P, max_depth: usize) -> KernelResult<(u64, u64)> {
    let opts = WalkOptions {
        max_depth,
        show_hidden: true,
        ..Default::default()
    };
    let stats = walk_visit(root, &opts, |_| WalkAction::Continue)?;
    Ok((stats.files, stats.dirs))
}

/// Calculate total size of all files under a path.
pub fn total_size<P: AsRef<Path> + ?Sized>(root: &P, max_depth: usize) -> KernelResult<u64> {
    let opts = WalkOptions {
        max_depth,
        show_hidden: true,
        ..Default::default()
    };
    let stats = walk_visit(root, &opts, |_| WalkAction::Continue)?;
    Ok(stats.total_size)
}

/// Find all files matching a glob pattern under a path.
///
/// The pattern is taken as bytes (`impl AsRef<[u8]>` accepts a `&str`
/// literal), so a name that does not decode as UTF-8 is still findable.
pub fn find<P: AsRef<Path> + ?Sized, G: AsRef<[u8]> + ?Sized>(
    root: &P,
    pattern: &G,
    max_depth: usize,
) -> KernelResult<Vec<PathBuf>> {
    let opts = WalkOptions {
        max_depth,
        filter: WalkFilter::FilesOnly,
        pattern: pattern.as_ref().to_vec(),
        show_hidden: true,
        ..Default::default()
    };
    let result = walk(root, &opts)?;
    Ok(result.entries.into_iter().map(|e| e.path).collect())
}

/// Get global walk statistics.
pub fn stats() -> (u64, u64, u64) {
    (
        WALK_COUNT.load(Ordering::Relaxed),
        TOTAL_ENTRIES.load(Ordering::Relaxed),
        TOTAL_ERRORS.load(Ordering::Relaxed),
    )
}

/// Reset global statistics.
pub fn reset_stats() {
    WALK_COUNT.store(0, Ordering::Relaxed);
    TOTAL_ENTRIES.store(0, Ordering::Relaxed);
    TOTAL_ERRORS.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Pop the next directory to process based on traversal order.
fn pop_next(pending: &mut Vec<(PathBuf, usize)>, order: WalkOrder) -> Option<(PathBuf, usize)> {
    match order {
        WalkOrder::DepthFirst => pending.pop(), // LIFO = DFS.
        WalkOrder::BreadthFirst => {
            if pending.is_empty() {
                None
            } else {
                Some(pending.remove(0)) // FIFO = BFS.
            }
        }
    }
}

/// Check if an entry matches the walk filter and pattern.
fn matches_filter(entry: &WalkEntry, opts: &WalkOptions) -> bool {
    // Type filter.
    match opts.filter {
        WalkFilter::All => {}
        WalkFilter::FilesOnly => {
            if entry.entry_type != EntryType::File {
                return false;
            }
        }
        WalkFilter::DirsOnly => {
            if entry.entry_type != EntryType::Directory {
                return false;
            }
        }
        WalkFilter::SymlinksOnly => {
            if entry.entry_type != EntryType::Symlink {
                return false;
            }
        }
    }

    // Pattern filter.  Matched against the final component only, and
    // case-sensitively: this is a case-sensitive filesystem, so two names that
    // differ only in case are two different files and a glob must say so.
    if !opts.pattern.is_empty() {
        let name = entry
            .path
            .file_name()
            .map_or_else(|| entry.path.as_bytes(), Path::as_bytes);
        if !crate::fs::vfs::glob_match(name, &opts.pattern, false) {
            return false;
        }
    }

    true
}

/// Check if a path should be excluded.
fn is_excluded(path: &Path, opts: &WalkOptions) -> bool {
    // Canonical subtree predicate tolerates a trailing slash on the exclude
    // entries. See fs::pathutil.
    for prefix in DEFAULT_EXCLUDES {
        if crate::fs::pathutil::path_in_subtree(path, prefix) {
            return true;
        }
    }
    for prefix in &opts.excludes {
        if crate::fs::pathutil::path_in_subtree(path, prefix) {
            return true;
        }
    }
    false
}

/// Format a byte size for human-readable display.
///
/// Delegates to [`crate::bytesize::iec`]. A directory walk can total more than
/// a TiB, which the private copy this replaced reported in GiB — a four- or
/// five-digit number where a unit change was wanted.
fn format_size(bytes: u64) -> String {
    crate::bytesize::iec(bytes)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[fswalk] Running self-test...");

    test_non_utf8_names();
    test_matches_filter();
    test_walk_options_default();
    test_pop_next();
    test_is_excluded();
    test_format_size();

    serial_println!("[fswalk] Self-test passed (6 tests).");
    Ok(())
}

/// A name that is not valid UTF-8 must survive the pattern and exclude
/// filters.
///
/// Before the byte-path conversion `WalkEntry.path` was a `String` and the
/// pattern was matched with `str` slicing, so the traversal engine every other
/// subsystem builds on - indexer, search, backup, dedup, health-check - was
/// structurally unable to report such a file.
fn test_non_utf8_names() {
    // 0xFF is not a legal UTF-8 byte anywhere in a sequence.
    let entry = WalkEntry {
        path: PathBuf::from(b"/data/re\xffport.txt".as_slice()),
        entry_type: EntryType::File,
        size: 7,
        depth: 1,
    };

    let by_ext = WalkOptions {
        pattern: b"*.txt".to_vec(),
        ..Default::default()
    };
    assert!(matches_filter(&entry, &by_ext));

    // `?` matches the single undecodable byte.
    let by_wildcard = WalkOptions {
        pattern: b"re?port.*".to_vec(),
        ..Default::default()
    };
    assert!(matches_filter(&entry, &by_wildcard));

    // The pattern itself may be undecodable too.
    let by_literal = WalkOptions {
        pattern: b"re\xffport.txt".to_vec(),
        ..Default::default()
    };
    assert!(matches_filter(&entry, &by_literal));

    // The pattern applies to the final component, not the whole path.
    let by_dir = WalkOptions {
        pattern: b"data*".to_vec(),
        ..Default::default()
    };
    assert!(!matches_filter(&entry, &by_dir));

    // And an exclude with an undecodable component still prunes.
    let excluded = WalkOptions {
        excludes: vec![PathBuf::from(b"/da\xffta".as_slice())],
        ..Default::default()
    };
    assert!(is_excluded(
        Path::new(b"/da\xffta/file".as_slice()),
        &excluded
    ));
    assert!(!is_excluded(Path::new("/data/file"), &excluded));

    serial_println!("[fswalk]   non-UTF-8 names: ok");
}

fn test_matches_filter() {
    let file_entry = WalkEntry {
        path: PathBuf::from("/test/file.txt"),
        entry_type: EntryType::File,
        size: 100,
        depth: 1,
    };
    let dir_entry = WalkEntry {
        path: PathBuf::from("/test/subdir"),
        entry_type: EntryType::Directory,
        size: 0,
        depth: 1,
    };

    let all_opts = WalkOptions::default();
    assert!(matches_filter(&file_entry, &all_opts));
    assert!(matches_filter(&dir_entry, &all_opts));

    let files_only = WalkOptions {
        filter: WalkFilter::FilesOnly,
        ..Default::default()
    };
    assert!(matches_filter(&file_entry, &files_only));
    assert!(!matches_filter(&dir_entry, &files_only));

    let with_pattern = WalkOptions {
        pattern: b"*.txt".to_vec(),
        ..Default::default()
    };
    assert!(matches_filter(&file_entry, &with_pattern));
    assert!(!matches_filter(&dir_entry, &with_pattern));

    // Case-sensitive: `FILE.TXT` and `file.txt` are two different files here.
    let upper = WalkOptions {
        pattern: b"*.TXT".to_vec(),
        ..Default::default()
    };
    assert!(!matches_filter(&file_entry, &upper));

    serial_println!("[fswalk]   matches_filter: ok");
}

fn test_walk_options_default() {
    let opts = WalkOptions::default();
    assert_eq!(opts.max_depth, DEFAULT_MAX_DEPTH);
    assert_eq!(opts.order, WalkOrder::DepthFirst);
    assert_eq!(opts.filter, WalkFilter::All);
    assert!(!opts.show_hidden);
    assert!(!opts.follow_symlinks);
    assert!(!opts.include_root);
    serial_println!("[fswalk]   walk_options_default: ok");
}

fn test_pop_next() {
    // DFS: LIFO.
    let mut stack = vec![
        (PathBuf::from("first"), 0),
        (PathBuf::from("second"), 1),
        (PathBuf::from("third"), 2),
    ];
    let (path, _) = pop_next(&mut stack, WalkOrder::DepthFirst).unwrap();
    assert_eq!(path, PathBuf::from("third"));

    // BFS: FIFO.
    let mut queue = vec![
        (PathBuf::from("first"), 0),
        (PathBuf::from("second"), 1),
        (PathBuf::from("third"), 2),
    ];
    let (path, _) = pop_next(&mut queue, WalkOrder::BreadthFirst).unwrap();
    assert_eq!(path, PathBuf::from("first"));

    serial_println!("[fswalk]   pop_next: ok");
}

fn test_is_excluded() {
    let opts = WalkOptions::default();
    assert!(is_excluded(Path::new("/proc/cpuinfo"), &opts));
    assert!(is_excluded(Path::new("/sys/class"), &opts));
    assert!(is_excluded(Path::new("/dev/null"), &opts));
    assert!(!is_excluded(Path::new("/home/user/file.txt"), &opts));

    // Component-aligned, not a byte prefix: `/devices` is not under `/dev`.
    assert!(!is_excluded(Path::new("/devices/pci0"), &opts));

    let custom = WalkOptions {
        excludes: vec![PathBuf::from("/tmp"), PathBuf::from(".git")],
        ..Default::default()
    };
    assert!(is_excluded(Path::new("/tmp/scratch"), &custom));
    assert!(!is_excluded(Path::new("/tmpfiles/scratch"), &custom));

    serial_println!("[fswalk]   is_excluded: ok");
}

fn test_format_size() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1024), "1.0 KiB");
    assert_eq!(format_size(1024 * 1024), "1.0 MiB");
    assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
    serial_println!("[fswalk]   format_size: ok");
}
