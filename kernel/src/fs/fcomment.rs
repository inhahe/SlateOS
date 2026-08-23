//! File comments and annotations.
//!
//! Attaches user-visible text comments to files and directories.
//! Comments appear in the file explorer Properties dialog and can
//! be searched.  Unlike tags (short labels) or queryable attributes
//! (typed key-value metadata), comments are free-form text meant
//! for human consumption.
//!
//! ## Design Reference
//!
//! design.txt lines 377, 391-392: "Arbitrary string/comment? ... max
//! size maybe 64 KiB? Stored in extended attributes."
//!
//! ## Architecture
//!
//! ```text
//! User types comment in Properties dialog
//!   → fcomment::set("/docs/report.pdf", "Q3 quarterly report, needs review")
//!   → stored in COMMENT_STORE
//!
//! File explorer shows comment in tooltip or detail column
//!   → fcomment::get("/docs/report.pdf")
//!   → "Q3 quarterly report, needs review"
//!
//! Search for files by comment content
//!   → fcomment::search("quarterly", "/docs")
//!   → [("/docs/report.pdf", "Q3 quarterly report, needs review")]
//! ```
//!
//! ## Limits
//!
//! - Max 65536 bytes per comment (design says 64 KiB)
//! - Max 65536 files with comments
//! - Comments are stored as plain UTF-8 text

#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::path::{Path, PathBuf};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum comment size in bytes (64 KiB).
const MAX_COMMENT_SIZE: usize = 65536;

/// Maximum files with comments.
const MAX_FILES: usize = 65536;

/// Maximum search results.
const MAX_SEARCH_RESULTS: usize = 4096;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

struct CommentStore {
    /// Path → comment text.
    /// Keyed by `PathBuf`, not `String`: the key is a filesystem name,
    /// which may hold any byte but `/` and NUL, while the value is a
    /// human-written comment and so is genuinely text. See
    /// `design-decisions.md` §261.
    comments: BTreeMap<PathBuf, String>,
}

impl CommentStore {
    const fn new() -> Self {
        Self {
            comments: BTreeMap::new(),
        }
    }
}

static STORE: Mutex<CommentStore> = Mutex::new(CommentStore::new());
static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static SEARCH_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// ASCII-lowercase for case-insensitive search.
fn to_ascii_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push((c as u8 + 32) as char);
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Set a comment on a file (replaces any existing comment).
pub fn set(path: impl AsRef<Path>, comment: &str) -> KernelResult<()> {
    let path = path.as_ref();
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if comment.len() > MAX_COMMENT_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut store = STORE.lock();
    if !store.comments.contains_key(path) && store.comments.len() >= MAX_FILES {
        return Err(KernelError::ResourceExhausted);
    }

    if comment.is_empty() {
        // Empty comment = remove.
        store.comments.remove(path);
    } else {
        store
            .comments
            .insert(path.to_path_buf(), String::from(comment));
    }
    Ok(())
}

/// Get the comment on a file (None if no comment).
pub fn get(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    GET_COUNT.fetch_add(1, Ordering::Relaxed);
    let store = STORE.lock();
    store.comments.get(path).cloned()
}

