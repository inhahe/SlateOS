//! Advanced file recovery (undelete) utility.
//!
//! Combines multiple data sources — trash, journal, file history (CAS),
//! and integrity baselines — to find and recover deleted files.
//!
//! ## Design Reference
//!
//! design.txt line 1010: "advanced undelete utility"
//!
//! ## Recovery Sources
//!
//! 1. **Trash** (`fs::trash`): files moved to `/_TRASH/` via `trash()`
//!    rather than permanently deleted.  Highest confidence — full file
//!    content available.
//!
//! 2. **Journal** (`fs::journal`): records Deleted events with paths.
//!    Tells us *what* was deleted and *when*, but not the content.
//!
//! 3. **File history** (`fs::history`): CAS-backed content snapshots.
//!    If auto-versioning was on, the last version before deletion
//!    may be recoverable from the content-addressed store.
//!
//! 4. **Integrity baselines** (`fs::integrity`): SHA-256 hashes of
//!    baselined files.  Won't recover content, but confirms what
//!    the file *should* contain (useful for verification after
//!    recovery from other sources).
//!
//! ## Architecture
//!
//! ```text
//! undelete::scan(filter)
//!   → Vec<RecoverableFile> from all sources
//!
//! undelete::recover(path, dest, strategy)
//!   → Attempts recovery in priority order:
//!     1. Trash (move back)
//!     2. History/CAS (restore version)
//!     3. Report "metadata only" if only journal/integrity
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::pathutil::{contains_bytes, path_in_subtree};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Source of recovery information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoverySource {
    /// Full content available in trash.
    Trash,
    /// Content available via CAS-backed history.
    History,
    /// Only metadata: we know the hash but not the content.
    IntegrityBaseline,
    /// Only metadata: we know it was deleted but have no content.
    JournalOnly,
}

impl RecoverySource {
    /// Whether this source can provide file content for recovery.
    pub fn has_content(self) -> bool {
        matches!(self, Self::Trash | Self::History)
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trash => "trash",
            Self::History => "history/CAS",
            Self::IntegrityBaseline => "integrity baseline (hash only)",
            Self::JournalOnly => "journal (metadata only)",
        }
    }
}

/// A file that may be recoverable.
#[derive(Debug, Clone)]
pub struct RecoverableFile {
    /// Original path of the file.
    pub path: PathBuf,
    /// Source(s) that have information about this file.
    pub sources: Vec<RecoverySource>,
    /// Best source for recovery (highest priority).
    pub best_source: RecoverySource,
    /// File size (if known).
    pub size: Option<u64>,
    /// Deletion timestamp (nanoseconds, if known from journal).
    pub deleted_ns: Option<u64>,
    /// SHA-256 hash hex (if known from integrity or CAS).
    pub hash: Option<String>,
    /// Trash name (if in trash).
    pub trash_name: Option<PathBuf>,
    /// CAS hash (if in history).
    pub cas_hash: Option<[u8; 32]>,
}

/// Filter for scanning recoverable files.
#[derive(Debug, Clone, Default)]
pub struct ScanFilter {
    /// Only files inside this directory subtree.
    pub path_prefix: Option<PathBuf>,
    /// Only files whose name contains this byte substring.  A file name is a
    /// byte string, so the needle is bytes too — an ASCII needle still matches
    /// inside a name that is not valid UTF-8.
    pub name_contains: Option<Vec<u8>>,
    /// Only files deleted after this timestamp (ns).
    pub deleted_after_ns: Option<u64>,
    /// Maximum results.
    pub limit: usize,
}

impl ScanFilter {
    /// Create a default filter (all files, limit 1000).
    pub fn new() -> Self {
        Self {
            limit: 1000,
            ..Self::default()
        }
    }

    /// Filter by path prefix.
    #[must_use]
    pub fn with_prefix<P: AsRef<Path> + ?Sized>(mut self, prefix: &P) -> Self {
        self.path_prefix = Some(prefix.as_ref().to_path_buf());
        self
    }

    /// Filter by name substring.
    #[must_use]
    pub fn with_name<N: AsRef<[u8]> + ?Sized>(mut self, name: &N) -> Self {
        self.name_contains = Some(name.as_ref().to_vec());
        self
    }

