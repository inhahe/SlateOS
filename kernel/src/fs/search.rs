//! Advanced file search engine with compound metadata queries.
//!
//! Provides BFS-inspired "search files by any attribute" capability,
//! going beyond the basic `fs::index` locate-style name search.
//! Supports compound queries combining multiple criteria:
//!
//! - Name patterns (glob, substring, regex-lite)
//! - Size ranges (min/max)
//! - Date ranges (modified/created/accessed before/after)
//! - File type (file, directory, symlink)
//! - Permission patterns
//! - Owner/group
//! - Extension
//! - Content hash match
//! - Depth limits
//!
//! ## Design Reference
//!
//! design.txt lines 35-37: BFS (Be File System) with "rich queryable
//! metadata built in — you could search files by any attribute"
//!
//! ## Architecture
//!
//! ```text
//! Query::new()
//!     .name_contains("report")
//!     .extension("pdf")
//!     .size_min(1024)
//!     .modified_after(timestamp)
//!     .execute("/home")
//!     → Vec<SearchResult>
//! ```
//!
//! Queries walk the VFS directory tree, applying filters at each node.
//! Directory-level filters (type, name) are applied before descending,
//! file-level filters (size, date, content) require a stat() call.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A search result entry.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Full path to the matched file/directory.
    pub path: String,
    /// Entry type (file, directory, symlink).
    pub entry_type: EntryType,
    /// File size in bytes.
    pub size: u64,
    /// Last modified timestamp (epoch nanoseconds).
    pub modified_ns: u64,
    /// Created timestamp (epoch nanoseconds).
    pub created_ns: u64,
    /// File permissions.
    pub permissions: u16,
    /// Owner UID.
    pub uid: u32,
}

/// Query builder for constructing compound search queries.
#[derive(Debug, Clone)]
pub struct Query {
    /// Name must contain this substring (case-insensitive).
    name_contains: Option<String>,
    /// Name must match this glob pattern.
    name_glob: Option<String>,
    /// Name must start with this prefix.
    name_prefix: Option<String>,
    /// Name must end with this suffix.
    name_suffix: Option<String>,
    /// File extension (without dot).
    extension: Option<String>,
    /// Minimum file size.
    size_min: Option<u64>,
    /// Maximum file size.
    size_max: Option<u64>,
    /// Modified after this timestamp (epoch ns).
    modified_after: Option<u64>,
    /// Modified before this timestamp (epoch ns).
    modified_before: Option<u64>,
    /// Created after this timestamp (epoch ns).
    created_after: Option<u64>,
    /// Created before this timestamp (epoch ns).
    created_before: Option<u64>,
    /// Only match this entry type.
    entry_type: Option<EntryType>,
    /// Owner UID.
    uid: Option<u32>,
    /// Group GID.
    gid: Option<u32>,
    /// Permissions mask (exact match).
    permissions: Option<u16>,
    /// Maximum recursion depth.
    max_depth: usize,
    /// Maximum results to return.
    max_results: usize,
    /// Paths to exclude.
    exclude_prefixes: Vec<String>,
    /// Content hash must match (SHA-256 hex string).
    content_hash: Option<String>,
    /// Minimum permissions bits that must be set (AND check).
    permissions_mask: Option<u16>,
}

impl Query {
    /// Create a new empty query.
    pub fn new() -> Self {
        Self {
            name_contains: None,
            name_glob: None,
            name_prefix: None,
            name_suffix: None,
            extension: None,
            size_min: None,
            size_max: None,
            modified_after: None,
            modified_before: None,
            created_after: None,
            created_before: None,
            entry_type: None,
            uid: None,
            gid: None,
            permissions: None,
            max_depth: 32,
            max_results: 10_000,
            exclude_prefixes: alloc::vec![
                String::from("/proc"),
                String::from("/dev"),
                String::from("/sys"),
            ],
            content_hash: None,
            permissions_mask: None,
        }
    }

    /// Name must contain this substring (case-insensitive).
    pub fn name_contains(mut self, s: &str) -> Self {
        self.name_contains = Some(String::from(s));
        self
    }

    /// Name must match this glob pattern.
    pub fn name_glob(mut self, pattern: &str) -> Self {
        self.name_glob = Some(String::from(pattern));
        self
    }

