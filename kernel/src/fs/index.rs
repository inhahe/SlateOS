//! Background filesystem indexer for fast file search.
//!
//! Provides an in-memory index of file metadata (path, name, size, type,
//! modification time) for fast `locate`-style searching.  The indexer
//! walks configured directories and builds a flat index that can be
//! queried by name substring, extension, or size range.
//!
//! ## Design
//!
//! - **Not automatic**: the indexer does not run by default.  A `locate
//!   --update` command or programmatic `rebuild()` call triggers the
//!   full walk.  Incremental updates are supported via `add_entry()`
//!   and `remove_entry()`.
//! - **In-memory only**: the index lives in a spinlock-protected struct.
//!   It does not persist across reboots (the design says "indexer does
//!   not run by default" — persistence is not needed for a rebuild-on-
//!   demand model).
//! - **Bounded**: the index caps at `max_entries` to prevent OOM.  When
//!   the cap is reached, the walk stops and the `truncated` flag is set.
//! - **Extension index**: a secondary `BTreeMap<ext, Vec<usize>>` maps
//!   file extensions to entry indices for O(1) extension-filtered search.
//!
//! ## Usage from kshell
//!
//! ```text
//! locate --update          Rebuild the full index
//! locate pattern           Search by case-insensitive substring
//! locate --ext rs          Search by extension
//! locate --stats           Show index statistics
//! ```
//!
//! ## Reference
//!
//! design.txt line 998: "background indexer for fast search of all files
//! in configured file/directory list with configured extensions/filespecs.
//! useful defaults, but indexer does not run by default."

#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::pathutil::contains_bytes;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single indexed file entry.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// Full absolute path (e.g. "/docs/readme.txt").
    pub path: PathBuf,
    /// Filename only (e.g. "readme.txt").
    pub name: PathBuf,
    /// ASCII-lowercased filename for case-insensitive search.
    ///
    /// Bytes, not a `String`: a filename is whatever bytes the filesystem
    /// stored, and only the ASCII range can be case-folded without knowing
    /// an encoding.  Non-ASCII bytes pass through untouched, so two names
    /// differing outside ASCII stay distinct.
    pub name_lower: Vec<u8>,
    /// File extension (ASCII-lowercased, without dot), empty if none.
    pub extension: Vec<u8>,
    /// File, Directory, or Symlink.
    pub entry_type: super::vfs::EntryType,
    /// File size in bytes.
    pub size: u64,
    /// Last-modified timestamp in nanoseconds since boot (0 = unknown).
    pub modified_ns: u64,
}

/// Configuration for what to index.
#[derive(Debug, Clone)]
pub struct IndexConfig {
    /// Directories to index (e.g. `["/"]`).
    pub watch_dirs: Vec<PathBuf>,
    /// Allowed file extensions (ASCII-lowercased, without dot).
    /// Empty means "index all files".
    pub extensions: Vec<Vec<u8>>,
    /// Directories to skip during walk (e.g. `["/_JOURNAL"]`).
    pub exclude_dirs: Vec<PathBuf>,
    /// Maximum number of entries before stopping the walk.
    pub max_entries: usize,
    /// Whether to index directories (not just files).
    pub include_dirs: bool,
}

