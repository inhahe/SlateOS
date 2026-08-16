//! Incremental backup engine.
//!
//! Provides full and incremental filesystem backups with manifests,
//! integrity verification, and point-in-time restore.
//!
//! ## Design Reference
//!
//! design.txt line 997: "backup program"
//!
//! ## Architecture
//!
//! ```text
//! backup::create("/data", "/backup", &opts)
//!   1. Walk source tree, collect file metadata + SHA-256 hashes
//!   2. If incremental: load previous manifest, diff against current
//!   3. Copy changed/new files to backup destination
//!   4. Write manifest (JSON-lines) to destination
//!   → BackupResult { files_copied, bytes_copied, ... }
//!
//! backup::restore("/backup", "/data", manifest_id, &opts)
//!   1. Load manifest
//!   2. Copy files from backup to destination
//!   3. Optionally verify hashes after copy
//!   → RestoreResult { files_restored, bytes_restored, ... }
//! ```
//!
//! ## Manifest Format (JSON-lines)
//!
//! Each backup writes a `.manifest` file containing one JSON object
//! per line:
//!
//! ```text
//! {"type":"header","id":"20240101_120000","src":"/data","mode":"full","timestamp_ns":...}
//! {"type":"file","path":"/foo.txt","size":1234,"modified_ns":...,"hash":"abcd..."}
//! {"type":"dir","path":"/subdir"}
//! {"type":"footer","files":42,"bytes":123456,"duration_ns":...}
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recursion depth when walking trees.
const MAX_DEPTH: usize = 32;

/// Maximum files per backup.
const MAX_FILES: usize = 100_000;

/// Manifest file extension.
const MANIFEST_EXT: &str = ".manifest";

/// Manifest format version.
///
/// Version 2 escapes every path field (see [`esc`]) and stores entry paths
/// genuinely relative to the backup root, with no leading separator.  A
/// version-1 manifest is rejected rather than parsed: its entry paths look
/// absolute (`/sub/file.txt`), so joining one onto the restore destination
/// replaces that destination and writes back over the *original* location.
/// A loud failure is the only safe reading of a record we know we would
/// misinterpret.
const MANIFEST_VERSION: u64 = 2;

/// Maximum manifest size to load (4 MiB).
const MAX_MANIFEST_SIZE: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Backup mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupMode {
    /// Copy all files regardless of changes.
    Full,
    /// Only copy files changed since last backup.
    Incremental,
}

/// A single file entry in a manifest.
#[derive(Debug, Clone)]
pub struct ManifestEntry {
    /// Path relative to the backup root, with no leading separator.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Last modified timestamp (nanoseconds).
    pub modified_ns: u64,
    /// SHA-256 hash (hex).
    pub hash: String,
    /// Entry type: "file" or "dir".
    pub entry_type: String,
}

/// A loaded backup manifest.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Unique backup identifier (timestamp-based).
    pub id: String,
    /// Source path that was backed up.
    pub source: PathBuf,
    /// Backup mode.
    pub mode: BackupMode,
    /// Creation timestamp (nanoseconds).
    pub timestamp_ns: u64,
    /// File entries.
    pub entries: Vec<ManifestEntry>,
    /// Total files.
    pub file_count: u64,
    /// Total bytes.
    pub total_bytes: u64,
}

/// Options for backup creation.
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// Backup mode.
    pub mode: BackupMode,
    /// Verify source file hashes (slower but ensures integrity).
    pub verify: bool,
    /// Dry run — report what would be done without copying.
    pub dry_run: bool,
    /// Maximum depth to recurse.
    pub max_depth: usize,
    /// Subtrees to exclude, named by their *absolute* source path.
    ///
    /// Matching is by component, via [`crate::fs::pathutil::path_in_subtree`],
    /// against the real path being visited - which is what the operator sees
    /// and types.  The previous byte-prefix test against the relative path
    /// meant excluding `/a` also excluded `/ab`.
    pub exclude: Vec<PathBuf>,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            mode: BackupMode::Full,
            verify: true,
            dry_run: false,
            max_depth: MAX_DEPTH,
            exclude: Vec::new(),
        }
    }
}

/// Options for backup restoration.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Verify hashes after copying.
    pub verify: bool,
    /// Dry run — report what would be done.
    pub dry_run: bool,
    /// Only restore these subtrees (empty = all).
    ///
    /// Named *relative to the backup root*, since that is the only form the
    /// manifest records; matched by component, not by byte prefix.
    pub filter_paths: Vec<PathBuf>,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            verify: true,
            dry_run: false,
            filter_paths: Vec::new(),
        }
    }
}

/// Result of a backup operation.
#[derive(Debug, Clone, Default)]
pub struct BackupResult {
    /// Manifest ID for this backup.
    pub manifest_id: String,
    /// Files copied.
    pub files_copied: u64,
    /// Files skipped (unchanged in incremental mode).
    pub files_skipped: u64,
    /// Directories created in destination.
    pub dirs_created: u64,
    /// Bytes copied.
    pub bytes_copied: u64,
    /// Non-fatal errors.
    pub errors: Vec<String>,
}

/// Result of a restore operation.
#[derive(Debug, Clone, Default)]
pub struct RestoreResult {
    /// Files restored.
    pub files_restored: u64,
    /// Directories created.
    pub dirs_created: u64,
    /// Bytes restored.
    pub bytes_restored: u64,
    /// Hash verification failures.
    pub verify_failures: u64,
    /// Non-fatal errors.
    pub errors: Vec<String>,
}

