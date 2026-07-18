//! File tagging system for BFS-inspired metadata queries.
//!
//! Attaches arbitrary string tags to files using extended attributes,
//! enabling tag-based file organization and search.  Tags are stored
//! in the `user.tags` xattr as comma-separated values, making them
//! visible to any tool that reads xattrs.
//!
//! ## Design Reference
//!
//! design.txt lines 35-37: "BFS (Be File System) had rich queryable
//! metadata built in — you could search files by any attribute."
//!
//! design.txt line 249: "Database-as-filesystem — rich metadata and
//! queries built into the filesystem."
//!
//! design.txt line 377: "arbitrary string/comment?" → stored as tags.
//!
//! ## Architecture
//!
//! ```text
//! tag::add("/home/doc.pdf", "work")      → xattr user.tags = "work"
//! tag::add("/home/doc.pdf", "important") → xattr user.tags = "work,important"
//! tag::search("work", "/")               → Vec of matching paths
//! tag::search_multi(&["work","2024"],"/") → intersection search
//! ```
//!
//! Tags are:
//! - Case-insensitive for matching, preserved-case for storage
//! - ASCII alphanumeric + hyphen + underscore + dot (validated)
//! - Max 64 characters each, max 32 tags per file
//! - Stored in `user.tags` xattr as comma-separated values
//!
//! ## Tag Index
//!
//! An in-memory reverse index (tag → set of paths) accelerates
//! tag-based searches.  The index is rebuilt from a filesystem scan
//! and updated incrementally on add/remove operations.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Xattr key used to store tags.
const TAG_XATTR_KEY: &str = "user.tags";

/// Maximum number of tags per file.
const MAX_TAGS_PER_FILE: usize = 32;

/// Maximum length of a single tag string.
const MAX_TAG_LEN: usize = 64;

/// Maximum number of entries in the tag index.
const MAX_INDEX_ENTRIES: usize = 100_000;

/// Maximum recursion depth for filesystem walks.
const MAX_SCAN_DEPTH: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A tagged file entry.
#[derive(Debug, Clone)]
pub struct TaggedFile {
    /// Full path to the file.
    pub path: String,
    /// Tags attached to this file.
    pub tags: Vec<String>,
}

/// Statistics about the tag system.
#[derive(Debug, Clone, Copy, Default)]
pub struct TagStats {
    /// Total unique tags in the index.
    pub unique_tags: u64,
    /// Total tagged files in the index.
    pub tagged_files: u64,
    /// Total tag→file associations.
    pub total_associations: u64,
    /// Number of tag add operations.
    pub adds: u64,
    /// Number of tag remove operations.
    pub removes: u64,
    /// Number of tag searches.
    pub searches: u64,
    /// Whether the index has been built.
    pub index_built: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Master enable flag.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Operation counters.
static ADDS: AtomicU64 = AtomicU64::new(0);
static REMOVES: AtomicU64 = AtomicU64::new(0);
static SEARCHES: AtomicU64 = AtomicU64::new(0);

/// Reverse index: tag → set of file paths.
struct TagIndex {
    /// Tag → set of paths.
    by_tag: BTreeMap<String, BTreeSet<String>>,
    /// Path → set of tags (for fast removal).
    by_path: BTreeMap<String, BTreeSet<String>>,
    /// Whether the index has been built.
    built: bool,
}

static INDEX: Mutex<TagIndex> = Mutex::new(TagIndex {
    by_tag: BTreeMap::new(),
    by_path: BTreeMap::new(),
    built: false,
});

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Enable or disable the tag system.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if the tag system is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Get tag system statistics.
pub fn stats() -> TagStats {
    let idx = INDEX.lock();
    TagStats {
        unique_tags: idx.by_tag.len() as u64,
        tagged_files: idx.by_path.len() as u64,
        total_associations: idx.by_tag.values().map(|s| s.len() as u64).sum(),
        adds: ADDS.load(Ordering::Relaxed),
        removes: REMOVES.load(Ordering::Relaxed),
        searches: SEARCHES.load(Ordering::Relaxed),
        index_built: idx.built,
    }
}

// ---------------------------------------------------------------------------
// Tag validation
// ---------------------------------------------------------------------------

/// Validate a tag string.
///
/// Tags must be 1-64 characters, ASCII alphanumeric plus hyphen,
/// underscore, and dot.  No commas (used as separator in xattr).
fn validate_tag(tag: &str) -> KernelResult<()> {
    if tag.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if tag.len() > MAX_TAG_LEN {
        return Err(KernelError::InvalidArgument);
    }
    for c in tag.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.' {
            return Err(KernelError::InvalidArgument);
        }
    }
    Ok(())
}