/// Statistics about the current index state.
#[derive(Debug, Clone, Copy)]
pub struct IndexStats {
    /// Total number of entries in the index.
    pub total_entries: usize,
    /// Total size (bytes) of all indexed files.
    pub total_size: u64,
    /// Number of unique extensions in the index.
    pub extension_count: usize,
    /// Timestamp (ns since boot) of the last full rebuild, or `None` if the
    /// index has never been rebuilt.
    ///
    /// Not a `u64` with `0` meaning "never": this clock counts nanoseconds
    /// since boot and starts *at* zero, so `0` is a legal instant — and it is
    /// the instant an index built during early boot would carry. Under the old
    /// encoding that rebuild reported itself as never having happened, which
    /// is the one case where the distinction matters, since an index built
    /// that early is the one most likely to be stale by the time anyone looks.
    pub last_rebuild_ns: Option<u64>,
    /// How many full rebuilds have been performed.
    pub rebuild_count: u64,
    /// True if the last rebuild hit `max_entries` and stopped early.
    pub truncated: bool,
    /// Whether the index has been initialized.
    pub initialized: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Inner state behind the spinlock.
struct IndexInner {
    entries: Vec<IndexEntry>,
    /// Secondary index: lowercase extension -> indices into `entries`.
    by_extension: BTreeMap<Vec<u8>, Vec<usize>>,
    config: IndexConfig,
    stats: IndexStats,
}

static INDEX: Mutex<IndexInner> = Mutex::new(IndexInner {
    entries: Vec::new(),
    by_extension: BTreeMap::new(),
    config: IndexConfig {
        watch_dirs: Vec::new(),
        extensions: Vec::new(),
        exclude_dirs: Vec::new(),
        max_entries: 16384,
        include_dirs: false,
    },
    stats: IndexStats {
        total_entries: 0,
        total_size: 0,
        extension_count: 0,
        last_rebuild_ns: None,
        rebuild_count: 0,
        truncated: false,
        initialized: false,
    },
});

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Returns a sensible default configuration.
///
/// Indexes everything under `/`, skipping internal directories like
/// `/_JOURNAL` and `/proc`/`/sys`/`/dev` (virtual filesystems that
/// don't represent real files on disk).
pub fn default_config() -> IndexConfig {
    IndexConfig {
        watch_dirs: alloc::vec![PathBuf::from("/")],
        extensions: Vec::new(), // all extensions
        exclude_dirs: alloc::vec![
            PathBuf::from("/_JOURNAL"),
            PathBuf::from("/proc"),
            PathBuf::from("/sys"),
            PathBuf::from("/dev"),
        ],
        max_entries: 16384,
        include_dirs: false,
    }
}

/// Initialize the indexer with a configuration.
///
/// Does NOT trigger a rebuild — call `rebuild()` separately.
pub fn init(config: IndexConfig) {
    let mut idx = INDEX.lock();
    idx.config = config;
    idx.stats.initialized = true;
}

// ---------------------------------------------------------------------------
// Full rebuild
// ---------------------------------------------------------------------------

/// Fully rebuild the index by walking the configured directories.
///
/// Drops the spinlock during VFS I/O to avoid holding it across
/// filesystem calls (which may acquire VFS locks).  The walk is
/// single-threaded; results are collected into a local Vec and then
/// swapped into the global state atomically.
pub fn rebuild() -> KernelResult<()> {
    // Snapshot the config under the lock, then release.
    let config = {
        let idx = INDEX.lock();
        if !idx.stats.initialized {
            return Err(KernelError::NotSupported);
        }
        idx.config.clone()
    };

    // Walk filesystem WITHOUT holding the index lock.
    let mut collected: Vec<IndexEntry> = Vec::new();
    let mut truncated = false;

    for dir in &config.watch_dirs {
        if truncated {
            break;
        }
        walk_directory(
            dir,
            &config,
            &mut collected,
            &mut truncated,
            0, // depth
        );
    }

    // Build the extension index.
    let mut by_ext: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, entry) in collected.iter().enumerate() {
        if !entry.extension.is_empty() {
            by_ext.entry(entry.extension.clone()).or_default().push(i);
        }
    }

    let total_size: u64 = collected.iter().map(|e| e.size).sum();
    let ext_count = by_ext.len();
    let entry_count = collected.len();
    let now = crate::hpet::elapsed_ns();

    // Swap results in under the lock.
    let mut idx = INDEX.lock();
    idx.entries = collected;
    idx.by_extension = by_ext;
    idx.stats.total_entries = entry_count;
    idx.stats.total_size = total_size;
    idx.stats.extension_count = ext_count;
    idx.stats.last_rebuild_ns = Some(now);
    idx.stats.rebuild_count = idx.stats.rebuild_count.saturating_add(1);
    idx.stats.truncated = truncated;

    Ok(())
}

/// Maximum directory recursion depth to prevent infinite loops from
/// symlink cycles or extremely deep trees.
const MAX_DEPTH: u32 = 64;

