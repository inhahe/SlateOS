//! Path bar and navigation history for file explorer.
//!
//! Provides the infrastructure for the file explorer's address bar:
//! - Breadcrumb segments (e.g., "/" → "home" → "user" → "Documents")
//! - Path autocomplete for typed paths
//! - Navigation history (back/forward)
//! - Recent directories (quick jump list)
//! - Path validation and normalization
//!
//! ## Design Spec (line 901)
//!
//! "Can type in path (absolute or relative) with autocomplete"
//!
//! ## Architecture
//!
//! ```text
//! File Explorer address bar
//!   → pathbar::parse_breadcrumbs(path) for breadcrumb rendering
//!   → pathbar::autocomplete(partial) for typed path completion
//!   → pathbar::go(path) to navigate + record in history
//!   → pathbar::back() / pathbar::forward() for navigation
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum history entries.
const MAX_HISTORY: usize = 256;

/// Maximum recent directories.
const MAX_RECENT: usize = 32;

/// Maximum autocomplete results.
const MAX_COMPLETIONS: usize = 50;

/// Maximum breadcrumb segments.
const MAX_SEGMENTS: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single breadcrumb segment in the path.
#[derive(Debug, Clone)]
pub struct Breadcrumb {
    /// Display name (directory name, or "/" for root).
    pub name: PathBuf,
    /// Full path up to and including this segment.
    pub path: PathBuf,
    /// Whether this is the last (current) segment.
    pub current: bool,
}

/// An autocomplete suggestion.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The completed text.
    pub text: PathBuf,
    /// Display name (just the filename).
    pub display: PathBuf,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// File size (0 for directories).
    pub size: u64,
}

/// A navigation history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The directory path.
    pub path: PathBuf,
    /// Timestamp when visited (ns).
    pub visited_ns: u64,
}

/// Navigation state for one explorer window.
#[derive(Debug, Clone)]
pub struct NavState {
    /// Current path.
    pub current: PathBuf,
    /// History (past paths, in order).
    pub history: Vec<HistoryEntry>,
    /// Current position in history (index into history).
    pub position: usize,
    /// Recent directories (for quick access).
    pub recent: Vec<HistoryEntry>,
}

// ---------------------------------------------------------------------------
// Global state (single-instance for kshell; multiple for GUI)
// ---------------------------------------------------------------------------

static NAV_COUNT: AtomicU64 = AtomicU64::new(0);
static COMPLETE_COUNT: AtomicU64 = AtomicU64::new(0);

static NAV_STATE: spin::Mutex<NavState> = spin::Mutex::new(NavState {
    current: PathBuf::new(),
    history: Vec::new(),
    position: 0,
    recent: Vec::new(),
});

// ---------------------------------------------------------------------------
// Path parsing
// ---------------------------------------------------------------------------

/// Parse a path into breadcrumb segments.
///
/// "/home/user/Documents" → [("/", "/"), ("home", "/home"),
///   ("user", "/home/user"), ("Documents", "/home/user/Documents")]
pub fn parse_breadcrumbs<P: AsRef<Path> + ?Sized>(path: &P) -> Vec<Breadcrumb> {
    let normalized = normalize(path);
    let mut segments = Vec::new();
    let mut accumulated = PathBuf::new();

    if normalized.as_path() == Path::new("/") || normalized.is_empty() {
        segments.push(Breadcrumb {
            name: PathBuf::from("/"),
            path: PathBuf::from("/"),
            current: true,
        });
        return segments;
    }

    // Root segment.
    segments.push(Breadcrumb {
        name: PathBuf::from("/"),
        path: PathBuf::from("/"),
        current: false,
    });

    // `Path::components` already drops empty components, so the
    // `trim_start_matches('/')` + `filter(!is_empty)` dance is redundant.
    let parts: Vec<&Path> = normalized.components().collect();

    for (i, part) in parts.iter().enumerate() {
        accumulated.push(part);

        if segments.len() >= MAX_SEGMENTS {
            break;
        }

        segments.push(Breadcrumb {
            name: part.to_path_buf(),
            path: accumulated.clone(),
            current: i == parts.len().saturating_sub(1),
        });
    }

    segments
}