/// Normalize a tag for comparison (lowercase).
fn normalize_tag(tag: &str) -> String {
    tag.chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                (c as u8 + 32) as char
            } else {
                c
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Xattr ←→ tag list conversion
// ---------------------------------------------------------------------------

/// Read tags from a file's xattr.
fn read_tags(path: &str) -> KernelResult<Vec<String>> {
    match Vfs::get_xattr(path, TAG_XATTR_KEY) {
        Ok(data) => {
            let s = core::str::from_utf8(&data).map_err(|_| KernelError::CorruptedData)?;
            if s.is_empty() {
                return Ok(Vec::new());
            }
            let tags: Vec<String> = s.split(',').map(|t| String::from(t.trim())).collect();
            Ok(tags)
        }
        Err(KernelError::NotFound) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Write tags to a file's xattr.
fn write_tags(path: &str, tags: &[String]) -> KernelResult<()> {
    if tags.is_empty() {
        // Remove the xattr entirely when no tags remain.
        match Vfs::remove_xattr(path, TAG_XATTR_KEY) {
            Ok(()) => Ok(()),
            Err(KernelError::NotFound) => Ok(()), // Already gone.
            Err(e) => Err(e),
        }
    } else {
        let joined = tags.join(",");
        Vfs::set_xattr(path, TAG_XATTR_KEY, joined.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// Core tag operations
// ---------------------------------------------------------------------------

/// Add a tag to a file.
///
/// If the file already has this tag (case-insensitive), this is a no-op.
/// Tags are stored in the file's extended attributes.
pub fn add(path: &str, tag: &str) -> KernelResult<()> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    validate_tag(tag)?;
    let norm = normalize_tag(tag);

    let mut tags = read_tags(path)?;

    // Check if already present (case-insensitive).
    if tags.iter().any(|t| normalize_tag(t) == norm) {
        return Ok(()); // Already tagged.
    }

    if tags.len() >= MAX_TAGS_PER_FILE {
        return Err(KernelError::DiskFull); // Reusing error for "too many tags".
    }

    tags.push(String::from(tag));
    write_tags(path, &tags)?;

    // Update index.
    {
        let mut idx = INDEX.lock();
        idx.by_tag
            .entry(norm.clone())
            .or_default()
            .insert(String::from(path));
        idx.by_path
            .entry(String::from(path))
            .or_default()
            .insert(norm);
    }

    ADDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Remove a tag from a file.
///
/// Case-insensitive removal.  Returns `NotFound` if the tag wasn't present.
pub fn remove(path: &str, tag: &str) -> KernelResult<()> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    let norm = normalize_tag(tag);
    let mut tags = read_tags(path)?;

    let before = tags.len();
    tags.retain(|t| normalize_tag(t) != norm);

    if tags.len() == before {
        return Err(KernelError::NotFound);
    }

    write_tags(path, &tags)?;

    // Update index.
    {
        let mut idx = INDEX.lock();
        if let Some(set) = idx.by_tag.get_mut(&norm) {
            set.remove(path);
            if set.is_empty() {
                idx.by_tag.remove(&norm);
            }
        }
        if let Some(set) = idx.by_path.get_mut(path) {
            set.remove(&norm);
            if set.is_empty() {
                idx.by_path.remove(path);
            }
        }
    }

    REMOVES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get all tags on a file.
pub fn get(path: &str) -> KernelResult<Vec<String>> {
    read_tags(path)
}

/// Set all tags on a file (replacing any existing tags).
pub fn set(path: &str, tags: &[&str]) -> KernelResult<()> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    // Validate all tags first.
    for tag in tags {
        validate_tag(tag)?;
    }
    if tags.len() > MAX_TAGS_PER_FILE {
        return Err(KernelError::DiskFull);
    }

    // Clear existing index entries for this path.
    {
        let mut idx = INDEX.lock();
        if let Some(old_tags) = idx.by_path.remove(path) {
            for t in &old_tags {
                if let Some(set) = idx.by_tag.get_mut(t) {
                    set.remove(path);
                    if set.is_empty() {
                        idx.by_tag.remove(t);
                    }
                }
            }
        }
    }

    let tag_strings: Vec<String> = tags.iter().map(|t| String::from(*t)).collect();
    write_tags(path, &tag_strings)?;

    // Re-index.
    {
        let mut idx = INDEX.lock();
        let mut path_tags = BTreeSet::new();
        for tag in tags {
            let norm = normalize_tag(tag);
            idx.by_tag
                .entry(norm.clone())
                .or_default()
                .insert(String::from(path));
            path_tags.insert(norm);
        }
        if !path_tags.is_empty() {
            idx.by_path.insert(String::from(path), path_tags);
        }
    }

    ADDS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Remove all tags from a file.
pub fn clear(path: &str) -> KernelResult<()> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    write_tags(path, &[])?;

    // Remove from index.
    {
        let mut idx = INDEX.lock();
        if let Some(old_tags) = idx.by_path.remove(path) {
            for t in &old_tags {
                if let Some(set) = idx.by_tag.get_mut(t) {
                    set.remove(path);
                    if set.is_empty() {
                        idx.by_tag.remove(t);
                    }
                }
            }
        }
    }

    REMOVES.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tag search
// ---------------------------------------------------------------------------

/// Find all files with a specific tag.
///
/// If the index is built, uses the in-memory index for O(1) lookup.
/// Otherwise falls back to a filesystem walk scanning xattrs.
pub fn search(tag: &str, root: &str) -> KernelResult<Vec<TaggedFile>> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    let norm = normalize_tag(tag);
    SEARCHES.fetch_add(1, Ordering::Relaxed);

    let idx = INDEX.lock();
    if idx.built {
        // Fast path: use index.
        let paths = idx.by_tag.get(&norm);
        match paths {
            Some(set) => {
                let mut results = Vec::new();
                for path in set {
                    // Filter by root prefix.
                    if path.starts_with(root) || root == "/" {
                        // Re-read tags from xattr for accuracy.
                        let tags = read_tags(path).unwrap_or_default();
                        results.push(TaggedFile {
                            path: path.clone(),
                            tags,
                        });
                    }
                }
                Ok(results)
            }
            None => Ok(Vec::new()),
        }
    } else {
        // Slow path: walk the filesystem.
        drop(idx); // Release lock before I/O.
        let mut results = Vec::new();
        search_walk(root, &norm, &mut results, 0);
        Ok(results)
    }
}

/// Find all files that have ALL of the specified tags (intersection).
pub fn search_multi(tags: &[&str], root: &str) -> KernelResult<Vec<TaggedFile>> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }
    if tags.is_empty() {
        return Ok(Vec::new());
    }

    SEARCHES.fetch_add(1, Ordering::Relaxed);

    let norms: Vec<String> = tags.iter().map(|t| normalize_tag(t)).collect();

    let idx = INDEX.lock();
    if idx.built {
        // Start with the smallest tag set for efficiency.
        let mut sets: Vec<&BTreeSet<String>> = Vec::new();
        for norm in &norms {
            match idx.by_tag.get(norm) {
                Some(set) => sets.push(set),
                None => return Ok(Vec::new()), // If any tag has no files, result is empty.
            }
        }

        // Sort by size (smallest first).
        sets.sort_by_key(|s| s.len());

        // Intersect iteratively.
        let mut candidates: BTreeSet<String> = sets[0].clone();
        for set in &sets[1..] {
            candidates = candidates.intersection(set).cloned().collect();
            if candidates.is_empty() {
                return Ok(Vec::new());
            }
        }

        let mut results = Vec::new();
        for path in &candidates {
            if path.starts_with(root) || root == "/" {
                let file_tags = read_tags(path).unwrap_or_default();
                results.push(TaggedFile {
                    path: path.clone(),
                    tags: file_tags,
                });
            }
        }
        Ok(results)
    } else {
        // Slow path: walk the filesystem.
        drop(idx);
        let mut results = Vec::new();
        search_walk_multi(root, &norms, &mut results, 0);
        Ok(results)
    }
}

/// Walk the filesystem looking for files with a specific tag.
fn search_walk(
    path: &str,
    norm_tag: &str,
    results: &mut Vec<TaggedFile>,
    depth: usize,
) {
    if depth > MAX_SCAN_DEPTH || results.len() >= 1000 {
        return;
    }

    // Skip virtual filesystems.
    if path.starts_with("/proc") || path.starts_with("/dev") || path.starts_with("/sys") {
        return;
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if results.len() >= 1000 {
            return;
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Check tags on this entry.
        if let Ok(tags) = read_tags(&full) {
            if tags.iter().any(|t| normalize_tag(t) == norm_tag) {
                results.push(TaggedFile {
                    path: full.clone(),
                    tags,
                });
            }
        }

        // Recurse into directories.
        if entry.entry_type == EntryType::Directory {
            search_walk(&full, norm_tag, results, depth + 1);
        }
    }
}

/// Walk the filesystem looking for files with ALL specified tags.
fn search_walk_multi(
    path: &str,
    norm_tags: &[String],
    results: &mut Vec<TaggedFile>,
    depth: usize,
) {
    if depth > MAX_SCAN_DEPTH || results.len() >= 1000 {
        return;
    }

    if path.starts_with("/proc") || path.starts_with("/dev") || path.starts_with("/sys") {
        return;
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if results.len() >= 1000 {
            return;
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Check tags on this entry.
        if let Ok(tags) = read_tags(&full) {
            let norms: Vec<String> = tags.iter().map(|t| normalize_tag(t)).collect();
            if norm_tags.iter().all(|nt| norms.contains(nt)) {
                results.push(TaggedFile {
                    path: full.clone(),
                    tags,
                });
            }
        }

        if entry.entry_type == EntryType::Directory {
            search_walk_multi(&full, norm_tags, results, depth + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Index management
// ---------------------------------------------------------------------------

/// Build (or rebuild) the tag index by scanning the filesystem.
///
/// Walks the specified root path and indexes all files that have
/// the `user.tags` xattr.
pub fn build_index(root: &str) -> KernelResult<u64> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::NotSupported);
    }

    let mut new_by_tag: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut new_by_path: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut count: u64 = 0;

    index_walk(root, &mut new_by_tag, &mut new_by_path, &mut count, 0);

    let mut idx = INDEX.lock();
    idx.by_tag = new_by_tag;
    idx.by_path = new_by_path;
    idx.built = true;

    serial_println!("[tags] Index built: {} tagged files", count);
    Ok(count)
}

/// Walk the filesystem to build the tag index.
fn index_walk(
    path: &str,
    by_tag: &mut BTreeMap<String, BTreeSet<String>>,
    by_path: &mut BTreeMap<String, BTreeSet<String>>,
    count: &mut u64,
    depth: usize,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    // Total entries cap.
    let total: usize = by_path.len();
    if total >= MAX_INDEX_ENTRIES {
        return;
    }

    // Skip virtual filesystems.
    if path.starts_with("/proc") || path.starts_with("/dev") || path.starts_with("/sys")
        || path.starts_with("/_")
    {
        return;
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if by_path.len() >= MAX_INDEX_ENTRIES {
            return;
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Check for tags.
        if let Ok(tags) = read_tags(&full) {
            if !tags.is_empty() {
                let mut path_tags = BTreeSet::new();
                for tag in &tags {
                    let norm = normalize_tag(tag);
                    by_tag
                        .entry(norm.clone())
                        .or_default()
                        .insert(full.clone());
                    path_tags.insert(norm);
                }
                by_path.insert(full.clone(), path_tags);
                *count = count.saturating_add(1);
            }
        }

        // Recurse into directories.
        if entry.entry_type == EntryType::Directory {
            index_walk(&full, by_tag, by_path, count, depth + 1);
        }
    }
}

/// Clear the tag index.
pub fn clear_index() {
    let mut idx = INDEX.lock();
    idx.by_tag.clear();
    idx.by_path.clear();
    idx.built = false;
}

/// List all known tags (from the index).
pub fn list_tags() -> Vec<(String, usize)> {
    let idx = INDEX.lock();
    idx.by_tag
        .iter()
        .map(|(tag, paths)| (tag.clone(), paths.len()))
        .collect()
}

/// List all files with any tag in the index.
pub fn list_tagged_files() -> Vec<TaggedFile> {
    let idx = INDEX.lock();
    idx.by_path
        .iter()
        .map(|(path, tags)| TaggedFile {
            path: path.clone(),
            tags: tags.iter().cloned().collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[tags] Running self-test...");

    test_validate_tag();
    test_add_remove();
    test_get_set();
    test_clear();
    test_search();
    test_multi_search();
    test_index();
    test_stats();

    serial_println!("[tags] Self-test passed (8 tests).");
    Ok(())
}

fn test_validate_tag() {
    assert!(validate_tag("work").is_ok());
    assert!(validate_tag("my-project").is_ok());
    assert!(validate_tag("v2.0").is_ok());
    assert!(validate_tag("under_score").is_ok());
    assert!(validate_tag("").is_err());
    assert!(validate_tag("has space").is_err());
    assert!(validate_tag("has,comma").is_err());
    assert!(validate_tag("has/slash").is_err());

    // Max length.
    let long = "a".repeat(64);
    assert!(validate_tag(&long).is_ok());
    let too_long = "a".repeat(65);
    assert!(validate_tag(&too_long).is_err());

    serial_println!("[tags]   validate tag: ok");
}

fn test_add_remove() {
    let _ = Vfs::mkdir("/tmp/tag_test");
    Vfs::write_file("/tmp/tag_test/doc.txt", b"hello").expect("write");

    // Add tags.
    add("/tmp/tag_test/doc.txt", "work").expect("add work");
    add("/tmp/tag_test/doc.txt", "important").expect("add important");

    // Verify tags are stored.
    let tags = get("/tmp/tag_test/doc.txt").expect("get");
    assert_eq!(tags.len(), 2);
    assert!(tags.contains(&String::from("work")));
    assert!(tags.contains(&String::from("important")));

    // Duplicate add (case-insensitive) is no-op.
    add("/tmp/tag_test/doc.txt", "Work").expect("dup add");
    let tags = get("/tmp/tag_test/doc.txt").expect("get after dup");
    assert_eq!(tags.len(), 2);

    // Remove a tag.
    remove("/tmp/tag_test/doc.txt", "work").expect("remove");
    let tags = get("/tmp/tag_test/doc.txt").expect("get after remove");
    assert_eq!(tags.len(), 1);
    assert!(tags.contains(&String::from("important")));

    // Remove non-existent tag.
    assert!(remove("/tmp/tag_test/doc.txt", "nonexistent").is_err());

    let _ = Vfs::remove("/tmp/tag_test/doc.txt");
    let _ = Vfs::rmdir("/tmp/tag_test");

    serial_println!("[tags]   add/remove: ok");
}

fn test_get_set() {
    let _ = Vfs::mkdir("/tmp/tag_gs");
    Vfs::write_file("/tmp/tag_gs/f.txt", b"data").expect("write");

    // Set multiple tags at once.
    set("/tmp/tag_gs/f.txt", &["alpha", "beta", "gamma"]).expect("set");
    let tags = get("/tmp/tag_gs/f.txt").expect("get");
    assert_eq!(tags.len(), 3);

    // Overwrite with new set.
    set("/tmp/tag_gs/f.txt", &["one"]).expect("set2");
    let tags = get("/tmp/tag_gs/f.txt").expect("get2");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0], "one");

    let _ = Vfs::remove("/tmp/tag_gs/f.txt");
    let _ = Vfs::rmdir("/tmp/tag_gs");

    serial_println!("[tags]   get/set: ok");
}

fn test_clear() {
    let _ = Vfs::mkdir("/tmp/tag_clr");
    Vfs::write_file("/tmp/tag_clr/f.txt", b"data").expect("write");

    add("/tmp/tag_clr/f.txt", "x").expect("add");
    add("/tmp/tag_clr/f.txt", "y").expect("add");
    assert_eq!(get("/tmp/tag_clr/f.txt").expect("get").len(), 2);

    clear("/tmp/tag_clr/f.txt").expect("clear");
    assert!(get("/tmp/tag_clr/f.txt").expect("get").is_empty());

    let _ = Vfs::remove("/tmp/tag_clr/f.txt");
    let _ = Vfs::rmdir("/tmp/tag_clr");

    serial_println!("[tags]   clear: ok");
}

fn test_search() {
    let _ = Vfs::mkdir("/tmp/tag_search");
    Vfs::write_file("/tmp/tag_search/a.txt", b"aa").expect("write");
    Vfs::write_file("/tmp/tag_search/b.txt", b"bb").expect("write");
    Vfs::write_file("/tmp/tag_search/c.txt", b"cc").expect("write");

    add("/tmp/tag_search/a.txt", "red").expect("add");
    add("/tmp/tag_search/b.txt", "red").expect("add");
    add("/tmp/tag_search/b.txt", "blue").expect("add");
    add("/tmp/tag_search/c.txt", "blue").expect("add");

    // Search for "red" — should find a and b.
    let results = search("red", "/tmp/tag_search").expect("search");
    assert!(results.len() >= 2, "should find 2 red files, got {}", results.len());

    // Search for "blue" — should find b and c.
    let results = search("blue", "/tmp/tag_search").expect("search");
    assert!(results.len() >= 2, "should find 2 blue files, got {}", results.len());

    // Search for "green" — should find nothing.
    let results = search("green", "/tmp/tag_search").expect("search");
    assert!(results.is_empty(), "no green files");

    // Cleanup.
    let _ = Vfs::remove("/tmp/tag_search/a.txt");
    let _ = Vfs::remove("/tmp/tag_search/b.txt");
    let _ = Vfs::remove("/tmp/tag_search/c.txt");
    let _ = Vfs::rmdir("/tmp/tag_search");

    serial_println!("[tags]   search: ok");
}

fn test_multi_search() {
    let _ = Vfs::mkdir("/tmp/tag_multi");
    Vfs::write_file("/tmp/tag_multi/x.txt", b"xx").expect("write");
    Vfs::write_file("/tmp/tag_multi/y.txt", b"yy").expect("write");

    add("/tmp/tag_multi/x.txt", "hot").expect("add");
    add("/tmp/tag_multi/x.txt", "urgent").expect("add");
    add("/tmp/tag_multi/y.txt", "hot").expect("add");

    // Search for files with BOTH "hot" AND "urgent".
    let results = search_multi(&["hot", "urgent"], "/tmp/tag_multi").expect("multi");
    assert_eq!(results.len(), 1, "only x.txt has both tags");
    assert!(results[0].path.contains("x.txt"));

    // Single-tag multi-search.
    let results = search_multi(&["hot"], "/tmp/tag_multi").expect("multi");
    assert!(results.len() >= 2, "both files are hot");

    // Cleanup.
    let _ = Vfs::remove("/tmp/tag_multi/x.txt");
    let _ = Vfs::remove("/tmp/tag_multi/y.txt");
    let _ = Vfs::rmdir("/tmp/tag_multi");

    serial_println!("[tags]   multi search: ok");
}

fn test_index() {
    let _ = Vfs::mkdir("/tmp/tag_idx");
    Vfs::write_file("/tmp/tag_idx/a.txt", b"aa").expect("write");
    Vfs::write_file("/tmp/tag_idx/b.txt", b"bb").expect("write");

    add("/tmp/tag_idx/a.txt", "indexed").expect("add");
    add("/tmp/tag_idx/b.txt", "indexed").expect("add");
    add("/tmp/tag_idx/b.txt", "special").expect("add");

    // Build index.
    let count = build_index("/tmp/tag_idx").expect("build_index");
    assert!(count >= 2, "should index at least 2 files");

    // Now search should use the fast index path.
    let results = search("indexed", "/tmp/tag_idx").expect("search");
    assert!(results.len() >= 2, "index search should find 2");

    // List tags.
    let all_tags = list_tags();
    assert!(!all_tags.is_empty(), "should have tags");

    // Clear index.
    clear_index();
    let s = stats();
    assert!(!s.index_built, "should be cleared");

    // Cleanup.
    let _ = Vfs::remove("/tmp/tag_idx/a.txt");
    let _ = Vfs::remove("/tmp/tag_idx/b.txt");
    let _ = Vfs::rmdir("/tmp/tag_idx");

    serial_println!("[tags]   index: ok");
}

fn test_stats() {
    let s = stats();
    assert!(s.adds > 0, "should have add operations");
    assert!(s.searches > 0, "should have search operations");

    serial_println!("[tags]   stats: ok");
}