/// Summary of a backup in the destination.
#[derive(Debug, Clone)]
pub struct BackupInfo {
    /// Manifest ID.
    pub id: String,
    /// Source path.
    pub source: PathBuf,
    /// Backup mode.
    pub mode: BackupMode,
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// File count.
    pub file_count: u64,
    /// Total bytes.
    pub total_bytes: u64,
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static BACKUPS_CREATED: AtomicU64 = AtomicU64::new(0);
static RESTORES_DONE: AtomicU64 = AtomicU64::new(0);
static BYTES_BACKED_UP: AtomicU64 = AtomicU64::new(0);

/// Get counters: (backups_created, restores_done, bytes_backed_up).
pub fn stats() -> (u64, u64, u64) {
    (
        BACKUPS_CREATED.load(Ordering::Relaxed),
        RESTORES_DONE.load(Ordering::Relaxed),
        BYTES_BACKED_UP.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Create backup
// ---------------------------------------------------------------------------

/// Create a backup of `src` into `dst`.
///
/// For full backups, copies all files.  For incremental backups,
/// loads the most recent manifest from `dst` and only copies files
/// that have changed (different size, mtime, or hash).
pub fn create<S: AsRef<Path> + ?Sized, D: AsRef<Path> + ?Sized>(
    src: &S,
    dst: &D,
    opts: &BackupOptions,
) -> KernelResult<BackupResult> {
    let (src, dst) = (src.as_ref(), dst.as_ref());
    // Generate a unique manifest ID from current timestamp.
    let now_ns = crate::timekeeping::clock_monotonic();
    let manifest_id = generate_id(now_ns);
    let mut result = BackupResult {
        manifest_id: manifest_id.clone(),
        ..BackupResult::default()
    };

    // Ensure destination exists.
    if !opts.dry_run {
        let _ = Vfs::mkdir(dst); // Ignore AlreadyExists.
    }

    // Collect source tree.
    let mut source_entries = Vec::new();
    collect_entries(
        src,
        src,
        &mut source_entries,
        0,
        opts.max_depth,
        &opts.exclude,
    )?;

    // Load previous manifest for incremental mode.
    let prev_manifest = if opts.mode == BackupMode::Incremental {
        load_latest_manifest(dst).ok()
    } else {
        None
    };

    // Build lookup of previous entries by path for quick comparison.
    let prev_index: BTreeMap<&Path, &ManifestEntry> = if let Some(ref m) = prev_manifest {
        m.entries
            .iter()
            .filter(|e| e.entry_type == "file")
            .map(|e| (e.path.as_path(), e))
            .collect()
    } else {
        BTreeMap::new()
    };

    // Create backup subdirectory for this run.
    let backup_dir = dst.join(&manifest_id);
    if !opts.dry_run {
        Vfs::mkdir(&backup_dir)
            .inspect_err(|&e| if matches!(e, KernelError::AlreadyExists) {})
            .or_else(|e| {
                if matches!(e, KernelError::AlreadyExists) {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
    }

    // Process each entry.
    let mut manifest_entries = Vec::new();

    for entry in &source_entries {
        if entry.entry_type == "dir" {
            // Create directory in backup.  `entry.path` is relative, so
            // `Path::join` appends it rather than replacing the backup root.
            let dst_path = backup_dir.join(&entry.path);
            if !opts.dry_run {
                match Vfs::mkdir(&dst_path) {
                    Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
                    Err(KernelError::AlreadyExists) => {}
                    Err(e) => {
                        result
                            .errors
                            .push(alloc::format!("mkdir {}: {:?}", dst_path.display(), e));
                        continue;
                    }
                }
            } else {
                result.dirs_created = result.dirs_created.saturating_add(1);
            }
            manifest_entries.push(entry.clone());
            continue;
        }

        // File: check if it changed (incremental mode).
        let should_copy = if opts.mode == BackupMode::Incremental {
            if let Some(prev) = prev_index.get(entry.path.as_path()) {
                // Changed if size, mtime, or hash differ.
                prev.size != entry.size
                    || prev.modified_ns != entry.modified_ns
                    || prev.hash != entry.hash
            } else {
                true // New file, not in previous backup.
            }
        } else {
            true // Full backup: always copy.
        };

        if !should_copy {
            result.files_skipped = result.files_skipped.saturating_add(1);
            // Still record in manifest (with current metadata).
            manifest_entries.push(entry.clone());
            continue;
        }

        // Copy file.
        let src_path = src.join(&entry.path);
        let dst_path = backup_dir.join(&entry.path);

        if opts.dry_run {
            result.files_copied = result.files_copied.saturating_add(1);
            result.bytes_copied = result.bytes_copied.saturating_add(entry.size);
        } else {
            match Vfs::copy(&src_path, &dst_path) {
                Ok(bytes) => {
                    result.files_copied = result.files_copied.saturating_add(1);
                    result.bytes_copied = result.bytes_copied.saturating_add(bytes);
                }
                Err(e) => {
                    result
                        .errors
                        .push(alloc::format!("copy {}: {:?}", entry.path.display(), e));
                    continue;
                }
            }
        }

        manifest_entries.push(entry.clone());
    }

    // Write manifest.
    if !opts.dry_run {
        let manifest_path = dst.join(alloc::format!("{}{}", manifest_id, MANIFEST_EXT));
        let manifest_data = serialize_manifest(
            &manifest_id,
            src,
            opts.mode,
            now_ns,
            &manifest_entries,
            result.bytes_copied,
        );
        Vfs::write_file(&manifest_path, manifest_data.as_bytes())?;
    }

    BACKUPS_CREATED.fetch_add(1, Ordering::Relaxed);
    BYTES_BACKED_UP.fetch_add(result.bytes_copied, Ordering::Relaxed);

    serial_println!(
        "[backup] Created {}: {} files copied, {} skipped, {} bytes, {} errors",
        manifest_id,
        result.files_copied,
        result.files_skipped,
        result.bytes_copied,
        result.errors.len(),
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// Restore backup
// ---------------------------------------------------------------------------

/// Restore a backup from `backup_root` to `dst`.
///
/// If `manifest_id` is `None`, restores the latest backup.
pub fn restore<R: AsRef<Path> + ?Sized, D: AsRef<Path> + ?Sized>(
    backup_root: &R,
    dst: &D,
    manifest_id: Option<&str>,
    opts: &RestoreOptions,
) -> KernelResult<RestoreResult> {
    let (backup_root, dst) = (backup_root.as_ref(), dst.as_ref());
    // Load manifest.
    let manifest = if let Some(id) = manifest_id {
        load_manifest(&manifest_path_for(backup_root, id)?)?
    } else {
        load_latest_manifest(backup_root)?
    };

    let mut result = RestoreResult::default();

    // Ensure destination exists.
    if !opts.dry_run {
        let _ = Vfs::mkdir(dst);
    }

    let backup_dir = backup_root.join(&manifest.id);

    for entry in &manifest.entries {
        // Apply path filter if set.  Component-aligned, so restoring only
        // `docs` does not also restore `docsets`.
        if !opts.filter_paths.is_empty()
            && !opts
                .filter_paths
                .iter()
                .any(|p| crate::fs::pathutil::path_in_subtree(&entry.path, p))
        {
            continue;
        }

        // `entry.path` was checked at parse time to be relative and free of
        // `..`, so this join cannot escape `dst`.
        let dst_path = dst.join(&entry.path);

        if entry.entry_type == "dir" {
            if !opts.dry_run {
                match Vfs::mkdir(&dst_path) {
                    Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
                    Err(KernelError::AlreadyExists) => {}
                    Err(e) => {
                        result
                            .errors
                            .push(alloc::format!("mkdir {}: {:?}", dst_path.display(), e));
                    }
                }
            } else {
                result.dirs_created = result.dirs_created.saturating_add(1);
            }
            continue;
        }

        // Copy file from backup.
        let backup_file = backup_dir.join(&entry.path);

        if opts.dry_run {
            result.files_restored = result.files_restored.saturating_add(1);
            result.bytes_restored = result.bytes_restored.saturating_add(entry.size);
            continue;
        }

        match Vfs::copy(&backup_file, &dst_path) {
            Ok(bytes) => {
                result.files_restored = result.files_restored.saturating_add(1);
                result.bytes_restored = result.bytes_restored.saturating_add(bytes);

                // Verify hash if requested.
                if opts.verify && !entry.hash.is_empty() {
                    if let Ok(data) = Vfs::read_file(&dst_path) {
                        let hash = crate::crypto::sha256(&data);
                        let hex = hash_to_hex(&hash);
                        if hex != entry.hash {
                            result.verify_failures = result.verify_failures.saturating_add(1);
                            result.errors.push(alloc::format!(
                                "verify {}: expected {}, got {}",
                                entry.path.display(),
                                entry.hash,
                                hex,
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                result
                    .errors
                    .push(alloc::format!("restore {}: {:?}", entry.path.display(), e));
            }
        }
    }

    RESTORES_DONE.fetch_add(1, Ordering::Relaxed);

    serial_println!(
        "[backup] Restored {}: {} files, {} bytes, {} verify failures, {} errors",
        manifest.id,
        result.files_restored,
        result.bytes_restored,
        result.verify_failures,
        result.errors.len(),
    );

    Ok(result)
}

// ---------------------------------------------------------------------------
// List backups
// ---------------------------------------------------------------------------

/// List all backups in a backup root directory.
pub fn list<R: AsRef<Path> + ?Sized>(backup_root: &R) -> KernelResult<Vec<BackupInfo>> {
    let backup_root = backup_root.as_ref();
    let entries = Vfs::readdir(backup_root)?;
    let mut backups = Vec::new();

    for entry in &entries {
        if is_manifest_name(&entry.name) {
            let path = backup_root.join(&entry.name);
            if let Ok(manifest) = load_manifest(&path) {
                backups.push(BackupInfo {
                    id: manifest.id,
                    source: manifest.source,
                    mode: manifest.mode,
                    timestamp_ns: manifest.timestamp_ns,
                    file_count: manifest.file_count,
                    total_bytes: manifest.total_bytes,
                });
            }
        }
    }

    // Sort by timestamp (newest first).
    backups.sort_by_key(|e| core::cmp::Reverse(e.timestamp_ns));

    Ok(backups)
}

/// Verify a backup's integrity by checking file hashes.
pub fn verify<R: AsRef<Path> + ?Sized>(
    backup_root: &R,
    manifest_id: Option<&str>,
) -> KernelResult<(u64, u64, Vec<String>)> {
    let backup_root = backup_root.as_ref();
    let manifest = if let Some(id) = manifest_id {
        load_manifest(&manifest_path_for(backup_root, id)?)?
    } else {
        load_latest_manifest(backup_root)?
    };

    let backup_dir = backup_root.join(&manifest.id);
    let mut ok_count: u64 = 0;
    let mut fail_count: u64 = 0;
    let mut failures = Vec::new();

    for entry in &manifest.entries {
        if entry.entry_type != "file" || entry.hash.is_empty() {
            continue;
        }

        let file_path = backup_dir.join(&entry.path);
        match Vfs::read_file(&file_path) {
            Ok(data) => {
                let hash = crate::crypto::sha256(&data);
                let hex = hash_to_hex(&hash);
                if hex == entry.hash {
                    ok_count = ok_count.saturating_add(1);
                } else {
                    fail_count = fail_count.saturating_add(1);
                    failures.push(alloc::format!(
                        "{}: expected {}, got {}",
                        entry.path.display(),
                        entry.hash,
                        hex,
                    ));
                }
            }
            Err(e) => {
                fail_count = fail_count.saturating_add(1);
                failures.push(alloc::format!(
                    "{}: read error: {:?}",
                    entry.path.display(),
                    e
                ));
            }
        }
    }

    serial_println!(
        "[backup] Verify {}: {} ok, {} failed",
        manifest.id,
        ok_count,
        fail_count,
    );

    Ok((ok_count, fail_count, failures))
}

// ---------------------------------------------------------------------------
// Manifest I/O
// ---------------------------------------------------------------------------

/// Generate a manifest ID from a timestamp.
fn generate_id(ns: u64) -> String {
    // Convert nanoseconds to a readable timestamp-like ID.
    // Format: bkp_<seconds>_<subsecond>
    let secs = ns / 1_000_000_000;
    let sub = (ns % 1_000_000_000) / 1_000_000; // milliseconds
    alloc::format!("bkp_{}_{:03}", secs, sub)
}

/// Escape a path for a `|`-delimited, line-oriented record.
///
/// `|` and newline are both perfectly legal bytes in a filename here (only `/`
/// and NUL are not), so a path written verbatim can split a record in two or
/// synthesise an entire new one.  That is not merely a display problem: on
/// restore a forged `F|` record names the destination to write to, so an
/// attacker who can choose a filename in the source tree could choose where
/// the restore writes.  The escaped form is pure ASCII and round-trips
/// exactly - see [`crate::fs::escape`].
fn esc(bytes: &[u8]) -> String {
    crate::fs::escape::escape_octal(bytes, b"|")
}

/// Serialize a manifest to string (simple delimited line format).
///
/// Uses a simple text format instead of JSON to avoid needing a JSON
/// library in no_std. Format is one entry per line, with every path field
/// octal-escaped:
///
/// ```text
/// V|<version>
/// H|<id>|<src>|<mode>|<timestamp_ns>
/// D|<rel_path>
/// F|<rel_path>|<size>|<modified_ns>|<hash_hex>
/// T|<file_count>|<total_bytes>
/// ```
fn serialize_manifest(
    id: &str,
    src: &Path,
    mode: BackupMode,
    timestamp_ns: u64,
    entries: &[ManifestEntry],
    total_bytes: u64,
) -> String {
    let mut out = String::new();

    // Version first: everything after it is read according to it, so a
    // manifest that announced its version late would be one we had already
    // misread.
    out.push_str(&alloc::format!("V|{}\n", MANIFEST_VERSION));

    // Header line.
    let mode_str = match mode {
        BackupMode::Full => "full",
        BackupMode::Incremental => "incr",
    };
    out.push_str(&alloc::format!(
        "H|{}|{}|{}|{}\n",
        esc(id.as_bytes()),
        esc(src.as_bytes()),
        mode_str,
        timestamp_ns,
    ));

    let mut file_count: u64 = 0;
    for entry in entries {
        if entry.entry_type == "dir" {
            out.push_str(&alloc::format!("D|{}\n", esc(entry.path.as_bytes())));
        } else {
            out.push_str(&alloc::format!(
                "F|{}|{}|{}|{}\n",
                esc(entry.path.as_bytes()),
                entry.size,
                entry.modified_ns,
                esc(entry.hash.as_bytes()),
            ));
            file_count = file_count.saturating_add(1);
        }
    }

    // Footer.
    out.push_str(&alloc::format!("T|{}|{}\n", file_count, total_bytes));

    out
}

/// Parse a decimal integer field.
///
/// Number fields are written by us and are always ASCII digits, so decoding
/// through `str` is correct here in a way it never is for a path.
fn parse_u64(bytes: &[u8]) -> Option<u64> {
    core::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Decode an escaped path field.
///
/// Returns `None` for a field no call to [`esc`] could have produced; a
/// malformed record is a corrupt record, and salvaging part of it would hand
/// the restorer a path naming the wrong file.
fn unesc_path(bytes: &[u8]) -> Option<PathBuf> {
    crate::fs::escape::unescape_octal(bytes).map(PathBuf::from)
}

/// Decode an escaped field that must be ASCII text (an ID or a hex hash).
fn unesc_text(bytes: &[u8]) -> Option<String> {
    String::from_utf8(crate::fs::escape::unescape_octal(bytes)?).ok()
}

/// Whether a manifest ID is safe to use as a single path component.
///
/// Both `restore` and `verify` build `<backup_root>/<id>` and
/// `<backup_root>/<id>.manifest`, and the ID reaching them comes either from
/// a caller or from inside a manifest file.  An ID of `..` or containing a
/// `/` would walk out of the backup root, so it is checked against the shape
/// [`generate_id`] actually produces rather than trusted.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Build the path of a named manifest, rejecting an ID that could escape the
/// backup root.
fn manifest_path_for(backup_root: &Path, id: &str) -> KernelResult<PathBuf> {
    if !is_valid_id(id) {
        return Err(KernelError::InvalidArgument);
    }
    Ok(backup_root.join(alloc::format!("{}{}", id, MANIFEST_EXT)))
}

/// Whether a directory entry names a manifest file.
///
/// Uses the extension rather than a byte suffix so that a file *called*
/// `.manifest` - a legal dotfile with no extension - is not mistaken for one.
fn is_manifest_name(name: &Path) -> bool {
    name.extension()
        .is_some_and(|e| e.as_bytes() == b"manifest")
}

/// Whether a manifest entry path is safe to join onto a destination.
///
/// This is the tar/zip path-traversal check: a relative path free of `..`
/// cannot leave the directory it is joined to, while an absolute one would
/// *replace* it (`Path::join` semantics) and a `..` would climb out of it.
/// The check lives at parse time so no later consumer has to remember it.
fn is_safe_entry_path(path: &Path) -> bool {
    !path.is_empty() && !path.is_absolute() && path.has_no_dot_components()
}

/// Parse a manifest from its serialized form.
///
/// Operates on bytes rather than `&str`: the escaped form is pure ASCII by
/// construction, but decoding the file as UTF-8 first would mean a manifest
/// truncated mid-escape is rejected with the wrong error, and it would tempt
/// the path fields back into `String`.
fn parse_manifest(data: &[u8]) -> KernelResult<Manifest> {
    let mut manifest = Manifest {
        id: String::new(),
        source: PathBuf::new(),
        mode: BackupMode::Full,
        timestamp_ns: 0,
        entries: Vec::new(),
        file_count: 0,
        total_bytes: 0,
    };

    let mut lines = data
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .filter(|l| !l.is_empty());

    // The version line must come first; see MANIFEST_VERSION for why an older
    // manifest is refused rather than read.
    let version = lines
        .next()
        .and_then(|l| l.strip_prefix(b"V|"))
        .and_then(parse_u64)
        .ok_or(KernelError::CorruptedData)?;
    if version != MANIFEST_VERSION {
        return Err(KernelError::NotSupported);
    }

    for line in lines {
        let parts: Vec<&[u8]> = line.splitn(6, |&b| b == b'|').collect();

        match parts.first().copied() {
            Some(b"H") => {
                // Header: H|id|src|mode|timestamp_ns
                let (Some(&id), Some(&src), Some(&mode), Some(&ts)) =
                    (parts.get(1), parts.get(2), parts.get(3), parts.get(4))
                else {
                    return Err(KernelError::CorruptedData);
                };
                let id = unesc_text(id).ok_or(KernelError::CorruptedData)?;
                if !is_valid_id(&id) {
                    // The ID becomes a path component under the backup root.
                    return Err(KernelError::CorruptedData);
                }
                manifest.id = id;
                manifest.source = unesc_path(src).ok_or(KernelError::CorruptedData)?;
                manifest.mode = if mode == b"incr" {
                    BackupMode::Incremental
                } else {
                    BackupMode::Full
                };
                manifest.timestamp_ns = parse_u64(ts).unwrap_or(0);
            }
            Some(b"D") => {
                // Directory: D|path
                let Some(&path) = parts.get(1) else { continue };
                let path = unesc_path(path).ok_or(KernelError::CorruptedData)?;
                if !is_safe_entry_path(&path) {
                    return Err(KernelError::CorruptedData);
                }
                manifest.entries.push(ManifestEntry {
                    path,
                    size: 0,
                    modified_ns: 0,
                    hash: String::new(),
                    entry_type: String::from("dir"),
                });
            }
            Some(b"F") => {
                // File: F|path|size|modified_ns|hash
                let (Some(&path), Some(&size), Some(&mtime), Some(&hash)) =
                    (parts.get(1), parts.get(2), parts.get(3), parts.get(4))
                else {
                    continue;
                };
                let path = unesc_path(path).ok_or(KernelError::CorruptedData)?;
                if !is_safe_entry_path(&path) {
                    return Err(KernelError::CorruptedData);
                }
                manifest.entries.push(ManifestEntry {
                    path,
                    size: parse_u64(size).unwrap_or(0),
                    modified_ns: parse_u64(mtime).unwrap_or(0),
                    hash: unesc_text(hash).ok_or(KernelError::CorruptedData)?,
                    entry_type: String::from("file"),
                });
            }
            Some(b"T") => {
                // Footer: T|file_count|total_bytes
                if let (Some(&count), Some(&bytes)) = (parts.get(1), parts.get(2)) {
                    manifest.file_count = parse_u64(count).unwrap_or(0);
                    manifest.total_bytes = parse_u64(bytes).unwrap_or(0);
                }
            }
            _ => {} // Skip unknown lines for forward compatibility.
        }
    }

    if manifest.id.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    Ok(manifest)
}

/// Load a manifest file.
fn load_manifest(path: &Path) -> KernelResult<Manifest> {
    let data = Vfs::read_file(path)?;
    if data.len() > MAX_MANIFEST_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    parse_manifest(&data)
}

/// Find and load the most recent manifest in a backup root.
fn load_latest_manifest(backup_root: &Path) -> KernelResult<Manifest> {
    let entries = Vfs::readdir(backup_root)?;
    let mut best: Option<(u64, PathBuf)> = None;

    for entry in &entries {
        if is_manifest_name(&entry.name) {
            let path = backup_root.join(&entry.name);
            if let Ok(m) = load_manifest(&path) {
                let ts = m.timestamp_ns;
                if best.as_ref().is_none_or(|(prev_ts, _)| ts > *prev_ts) {
                    best = Some((ts, path));
                }
            }
        }
    }

    match best {
        Some((_, path)) => load_manifest(&path),
        None => Err(KernelError::NotFound),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Recursively collect file/directory entries from a source tree.
fn collect_entries(
    root: &Path,
    path: &Path,
    out: &mut Vec<ManifestEntry>,
    depth: usize,
    max_depth: usize,
    exclude: &[PathBuf],
) -> KernelResult<()> {
    if depth > max_depth || out.len() >= MAX_FILES {
        return Ok(());
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in &entries {
        if entry.name.as_path() == Path::new(".") || entry.name.as_path() == Path::new("..") {
            continue;
        }
        if out.len() >= MAX_FILES {
            return Ok(());
        }

        // `Path::join` collapses the root case itself, so the `path == "/"`
        // arm that existed only to avoid a doubled separator is gone.
        let full = path.join(&entry.name);

        // Relative path, recorded in the manifest.  `Path::strip_prefix` is
        // component-aligned and returns a *genuinely* relative remainder; the
        // byte-wise `str::strip_prefix` it replaces both left a leading
        // separator (so joining the result onto a restore destination would
        // have replaced it) and mis-stripped, turning `/ab/c` under root `/a`
        // into `b/c`.
        let Some(rel) = full.strip_prefix(root) else {
            // Not under the root at all: a symlinked or racing readdir.
            // Skipping is safer than recording a path we cannot place.
            continue;
        };

        // Check exclusions against the real path being visited, which is what
        // the operator sees and types.
        if exclude
            .iter()
            .any(|ex| crate::fs::pathutil::path_in_subtree(&full, ex))
        {
            continue;
        }

        match entry.entry_type {
            EntryType::File => {
                if let Ok(meta) = Vfs::metadata(&full) {
                    // Compute hash for integrity.
                    let hash_hex = if let Ok(data) = Vfs::read_file(&full) {
                        let hash = crate::crypto::sha256(&data);
                        hash_to_hex(&hash)
                    } else {
                        String::new()
                    };

                    out.push(ManifestEntry {
                        path: rel,
                        size: meta.size,
                        modified_ns: meta.modified_ns,
                        hash: hash_hex,
                        entry_type: String::from("file"),
                    });
                }
            }
            EntryType::Directory => {
                out.push(ManifestEntry {
                    path: rel.clone(),
                    size: 0,
                    modified_ns: 0,
                    hash: String::new(),
                    entry_type: String::from("dir"),
                });
                collect_entries(
                    root,
                    &full,
                    out,
                    depth.saturating_add(1),
                    max_depth,
                    exclude,
                )?;
            }
            _ => {} // Skip symlinks etc.
        }
    }

    Ok(())
}

/// Convert a SHA-256 hash to hex string.
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
    serial_println!("[backup] Running self-test...");

    test_manifest_roundtrip();
    test_manifest_hostile_names();
    test_full_backup();
    test_incremental_backup();
    test_restore();
    test_verify();
    test_list();
    test_dry_run();
    test_stats();

    serial_println!("[backup] Self-test passed (9 tests).");
    Ok(())
}

fn test_manifest_roundtrip() {
    let entries = alloc::vec![
        ManifestEntry {
            path: PathBuf::from("sub"),
            size: 0,
            modified_ns: 0,
            hash: String::new(),
            entry_type: String::from("dir"),
        },
        ManifestEntry {
            path: PathBuf::from("sub/file.txt"),
            size: 42,
            modified_ns: 1000,
            hash: String::from("abcd1234"),
            entry_type: String::from("file"),
        },
    ];

    let serialized = serialize_manifest(
        "bkp_100_000",
        Path::new("/src"),
        BackupMode::Full,
        999_000_000,
        &entries,
        42,
    );

    let parsed = parse_manifest(serialized.as_bytes()).expect("parse");
    assert_eq!(parsed.id, "bkp_100_000");
    assert_eq!(parsed.source, PathBuf::from("/src"));
    assert_eq!(parsed.mode, BackupMode::Full);
    assert_eq!(parsed.entries.len(), 2);
    assert_eq!(parsed.entries[1].hash, "abcd1234");
    assert_eq!(parsed.file_count, 1);
    assert_eq!(parsed.total_bytes, 42);

    serial_println!("[backup]   manifest roundtrip: ok");
}

/// A manifest must survive names that are not text and must refuse records
/// that would write outside the destination.
///
/// Every case here was a live defect before the format gained escaping and a
/// version line: a `|` or a newline in a filename forged a record, and an
/// absolute or `..`-bearing entry path escaped the restore destination
/// entirely (the classic tar traversal).
fn test_manifest_hostile_names() {
    // A name that is not UTF-8, one carrying the field delimiter, and one
    // carrying the record delimiter.
    let entries = alloc::vec![
        ManifestEntry {
            path: PathBuf::from(b"re\xffport.txt".as_slice()),
            size: 1,
            modified_ns: 0,
            hash: String::from("aa"),
            entry_type: String::from("file"),
        },
        ManifestEntry {
            path: PathBuf::from("we|ird\nname"),
            size: 2,
            modified_ns: 0,
            hash: String::from("bb"),
            entry_type: String::from("file"),
        },
    ];
    let serialized = serialize_manifest(
        "bkp_1",
        Path::new(b"/sr\xfec".as_slice()),
        BackupMode::Incremental,
        7,
        &entries,
        3,
    );
    // The whole point of escaping: the file stays line-oriented ASCII, so the
    // two entries are still exactly two records.
    assert!(serialized.is_ascii());
    // V, H, F, F, T - the newline inside the second name did not split it.
    assert_eq!(serialized.lines().count(), 5);

    let parsed = parse_manifest(serialized.as_bytes()).expect("parse");
    assert_eq!(parsed.source, PathBuf::from(b"/sr\xfec".as_slice()));
    assert_eq!(parsed.mode, BackupMode::Incremental);
    assert_eq!(parsed.entries.len(), 2);
    assert_eq!(
        parsed.entries[0].path,
        PathBuf::from(b"re\xffport.txt".as_slice())
    );
    assert_eq!(parsed.entries[1].path, PathBuf::from("we|ird\nname"));

    // Path traversal: an absolute entry path would *replace* the restore
    // destination, and a `..` would climb out of it.  Both are corrupt.
    for bad in [
        // Absolute entry path.
        "V|2\nH|bkp_1|/src|full|0\nF|\\057etc\\057passwd|1|0|aa\nT|1|1\n",
        // `../../etc` - `\057` is an escaped separator.
        "V|2\nH|bkp_1|/src|full|0\nF|..\\057..\\057etc|1|0|aa\nT|1|1\n",
        // A `..` in the middle still climbs out once joined.
        "V|2\nH|bkp_1|/src|full|0\nD|sub\\057..\\057..\nT|0|0\n",
        // An ID is joined onto the backup root to name the data directory.
        "V|2\nH|..|/src|full|0\nT|0|0\n",
        // A field no `esc` call could have produced.
        "V|2\nH|bkp_1|/sr\\09c|full|0\nT|0|0\n",
    ] {
        assert!(
            matches!(
                parse_manifest(bad.as_bytes()),
                Err(KernelError::CorruptedData)
            ),
            "corrupt manifest accepted: {bad}"
        );
    }

    // A version-1 manifest recorded absolute entry paths; reading one with
    // version-2 rules would restore over the original files.
    assert!(matches!(
        parse_manifest(b"V|1\nH|bkp_1|/src|full|0\nT|0|0\n"),
        Err(KernelError::NotSupported)
    ));
    // No version line at all is corrupt, not "version 0".
    assert!(matches!(
        parse_manifest(b"H|bkp_1|/src|full|0\nT|0|0\n"),
        Err(KernelError::CorruptedData)
    ));

    // An ID must be a single safe component wherever it comes from.
    assert!(manifest_path_for(Path::new("/bkp"), "../evil").is_err());
    assert_eq!(
        manifest_path_for(Path::new("/bkp"), "bkp_1").ok(),
        Some(PathBuf::from("/bkp/bkp_1.manifest"))
    );

    serial_println!("[backup]   hostile manifest names: ok");
}

fn test_full_backup() {
    // Setup source.
    let _ = Vfs::mkdir("/tmp/bkp_src");
    let _ = Vfs::mkdir("/tmp/bkp_src/sub");
    Vfs::write_file("/tmp/bkp_src/a.txt", b"alpha").expect("write");
    Vfs::write_file("/tmp/bkp_src/sub/b.txt", b"beta").expect("write");

    // Setup destination.
    let _ = Vfs::mkdir("/tmp/bkp_dst");

    let opts = BackupOptions::default();
    let result = create("/tmp/bkp_src", "/tmp/bkp_dst", &opts).expect("backup");
    assert!(
        result.files_copied >= 2,
        "should copy 2 files, got {}",
        result.files_copied
    );
    assert!(result.dirs_created >= 1, "should create at least 1 dir");

    // Verify files exist in backup.
    let backup_dir = alloc::format!("/tmp/bkp_dst/{}", result.manifest_id);
    let data = Vfs::read_file(alloc::format!("{}/a.txt", backup_dir)).expect("read a.txt");
    assert_eq!(&data, b"alpha");
    let data = Vfs::read_file(alloc::format!("{}/sub/b.txt", backup_dir)).expect("read b.txt");
    assert_eq!(&data, b"beta");

    // Cleanup.
    let _ = Vfs::remove("/tmp/bkp_src/a.txt");
    let _ = Vfs::remove("/tmp/bkp_src/sub/b.txt");
    let _ = Vfs::rmdir("/tmp/bkp_src/sub");
    let _ = Vfs::rmdir("/tmp/bkp_src");
    // Leave bkp_dst for incremental test.

    serial_println!("[backup]   full backup: ok");
}

fn test_incremental_backup() {
    // Re-setup source with one changed and one new file.
    let _ = Vfs::mkdir("/tmp/bkp_src2");
    let _ = Vfs::mkdir("/tmp/bkp_src2/sub");
    Vfs::write_file("/tmp/bkp_src2/a.txt", b"alpha modified").expect("write");
    Vfs::write_file("/tmp/bkp_src2/sub/b.txt", b"beta").expect("write");
    Vfs::write_file("/tmp/bkp_src2/c.txt", b"charlie").expect("write");

    // Do a full backup first.
    let _ = Vfs::mkdir("/tmp/bkp_inc");
    let full_opts = BackupOptions::default();
    let _ = create("/tmp/bkp_src2", "/tmp/bkp_inc", &full_opts).expect("full");

    // Modify source.
    Vfs::write_file("/tmp/bkp_src2/a.txt", b"alpha changed again").expect("write");

    // Incremental backup.
    let inc_opts = BackupOptions {
        mode: BackupMode::Incremental,
        ..BackupOptions::default()
    };
    let result = create("/tmp/bkp_src2", "/tmp/bkp_inc", &inc_opts).expect("incremental");
    // a.txt changed → copied; b.txt and c.txt unchanged → skipped
    assert!(result.files_copied >= 1, "should copy changed file");
    // Some files should be skipped.
    assert!(
        result.files_skipped >= 1,
        "should skip unchanged files, skipped={}",
        result.files_skipped
    );

    // Cleanup.
    let _ = Vfs::remove("/tmp/bkp_src2/a.txt");
    let _ = Vfs::remove("/tmp/bkp_src2/sub/b.txt");
    let _ = Vfs::remove("/tmp/bkp_src2/c.txt");
    let _ = Vfs::rmdir("/tmp/bkp_src2/sub");
    let _ = Vfs::rmdir("/tmp/bkp_src2");

    serial_println!("[backup]   incremental backup: ok");
}

fn test_restore() {
    // Setup: create a backup.
    let _ = Vfs::mkdir("/tmp/bkp_rsrc");
    Vfs::write_file("/tmp/bkp_rsrc/data.txt", b"restore me").expect("write");

    let _ = Vfs::mkdir("/tmp/bkp_rdst");
    let result =
        create("/tmp/bkp_rsrc", "/tmp/bkp_rdst", &BackupOptions::default()).expect("backup");

    // Restore to a new location.
    let _ = Vfs::mkdir("/tmp/bkp_restored");
    let restore_result = restore(
        "/tmp/bkp_rdst",
        "/tmp/bkp_restored",
        Some(&result.manifest_id),
        &RestoreOptions::default(),
    )
    .expect("restore");

    assert!(restore_result.files_restored >= 1, "should restore file");
    assert_eq!(restore_result.verify_failures, 0, "no verify failures");

    let data = Vfs::read_file("/tmp/bkp_restored/data.txt").expect("read restored");
    assert_eq!(&data, b"restore me");

    // Cleanup.
    let _ = Vfs::remove("/tmp/bkp_rsrc/data.txt");
    let _ = Vfs::rmdir("/tmp/bkp_rsrc");
    let _ = Vfs::remove("/tmp/bkp_restored/data.txt");
    let _ = Vfs::rmdir("/tmp/bkp_restored");

    serial_println!("[backup]   restore: ok");
}

fn test_verify() {
    // Setup.
    let _ = Vfs::mkdir("/tmp/bkp_vsrc");
    Vfs::write_file("/tmp/bkp_vsrc/v.txt", b"verify content").expect("write");

    let _ = Vfs::mkdir("/tmp/bkp_vdst");
    let result =
        create("/tmp/bkp_vsrc", "/tmp/bkp_vdst", &BackupOptions::default()).expect("backup");

    // Verify should pass.
    let (ok, fail, _) = verify("/tmp/bkp_vdst", Some(&result.manifest_id)).expect("verify");
    assert!(ok >= 1, "should have ok files");
    assert_eq!(fail, 0, "no failures");

    // Cleanup.
    let _ = Vfs::remove("/tmp/bkp_vsrc/v.txt");
    let _ = Vfs::rmdir("/tmp/bkp_vsrc");

    serial_println!("[backup]   verify: ok");
}

fn test_list() {
    // bkp_dst and bkp_rdst should have manifests from earlier tests.
    // Use bkp_rdst which should have exactly one manifest.
    if let Ok(backups) = list("/tmp/bkp_rdst") {
        assert!(!backups.is_empty(), "should find backups");
        assert!(
            backups[0].id.starts_with("bkp_"),
            "id should start with bkp_"
        );
    }
    // Even if earlier dirs were cleaned, list on empty dir shouldn't panic.
    let _ = Vfs::mkdir("/tmp/bkp_empty_list");
    let backups = list("/tmp/bkp_empty_list").expect("list empty");
    assert!(backups.is_empty());
    let _ = Vfs::rmdir("/tmp/bkp_empty_list");

    serial_println!("[backup]   list: ok");
}

fn test_dry_run() {
    let _ = Vfs::mkdir("/tmp/bkp_drysrc");
    Vfs::write_file("/tmp/bkp_drysrc/dry.txt", b"dry data").expect("write");

    let _ = Vfs::mkdir("/tmp/bkp_drydst");
    let opts = BackupOptions {
        dry_run: true,
        ..BackupOptions::default()
    };
    let result = create("/tmp/bkp_drysrc", "/tmp/bkp_drydst", &opts).expect("dry run");
    assert!(result.files_copied >= 1, "dry run should report copies");

    // No backup directory should have been created.
    let entries = Vfs::readdir("/tmp/bkp_drydst").expect("readdir");
    let has_bkp = entries.iter().any(|e| e.name.starts_with("bkp_"));
    assert!(!has_bkp, "dry run should not create backup dir");

    // Cleanup.
    let _ = Vfs::remove("/tmp/bkp_drysrc/dry.txt");
    let _ = Vfs::rmdir("/tmp/bkp_drysrc");
    let _ = Vfs::rmdir("/tmp/bkp_drydst");

    serial_println!("[backup]   dry run: ok");
}

fn test_stats() {
    let (backups, restores, bytes) = stats();
    assert!(backups > 0, "should have backups");
    assert!(restores > 0, "should have restores");
    // bytes may be 0 in some edge cases, just verify it's accessible.
    let _ = bytes;

    serial_println!("[backup]   stats: ok");
}
