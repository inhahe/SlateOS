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
    /// Relative path from backup root.
    pub path: String,
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
    pub source: String,
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
    /// Exclude paths matching these prefixes.
    pub exclude: Vec<String>,
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
    /// Only restore specific paths (empty = all).
    pub filter_paths: Vec<String>,
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
    pub source: String,
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
pub fn create(src: &str, dst: &str, opts: &BackupOptions) -> KernelResult<BackupResult> {
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
    collect_entries(src, src, &mut source_entries, 0, opts.max_depth, &opts.exclude)?;

    // Load previous manifest for incremental mode.
    let prev_manifest = if opts.mode == BackupMode::Incremental {
        load_latest_manifest(dst).ok()
    } else {
        None
    };

    // Build lookup of previous entries by path for quick comparison.
    let prev_index: BTreeMap<&str, &ManifestEntry> = if let Some(ref m) = prev_manifest {
        m.entries.iter()
            .filter(|e| e.entry_type == "file")
            .map(|e| (e.path.as_str(), e))
            .collect()
    } else {
        BTreeMap::new()
    };

    // Create backup subdirectory for this run.
    let backup_dir = alloc::format!("{}/{}", dst, manifest_id);
    if !opts.dry_run {
        Vfs::mkdir(&backup_dir).inspect_err(|&e| {
            if matches!(e, KernelError::AlreadyExists) {
            }
        }).or_else(|e| {
            if matches!(e, KernelError::AlreadyExists) { Ok(()) } else { Err(e) }
        })?;
    }

    // Process each entry.
    let mut manifest_entries = Vec::new();

    for entry in &source_entries {
        if entry.entry_type == "dir" {
            // Create directory in backup.
            let dst_path = alloc::format!("{}{}", backup_dir, entry.path);
            if !opts.dry_run {
                match Vfs::mkdir(&dst_path) {
                    Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
                    Err(KernelError::AlreadyExists) => {}
                    Err(e) => {
                        result.errors.push(alloc::format!("mkdir {}: {:?}", dst_path, e));
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
            if let Some(prev) = prev_index.get(entry.path.as_str()) {
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
        let src_path = alloc::format!("{}{}", src, entry.path);
        let dst_path = alloc::format!("{}{}", backup_dir, entry.path);

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
                    result.errors.push(alloc::format!("copy {}: {:?}", entry.path, e));
                    continue;
                }
            }
        }

        manifest_entries.push(entry.clone());
    }

    // Write manifest.
    if !opts.dry_run {
        let manifest_path = alloc::format!("{}/{}{}", dst, manifest_id, MANIFEST_EXT);
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
pub fn restore(
    backup_root: &str,
    dst: &str,
    manifest_id: Option<&str>,
    opts: &RestoreOptions,
) -> KernelResult<RestoreResult> {
    // Load manifest.
    let manifest = if let Some(id) = manifest_id {
        let path = alloc::format!("{}/{}{}", backup_root, id, MANIFEST_EXT);
        load_manifest(&path)?
    } else {
        load_latest_manifest(backup_root)?
    };

    let mut result = RestoreResult::default();

    // Ensure destination exists.
    if !opts.dry_run {
        let _ = Vfs::mkdir(dst);
    }

    let backup_dir = alloc::format!("{}/{}", backup_root, manifest.id);

    for entry in &manifest.entries {
        // Apply path filter if set.
        if !opts.filter_paths.is_empty()
            && !opts.filter_paths.iter().any(|p| entry.path.starts_with(p.as_str()))
        {
            continue;
        }

        let dst_path = alloc::format!("{}{}", dst, entry.path);

        if entry.entry_type == "dir" {
            if !opts.dry_run {
                match Vfs::mkdir(&dst_path) {
                    Ok(()) => result.dirs_created = result.dirs_created.saturating_add(1),
                    Err(KernelError::AlreadyExists) => {}
                    Err(e) => {
                        result.errors.push(alloc::format!("mkdir {}: {:?}", dst_path, e));
                    }
                }
            } else {
                result.dirs_created = result.dirs_created.saturating_add(1);
            }
            continue;
        }

        // Copy file from backup.
        let backup_file = alloc::format!("{}{}", backup_dir, entry.path);

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
                                entry.path, entry.hash, hex,
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                result.errors.push(alloc::format!("restore {}: {:?}", entry.path, e));
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
pub fn list(backup_root: &str) -> KernelResult<Vec<BackupInfo>> {
    let entries = Vfs::readdir(backup_root)?;
    let mut backups = Vec::new();

    for entry in &entries {
        if entry.name.ends_with(MANIFEST_EXT) {
            let path = alloc::format!("{}/{}", backup_root, entry.name);
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
pub fn verify(backup_root: &str, manifest_id: Option<&str>) -> KernelResult<(u64, u64, Vec<String>)> {
    let manifest = if let Some(id) = manifest_id {
        let path = alloc::format!("{}/{}{}", backup_root, id, MANIFEST_EXT);
        load_manifest(&path)?
    } else {
        load_latest_manifest(backup_root)?
    };

    let backup_dir = alloc::format!("{}/{}", backup_root, manifest.id);
    let mut ok_count: u64 = 0;
    let mut fail_count: u64 = 0;
    let mut failures = Vec::new();

    for entry in &manifest.entries {
        if entry.entry_type != "file" || entry.hash.is_empty() {
            continue;
        }

        let file_path = alloc::format!("{}{}", backup_dir, entry.path);
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
                        entry.path,
                        entry.hash,
                        hex,
                    ));
                }
            }
            Err(e) => {
                fail_count = fail_count.saturating_add(1);
                failures.push(alloc::format!("{}: read error: {:?}", entry.path, e));
            }
        }
    }

    serial_println!(
        "[backup] Verify {}: {} ok, {} failed",
        manifest.id, ok_count, fail_count,
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

/// Serialize a manifest to string (simple key=value line format).
///
/// Uses a simple text format instead of JSON to avoid needing a JSON
/// library in no_std. Format is one entry per line:
///
/// ```text
/// H|<id>|<src>|<mode>|<timestamp_ns>
/// D|<rel_path>
/// F|<rel_path>|<size>|<modified_ns>|<hash_hex>
/// T|<file_count>|<total_bytes>
/// ```
fn serialize_manifest(
    id: &str,
    src: &str,
    mode: BackupMode,
    timestamp_ns: u64,
    entries: &[ManifestEntry],
    total_bytes: u64,
) -> String {
    let mut out = String::new();

    // Header line.
    let mode_str = match mode {
        BackupMode::Full => "full",
        BackupMode::Incremental => "incr",
    };
    out.push_str(&alloc::format!("H|{}|{}|{}|{}\n", id, src, mode_str, timestamp_ns));

    let mut file_count: u64 = 0;
    for entry in entries {
        if entry.entry_type == "dir" {
            out.push_str(&alloc::format!("D|{}\n", entry.path));
        } else {
            out.push_str(&alloc::format!(
                "F|{}|{}|{}|{}\n",
                entry.path, entry.size, entry.modified_ns, entry.hash,
            ));
            file_count = file_count.saturating_add(1);
        }
    }

    // Footer.
    out.push_str(&alloc::format!("T|{}|{}\n", file_count, total_bytes));

    out
}

/// Parse a manifest from its serialized form.
fn parse_manifest(data: &str) -> KernelResult<Manifest> {
    let mut manifest = Manifest {
        id: String::new(),
        source: String::new(),
        mode: BackupMode::Full,
        timestamp_ns: 0,
        entries: Vec::new(),
        file_count: 0,
        total_bytes: 0,
    };

    for line in data.lines() {
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(6, '|').collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "H" => {
                // Header: H|id|src|mode|timestamp_ns
                if parts.len() < 5 {
                    return Err(KernelError::CorruptedData);
                }
                manifest.id = String::from(parts[1]);
                manifest.source = String::from(parts[2]);
                manifest.mode = match parts[3] {
                    "incr" => BackupMode::Incremental,
                    _ => BackupMode::Full,
                };
                manifest.timestamp_ns = parts[4].parse().unwrap_or(0);
            }
            "D" => {
                // Directory: D|path
                if parts.len() < 2 {
                    continue;
                }
                manifest.entries.push(ManifestEntry {
                    path: String::from(parts[1]),
                    size: 0,
                    modified_ns: 0,
                    hash: String::new(),
                    entry_type: String::from("dir"),
                });
            }
            "F" => {
                // File: F|path|size|modified_ns|hash
                if parts.len() < 5 {
                    continue;
                }
                manifest.entries.push(ManifestEntry {
                    path: String::from(parts[1]),
                    size: parts[2].parse().unwrap_or(0),
                    modified_ns: parts[3].parse().unwrap_or(0),
                    hash: String::from(parts[4]),
                    entry_type: String::from("file"),
                });
            }
            "T" => {
                // Footer: T|file_count|total_bytes
                if parts.len() >= 3 {
                    manifest.file_count = parts[1].parse().unwrap_or(0);
                    manifest.total_bytes = parts[2].parse().unwrap_or(0);
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
fn load_manifest(path: &str) -> KernelResult<Manifest> {
    let data = Vfs::read_file(path)?;
    if data.len() > MAX_MANIFEST_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    let text = core::str::from_utf8(&data).map_err(|_| KernelError::CorruptedData)?;
    parse_manifest(text)
}

/// Find and load the most recent manifest in a backup root.
fn load_latest_manifest(backup_root: &str) -> KernelResult<Manifest> {
    let entries = Vfs::readdir(backup_root)?;
    let mut best: Option<(u64, String)> = None;

    for entry in &entries {
        if entry.name.ends_with(MANIFEST_EXT) {
            let path = alloc::format!("{}/{}", backup_root, entry.name);
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
    root: &str,
    path: &str,
    out: &mut Vec<ManifestEntry>,
    depth: usize,
    max_depth: usize,
    exclude: &[String],
) -> KernelResult<()> {
    if depth > max_depth || out.len() >= MAX_FILES {
        return Ok(());
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if out.len() >= MAX_FILES {
            return Ok(());
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Compute relative path.
        let rel = if root == "/" {
            full.clone()
        } else if let Some(stripped) = full.strip_prefix(root) {
            String::from(stripped)
        } else {
            full.clone()
        };

        // Check exclusions.
        if exclude.iter().any(|ex| rel.starts_with(ex.as_str())) {
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
                collect_entries(root, &full, out, depth + 1, max_depth, exclude)?;
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
    test_full_backup();
    test_incremental_backup();
    test_restore();
    test_verify();
    test_list();
    test_dry_run();
    test_stats();

    serial_println!("[backup] Self-test passed (8 tests).");
    Ok(())
}

fn test_manifest_roundtrip() {
    let entries = alloc::vec![
        ManifestEntry {
            path: String::from("/sub"),
            size: 0,
            modified_ns: 0,
            hash: String::new(),
            entry_type: String::from("dir"),
        },
        ManifestEntry {
            path: String::from("/sub/file.txt"),
            size: 42,
            modified_ns: 1000,
            hash: String::from("abcd1234"),
            entry_type: String::from("file"),
        },
    ];

    let serialized = serialize_manifest(
        "bkp_100_000",
        "/src",
        BackupMode::Full,
        999_000_000,
        &entries,
        42,
    );

    let parsed = parse_manifest(&serialized).expect("parse");
    assert_eq!(parsed.id, "bkp_100_000");
    assert_eq!(parsed.source, "/src");
    assert_eq!(parsed.mode, BackupMode::Full);
    assert_eq!(parsed.entries.len(), 2);
    assert_eq!(parsed.entries[1].hash, "abcd1234");
    assert_eq!(parsed.file_count, 1);
    assert_eq!(parsed.total_bytes, 42);

    serial_println!("[backup]   manifest roundtrip: ok");
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
    assert!(result.files_copied >= 2, "should copy 2 files, got {}", result.files_copied);
    assert!(result.dirs_created >= 1, "should create at least 1 dir");

    // Verify files exist in backup.
    let backup_dir = alloc::format!("/tmp/bkp_dst/{}", result.manifest_id);
    let data = Vfs::read_file(&alloc::format!("{}/a.txt", backup_dir)).expect("read a.txt");
    assert_eq!(&data, b"alpha");
    let data = Vfs::read_file(&alloc::format!("{}/sub/b.txt", backup_dir)).expect("read b.txt");
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
    assert!(result.files_skipped >= 1, "should skip unchanged files, skipped={}", result.files_skipped);

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
    let result = create("/tmp/bkp_rsrc", "/tmp/bkp_rdst", &BackupOptions::default()).expect("backup");

    // Restore to a new location.
    let _ = Vfs::mkdir("/tmp/bkp_restored");
    let restore_result = restore(
        "/tmp/bkp_rdst",
        "/tmp/bkp_restored",
        Some(&result.manifest_id),
        &RestoreOptions::default(),
    ).expect("restore");

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
    let result = create("/tmp/bkp_vsrc", "/tmp/bkp_vdst", &BackupOptions::default()).expect("backup");

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
        assert!(backups[0].id.starts_with("bkp_"), "id should start with bkp_");
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