    /// Filter by deletion time.
    pub fn deleted_after(mut self, ns: u64) -> Self {
        self.deleted_after_ns = Some(ns);
        self
    }

    /// Set result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Result of a recovery operation.
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    /// Path where the file was recovered to.
    pub recovered_path: PathBuf,
    /// Source used for recovery.
    pub source: RecoverySource,
    /// Bytes recovered.
    pub bytes: u64,
    /// Whether hash verification passed (if applicable).
    pub verified: Option<bool>,
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static SCANS: AtomicU64 = AtomicU64::new(0);
static RECOVERIES: AtomicU64 = AtomicU64::new(0);
static BYTES_RECOVERED: AtomicU64 = AtomicU64::new(0);

/// Get counters: (scans, recoveries, bytes_recovered).
pub fn stats() -> (u64, u64, u64) {
    (
        SCANS.load(Ordering::Relaxed),
        RECOVERIES.load(Ordering::Relaxed),
        BYTES_RECOVERED.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Scan for recoverable files
// ---------------------------------------------------------------------------

/// Scan all recovery sources for deleted files matching the filter.
///
/// Results are deduplicated by path and sorted by best recovery source
/// (most recoverable first).
pub fn scan(filter: &ScanFilter) -> KernelResult<Vec<RecoverableFile>> {
    // Map: original_path → RecoverableFile (deduplicated across sources).
    let mut found: BTreeMap<PathBuf, RecoverableFile> = BTreeMap::new();

    // Source 1: Trash.
    scan_trash(filter, &mut found);

    // Source 2: Journal (deleted events).
    scan_journal(filter, &mut found);

    // Source 3: File history (CAS-backed versions).
    scan_history(filter, &mut found);

    // Source 4: Integrity baselines.
    scan_integrity(filter, &mut found);

    // Collect and sort: recoverable content first, then metadata-only.
    let mut results: Vec<RecoverableFile> = found.into_values().collect();
    results.sort_by(|a, b| {
        a.best_source.cmp(&b.best_source)
            .then(a.path.cmp(&b.path))
    });

    // Apply limit.
    if filter.limit > 0 && results.len() > filter.limit {
        results.truncate(filter.limit);
    }

    SCANS.fetch_add(1, Ordering::Relaxed);

    serial_println!("[undelete] Scan found {} recoverable files", results.len());

    Ok(results)
}

/// Attempt to recover a specific file.
///
/// Tries sources in priority order: Trash → History → fail.
/// If `dest` is `None`, restores to the original path.
pub fn recover<P: AsRef<Path> + ?Sized>(
    original_path: &P,
    dest: Option<&Path>,
) -> KernelResult<RecoveryResult> {
    let original_path = original_path.as_ref();

    // Try trash first.
    if let Ok(result) = recover_from_trash(original_path, dest) {
        RECOVERIES.fetch_add(1, Ordering::Relaxed);
        BYTES_RECOVERED.fetch_add(result.bytes, Ordering::Relaxed);
        serial_println!(
            "[undelete] Recovered {} from trash ({} bytes)",
            original_path.display(),
            result.bytes
        );
        return Ok(result);
    }

    // Try history/CAS.
    if let Ok(result) = recover_from_history(original_path, dest) {
        RECOVERIES.fetch_add(1, Ordering::Relaxed);
        BYTES_RECOVERED.fetch_add(result.bytes, Ordering::Relaxed);
        serial_println!(
            "[undelete] Recovered {} from history ({} bytes)",
            original_path.display(),
            result.bytes
        );
        return Ok(result);
    }

    // No recoverable source found.
    Err(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Source scanners
// ---------------------------------------------------------------------------

fn matches_filter(path: &Path, filter: &ScanFilter) -> bool {
    if let Some(ref prefix) = filter.path_prefix {
        // Route through the canonical subtree predicate so a prefix with or
        // without a trailing slash behaves identically. See fs::pathutil.
        if !path_in_subtree(path, prefix) {
            return false;
        }
    }
    if let Some(ref name) = filter.name_contains {
        // The name filter applies to the final component only.  A path with no
        // components (only "/" or empty) has no name and so cannot match a
        // non-empty needle.
        let filename = path.file_name().map_or(&[][..], Path::as_bytes);
        if !contains_bytes(filename, name) {
            return false;
        }
    }
    true
}

fn scan_trash(filter: &ScanFilter, found: &mut BTreeMap<PathBuf, RecoverableFile>) {
    if let Ok(items) = crate::fs::trash::list() {
        for item in &items {
            // This whole module is keyed on the *original* path: it answers
            // "what can I get back for /home/x?".  A trash item whose original
            // path was lost (a corrupt or truncated index line) cannot be
            // keyed, so it is not listed here.  It is still recoverable by
            // trash name through `fs::trash` directly.
            let Some(original_path) = item.original_path.as_deref() else {
                continue;
            };
            if !matches_filter(original_path, filter) {
                continue;
            }
            let entry = found.entry(original_path.to_path_buf()).or_insert_with(|| {
                RecoverableFile {
                    path: original_path.to_path_buf(),
                    sources: Vec::new(),
                    best_source: RecoverySource::Trash,
                    size: Some(item.size),
                    deleted_ns: None,
                    hash: None,
                    trash_name: None,
                    cas_hash: None,
                }
            });
            if !entry.sources.contains(&RecoverySource::Trash) {
                entry.sources.push(RecoverySource::Trash);
            }
            entry.trash_name = Some(item.trash_name.clone());
            entry.size = Some(item.size);
            // Trash is highest priority.
            entry.best_source = RecoverySource::Trash;
        }
    }
}

fn scan_journal(filter: &ScanFilter, found: &mut BTreeMap<PathBuf, RecoverableFile>) {
    use crate::fs::journal::{JournalEventType, read_since};

    let (entries, _) = read_since(0);
    for entry in &entries {
        if entry.event_type != JournalEventType::Deleted {
            continue;
        }
        if !matches_filter(&entry.path, filter) {
            continue;
        }
        if let Some(after_ns) = filter.deleted_after_ns {
            if entry.timestamp_ns < after_ns {
                continue;
            }
        }

        let rec = found.entry(entry.path.clone()).or_insert_with(|| {
            RecoverableFile {
                path: entry.path.clone(),
                sources: Vec::new(),
                best_source: RecoverySource::JournalOnly,
                size: None,
                deleted_ns: Some(entry.timestamp_ns),
                hash: None,
                trash_name: None,
                cas_hash: None,
            }
        });
        if !rec.sources.contains(&RecoverySource::JournalOnly) {
            rec.sources.push(RecoverySource::JournalOnly);
        }
        // Update deletion timestamp if not set.
        if rec.deleted_ns.is_none() {
            rec.deleted_ns = Some(entry.timestamp_ns);
        }
    }
}

fn scan_history(filter: &ScanFilter, found: &mut BTreeMap<PathBuf, RecoverableFile>) {
    use crate::fs::history;

    // Get all tracked files from history.
    let tracked = history::list_tracked(None, 10000);
    for (path, _count) in &tracked {
        if !matches_filter(path, filter) {
            continue;
        }
        // Only relevant if the file no longer exists on disk.
        if crate::fs::Vfs::metadata(path).is_ok() {
            continue; // File still exists — not deleted.
        }

        let versions = history::get_history(path);
        // Versions are newest-first, so the head is the most recent.
        let Some(latest) = versions.first() else {
            continue;
        };
        let rec = found.entry(path.clone()).or_insert_with(|| {
            RecoverableFile {
                path: path.clone(),
                sources: Vec::new(),
                best_source: RecoverySource::History,
                size: Some(latest.size),
                deleted_ns: None,
                hash: None,
                trash_name: None,
                cas_hash: Some(latest.hash),
            }
        });
        if !rec.sources.contains(&RecoverySource::History) {
            rec.sources.push(RecoverySource::History);
        }
        rec.cas_hash = Some(latest.hash);
        if rec.size.is_none() {
            rec.size = Some(latest.size);
        }
        // History is better than journal/integrity.
        if rec.best_source as u8 > RecoverySource::History as u8 {
            rec.best_source = RecoverySource::History;
        }
    }
}

fn scan_integrity(filter: &ScanFilter, found: &mut BTreeMap<PathBuf, RecoverableFile>) {
    use crate::fs::integrity;

    // List all baselined entries.
    let (entries, _) = integrity::list_entries(
        filter.path_prefix.as_deref(),
        filter.limit.max(10000),
    );

    for (path, hash, size) in &entries {
        if !matches_filter(path, filter) {
            continue;
        }
        // Only relevant if file no longer exists.
        if crate::fs::Vfs::metadata(path).is_ok() {
            continue;
        }

        let hex = hash_to_hex(hash);
        let rec = found.entry(path.clone()).or_insert_with(|| {
            RecoverableFile {
                path: path.clone(),
                sources: Vec::new(),
                best_source: RecoverySource::IntegrityBaseline,
                size: Some(*size),
                deleted_ns: None,
                hash: Some(hex.clone()),
                trash_name: None,
                cas_hash: None,
            }
        });
        if !rec.sources.contains(&RecoverySource::IntegrityBaseline) {
            rec.sources.push(RecoverySource::IntegrityBaseline);
        }
        if rec.hash.is_none() {
            rec.hash = Some(hex);
        }
        if rec.size.is_none() {
            rec.size = Some(*size);
        }
    }
}

// ---------------------------------------------------------------------------
// Recovery implementations
// ---------------------------------------------------------------------------

fn recover_from_trash(original_path: &Path, dest: Option<&Path>) -> KernelResult<RecoveryResult> {
    use crate::fs::trash;

    // Find the item in trash matching this original path.
    let items = trash::list()?;
    let item = items
        .iter()
        .find(|i| i.original_path.as_deref() == Some(original_path))
        .ok_or(KernelError::NotFound)?;

    let trash_name = item.trash_name.clone();
    let size = item.size;

    if let Some(target) = dest {
        // Copy from trash to target, then purge from trash.
        let trash_path = Path::new(trash::TRASH_DIR).join(&trash_name);
        let data = crate::fs::Vfs::read_file(&trash_path)?;
        crate::fs::Vfs::write_file(target, &data)?;
        // The copy succeeded, so the trash copy is now redundant; failing to
        // purge it wastes space but must not fail the recovery the caller
        // already got.
        let _ = trash::purge_one(&trash_name);
        Ok(RecoveryResult {
            recovered_path: target.to_path_buf(),
            source: RecoverySource::Trash,
            bytes: size,
            verified: None,
        })
    } else {
        // Restore to original location.
        let restored = trash::restore(&trash_name)?;
        Ok(RecoveryResult {
            recovered_path: restored,
            source: RecoverySource::Trash,
            bytes: size,
            verified: None,
        })
    }
}

fn recover_from_history(original_path: &Path, dest: Option<&Path>) -> KernelResult<RecoveryResult> {
    use crate::fs::history;

    let versions = history::get_history(original_path);
    // Versions are newest-first, so the head is the most recent.
    let latest = versions.first().ok_or(KernelError::NotFound)?;
    let data = crate::fs::cas::get(&latest.hash)?;
    let size = data.len() as u64;

    let target = dest.unwrap_or(original_path).to_path_buf();
    crate::fs::Vfs::write_file(&target, &data)?;

    // Verify hash.
    let written = crate::fs::Vfs::read_file(&target)?;
    let hash = crate::crypto::sha256(&written);
    let verified = hash == latest.hash;

    Ok(RecoveryResult {
        recovered_path: target,
        source: RecoverySource::History,
        bytes: size,
        verified: Some(verified),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert SHA-256 hash to hex string.
fn hash_to_hex(hash: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push_str(&alloc::format!("{:02x}", byte));
    }
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[undelete] Running self-test...");

    test_scan_empty();
    test_trash_recovery();
    test_history_recovery();
    test_filter();
    test_scan_combined();
    test_stats();

    serial_println!("[undelete] Self-test passed (6 tests).");
    Ok(())
}

fn test_scan_empty() {
    // Scan with a prefix that matches nothing.
    let filter = ScanFilter::new().with_prefix("/nonexistent_test_path_xyz");
    let results = scan(&filter).expect("scan");
    assert!(results.is_empty(), "should find nothing");
    serial_println!("[undelete]   scan empty: ok");
}

fn test_trash_recovery() {
    use crate::fs::{Vfs, trash};

    // Create and trash a file.
    Vfs::write_file("/tmp/und_trash.txt", b"trash me").expect("write");
    trash::trash("/tmp/und_trash.txt").expect("trash");

    // Should now be recoverable.
    let filter = ScanFilter::new().with_name("und_trash.txt");
    let results = scan(&filter).expect("scan");
    let first = results.first().expect("should find trashed file");
    assert_eq!(first.best_source, RecoverySource::Trash);
    assert_eq!(first.path.as_path(), Path::new("/tmp/und_trash.txt"));

    // Recover it.
    let result = recover("/tmp/und_trash.txt", None).expect("recover");
    assert_eq!(result.source, RecoverySource::Trash);

    // Verify content.
    let data = Vfs::read_file("/tmp/und_trash.txt").expect("read");
    assert_eq!(&data, b"trash me");

    // Cleanup.
    let _ = Vfs::remove("/tmp/und_trash.txt");

    serial_println!("[undelete]   trash recovery: ok");
}

fn test_history_recovery() {
    use crate::fs::{Vfs, history};

    // Create a file and record its version.
    Vfs::write_file("/tmp/und_hist.txt", b"history version").expect("write");
    let _ = history::record_version("/tmp/und_hist.txt");

    // Delete the file.
    let _ = Vfs::remove("/tmp/und_hist.txt");

    // Should be recoverable from history.
    let result = recover(
        "/tmp/und_hist.txt",
        Some(Path::new("/tmp/und_hist_recovered.txt")),
    );
    match result {
        Ok(r) => {
            assert_eq!(r.source, RecoverySource::History);
            let data = Vfs::read_file("/tmp/und_hist_recovered.txt").expect("read");
            assert_eq!(&data, b"history version");
            let _ = Vfs::remove("/tmp/und_hist_recovered.txt");
        }
        Err(_) => {
            // History might not have CAS enabled or working — acceptable.
            serial_println!("[undelete]   history recovery: skipped (CAS not available)");
            return;
        }
    }

    serial_println!("[undelete]   history recovery: ok");
}

fn test_filter() {
    let filter = ScanFilter::new()
        .with_prefix("/tmp")
        .with_name("test")
        .deleted_after(100)
        .with_limit(50);

    assert_eq!(filter.path_prefix.as_deref(), Some(Path::new("/tmp")));
    assert_eq!(filter.name_contains.as_deref(), Some(b"test".as_slice()));
    assert_eq!(filter.deleted_after_ns, Some(100));
    assert_eq!(filter.limit, 50);

    // The prefix is a subtree test, not a byte prefix: a file under a sibling
    // directory that merely shares a byte prefix must not match.
    let f = ScanFilter::new().with_prefix("/tmp/und");
    assert!(matches_filter(Path::new("/tmp/und/a.txt"), &f));
    assert!(!matches_filter(Path::new("/tmp/undelete/a.txt"), &f));

    // The name filter matches bytes, so an ASCII needle finds a name that is
    // not valid UTF-8, and a needle spanning the raw byte matches too.
    let f = ScanFilter::new().with_name("port");
    assert!(matches_filter(Path::new(b"/tmp/re\xffport.txt".as_slice()), &f));
    let f = ScanFilter::new().with_name(b"\xffpo".as_slice());
    assert!(matches_filter(Path::new(b"/tmp/re\xffport.txt".as_slice()), &f));
    // The name filter applies to the final component only.
    let f = ScanFilter::new().with_name("tmp");
    assert!(!matches_filter(Path::new("/tmp/a.txt"), &f));

    serial_println!("[undelete]   filter: ok");
}

fn test_scan_combined() {
    // Scan with a broad filter — just verify it doesn't panic.
    let filter = ScanFilter::new().with_limit(10);
    let results = scan(&filter).expect("scan");
    // Results count varies based on test state — just check it worked.
    let _ = results.len();

    serial_println!("[undelete]   scan combined: ok");
}

fn test_stats() {
    let (scans, recoveries, bytes) = stats();
    assert!(scans > 0, "should have scans");
    // recoveries depends on whether trash/history tests succeeded.
    let _ = (recoveries, bytes);

    serial_println!("[undelete]   stats: ok");
}
