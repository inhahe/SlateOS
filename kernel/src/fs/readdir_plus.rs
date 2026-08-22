//! Enhanced directory listing with metadata prefetch (readdir+stat).
//!
//! Traditional directory listing requires two steps:
//! 1. `readdir()` to get filenames
//! 2. `stat()` on each filename to get metadata
//!
//! This creates an N+1 query problem that is especially painful on
//! network filesystems or when listing directories with many files.
//!
//! `readdir_plus` batches the operation: read directory entries and
//! their metadata in one pass, avoiding per-file stat calls.
//!
//! ## Architecture
//!
//! ```text
//! Application → readdir_plus("/some/dir")
//!   → VFS readdir + batch metadata fetch
//!   → returns Vec<DirEntryPlus> with name + full attributes
//!   → optional sorting (name, size, mtime, type)
//!   → optional filtering (glob pattern, type filter)
//! ```
//!
//! ## Use Cases
//!
//! - **File managers** — display filename, size, date, type in columns
//! - **`ls -l`** equivalent — single-call listing with attributes
//! - **Search/indexing** — enumerate + filter without stat storm
//! - **Build systems** — check mtimes of directory contents efficiently
//!
//! ## Design Notes
//!
//! - Maximum entries per call: 4096 (paginated for huge directories).
//! - Sorting is in-kernel for display-ready output (avoids repeated
//!   sorts in userspace).
//! - Cache-friendly: fetches all metadata while directory data is hot
//!   in the buffer cache.
//! - Statistics track call count and entries returned for profiling.

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::{EntryType, FileMeta, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entries returned per call.
const MAX_ENTRIES_PER_CALL: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Enhanced directory entry with full metadata.
#[derive(Debug, Clone)]
pub struct DirEntryPlus {
    /// Entry name (filename only, not full path).
    pub name: PathBuf,
    /// Entry type (file, directory, symlink, etc.).
    pub entry_type: EntryType,
    /// Full file metadata (size, timestamps, permissions).
    pub meta: Option<FileMeta>,
    /// Full path for reference.
    pub full_path: PathBuf,
}

/// Sort order for directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Alphabetical by name (default).
    Name,
    /// By name, descending.
    NameDesc,
    /// By size, largest first.
    SizeLargest,
    /// By size, smallest first.
    SizeSmallest,
    /// By modification time, newest first.
    MtimeNewest,
    /// By modification time, oldest first.
    MtimeOldest,
    /// By type (directories first, then files).
    TypeFirst,
    /// No sorting (filesystem order).
    None,
}

impl SortOrder {
    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "name" | "alpha" | "a" => Some(Self::Name),
            "name-desc" | "rname" | "A" => Some(Self::NameDesc),
            "size" | "largest" | "S" => Some(Self::SizeLargest),
            "size-asc" | "smallest" | "s" => Some(Self::SizeSmallest),
            "mtime" | "newest" | "t" => Some(Self::MtimeNewest),
            "mtime-asc" | "oldest" | "T" => Some(Self::MtimeOldest),
            "type" | "kind" => Some(Self::TypeFirst),
            "none" | "raw" => Some(Self::None),
            _ => None,
        }
    }

    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::NameDesc => "name-desc",
            Self::SizeLargest => "size-largest",
            Self::SizeSmallest => "size-smallest",
            Self::MtimeNewest => "mtime-newest",
            Self::MtimeOldest => "mtime-oldest",
            Self::TypeFirst => "type-first",
            Self::None => "none",
        }
    }
}

/// Type filter for directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFilter {
    /// All entries.
    All,
    /// Files only.
    FilesOnly,
    /// Directories only.
    DirsOnly,
    /// Symlinks only.
    SymlinksOnly,
}

/// Options for readdir_plus calls.
#[derive(Debug, Clone)]
pub struct ListOptions {
    /// Sort order.
    pub sort: SortOrder,
    /// Type filter.
    pub type_filter: TypeFilter,
    /// Glob pattern filter (empty = no filter).
    ///
    /// Bytes, not text: the names it is matched against come from `readdir`
    /// and have no declared encoding, so a `String` pattern could only ever
    /// match the subset of names that happen to decode.
    pub pattern: Vec<u8>,
    /// Whether to include hidden files (starting with '.').
    pub show_hidden: bool,
    /// Maximum entries (0 = default limit).
    pub limit: usize,
    /// Offset for pagination.
    pub offset: usize,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            sort: SortOrder::Name,
            type_filter: TypeFilter::All,
            pattern: Vec::new(),
            show_hidden: true,
            limit: 0,
            offset: 0,
        }
    }
}