    /// Name must start with this prefix.
    pub fn name_prefix(mut self, pfx: &str) -> Self {
        self.name_prefix = Some(String::from(pfx));
        self
    }

    /// Name must end with this suffix.
    pub fn name_suffix(mut self, sfx: &str) -> Self {
        self.name_suffix = Some(String::from(sfx));
        self
    }

    /// File extension (without dot, case-insensitive).
    pub fn extension(mut self, ext: &str) -> Self {
        self.extension = Some(String::from(ext));
        self
    }

    /// Minimum file size in bytes.
    pub fn size_min(mut self, min: u64) -> Self {
        self.size_min = Some(min);
        self
    }

    /// Maximum file size in bytes.
    pub fn size_max(mut self, max: u64) -> Self {
        self.size_max = Some(max);
        self
    }

    /// Modified after this timestamp (epoch nanoseconds).
    pub fn modified_after(mut self, ts: u64) -> Self {
        self.modified_after = Some(ts);
        self
    }

    /// Modified before this timestamp (epoch nanoseconds).
    pub fn modified_before(mut self, ts: u64) -> Self {
        self.modified_before = Some(ts);
        self
    }

    /// Created after this timestamp (epoch nanoseconds).
    pub fn created_after(mut self, ts: u64) -> Self {
        self.created_after = Some(ts);
        self
    }

    /// Created before this timestamp (epoch nanoseconds).
    pub fn created_before(mut self, ts: u64) -> Self {
        self.created_before = Some(ts);
        self
    }

    /// Only match files (not directories or symlinks).
    pub fn files_only(mut self) -> Self {
        self.entry_type = Some(EntryType::File);
        self
    }

    /// Only match directories.
    pub fn dirs_only(mut self) -> Self {
        self.entry_type = Some(EntryType::Directory);
        self
    }

    /// Match a specific entry type.
    pub fn of_type(mut self, t: EntryType) -> Self {
        self.entry_type = Some(t);
        self
    }

    /// Match by owner UID.
    pub fn owned_by(mut self, uid: u32) -> Self {
        self.uid = Some(uid);
        self
    }

    /// Match by group GID.
    pub fn group(mut self, gid: u32) -> Self {
        self.gid = Some(gid);
        self
    }

    /// Exact permissions match.
    pub fn with_permissions(mut self, perms: u16) -> Self {
        self.permissions = Some(perms);
        self
    }

    /// Permissions mask — at least these bits must be set.
    pub fn has_permissions(mut self, mask: u16) -> Self {
        self.permissions_mask = Some(mask);
        self
    }

    /// Maximum recursion depth.
    pub fn depth(mut self, d: usize) -> Self {
        self.max_depth = d;
        self
    }

    /// Maximum number of results.
    pub fn limit(mut self, n: usize) -> Self {
        self.max_results = n;
        self
    }

    /// Add a path prefix to exclude.
    pub fn exclude(mut self, prefix: &str) -> Self {
        self.exclude_prefixes.push(String::from(prefix));
        self
    }

    /// Match by content hash (SHA-256 hex string).
    pub fn with_hash(mut self, hash: &str) -> Self {
        self.content_hash = Some(String::from(hash));
        self
    }

    /// Execute the query starting from the given root path.
    pub fn execute(&self, root: &str) -> KernelResult<Vec<SearchResult>> {
        let mut results = Vec::new();
        search_recursive(root, self, &mut results, 0);

        // Update stats.
        TOTAL_SEARCHES.fetch_add(1, Ordering::Relaxed);
        TOTAL_RESULTS.fetch_add(results.len() as u64, Ordering::Relaxed);

        Ok(results)
    }
}