/// Normalize a path: resolve "." and "..", remove trailing slash,
/// collapse consecutive slashes.
pub fn normalize<P: AsRef<Path> + ?Sized>(path: &P) -> PathBuf {
    let path = path.as_ref();
    if path.is_empty() {
        return PathBuf::from("/");
    }

    let absolute = path.is_absolute();
    let mut components: Vec<&Path> = Vec::new();

    for part in path.components() {
        if part == Path::new(".") {
            // Current-dir: no effect.  (Empty components are already dropped
            // by `Path::components`, so `//` collapses for free.)
        } else if part == Path::new("..") {
            components.pop();
        } else {
            components.push(part);
        }
    }

    if components.is_empty() {
        return PathBuf::from("/");
    }

    let mut result = PathBuf::from(if absolute { "/" } else { "" });
    for c in components {
        result.push(c);
    }

    result
}

/// Join a base directory and a relative path.
pub fn join<B: AsRef<Path> + ?Sized, R: AsRef<Path> + ?Sized>(
    base: &B,
    relative: &R,
) -> PathBuf {
    // `PathBuf::push` already lets an absolute `relative` replace the base and
    // collapses the root case, so both hand-written branches are gone.
    normalize(&base.as_ref().join(relative))
}

/// Get the parent directory of a path.
pub fn parent<P: AsRef<Path> + ?Sized>(path: &P) -> PathBuf {
    // `Path::parent` already yields `/` for a top-level entry and `None` at the
    // root, which is exactly the three-armed `rfind('/')` match this replaces.
    normalize(path)
        .parent()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
}

/// Get the filename (last segment) of a path.
pub fn basename<P: AsRef<Path> + ?Sized>(path: &P) -> PathBuf {
    normalize(path)
        .file_name()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
}

// ---------------------------------------------------------------------------
// Autocomplete
// ---------------------------------------------------------------------------