/// Remove the comment from a file.
pub fn remove(path: impl AsRef<Path>) -> KernelResult<()> {
    let path = path.as_ref();
    let mut store = STORE.lock();
    store.comments.remove(path).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Append text to an existing comment (or create a new one).
pub fn append(path: impl AsRef<Path>, text: &str) -> KernelResult<()> {
    let path = path.as_ref();
    if text.is_empty() {
        return Ok(());
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut store = STORE.lock();
    if let Some(existing) = store.comments.get_mut(path) {
        if existing.len() + text.len() + 1 > MAX_COMMENT_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        existing.push('\n');
        existing.push_str(text);
    } else {
        if store.comments.len() >= MAX_FILES {
            return Err(KernelError::ResourceExhausted);
        }
        if text.len() > MAX_COMMENT_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        store
            .comments
            .insert(path.to_path_buf(), String::from(text));
    }
    Ok(())
}

/// Check if a file has a comment.
pub fn has_comment(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let store = STORE.lock();
    store.comments.contains_key(path)
}

/// Get the comment length in bytes.
pub fn comment_len(path: impl AsRef<Path>) -> usize {
    let path = path.as_ref();
    let store = STORE.lock();
    store.comments.get(path).map(|c| c.len()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search for files whose comments contain the given substring
/// (case-insensitive).  Optionally restrict to paths under `root`.
pub fn search(needle: &str, root: Option<&Path>) -> Vec<(PathBuf, String)> {
    SEARCH_COUNT.fetch_add(1, Ordering::Relaxed);

    if needle.is_empty() {
        return Vec::new();
    }

    let needle_lower = to_ascii_lower(needle);
    let store = STORE.lock();
    let mut results = Vec::new();

    for (path, comment) in &store.comments {
        if let Some(root_path) = root {
            // Canonical subtree predicate; see fs::pathutil.
            if !crate::fs::pathutil::path_in_subtree(path, root_path) {
                continue;
            }
        }
        let comment_lower = to_ascii_lower(comment);
        if comment_lower.contains(needle_lower.as_str()) {
            results.push((path.clone(), comment.clone()));
            if results.len() >= MAX_SEARCH_RESULTS {
                break;
            }
        }
    }

    results
}

/// List all files with comments (optionally under a root path).
pub fn list(root: Option<&Path>) -> Vec<(PathBuf, String)> {
    let store = STORE.lock();
    store
        .comments
        .iter()
        .filter(|(path, _)| {
            root.is_none_or(|r| crate::fs::pathutil::path_in_subtree(path.as_path(), r))
        })
        .map(|(p, c)| (p.clone(), c.clone()))
        .collect()
}

/// Count files with comments.
pub fn count() -> usize {
    let store = STORE.lock();
    store.comments.len()
}

// ---------------------------------------------------------------------------
// Rename / bulk operations
// ---------------------------------------------------------------------------

/// Update comment when a file is renamed.
pub fn rename_path(old_path: impl AsRef<Path>, new_path: impl AsRef<Path>) -> KernelResult<()> {
    let mut store = STORE.lock();
    if let Some(comment) = store.comments.remove(old_path.as_ref()) {
        store.comments.insert(new_path.as_ref().to_path_buf(), comment);
        Ok(())
    } else {
        Ok(()) // No comment to move — that's fine.
    }
}

/// Remove comments for all files under a path prefix (e.g., when
/// deleting a directory).
pub fn remove_under(path_prefix: impl AsRef<Path>) -> usize {
    let path_prefix = path_prefix.as_ref();
    let mut store = STORE.lock();
    let to_remove: Vec<PathBuf> = store
        .comments
        .keys()
        .filter(|p| crate::fs::pathutil::path_in_subtree(p.as_path(), path_prefix))
        .cloned()
        .collect();
    let count = to_remove.len();
    for path in to_remove {
        store.comments.remove(&path);
    }
    count
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (comment_count, set_ops, get_ops, search_ops).
pub fn stats() -> (usize, u64, u64, u64) {
    (
        count(),
        SET_COUNT.load(Ordering::Relaxed),
        GET_COUNT.load(Ordering::Relaxed),
        SEARCH_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    GET_COUNT.store(0, Ordering::Relaxed);
    SEARCH_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all comment data.
pub fn clear_all() {
    let mut store = STORE.lock();
    store.comments.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the file comment module.
///
/// The suite asserts exact table contents, so it needs a table of its own.
/// It used to get one by calling `clear_all()`, which — since this suite is
/// reachable from the shell — deleted whatever the user had stored here and
/// then reported success.  The live state is moved aside for the duration and
/// put back afterwards; `crate::fs::selftest` records why this shape rather
/// than the alternatives.
pub fn self_test() -> KernelResult<()> {
    // These counters live outside the table, so `with_pristine` cannot
    // see them; save and restore them here so a run leaves no trace.
    let saved_set_count = SET_COUNT.load(Ordering::Relaxed);
    let saved_get_count = GET_COUNT.load(Ordering::Relaxed);
    let saved_search_count = SEARCH_COUNT.load(Ordering::Relaxed);
    let result = crate::fs::selftest::with_pristine(&STORE, CommentStore::new(), self_test_inner);
    SET_COUNT.store(saved_set_count, Ordering::Relaxed);
    GET_COUNT.store(saved_get_count, Ordering::Relaxed);
    SEARCH_COUNT.store(saved_search_count, Ordering::Relaxed);
    result
}

fn self_test_inner() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: set and get.
    {
        set("/test/file.txt", "This is a test file")?;
        let comment = get("/test/file.txt");
        assert!(comment.is_some());
        assert_eq!(comment.unwrap(), "This is a test file");
        serial_println!("[fcomment] test 1 passed: set/get");
    }

    // Test 2: has_comment and comment_len.
    {
        assert!(has_comment("/test/file.txt"));
        assert_eq!(comment_len("/test/file.txt"), 19);
        assert!(!has_comment("/nonexistent"));
        serial_println!("[fcomment] test 2 passed: has_comment/comment_len");
    }

    // Test 3: append.
    {
        append("/test/file.txt", "Additional note")?;
        let comment = get("/test/file.txt").unwrap();
        assert!(comment.contains("This is a test file"));
        assert!(comment.contains("Additional note"));
        serial_println!("[fcomment] test 3 passed: append");
    }

    // Test 4: search.
    {
        set("/test/report.pdf", "Q3 quarterly report for review")?;
        set("/docs/memo.txt", "Internal memo about Q4 planning")?;
        let results = search("quarterly", None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.as_path(), Path::new("/test/report.pdf"));

        // Case-insensitive.
        let results = search("INTERNAL", None);
        assert_eq!(results.len(), 1);
        serial_println!("[fcomment] test 4 passed: search");
    }

    // Test 5: search with root filter.
    {
        let results = search("report", Some(Path::new("/test")));
        assert_eq!(results.len(), 1);
        let results = search("report", Some(Path::new("/docs")));
        assert_eq!(results.len(), 0);
        serial_println!("[fcomment] test 5 passed: search with root filter");
    }

    // Test 6: remove and rename.
    {
        remove("/docs/memo.txt")?;
        assert!(!has_comment("/docs/memo.txt"));

        rename_path("/test/report.pdf", "/archive/report.pdf")?;
        assert!(!has_comment("/test/report.pdf"));
        assert!(has_comment("/archive/report.pdf"));
        serial_println!("[fcomment] test 6 passed: remove/rename");
    }

    // Test 7: remove_under and list.
    {
        set("/test/a.txt", "Comment A")?;
        set("/test/b.txt", "Comment B")?;
        let all = list(Some(Path::new("/test")));
        assert!(all.len() >= 2); // file.txt, a.txt, b.txt
        let removed = remove_under("/test");
        assert!(removed >= 2);
        assert!(!has_comment("/test/a.txt"));
        serial_println!("[fcomment] test 7 passed: remove_under/list");
    }

    // Test 8: a path that is not valid UTF-8 keys, reads back, and is
    // distinguished from its neighbours.
    //
    // Every test above uses an ASCII name, which a `String` key handled
    // perfectly well -- so none of them could see the defect this change
    // fixes. The two names below differ only in a byte that cannot appear in
    // UTF-8; a lossy key folds both to U+FFFD, which would make the second
    // `set` overwrite the first and `get` answer for the wrong file.
    {
        clear_all();
        let a = Path::new(&b"/test/\xFFnote.txt"[..]);
        let b = Path::new(&b"/test/\xFEnote.txt"[..]);

        set(a, "comment on A")?;
        set(b, "comment on B")?;
        assert_eq!(get(a).as_deref(), Some("comment on A"));
        assert_eq!(get(b).as_deref(), Some("comment on B"));
        assert_eq!(count(), 2);

        // Subtree filtering and search must be byte-exact as well.
        let listed = list(Some(Path::new("/test")));
        assert_eq!(listed.len(), 2);
        let found = search("comment on A", None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.as_path(), a);

        // Rename must move the comment to the new bytes, not to a mangled
        // approximation of them.
        rename_path(a, &b"/test/\xFFrenamed.txt"[..])?;
        assert!(!has_comment(a));
        assert_eq!(
            get(Path::new(&b"/test/\xFFrenamed.txt"[..])).as_deref(),
            Some("comment on A")
        );

        assert_eq!(remove_under("/test"), 2);
        serial_println!("[fcomment] test 8 passed: non-UTF-8 paths");
    }

    clear_all();
    reset_stats();

    serial_println!("[fcomment] all 8 self-tests passed");
    Ok(())
}