impl Default for Query {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static TOTAL_SEARCHES: AtomicU64 = AtomicU64::new(0);
static TOTAL_RESULTS: AtomicU64 = AtomicU64::new(0);

/// Get search stats: (total_searches, total_results).
pub fn stats() -> (u64, u64) {
    (
        TOTAL_SEARCHES.load(Ordering::Relaxed),
        TOTAL_RESULTS.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Search implementation
// ---------------------------------------------------------------------------

/// Recursively search a directory tree.
fn search_recursive(
    path: &str,
    query: &Query,
    results: &mut Vec<SearchResult>,
    depth: usize,
) {
    if depth > query.max_depth {
        return;
    }
    if results.len() >= query.max_results {
        return;
    }

    // Check exclude prefixes (canonical subtree predicate tolerates a
    // trailing slash on the exclude entry). See fs::pathutil.
    for excl in &query.exclude_prefixes {
        if crate::fs::pathutil::path_in_subtree(path, excl.as_str()) {
            return;
        }
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if results.len() >= query.max_results {
            return;
        }

        if entry.name == "." || entry.name == ".." {
            continue;
        }

        let full_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Quick name/type filters before stat.
        if !matches_name_filters(&entry.name, query) {
            // Even if name doesn't match, recurse into dirs.
            if entry.entry_type == EntryType::Directory {
                search_recursive(&full_path, query, results, depth + 1);
            }
            continue;
        }

        // Type filter.
        if let Some(ref t) = query.entry_type {
            if &entry.entry_type != t {
                // Still recurse into dirs.
                if entry.entry_type == EntryType::Directory {
                    search_recursive(&full_path, query, results, depth + 1);
                }
                continue;
            }
        }

        // Need metadata for remaining filters.
        let meta = match Vfs::metadata(&full_path) {
            Ok(m) => m,
            Err(_) => {
                if entry.entry_type == EntryType::Directory {
                    search_recursive(&full_path, query, results, depth + 1);
                }
                continue;
            }
        };

        // Apply metadata filters.
        let passes = matches_metadata_filters(&meta, query, &full_path);

        if passes {
            results.push(SearchResult {
                path: full_path.clone(),
                entry_type: entry.entry_type,
                size: meta.size,
                modified_ns: meta.modified_ns,
                created_ns: meta.created_ns,
                permissions: meta.permissions,
                uid: meta.uid,
            });
        }

        // Recurse into directories.
        if entry.entry_type == EntryType::Directory {
            search_recursive(&full_path, query, results, depth + 1);
        }
    }
}

/// Check name-based filters (fast, no I/O needed).
fn matches_name_filters(name: &str, query: &Query) -> bool {
    // Name contains (case-insensitive).
    if let Some(ref substr) = query.name_contains {
        let name_lower = to_lower(name);
        let substr_lower = to_lower(substr);
        if !name_lower.contains(&substr_lower) {
            return false;
        }
    }

    // Name prefix.
    if let Some(ref pfx) = query.name_prefix {
        if !name.starts_with(pfx.as_str()) {
            return false;
        }
    }

    // Name suffix.
    if let Some(ref sfx) = query.name_suffix {
        if !name.ends_with(sfx.as_str()) {
            return false;
        }
    }

    // Extension.
    if let Some(ref ext) = query.extension {
        let file_ext = file_extension(name);
        let ext_lower = to_lower(ext);
        let file_ext_lower = to_lower(file_ext);
        if file_ext_lower != ext_lower {
            return false;
        }
    }

    // Glob pattern.
    if let Some(ref pattern) = query.name_glob {
        if !glob_match(pattern, name) {
            return false;
        }
    }

    true
}

/// Check metadata-based filters (requires stat data).
fn matches_metadata_filters(
    meta: &crate::fs::FileMeta,
    query: &Query,
    path: &str,
) -> bool {
    // Size filters.
    if let Some(min) = query.size_min {
        if meta.size < min {
            return false;
        }
    }
    if let Some(max) = query.size_max {
        if meta.size > max {
            return false;
        }
    }

    // Modified time filters.
    if let Some(after) = query.modified_after {
        if meta.modified_ns < after {
            return false;
        }
    }
    if let Some(before) = query.modified_before {
        if meta.modified_ns > before {
            return false;
        }
    }

    // Created time filters.
    if let Some(after) = query.created_after {
        if meta.created_ns < after {
            return false;
        }
    }
    if let Some(before) = query.created_before {
        if meta.created_ns > before {
            return false;
        }
    }

    // Owner.
    if let Some(uid) = query.uid {
        if meta.uid != uid {
            return false;
        }
    }

    // Group.
    if let Some(gid) = query.gid {
        if meta.gid != gid {
            return false;
        }
    }

    // Permissions (exact).
    if let Some(perms) = query.permissions {
        if meta.permissions != perms {
            return false;
        }
    }

    // Permissions mask (at least these bits).
    if let Some(mask) = query.permissions_mask {
        if meta.permissions & mask != mask {
            return false;
        }
    }

    // Content hash (expensive — only for files, last filter).
    if let Some(ref expected_hash) = query.content_hash {
        if meta.entry_type == EntryType::File {
            match Vfs::read_file(path) {
                Ok(data) => {
                    let hash = crate::crypto::sha256(&data);
                    let hex = hex_encode(&hash);
                    if hex != *expected_hash {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        } else {
            return false; // Hash filter only applies to files.
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract file extension (without dot).
fn file_extension(name: &str) -> &str {
    if let Some(pos) = name.rfind('.') {
        &name[pos + 1..]
    } else {
        ""
    }
}

/// Simple lowercase conversion (ASCII only).
fn to_lower(s: &str) -> String {
    s.chars().map(|c| {
        if c.is_ascii_uppercase() {
            (c as u8 + 32) as char
        } else {
            c
        }
    }).collect()
}

/// Simple glob matching (*, ?).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_impl(&pat, &txt, 0, 0)
}

fn glob_match_impl(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }

    match pat[pi] {
        '*' => {
            // Try matching 0 or more characters.
            for skip in 0..=txt.len().saturating_sub(ti) {
                if glob_match_impl(pat, txt, pi + 1, ti + skip) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < txt.len() {
                glob_match_impl(pat, txt, pi + 1, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < txt.len() && (txt[ti] == c || txt[ti].eq_ignore_ascii_case(&c)) {
                glob_match_impl(pat, txt, pi + 1, ti + 1)
            } else {
                false
            }
        }
    }
}

/// Encode bytes as hex string.
fn hex_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        use core::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[search] Running self-test...");

    test_empty_query();
    test_name_contains();
    test_extension_filter();
    test_size_filter();
    test_type_filter();
    test_glob_match();
    test_compound_query();
    test_stats();

    serial_println!("[search] Self-test passed (8 tests).");
    Ok(())
}

fn test_empty_query() {
    // Query with no filters on a small directory.
    let _ = Vfs::mkdir("/tmp/search_test");
    Vfs::write_file("/tmp/search_test/a.txt", b"hello").expect("write");

    let results = Query::new()
        .depth(1)
        .limit(100)
        .execute("/tmp/search_test")
        .expect("execute");

    assert!(!results.is_empty(), "should find at least one file");

    let _ = Vfs::remove("/tmp/search_test/a.txt");
    let _ = Vfs::rmdir("/tmp/search_test");

    serial_println!("[search]   empty query: ok");
}

fn test_name_contains() {
    let _ = Vfs::mkdir("/tmp/search_nc");
    Vfs::write_file("/tmp/search_nc/report.txt", b"data").expect("write");
    Vfs::write_file("/tmp/search_nc/notes.txt", b"data").expect("write");
    Vfs::write_file("/tmp/search_nc/report2.log", b"data").expect("write");

    let results = Query::new()
        .name_contains("report")
        .execute("/tmp/search_nc")
        .expect("execute");

    assert!(results.len() >= 2, "should find 2 files containing 'report'");
    for r in &results {
        let name = r.path.rsplit('/').next().unwrap_or("");
        assert!(
            to_lower(name).contains("report"),
            "name should contain 'report': {}",
            name
        );
    }

    let _ = Vfs::remove("/tmp/search_nc/report.txt");
    let _ = Vfs::remove("/tmp/search_nc/notes.txt");
    let _ = Vfs::remove("/tmp/search_nc/report2.log");
    let _ = Vfs::rmdir("/tmp/search_nc");

    serial_println!("[search]   name contains: ok");
}

fn test_extension_filter() {
    let _ = Vfs::mkdir("/tmp/search_ext");
    Vfs::write_file("/tmp/search_ext/a.txt", b"text").expect("write");
    Vfs::write_file("/tmp/search_ext/b.log", b"log").expect("write");
    Vfs::write_file("/tmp/search_ext/c.txt", b"text2").expect("write");

    let results = Query::new()
        .extension("txt")
        .execute("/tmp/search_ext")
        .expect("execute");

    assert!(results.len() >= 2, "should find 2 .txt files");
    for r in &results {
        assert!(r.path.ends_with(".txt"), "should be .txt: {}", r.path);
    }

    let _ = Vfs::remove("/tmp/search_ext/a.txt");
    let _ = Vfs::remove("/tmp/search_ext/b.log");
    let _ = Vfs::remove("/tmp/search_ext/c.txt");
    let _ = Vfs::rmdir("/tmp/search_ext");

    serial_println!("[search]   extension filter: ok");
}

fn test_size_filter() {
    let _ = Vfs::mkdir("/tmp/search_sz");
    Vfs::write_file("/tmp/search_sz/small.txt", b"hi").expect("write");
    Vfs::write_file("/tmp/search_sz/big.txt", b"this is a much bigger file with plenty of content").expect("write");

    let results = Query::new()
        .size_min(10)
        .files_only()
        .execute("/tmp/search_sz")
        .expect("execute");

    // Only the big file should match.
    for r in &results {
        assert!(r.size >= 10, "size should be >= 10: {} has {}", r.path, r.size);
    }

    let _ = Vfs::remove("/tmp/search_sz/small.txt");
    let _ = Vfs::remove("/tmp/search_sz/big.txt");
    let _ = Vfs::rmdir("/tmp/search_sz");

    serial_println!("[search]   size filter: ok");
}

fn test_type_filter() {
    let _ = Vfs::mkdir("/tmp/search_type");
    let _ = Vfs::mkdir("/tmp/search_type/subdir");
    Vfs::write_file("/tmp/search_type/file.txt", b"data").expect("write");

    // Files only.
    let files = Query::new()
        .files_only()
        .depth(1)
        .execute("/tmp/search_type")
        .expect("execute");

    for r in &files {
        assert_eq!(r.entry_type, EntryType::File, "should be file: {}", r.path);
    }

    // Dirs only.
    let dirs = Query::new()
        .dirs_only()
        .depth(1)
        .execute("/tmp/search_type")
        .expect("execute");

    for r in &dirs {
        assert_eq!(r.entry_type, EntryType::Directory, "should be dir: {}", r.path);
    }

    let _ = Vfs::remove("/tmp/search_type/file.txt");
    let _ = Vfs::rmdir("/tmp/search_type/subdir");
    let _ = Vfs::rmdir("/tmp/search_type");

    serial_println!("[search]   type filter: ok");
}

fn test_glob_match() {
    assert!(glob_match("*.txt", "hello.txt"));
    assert!(!glob_match("*.txt", "hello.log"));
    assert!(glob_match("report*", "report_2024.pdf"));
    assert!(glob_match("?at", "cat"));
    assert!(!glob_match("?at", "chat"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("a*b", "aXb"));
    assert!(glob_match("a*b", "ab"));

    serial_println!("[search]   glob match: ok");
}

fn test_compound_query() {
    let _ = Vfs::mkdir("/tmp/search_cq");
    Vfs::write_file("/tmp/search_cq/report.txt", b"a short report file with some content").expect("write");
    Vfs::write_file("/tmp/search_cq/report.log", b"log data").expect("write");
    Vfs::write_file("/tmp/search_cq/data.txt", b"x").expect("write");

    // Name contains "report" AND extension "txt" AND size >= 10.
    let results = Query::new()
        .name_contains("report")
        .extension("txt")
        .size_min(10)
        .execute("/tmp/search_cq")
        .expect("execute");

    assert!(!results.is_empty(), "should find at least 1 matching file");
    for r in &results {
        assert!(r.path.contains("report"), "should contain 'report'");
        assert!(r.path.ends_with(".txt"), "should be .txt");
        assert!(r.size >= 10, "size should be >= 10");
    }

    let _ = Vfs::remove("/tmp/search_cq/report.txt");
    let _ = Vfs::remove("/tmp/search_cq/report.log");
    let _ = Vfs::remove("/tmp/search_cq/data.txt");
    let _ = Vfs::rmdir("/tmp/search_cq");

    serial_println!("[search]   compound query: ok");
}

fn test_stats() {
    let (searches, _results) = stats();
    assert!(searches > 0, "should have recorded searches");
    serial_println!("[search]   stats: ok");
}