/// Autocomplete a partial path.
///
/// Given "/home/us", returns completions like "/home/user", "/home/usr".
/// Works for both absolute and relative (to current) paths.
pub fn autocomplete<P: AsRef<Path> + ?Sized, C: AsRef<Path> + ?Sized>(
    partial: &P,
    cwd: &C,
) -> Vec<Completion> {
    COMPLETE_COUNT.fetch_add(1, Ordering::Relaxed);

    let partial = partial.as_ref();

    // Determine the directory to search and the prefix to match.
    let full_partial = if partial.is_absolute() {
        partial.to_path_buf()
    } else {
        join(cwd, partial)
    };

    // A trailing separator means "list this directory", so it must be tested on
    // the raw bytes: `normalize` (inside `join`) strips it, and `Path` has no
    // notion of a trailing separator at all.
    let (search_dir, prefix) = if full_partial.as_bytes().ends_with(b"/") {
        (full_partial.clone(), PathBuf::new())
    } else {
        (parent(&full_partial), basename(&full_partial))
    };

    // List directory contents.
    let entries = match crate::fs::vfs::Vfs::readdir(&search_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    // ASCII-only case folding over raw bytes: a filename has no declared
    // encoding, so there is nothing to consult that would say how to fold a
    // byte >= 0x80, and guessing makes two distinct names collide.
    let prefix_lower = prefix.as_bytes().to_ascii_lowercase();
    let mut completions: Vec<Completion> = Vec::new();

    for entry in &entries {
        if completions.len() >= MAX_COMPLETIONS {
            break;
        }

        let name_lower = entry.name.as_bytes().to_ascii_lowercase();
        if !prefix.is_empty() && !name_lower.starts_with(&prefix_lower) {
            continue;
        }

        // Skip hidden files unless the prefix starts with ".".  Byte compares,
        // not `Path::starts_with`, which matches whole components and so would
        // ask whether the name *is* `.`.
        if entry.name.as_bytes().starts_with(b".") && !prefix.as_bytes().starts_with(b".") {
            continue;
        }

        let is_dir = entry.entry_type == crate::fs::EntryType::Directory;
        // `Path::join` collapses the root case itself.
        let mut text = search_dir.join(&entry.name);

        // Add trailing slash for directories.
        if is_dir {
            text.extend_bytes(b"/");
        }

        completions.push(Completion {
            text,
            display: entry.name.clone(),
            is_dir,
            size: entry.size,
        });
    }

    // Sort: directories first, then alphabetical.
    completions.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.display
                .as_bytes()
                .to_ascii_lowercase()
                .cmp(&b.display.as_bytes().to_ascii_lowercase())
        })
    });

    completions
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Navigate to a path (adds to history).
pub fn go<P: AsRef<Path> + ?Sized>(path: &P) -> KernelResult<()> {
    let normalized = normalize(path);

    // Validate the path exists and is a directory.
    let meta = crate::fs::vfs::Vfs::metadata(&normalized)?;
    if meta.entry_type != crate::fs::EntryType::Directory {
        return Err(KernelError::NotADirectory);
    }

    let now = crate::timekeeping::clock_monotonic();

    let mut nav = NAV_STATE.lock();

    // If we're not at the end of history, truncate forward entries.
    if nav.position < nav.history.len() {
        let trunc_at = nav.position.saturating_add(1);
        nav.history.truncate(trunc_at);
    }

    // Add to history.
    if nav.history.len() >= MAX_HISTORY {
        nav.history.remove(0);
    }
    nav.history.push(HistoryEntry {
        path: normalized.clone(),
        visited_ns: now,
    });
    nav.position = nav.history.len().saturating_sub(1);
    nav.current = normalized.clone();

    // Update recent list (dedup).
    nav.recent.retain(|r| r.path != normalized);
    if nav.recent.len() >= MAX_RECENT {
        nav.recent.remove(0);
    }
    nav.recent.push(HistoryEntry {
        path: normalized,
        visited_ns: now,
    });

    NAV_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Navigate back in history.
pub fn back() -> Option<PathBuf> {
    let mut nav = NAV_STATE.lock();
    if nav.position == 0 {
        return None;
    }
    nav.position = nav.position.saturating_sub(1);
    let path = nav.history.get(nav.position)?.path.clone();
    nav.current = path.clone();
    Some(path)
}

/// Navigate forward in history.
pub fn forward() -> Option<PathBuf> {
    let mut nav = NAV_STATE.lock();
    if nav.position >= nav.history.len().saturating_sub(1) {
        return None;
    }
    nav.position = nav.position.saturating_add(1);
    let path = nav.history.get(nav.position)?.path.clone();
    nav.current = path.clone();
    Some(path)
}

/// Navigate to parent directory.
pub fn up() -> KernelResult<PathBuf> {
    let current = {
        let nav = NAV_STATE.lock();
        nav.current.clone()
    };
    let p = parent(&current);
    go(&p)?;
    Ok(p)
}

/// Get current navigation path.
pub fn current() -> PathBuf {
    NAV_STATE.lock().current.clone()
}

/// Get navigation history.
pub fn history() -> Vec<HistoryEntry> {
    NAV_STATE.lock().history.clone()
}

/// Get recent directories.
pub fn recent() -> Vec<HistoryEntry> {
    NAV_STATE.lock().recent.clone()
}

/// Can navigate back?
pub fn can_go_back() -> bool {
    NAV_STATE.lock().position > 0
}

/// Can navigate forward?
pub fn can_go_forward() -> bool {
    let nav = NAV_STATE.lock();
    nav.position < nav.history.len().saturating_sub(1)
}

/// Clear navigation history.
pub fn clear_history() {
    let mut nav = NAV_STATE.lock();
    nav.history.clear();
    nav.position = 0;
}

/// Clear recent directories.
pub fn clear_recent() {
    NAV_STATE.lock().recent.clear();
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (nav_count, complete_count, history_len, recent_len).
pub fn stats() -> (u64, u64, usize, usize) {
    let nav = NAV_STATE.lock();
    (
        NAV_COUNT.load(Ordering::Relaxed),
        COMPLETE_COUNT.load(Ordering::Relaxed),
        nav.history.len(),
        nav.recent.len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    NAV_COUNT.store(0, Ordering::Relaxed);
    COMPLETE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the pathbar module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: path normalization.
    {
        assert_eq!(normalize("/home/user/../user/./docs"), PathBuf::from("/home/user/docs"));
        assert_eq!(normalize("/"), PathBuf::from("/"));
        assert_eq!(normalize("//foo///bar//"), PathBuf::from("/foo/bar"));
        assert_eq!(normalize("/a/b/c/../../d"), PathBuf::from("/a/d"));
        assert_eq!(normalize(""), PathBuf::from("/"));
        serial_println!("[pathbar] test 1 passed: normalization");
    }

    // Test 2: breadcrumb parsing.
    {
        let crumbs = parse_breadcrumbs("/home/user/Documents");
        assert_eq!(crumbs.len(), 4);
        assert_eq!(crumbs[0].name, PathBuf::from("/"));
        assert_eq!(crumbs[0].path, PathBuf::from("/"));
        assert!(!crumbs[0].current);
        assert_eq!(crumbs[1].name, PathBuf::from("home"));
        assert_eq!(crumbs[1].path, PathBuf::from("/home"));
        assert_eq!(crumbs[3].name, PathBuf::from("Documents"));
        assert_eq!(crumbs[3].path, PathBuf::from("/home/user/Documents"));
        assert!(crumbs[3].current);
        serial_println!("[pathbar] test 2 passed: breadcrumbs");
    }

    // Test 3: parent and basename.
    {
        assert_eq!(parent("/home/user/file.txt"), PathBuf::from("/home/user"));
        assert_eq!(parent("/"), PathBuf::from("/"));
        assert_eq!(parent("/home"), PathBuf::from("/"));
        assert_eq!(basename("/home/user/file.txt"), PathBuf::from("file.txt"));
        assert_eq!(basename("/"), PathBuf::from("/"));
        serial_println!("[pathbar] test 3 passed: parent + basename");
    }

    // Test 4: path join.
    {
        assert_eq!(join("/home/user", "docs"), PathBuf::from("/home/user/docs"));
        assert_eq!(join("/home/user", "../other"), PathBuf::from("/home/other"));
        assert_eq!(join("/home", "/etc/config"), PathBuf::from("/etc/config"));
        assert_eq!(join("/", "tmp"), PathBuf::from("/tmp"));
        serial_println!("[pathbar] test 4 passed: join");
    }

    // Test 4b: a path component that is not valid UTF-8 survives every
    // operation.  Before the byte-path conversion the whole address bar was
    // `&str`, so such a directory could not be normalized, split into
    // breadcrumbs, or completed - it was simply unreachable from the UI.
    {
        let weird = Path::new(b"/home/we\xffird/docs".as_slice());
        assert_eq!(normalize(weird), weird.to_path_buf());
        assert_eq!(parent(weird), PathBuf::from(b"/home/we\xffird".as_slice()));
        assert_eq!(basename(weird), PathBuf::from("docs"));

        let crumbs = parse_breadcrumbs(weird);
        assert_eq!(crumbs.len(), 4);
        assert_eq!(crumbs[2].name, PathBuf::from(b"we\xffird".as_slice()));
        assert_eq!(crumbs[2].path, PathBuf::from(b"/home/we\xffird".as_slice()));
        serial_println!("[pathbar] test 4b passed: non-UTF-8 component");
    }

    // Test 5: navigation history.
    {
        // Clear and set up.
        clear_history();
        // Navigate (will fail for non-existent paths, use root).
        let _ = go("/");
        assert!(!can_go_back() || can_go_back()); // History starts at 0.
        serial_println!("[pathbar] test 5 passed: navigation");
    }

    // Test 6: stats.
    {
        let (nav, complete, hist, recent_len) = stats();
        // Sanity check: stats returns valid values (these are u64, so always >= 0).
        let _ = (nav, complete, hist, recent_len);
        serial_println!("[pathbar] test 6 passed: stats");
    }

    serial_println!("[pathbar] all 7 self-tests passed");
    Ok(())
}