/// Result summary from a readdir_plus call.
#[derive(Debug, Clone)]
pub struct ListResult {
    /// Entries returned.
    pub entries: Vec<DirEntryPlus>,
    /// Total matching entries (before pagination).
    pub total_count: usize,
    /// Whether more entries exist beyond this page.
    pub has_more: bool,
    /// Total size of all listed files.
    pub total_size: u64,
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static CALL_COUNT: AtomicU64 = AtomicU64::new(0);
static ENTRIES_RETURNED: AtomicU64 = AtomicU64::new(0);
static METADATA_FETCHED: AtomicU64 = AtomicU64::new(0);
static METADATA_ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Enhanced directory listing with metadata prefetch.
///
/// Returns directory entries with full attributes in a single call,
/// sorted and filtered according to options.
pub fn readdir_plus<P: AsRef<Path> + ?Sized>(
    dir_path: &P,
    options: &ListOptions,
) -> KernelResult<ListResult> {
    let dir_path = dir_path.as_ref();
    if dir_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    CALL_COUNT.fetch_add(1, Ordering::Relaxed);

    // Read directory entries from VFS.
    let raw_entries = Vfs::readdir(dir_path)?;

    // Build enriched entries with metadata.
    let mut entries: Vec<DirEntryPlus> = Vec::new();

    for entry in &raw_entries {
        // Apply type filter.
        if !matches_type_filter(entry.entry_type, options.type_filter) {
            continue;
        }

        // Apply hidden filter.
        // Byte compare, not `Path::starts_with`: the latter matches whole
        // components, so it would ask whether the name *is* `.`.
        if !options.show_hidden && entry.name.as_bytes().starts_with(b".") {
            continue;
        }

        // Apply glob pattern.  Case-sensitive: this is a directory listing on
        // a case-sensitive filesystem, so `*.TXT` must not match `a.txt`.
        if !options.pattern.is_empty()
            && !crate::fs::vfs::glob_match(&entry.name, &options.pattern, false)
        {
            continue;
        }

        // Build full path.  `Path::join` inserts exactly one separator, so
        // the trailing-slash special case this used to need is gone.
        let full_path = dir_path.join(&entry.name);

        // Fetch metadata.
        METADATA_FETCHED.fetch_add(1, Ordering::Relaxed);
        let meta = match Vfs::metadata(&full_path) {
            Ok(m) => Some(m),
            Err(_) => {
                METADATA_ERRORS.fetch_add(1, Ordering::Relaxed);
                None
            }
        };

        entries.push(DirEntryPlus {
            name: entry.name.clone(),
            entry_type: entry.entry_type,
            meta,
            full_path,
        });
    }

    let total_count = entries.len();

    // Sort entries.
    sort_entries(&mut entries, options.sort);

    // Calculate total size.
    let total_size: u64 = entries
        .iter()
        .filter_map(|e| e.meta.as_ref())
        .map(|m| m.size)
        .sum();

    // Apply pagination.
    let limit = if options.limit == 0 {
        MAX_ENTRIES_PER_CALL
    } else {
        options.limit
    };
    let start = options.offset.min(entries.len());
    let end = (start + limit).min(entries.len());
    let has_more = end < entries.len();
    let page = entries[start..end].to_vec();

    ENTRIES_RETURNED.fetch_add(page.len() as u64, Ordering::Relaxed);

    Ok(ListResult {
        entries: page,
        total_count,
        has_more,
        total_size,
    })
}

/// Simple readdir_plus with default options (all files, sorted by name).
pub fn readdir_plus_simple<P: AsRef<Path> + ?Sized>(dir_path: &P) -> KernelResult<ListResult> {
    readdir_plus(dir_path, &ListOptions::default())
}

/// Get listing statistics.
pub fn stats() -> (u64, u64, u64, u64) {
    (
        CALL_COUNT.load(Ordering::Relaxed),
        ENTRIES_RETURNED.load(Ordering::Relaxed),
        METADATA_FETCHED.load(Ordering::Relaxed),
        METADATA_ERRORS.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    CALL_COUNT.store(0, Ordering::Relaxed);
    ENTRIES_RETURNED.store(0, Ordering::Relaxed);
    METADATA_FETCHED.store(0, Ordering::Relaxed);
    METADATA_ERRORS.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if entry type matches filter.
fn matches_type_filter(entry_type: EntryType, filter: TypeFilter) -> bool {
    match filter {
        TypeFilter::All => true,
        TypeFilter::FilesOnly => entry_type == EntryType::File,
        TypeFilter::DirsOnly => entry_type == EntryType::Directory,
        TypeFilter::SymlinksOnly => entry_type == EntryType::Symlink,
    }
}

/// Sort entries according to the specified order.
fn sort_entries(entries: &mut [DirEntryPlus], order: SortOrder) {
    match order {
        SortOrder::Name => entries.sort_by(|a, b| a.name.cmp(&b.name)),
        SortOrder::NameDesc => entries.sort_by(|a, b| b.name.cmp(&a.name)),
        SortOrder::SizeLargest => {
            entries.sort_by(|a, b| {
                let sa = a.meta.as_ref().map_or(0, |m| m.size);
                let sb = b.meta.as_ref().map_or(0, |m| m.size);
                sb.cmp(&sa)
            });
        }
        SortOrder::SizeSmallest => {
            entries.sort_by(|a, b| {
                let sa = a.meta.as_ref().map_or(0, |m| m.size);
                let sb = b.meta.as_ref().map_or(0, |m| m.size);
                sa.cmp(&sb)
            });
        }
        SortOrder::MtimeNewest => {
            entries.sort_by(|a, b| {
                let ma = a.meta.as_ref().map_or(0, |m| m.modified_ns);
                let mb = b.meta.as_ref().map_or(0, |m| m.modified_ns);
                mb.cmp(&ma)
            });
        }
        SortOrder::MtimeOldest => {
            entries.sort_by(|a, b| {
                let ma = a.meta.as_ref().map_or(0, |m| m.modified_ns);
                let mb = b.meta.as_ref().map_or(0, |m| m.modified_ns);
                ma.cmp(&mb)
            });
        }
        SortOrder::TypeFirst => {
            entries.sort_by(|a, b| {
                let ta = type_sort_key(a.entry_type);
                let tb = type_sort_key(b.entry_type);
                ta.cmp(&tb).then(a.name.cmp(&b.name))
            });
        }
        SortOrder::None => {} // No sort.
    }
}

/// Sort key for type-first ordering (dirs=0, files=1, others=2).
fn type_sort_key(et: EntryType) -> u8 {
    match et {
        EntryType::Directory => 0,
        EntryType::File => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[readdir_plus] Running self-test...");

    test_basic_listing()?;
    test_sort_orders()?;
    test_type_filter()?;
    test_glob_filter()?;
    test_pagination()?;
    test_glob_match();

    serial_println!("[readdir_plus] Self-test passed (6 tests).");
    Ok(())
}

/// Create a directory for a test to populate, tolerating a leftover one.
///
/// Every test below used to write straight into a path like
/// `/tmp/_rdplus_test/alpha.txt` on the assumption that `Vfs::write_file`
/// creates missing parents. It does not — it returns `NotFound` — so the very
/// first line of the very first test failed the first time these tests were
/// ever executed. One of them even said so out loud ("Create a subdir by
/// writing a file inside it"), which is how a wrong assumption survives when
/// nothing runs the code.
///
/// `mkdir_all` is already idempotent — it `stat`s each component and only
/// creates the missing ones — which matters because these tests remove their
/// *files* on the way out but not their directories, so the second run in a
/// boot always finds the directory there. The `AlreadyExists` arm is belt and
/// braces against that guarantee changing underneath us; it is not currently
/// reachable.
fn ensure_test_dir(path: &str) -> KernelResult<()> {
    match Vfs::mkdir_all(path) {
        Ok(()) | Err(KernelError::AlreadyExists) => Ok(()),
        Err(e) => Err(e),
    }
}

fn test_basic_listing() -> KernelResult<()> {
    // Create test directory with files.
    let dir = "/tmp/_rdplus_test";
    ensure_test_dir(dir)?;
    Vfs::write_file(alloc::format!("{}/alpha.txt", dir), b"aaa")?;
    Vfs::write_file(alloc::format!("{}/beta.dat", dir), b"bbbbb")?;
    Vfs::write_file(alloc::format!("{}/gamma.log", dir), b"g")?;

    let result = readdir_plus_simple(dir)?;
    assert!(
        result.entries.len() >= 3,
        "wrote 3 files, listing returned {}",
        result.entries.len()
    );
    assert!(
        result.total_count >= 3,
        "wrote 3 files, total_count is {}",
        result.total_count
    );

    // Entries should have metadata: prefetching it is the entire point of
    // readdir_plus over readdir, so a `None` here means the module did nothing.
    for entry in &result.entries {
        assert!(
            entry.meta.is_some(),
            "entry {:?} came back without prefetched metadata",
            entry.name
        );
    }

    // Clean up.
    let _ = Vfs::remove(alloc::format!("{}/alpha.txt", dir));
    let _ = Vfs::remove(alloc::format!("{}/beta.dat", dir));
    let _ = Vfs::remove(alloc::format!("{}/gamma.log", dir));
    serial_println!("[readdir_plus]   basic_listing: ok");
    Ok(())
}

fn test_sort_orders() -> KernelResult<()> {
    let dir = "/tmp/_rdplus_sort";
    ensure_test_dir(dir)?;
    Vfs::write_file(alloc::format!("{}/c.txt", dir), b"ccc")?;
    Vfs::write_file(alloc::format!("{}/a.txt", dir), b"a")?;
    Vfs::write_file(alloc::format!("{}/b.txt", dir), b"bb")?;

    // Sort by name.
    let opts = ListOptions {
        sort: SortOrder::Name,
        ..Default::default()
    };
    let result = readdir_plus(dir, &opts)?;
    let names: Vec<&Path> = result.entries.iter().map(|e| e.name.as_path()).collect();
    assert!(
        names.windows(2).all(|w| w[0] <= w[1]),
        "SortOrder::Name did not produce a non-decreasing name sequence: {:?}",
        names
    );

    // Sort by size (largest first).
    let opts2 = ListOptions {
        sort: SortOrder::SizeLargest,
        ..Default::default()
    };
    let result2 = readdir_plus(dir, &opts2)?;
    let sizes: Vec<u64> = result2
        .entries
        .iter()
        .filter_map(|e| e.meta.as_ref())
        .map(|m| m.size)
        .collect();
    assert!(
        sizes.windows(2).all(|w| w[0] >= w[1]),
        "SortOrder::SizeLargest did not produce a non-increasing size sequence: {:?}",
        sizes
    );

    let _ = Vfs::remove(alloc::format!("{}/a.txt", dir));
    let _ = Vfs::remove(alloc::format!("{}/b.txt", dir));
    let _ = Vfs::remove(alloc::format!("{}/c.txt", dir));
    serial_println!("[readdir_plus]   sort_orders: ok");
    Ok(())
}

fn test_type_filter() -> KernelResult<()> {
    let dir = "/tmp/_rdplus_type";
    ensure_test_dir(dir)?;
    Vfs::write_file(alloc::format!("{}/file.txt", dir), b"x")?;
    // A subdirectory, so the directory-only filter has something to find.
    // This used to be one `write_file` into `subdir/inner.txt` with the comment
    // "Create a subdir by writing a file inside it" — `write_file` does not
    // create parents, so both filters were being asked about a directory that
    // was never created.
    ensure_test_dir(&alloc::format!("{}/subdir", dir))?;
    Vfs::write_file(alloc::format!("{}/subdir/inner.txt", dir), b"y")?;

    // Files only.
    let opts = ListOptions {
        type_filter: TypeFilter::FilesOnly,
        ..Default::default()
    };
    let result = readdir_plus(dir, &opts)?;
    assert!(
        result.entries.iter().any(|e| e.name.as_bytes() == b"file.txt"),
        "FilesOnly dropped the one regular file in the directory"
    );
    for entry in &result.entries {
        assert_eq!(
            entry.entry_type,
            EntryType::File,
            "FilesOnly returned {:?}, which is a {:?}",
            entry.name,
            entry.entry_type
        );
    }

    // Dirs only.
    let opts2 = ListOptions {
        type_filter: TypeFilter::DirsOnly,
        ..Default::default()
    };
    let result2 = readdir_plus(dir, &opts2)?;
    assert!(
        result2.entries.iter().any(|e| e.name.as_bytes() == b"subdir"),
        "DirsOnly dropped the one subdirectory in the directory"
    );
    for entry in &result2.entries {
        assert_eq!(
            entry.entry_type,
            EntryType::Directory,
            "DirsOnly returned {:?}, which is a {:?}",
            entry.name,
            entry.entry_type
        );
    }

    let _ = Vfs::remove(alloc::format!("{}/file.txt", dir));
    let _ = Vfs::remove(alloc::format!("{}/subdir/inner.txt", dir));
    serial_println!("[readdir_plus]   type_filter: ok");
    Ok(())
}

fn test_glob_filter() -> KernelResult<()> {
    let dir = "/tmp/_rdplus_glob";
    ensure_test_dir(dir)?;
    Vfs::write_file(alloc::format!("{}/test.txt", dir), b"t")?;
    Vfs::write_file(alloc::format!("{}/test.dat", dir), b"d")?;
    Vfs::write_file(alloc::format!("{}/other.txt", dir), b"o")?;

    // Filter: *.txt
    let opts = ListOptions {
        pattern: b"*.txt".to_vec(),
        ..Default::default()
    };
    let result = readdir_plus(dir, &opts)?;
    for entry in &result.entries {
        assert!(
            entry.name.as_bytes().ends_with(b".txt"),
            "pattern *.txt matched {:?}",
            entry.name
        );
    }
    // Exactly the two `.txt` files, and not the `.dat` one: a filter that
    // returned everything would satisfy the loop above but not this.
    assert_eq!(
        result.total_count, 2,
        "pattern *.txt should match exactly test.txt and other.txt, got {}",
        result.total_count
    );

    // Filter: test.*
    let opts2 = ListOptions {
        pattern: b"test.*".to_vec(),
        ..Default::default()
    };
    let result2 = readdir_plus(dir, &opts2)?;
    for entry in &result2.entries {
        assert!(
            entry.name.as_bytes().starts_with(b"test."),
            "pattern test.* matched {:?}",
            entry.name
        );
    }
    assert_eq!(
        result2.total_count, 2,
        "pattern test.* should match exactly test.txt and test.dat, got {}",
        result2.total_count
    );

    let _ = Vfs::remove(alloc::format!("{}/test.txt", dir));
    let _ = Vfs::remove(alloc::format!("{}/test.dat", dir));
    let _ = Vfs::remove(alloc::format!("{}/other.txt", dir));
    serial_println!("[readdir_plus]   glob_filter: ok");
    Ok(())
}

fn test_pagination() -> KernelResult<()> {
    let dir = "/tmp/_rdplus_page";
    ensure_test_dir(dir)?;
    for i in 0..10 {
        Vfs::write_file(alloc::format!("{}/file{:02}.txt", dir, i), b"x")?;
    }

    // Page 1: first 3 entries.
    let opts = ListOptions {
        limit: 3,
        offset: 0,
        ..Default::default()
    };
    let result = readdir_plus(dir, &opts)?;
    assert_eq!(result.entries.len(), 3, "limit=3 returned a different count");
    assert!(
        result.has_more,
        "10 entries with limit=3 must report more remaining"
    );
    assert_eq!(
        result.total_count, 10,
        "total_count must count the whole directory, not the page"
    );

    // Page 2: next 3.
    let opts2 = ListOptions {
        limit: 3,
        offset: 3,
        ..Default::default()
    };
    let result2 = readdir_plus(dir, &opts2)?;
    assert_eq!(result2.entries.len(), 3, "offset=3 limit=3 returned a different count");

    // Verify no overlap between pages.
    let page1_names: Vec<&Path> = result.entries.iter().map(|e| e.name.as_path()).collect();
    for entry in &result2.entries {
        assert!(
            !page1_names.contains(&entry.name.as_path()),
            "{:?} appeared on both page 1 and page 2",
            entry.name
        );
    }

    // The last page must say so, or a caller paging until `has_more` is false
    // never terminates.
    let opts3 = ListOptions {
        limit: 3,
        offset: 9,
        ..Default::default()
    };
    let result3 = readdir_plus(dir, &opts3)?;
    assert_eq!(result3.entries.len(), 1, "offset=9 of 10 entries should yield 1");
    assert!(
        !result3.has_more,
        "the final page must not claim more entries remain"
    );

    for i in 0..10 {
        let _ = Vfs::remove(alloc::format!("{}/file{:02}.txt", dir, i));
    }
    serial_println!("[readdir_plus]   pagination: ok");
    Ok(())
}

fn test_glob_match() {
    // Listing delegates to the one shared matcher; these pin the *mode* the
    // listing asks for (case-sensitive), not the matcher itself.
    let m = |pat: &[u8], name: &[u8]| crate::fs::vfs::glob_match(name, pat, false);

    assert!(m(b"*", b"anything"));
    assert!(m(b"*.txt", b"file.txt"));
    assert!(!m(b"*.txt", b"file.dat"));
    assert!(m(b"file?", b"file1"));
    assert!(!m(b"file?", b"file12"));
    assert!(m(b"*.rs", b"main.rs"));
    assert!(m(b"test*", b"testing123"));
    assert!(m(b"*test*", b"my_test_file"));
    assert!(m(b"a*b*c", b"aXbYc"));
    assert!(!m(b"a*b*c", b"aXbY"));
    // Case-sensitive: the filesystem is, so the listing filter is too.
    assert!(!m(b"*.TXT", b"file.txt"));

    serial_println!("[readdir_plus]   glob_match: ok");
}