/// Recursively walk a directory, collecting entries.
fn walk_directory(
    path: &Path,
    config: &IndexConfig,
    collected: &mut Vec<IndexEntry>,
    truncated: &mut bool,
    depth: u32,
) {
    if depth > MAX_DEPTH || *truncated {
        return;
    }

    // Check if this directory should be excluded (canonical subtree
    // predicate tolerates a trailing slash on the exclude entry). See
    // fs::pathutil.
    for excl in &config.exclude_dirs {
        if crate::fs::pathutil::path_in_subtree(path, excl) {
            return;
        }
    }

    let entries = match super::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return, // skip unreadable directories
    };

    for entry in &entries {
        if collected.len() >= config.max_entries {
            *truncated = true;
            return;
        }

        // `PathBuf::push` inserts the separator only when one is needed, so
        // the root case needs no special handling.
        let mut full_path = path.to_path_buf();
        full_path.push(&entry.name);

        match entry.entry_type {
            super::vfs::EntryType::Directory => {
                if config.include_dirs {
                    let idx_entry =
                        make_index_entry(&full_path, &entry.name, entry.entry_type, entry.size);
                    collected.push(idx_entry);
                }
                // Recurse into subdirectory.
                walk_directory(
                    &full_path,
                    config,
                    collected,
                    truncated,
                    depth.saturating_add(1),
                );
            }
            super::vfs::EntryType::File | super::vfs::EntryType::Symlink => {
                let ext = extract_extension(&entry.name);

                // Apply extension filter.
                if !config.extensions.is_empty()
                    && !config
                        .extensions
                        .iter()
                        .any(|e| e.as_slice() == ext.as_slice())
                {
                    continue;
                }

                let idx_entry =
                    make_index_entry(&full_path, &entry.name, entry.entry_type, entry.size);
                collected.push(idx_entry);
            }
            _ => {} // skip VolumeLabel etc.
        }
    }
}

/// Build an `IndexEntry` from path components.
///
/// Tries to read file metadata for the modification timestamp; if
/// the metadata call fails, falls back to 0.
fn make_index_entry(
    full_path: &Path,
    name: &Path,
    entry_type: super::vfs::EntryType,
    size: u64,
) -> IndexEntry {
    let modified_ns = super::Vfs::metadata(full_path)
        .map(|m| m.modified_ns)
        .unwrap_or(0);

    let name_lower = to_ascii_lower(name.as_bytes());
    let extension = extract_extension(name);

    IndexEntry {
        path: full_path.to_path_buf(),
        name: name.to_path_buf(),
        name_lower,
        extension,
        entry_type,
        size,
        modified_ns,
    }
}

// ---------------------------------------------------------------------------
// Incremental updates
// ---------------------------------------------------------------------------

/// Add a single file to the index.
///
/// If the file is already indexed (same path), it is updated in place.
pub fn add_entry(path: impl AsRef<Path>) -> KernelResult<()> {
    let path = path.as_ref();

    // Stat the file without holding the lock.
    let dir_entry = super::Vfs::stat(path)?;
    let meta_modified = super::Vfs::metadata(path)
        .map(|m| m.modified_ns)
        .unwrap_or(0);

    let name = path_filename(path);
    let name_lower = to_ascii_lower(name.as_bytes());
    let extension = extract_extension(&name);

    let entry = IndexEntry {
        path: path.to_path_buf(),
        name,
        name_lower,
        extension: extension.clone(),
        entry_type: dir_entry.entry_type,
        size: dir_entry.size,
        modified_ns: meta_modified,
    };

    let mut idx = INDEX.lock();

    // Check for existing entry with same path → update.
    if let Some(pos) = idx.entries.iter().position(|e| e.path.as_path() == path) {
        let old_ext = idx
            .entries
            .get(pos)
            .map(|e| e.extension.clone())
            .unwrap_or_default();
        // Remove from old extension index.
        if !old_ext.is_empty() {
            if let Some(list) = idx.by_extension.get_mut(&old_ext) {
                list.retain(|&i| i != pos);
            }
        }
        // Update the entry.
        let new_size = entry.size;
        if let Some(old) = idx.entries.get(pos) {
            idx.stats.total_size = idx.stats.total_size.saturating_sub(old.size);
        }
        if let Some(slot) = idx.entries.get_mut(pos) {
            *slot = entry;
        }
        idx.stats.total_size = idx.stats.total_size.saturating_add(new_size);
        // Add to new extension index.
        if !extension.is_empty() {
            idx.by_extension.entry(extension).or_default().push(pos);
        }
    } else {
        // New entry.
        if idx.entries.len() >= idx.config.max_entries {
            return Err(KernelError::ResourceExhausted);
        }
        let new_idx = idx.entries.len();
        idx.stats.total_size = idx.stats.total_size.saturating_add(entry.size);
        idx.entries.push(entry);
        idx.stats.total_entries = idx.entries.len();
        if !extension.is_empty() {
            idx.by_extension.entry(extension).or_default().push(new_idx);
        }
    }

    Ok(())
}

/// Remove all entries whose path starts with `prefix`.
///
/// Returns the number of entries removed.
pub fn remove_entry(prefix: impl AsRef<Path>) -> usize {
    let prefix = prefix.as_ref();
    let mut idx = INDEX.lock();
    let before = idx.entries.len();

    // Collect indices to remove.
    let to_remove: Vec<usize> = idx
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| crate::fs::pathutil::path_in_subtree(&e.path, prefix))
        .map(|(i, _)| i)
        .collect();

    if to_remove.is_empty() {
        return 0;
    }

    // Subtract removed sizes.
    for &i in &to_remove {
        if let Some(e) = idx.entries.get(i) {
            idx.stats.total_size = idx.stats.total_size.saturating_sub(e.size);
        }
    }

    // Remove in reverse order to keep indices stable.
    for &i in to_remove.iter().rev() {
        idx.entries.swap_remove(i);
    }

    // Rebuild the extension index from scratch (swap_remove invalidates indices).
    rebuild_ext_index(&mut idx);

    idx.stats.total_entries = idx.entries.len();
    before.saturating_sub(idx.entries.len())
}

/// Rebuild the extension BTreeMap from the entries list.
fn rebuild_ext_index(idx: &mut IndexInner) {
    idx.by_extension.clear();
    for (i, entry) in idx.entries.iter().enumerate() {
        if !entry.extension.is_empty() {
            idx.by_extension
                .entry(entry.extension.clone())
                .or_default()
                .push(i);
        }
    }
    idx.stats.extension_count = idx.by_extension.len();
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search by case-insensitive substring in the filename (`locate`-style).
///
/// Returns cloned results (the lock is released before returning).
pub fn search_name(query: impl AsRef<[u8]>) -> Vec<IndexEntry> {
    let query_lower = to_ascii_lower(query.as_ref());
    let idx = INDEX.lock();
    idx.entries
        .iter()
        .filter(|e| contains_bytes(&e.name_lower, &query_lower))
        .cloned()
        .collect()
}

/// Search by case-insensitive substring in the full path.
pub fn search_path(query: impl AsRef<[u8]>) -> Vec<IndexEntry> {
    let query_lower = to_ascii_lower(query.as_ref());
    let idx = INDEX.lock();
    idx.entries
        .iter()
        .filter(|e| contains_bytes(&to_ascii_lower(e.path.as_bytes()), &query_lower))
        .cloned()
        .collect()
}

/// Search by file extension (case-insensitive, without dot).
pub fn search_ext(ext: impl AsRef<[u8]>) -> Vec<IndexEntry> {
    let ext_lower = to_ascii_lower(ext.as_ref());
    let idx = INDEX.lock();
    if let Some(indices) = idx.by_extension.get(&ext_lower) {
        indices
            .iter()
            .filter_map(|&i| idx.entries.get(i).cloned())
            .collect()
    } else {
        Vec::new()
    }
}

/// Search by file size range (inclusive bounds).
pub fn search_size(min: u64, max: u64) -> Vec<IndexEntry> {
    let idx = INDEX.lock();
    idx.entries
        .iter()
        .filter(|e| e.size >= min && e.size <= max)
        .cloned()
        .collect()
}

/// Search by entry type (e.g., only files or only directories).
pub fn search_type(entry_type: super::vfs::EntryType) -> Vec<IndexEntry> {
    let idx = INDEX.lock();
    idx.entries
        .iter()
        .filter(|e| e.entry_type == entry_type)
        .cloned()
        .collect()
}

/// Combined search: name substring AND optional extension AND optional size range.
pub fn search(
    name_query: Option<&[u8]>,
    ext_filter: Option<&[u8]>,
    min_size: Option<u64>,
    max_size: Option<u64>,
) -> Vec<IndexEntry> {
    let name_lower = name_query.map(to_ascii_lower);
    let ext_lower = ext_filter.map(to_ascii_lower);

    let idx = INDEX.lock();
    idx.entries
        .iter()
        .filter(|e| {
            if let Some(ref q) = name_lower {
                if !contains_bytes(&e.name_lower, q) {
                    return false;
                }
            }
            if let Some(ref ext) = ext_lower {
                if e.extension != *ext {
                    return false;
                }
            }
            if let Some(min) = min_size {
                if e.size < min {
                    return false;
                }
            }
            if let Some(max) = max_size {
                if e.size > max {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Return a snapshot of the current index statistics.
pub fn stats() -> IndexStats {
    INDEX.lock().stats
}

/// How long ago the index was last fully rebuilt, in nanoseconds.
///
/// `None` means it never has been — which is a different statement from
/// "rebuilt just now", and the reason [`IndexStats::last_rebuild_ns`] is an
/// `Option` rather than a `u64` with a zero sentinel.
///
/// This is the number that says whether a `locate` result can be trusted: the
/// index is a snapshot, and nothing between rebuilds tells the caller how old
/// that snapshot is. Incremental VFS hooks keep it roughly current for watched
/// directories, but only a rebuild sees the rest of the tree.
#[must_use]
pub fn age_of_last_rebuild_ns() -> Option<u64> {
    let last = INDEX.lock().stats.last_rebuild_ns?;
    // Saturating rather than wrapping: the HPET is monotonic, so `now < last`
    // should be impossible, and reporting an age of zero is a better failure
    // than reporting one of several hundred years.
    Some(crate::hpet::elapsed_ns().saturating_sub(last))
}

/// Return the total number of indexed entries.
pub fn count() -> usize {
    INDEX.lock().entries.len()
}

/// Clear the entire index (keeps configuration).
pub fn clear() {
    let mut idx = INDEX.lock();
    idx.entries.clear();
    idx.by_extension.clear();
    idx.stats.total_entries = 0;
    idx.stats.total_size = 0;
    idx.stats.extension_count = 0;
    idx.stats.truncated = false;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the file extension (ASCII-lowercased, without the dot) from a
/// filename, as bytes.
///
/// Returns an empty vector if there's no extension or the name starts with
/// a dot and has no other dots (e.g. `.bashrc` → "bashrc" is the full
/// name, not an extension).
fn extract_extension(name: &Path) -> Vec<u8> {
    // `Path::extension` already encodes the dotfile rule (a leading `.` that
    // is the only dot is part of the name, not an extension) and the
    // trailing-dot rule (nothing after the dot is not an extension), so the
    // hand-rolled rfind logic that used to live here is gone.
    name.extension()
        .map_or_else(Vec::new, |e| to_ascii_lower(e.as_bytes()))
}

/// Extract the filename component from an absolute path.
///
/// Returns the whole path when it has no components (`/` or empty), which
/// preserves the previous behaviour for those degenerate inputs.
fn path_filename(path: &Path) -> PathBuf {
    path.file_name().unwrap_or(path).to_path_buf()
}

/// ASCII-only lowercase conversion over bytes.
///
/// Only the ASCII range is folded.  Case folding any other byte would
/// require knowing an encoding, and a filename has none — it is bytes.
/// Leaving them untouched keeps two names that differ only outside ASCII
/// distinct, which is the property a `from_utf8_lossy`-based fold destroys.
fn to_ascii_lower(s: &[u8]) -> Vec<u8> {
    s.iter().map(u8::to_ascii_lowercase).collect()
}

// ---------------------------------------------------------------------------
// VFS event hooks — automatic incremental updates
// ---------------------------------------------------------------------------
//
// These functions are called by VFS write/create/delete/rename operations
// to keep the index current without requiring manual `updatedb`.  They are
// best-effort: failures are silently ignored so filesystem operations are
// never blocked by indexer issues.

/// Check if the indexer is initialized and has been rebuilt at least once.
fn is_live() -> bool {
    let idx = INDEX.lock();
    idx.stats.initialized && idx.stats.rebuild_count > 0
}

/// Check if a path is within a configured watch directory.
///
/// Returns false if the path is in an excluded directory.
fn is_watched(path: &Path) -> bool {
    let idx = INDEX.lock();
    if idx.config.watch_dirs.is_empty() {
        return false;
    }

    // Check exclusions first.  The canonical subtree predicate avoids
    // /sys matching /system and tolerates a trailing slash. See fs::pathutil.
    for excl in &idx.config.exclude_dirs {
        if crate::fs::pathutil::path_in_subtree(path, excl) {
            return false;
        }
    }

    // Check if within any watch dir.
    for dir in &idx.config.watch_dirs {
        if crate::fs::pathutil::path_in_subtree(path, dir) {
            return true;
        }
    }
    false
}

/// Called by VFS when a file is created or modified.
///
/// Adds or updates the file in the index.  No-op if the indexer hasn't
/// been initialized or rebuilt, or if the path is outside watch dirs.
pub fn on_file_changed(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !is_live() || !is_watched(path) {
        return;
    }
    // add_entry calls Vfs::stat internally — the VFS lock must NOT be
    // held by the caller (it isn't: VFS calls emit after releasing).
    let _ = add_entry(path);
}

/// Called by VFS when a file or directory is deleted.
///
/// Removes the entry (and children for directories) from the index.
pub fn on_file_deleted(path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !is_live() || !is_watched(path) {
        return;
    }
    remove_entry(path);
}

/// Called by VFS when a file or directory is renamed.
///
/// Removes the old path and adds the new path.
pub fn on_file_renamed(old_path: impl AsRef<Path>, new_path: impl AsRef<Path>) {
    let (old_path, new_path) = (old_path.as_ref(), new_path.as_ref());
    if !is_live() {
        return;
    }
    // Remove old entry if it was watched.
    if is_watched(old_path) {
        remove_entry(old_path);
    }
    // Add new entry if it's now in a watched directory.
    if is_watched(new_path) {
        let _ = add_entry(new_path);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the filesystem indexer.
///
/// Tests configuration, helpers (extension extraction, case folding),
/// in-memory add/search/remove, and optionally rebuild + VFS-based
/// tests if a filesystem is mounted.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[index] Running self-test...");

    // --- Test 1: extension extraction (pure logic, no FS needed) ---
    {
        if extract_extension(Path::new("file.txt")).as_slice() != b"txt" {
            serial_println!("[index]   ERROR: ext('file.txt') != 'txt'");
            return Err(KernelError::InternalError);
        }
        if extract_extension(Path::new("archive.tar.gz")).as_slice() != b"gz" {
            serial_println!("[index]   ERROR: ext('archive.tar.gz') != 'gz'");
            return Err(KernelError::InternalError);
        }
        if !extract_extension(Path::new("Makefile")).is_empty() {
            serial_println!("[index]   ERROR: ext('Makefile') should be empty");
            return Err(KernelError::InternalError);
        }
        if !extract_extension(Path::new(".bashrc")).is_empty() {
            serial_println!("[index]   ERROR: ext('.bashrc') should be empty");
            return Err(KernelError::InternalError);
        }
        if extract_extension(Path::new("PHOTO.JPG")).as_slice() != b"jpg" {
            serial_println!("[index]   ERROR: ext('PHOTO.JPG') != 'jpg'");
            return Err(KernelError::InternalError);
        }
        if !extract_extension(Path::new("file.")).is_empty() {
            serial_println!("[index]   ERROR: ext('file.') should be empty");
            return Err(KernelError::InternalError);
        }
        // A non-UTF-8 extension must survive intact: the indexer stores
        // extensions as bytes precisely so `\xff` is not replaced or dropped.
        if extract_extension(Path::new(b"photo.j\xffg".as_slice())).as_slice() != b"j\xffg" {
            serial_println!("[index]   ERROR: non-UTF-8 extension mangled");
            return Err(KernelError::InternalError);
        }
        if path_filename(Path::new("/docs/readme.txt")).as_path() != Path::new("readme.txt") {
            serial_println!("[index]   ERROR: filename('/docs/readme.txt') wrong");
            return Err(KernelError::InternalError);
        }
        serial_println!("[index]   extension extraction OK");
    }

    // --- Test 2: init + config ---
    {
        let cfg = default_config();
        if cfg.watch_dirs.is_empty() {
            serial_println!("[index]   ERROR: default config has no watch dirs");
            return Err(KernelError::InternalError);
        }
        if cfg.max_entries == 0 {
            serial_println!("[index]   ERROR: default config max_entries is 0");
            return Err(KernelError::InternalError);
        }
        init(cfg);
        let st = stats();
        if !st.initialized {
            serial_println!("[index]   ERROR: not initialized after init()");
            return Err(KernelError::InternalError);
        }
        serial_println!("[index]   init OK");
    }

    // --- Test 3: in-memory add/search/remove (no VFS needed) ---
    {
        // Manually inject entries to test search and remove.
        clear();
        {
            let mut idx = INDEX.lock();
            let entries: [(&[u8], &[u8], u64); 6] = [
                (b"hello.txt", b"/docs/hello.txt", 100u64),
                (b"README.md", b"/README.md", 2048),
                (b"photo.JPG", b"/pics/photo.JPG", 500_000),
                (b"_CaseTest.RS", b"/src/_CaseTest.RS", 42),
                (b"data.bin", b"/tmp/data.bin", 1024),
                // Non-UTF-8 name and extension: must be storable, searchable
                // and removable by its exact bytes.
                (b"re\xffport.l\xfeg", b"/logs/re\xffport.l\xfeg", 77),
            ];
            for (name, path, size) in &entries {
                let name = Path::new(*name);
                let e = IndexEntry {
                    path: Path::new(*path).to_path_buf(),
                    name: name.to_path_buf(),
                    name_lower: to_ascii_lower(name.as_bytes()),
                    extension: extract_extension(name),
                    entry_type: super::vfs::EntryType::File,
                    size: *size,
                    modified_ns: 0,
                };
                let idx_pos = idx.entries.len();
                if !e.extension.is_empty() {
                    idx.by_extension
                        .entry(e.extension.clone())
                        .or_default()
                        .push(idx_pos);
                }
                idx.entries.push(e);
            }
            idx.stats.total_entries = idx.entries.len();
            idx.stats.total_size = idx.entries.iter().map(|e| e.size).sum();
            idx.stats.extension_count = idx.by_extension.len();
        }

        // search_name (case-insensitive)
        let r1 = search_name("hello");
        if r1.len() != 1
            || r1.first().map(|e| e.path.as_path()) != Some(Path::new("/docs/hello.txt"))
        {
            serial_println!("[index]   ERROR: search_name('hello') failed");
            return Err(KernelError::InternalError);
        }
        let r2 = search_name("CASETEST");
        if r2.is_empty() {
            serial_println!("[index]   ERROR: case-insensitive name search failed");
            return Err(KernelError::InternalError);
        }

        // search_ext
        let r3 = search_ext("txt");
        if r3.len() != 1 {
            serial_println!(
                "[index]   ERROR: search_ext('txt') expected 1, got {}",
                r3.len()
            );
            return Err(KernelError::InternalError);
        }
        let r4 = search_ext("rs");
        if r4.is_empty() {
            serial_println!("[index]   ERROR: search_ext('rs') should find .RS file");
            return Err(KernelError::InternalError);
        }
        let r5 = search_ext("jpg");
        if r5.len() != 1 {
            serial_println!(
                "[index]   ERROR: search_ext('jpg') expected 1, got {}",
                r5.len()
            );
            return Err(KernelError::InternalError);
        }

        // search_size
        let r6 = search_size(100, 2048);
        if r6.len() != 3 {
            serial_println!(
                "[index]   ERROR: search_size(100,2048) expected 3, got {}",
                r6.len()
            );
            return Err(KernelError::InternalError);
        }

        // combined search
        let r7 = search(Some(b"data"), None, Some(512), Some(2048));
        if r7.len() != 1 || r7.first().map(|e| e.path.as_path()) != Some(Path::new("/tmp/data.bin"))
        {
            serial_println!("[index]   ERROR: combined search failed");
            return Err(KernelError::InternalError);
        }

        // search_path
        let r8 = search_path("/docs/");
        if r8.len() != 1 {
            serial_println!(
                "[index]   ERROR: search_path('/docs/') expected 1, got {}",
                r8.len()
            );
            return Err(KernelError::InternalError);
        }

        // Non-UTF-8 entry: searchable by name bytes and by extension bytes.
        // A lossy decode would fold `\xff`/`\xfe` to U+FFFD and both of these
        // would then match the wrong thing (or nothing).
        let r9 = search_name(b"re\xffport".as_slice());
        if r9.len() != 1
            || r9.first().map(|e| e.path.as_path())
                != Some(Path::new(b"/logs/re\xffport.l\xfeg".as_slice()))
        {
            serial_println!("[index]   ERROR: non-UTF-8 name search failed");
            return Err(KernelError::InternalError);
        }
        if search_ext(b"l\xfeg".as_slice()).len() != 1 {
            serial_println!("[index]   ERROR: non-UTF-8 extension search failed");
            return Err(KernelError::InternalError);
        }
        // A name differing only in that one byte must NOT match.
        if !search_name(b"re\xfeport".as_slice()).is_empty() {
            serial_println!("[index]   ERROR: non-UTF-8 name search matched wrong byte");
            return Err(KernelError::InternalError);
        }

        // remove_entry
        let removed = remove_entry("/docs/hello.txt");
        if removed != 1 {
            serial_println!("[index]   ERROR: remove_entry expected 1, got {}", removed);
            return Err(KernelError::InternalError);
        }
        // Removing by exact bytes must find the non-UTF-8 entry — this is the
        // "you can delete a file you can name" property the whole byte-path
        // conversion exists for.
        if remove_entry(Path::new(b"/logs/re\xffport.l\xfeg".as_slice())) != 1 {
            serial_println!("[index]   ERROR: remove_entry on non-UTF-8 path failed");
            return Err(KernelError::InternalError);
        }
        if count() != 4 {
            serial_println!(
                "[index]   ERROR: count after remove expected 4, got {}",
                count()
            );
            return Err(KernelError::InternalError);
        }
        let gone = search_name("hello");
        if !gone.is_empty() {
            serial_println!("[index]   ERROR: removed entry still searchable");
            return Err(KernelError::InternalError);
        }

        serial_println!("[index]   in-memory add/search/remove OK");
    }

    // --- Test 4: clear ---
    {
        clear();
        if count() != 0 {
            serial_println!("[index]   ERROR: clear didn't empty index");
            return Err(KernelError::InternalError);
        }
        serial_println!("[index]   clear OK");
    }

    // --- Test 5: rebuild (VFS-dependent, best-effort) ---
    {
        rebuild()?;
        let st = stats();
        if st.rebuild_count < 1 {
            serial_println!("[index]   ERROR: rebuild_count not incremented");
            return Err(KernelError::InternalError);
        }
        if st.last_rebuild_ns.is_none() {
            serial_println!("[index]   ERROR: last_rebuild_ns still None after a rebuild");
            return Err(KernelError::InternalError);
        }
        // A rebuild that lands at uptime 0 is legal, and used to be
        // indistinguishable from "never rebuilt". Check the age accessor
        // agrees with the timestamp so the two cannot drift apart.
        if age_of_last_rebuild_ns().is_none() {
            serial_println!("[index]   ERROR: age_of_last_rebuild_ns disagrees with stats");
            return Err(KernelError::InternalError);
        }
        if st.total_entries == 0 {
            // No filesystem mounted — not an error, just limited test.
            serial_println!("[index]   rebuild OK (0 entries — no filesystem mounted)");
        } else {
            serial_println!(
                "[index]   rebuild OK ({} entries, {} bytes, {} extensions)",
                st.total_entries,
                st.total_size,
                st.extension_count
            );
        }
    }

    // --- Test 6: VFS add/search (only if /tmp is available) ---
    {
        let test_path = "/tmp/_idx_test_file.txt";
        let test_data = b"index self-test data";
        if super::Vfs::write_file(test_path, test_data).is_ok() {
            add_entry(test_path)?;
            let results = search_name("_idx_test");
            let found = results
                .iter()
                .any(|e| e.path.as_path() == Path::new(test_path));
            if !found {
                serial_println!("[index]   ERROR: VFS add_entry + search failed");
                let _ = super::Vfs::remove(test_path);
                return Err(KernelError::InternalError);
            }
            // Verify size.
            let size_ok = results.iter().any(|e| e.size == test_data.len() as u64);
            if !size_ok {
                serial_println!("[index]   ERROR: VFS entry size mismatch");
                let _ = super::Vfs::remove(test_path);
                return Err(KernelError::InternalError);
            }
            let removed = remove_entry(test_path);
            if removed == 0 {
                serial_println!("[index]   ERROR: VFS remove_entry returned 0");
                let _ = super::Vfs::remove(test_path);
                return Err(KernelError::InternalError);
            }
            let _ = super::Vfs::remove(test_path);
            serial_println!("[index]   VFS add/search/remove OK");
        } else {
            serial_println!("[index]   (skipped VFS tests: /tmp not mounted)");
        }
    }

    let final_stats = stats();
    serial_println!(
        "[index] Self-test passed ({} entries, {} rebuilds).",
        final_stats.total_entries,
        final_stats.rebuild_count,
    );

    Ok(())
}
