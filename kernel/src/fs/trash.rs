//! Recycle bin (trash) for filesystem delete operations.
//!
//! Provides a per-filesystem recycle bin that moves deleted files to
//! a `/_TRASH/` directory instead of permanently removing them.
//! Files can be restored to their original location or permanently
//! purged to free disk space.
//!
//! ## Design
//!
//! Per the design spec:
//! - **Per-filesystem recycle bins** — each mounted filesystem has its own
//!   `/_TRASH/` directory.  Moving a file to trash never crosses filesystem
//!   boundaries (no slow copy+delete).
//! - **Two delete modes**: trash-capable delete (default for shell/explorer)
//!   and permanent delete (for temp files, compilers, etc.).
//! - **Auto-prune**: when disk space is low, delete oldest trash items first.
//! - **Bypass-recycle-bin capability**: programs can skip the trash for
//!   non-temp directories if they hold the `fs.bypass_recycle` capability.
//!
//! ## Trash directory layout
//!
//! ```text
//! /_TRASH/
//!   _INDEX           — line-delimited metadata: "trash_name=original_path"
//!   HELLO.TXT        — trashed file data
//!   REPORT.TXT       — another trashed file
//! ```
//!
//! The `_INDEX` file maps each trashed filename to its original path.
//! This avoids the FAT 8.3 naming issue of per-file metadata files
//! (e.g., `HELLO.TXT.ORI` would have a 10-char base, invalid in 8.3).
//!
//! If a name collision occurs (two files with the same name trashed),
//! a numeric suffix is appended: `HELLO_2.TXT`, `HELLO_3.TXT`, etc.
//!
//! ## Syscall interface
//!
//! - `SYS_FS_TRASH` (618): move file to recycle bin
//! - `SYS_FS_TRASH_LIST` (619): list recycle bin contents
//! - `SYS_FS_TRASH_RESTORE` (620): restore file from recycle bin
//! - `SYS_FS_TRASH_EMPTY` (621): permanently delete all trash items
//!
//! ## Limitations
//!
//! - Currently only supports the root mount (`/`).  When multiple mount
//!   points are added, each will get its own `/_TRASH/` directory.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::escape::{escape_octal, unescape_octal};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{EntryType, Vfs};

/// Disk usage percentage (0–100) above which auto-prune activates.
///
/// When the root filesystem exceeds this threshold, the oldest trash
/// items are permanently deleted until usage drops below the target
/// or the trash is empty.
const AUTO_PRUNE_THRESHOLD: u64 = 90;

/// Disk usage percentage that auto-prune tries to reach.
///
/// Slightly below the threshold to avoid flip-flopping.
const AUTO_PRUNE_TARGET: u64 = 85;

/// Name of the trash directory on each filesystem.
///
/// Uses `_TRASH` (not `.trash`) because FAT 8.3 naming doesn't support
/// dot-prefixed filenames (the dot is the base/extension separator,
/// so `.trash` would have an empty base → invalid).
pub(crate) const TRASH_DIR: &str = "/_TRASH";

/// Name of the index file inside the trash directory.
///
/// Maps trashed filenames to their original paths.
/// Format: one entry per line, `trash_name=original_path`, with both fields
/// octal-escaped (see [`INDEX_ESCAPE`] and [`crate::fs::escape`]).
const INDEX_FILE: &str = "/_TRASH/_INDEX";

/// Bytes that must not appear raw inside an index field.
///
/// `=` is the field separator; the record separator `\n` and every other
/// non-printable byte are already escaped by default.  Both are perfectly
/// legal in a filename here — our paths allow every byte but `/` and NUL — so
/// without escaping, trashing a file whose name contains a newline would
/// inject a second, bogus record into the index and make an unrelated file
/// restorable to an attacker-chosen path.
const INDEX_ESCAPE: &[u8] = b"=";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A single item in the recycle bin.
#[derive(Debug, Clone)]
pub struct TrashItem {
    /// Filename as it appears in the trash directory.
    pub trash_name: PathBuf,
    /// Original path where the file was before deletion, or `None` if the
    /// index has no record of it (the file was put there out of band, or the
    /// index was truncated).
    pub original_path: Option<PathBuf>,
    /// File size in bytes.
    pub size: u64,
    /// Whether this is a directory (currently only files are supported).
    pub is_directory: bool,
}

/// Move a file to the recycle bin instead of permanently deleting it.
///
/// The file is renamed from its current location to `/_TRASH/<name>`.
/// The original path is recorded in the `_INDEX` file for later
/// restoration.
///
/// Returns `Ok(())` on success, or an error if the file doesn't exist
/// or the trash directory can't be created.
pub fn trash(path: impl AsRef<Path>) -> KernelResult<()> {
    let path = path.as_ref();
    // Verify the source exists.
    let stat = Vfs::stat(path)?;
    let _ = stat; // Used for existence check only.

    // Ensure the trash directory exists.
    ensure_trash_dir()?;

    // Extract the filename from the path.  `file_name` yields `None` only for
    // the root and for all-separator paths, neither of which can be trashed.
    let filename = path.file_name().ok_or(KernelError::InvalidArgument)?;

    // Find a unique name in the trash directory.
    let trash_name = unique_trash_name(filename)?;
    let trash_path = format_trash_path(&trash_name);

    // Move the file to trash via rename.
    // This is O(1) on the same filesystem — only directory entries change.
    Vfs::rename(path, &trash_path)?;

    // Update the index file with the mapping.
    index_add(&trash_name, path)?;

    crate::serial_println!(
        "[trash] Moved '{}' to trash as '{}'",
        path.display(),
        trash_name.display()
    );

    // Check disk space and prune oldest trash items if needed.
    let _ = auto_prune();

    Ok(())
}

/// List all items in the recycle bin.
///
/// Returns a vector of [`TrashItem`] structs with the trash name,
/// original path, size, and type of each item.
pub fn list() -> KernelResult<Vec<TrashItem>> {
    // If the trash directory doesn't exist, return empty.
    let entries = match Vfs::readdir(TRASH_DIR) {
        Err(KernelError::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e),
        Ok(e) => e,
    };

    // Load the index for original-path lookups.
    let index = index_load();

    let mut items = Vec::new();

    for entry in &entries {
        // Skip the _INDEX metadata file.
        if entry.name.eq_ignore_ascii_case("_INDEX") {
            continue;
        }

        // Look up the original path from the index.
        let original = index_lookup(&index, &entry.name);

        items.push(TrashItem {
            trash_name: entry.name.clone(),
            original_path: original,
            size: entry.size,
            is_directory: entry.entry_type == EntryType::Directory,
        });
    }

    Ok(items)
}

/// Restore a file from the recycle bin to its original location.
///
/// `trash_name` is the filename as it appears in `/_TRASH/`.
/// The file is moved back to the path stored in the index file.
///
/// Returns the original path on success.
pub fn restore(trash_name: impl AsRef<Path>) -> KernelResult<PathBuf> {
    let trash_name = trash_name.as_ref();
    let trash_path = format_trash_path(trash_name);

    // Look up the original path from the index.
    let index = index_load();
    let original = index_lookup(&index, trash_name).ok_or(KernelError::NotFound)?;

    // Move the file back to its original location.
    Vfs::rename(&trash_path, &original)?;

    // Remove the entry from the index.
    index_remove(trash_name)?;

    crate::serial_println!(
        "[trash] Restored '{}' to '{}'",
        trash_name.display(),
        original.display()
    );

    Ok(original)
}

/// Permanently delete all items in the recycle bin.
///
/// This frees disk space by removing all files and their metadata
/// from the trash directory.
pub fn empty() -> KernelResult<()> {
    let entries = match Vfs::readdir(TRASH_DIR) {
        Err(KernelError::NotFound) => return Ok(()),
        Err(e) => return Err(e),
        Ok(e) => e,
    };

    let mut count = 0usize;
    let mut errors: Option<KernelError> = None;

    for entry in &entries {
        // Skip the _INDEX file — we'll delete it after everything else.
        if entry.name.eq_ignore_ascii_case("_INDEX") {
            continue;
        }

        let item_path = format_trash_path(&entry.name);
        let result = if entry.entry_type == EntryType::Directory {
            recursive_delete(&item_path)
        } else {
            Vfs::remove(&item_path)
        };

        if let Err(e) = result {
            errors = Some(e);
        } else {
            count = count.wrapping_add(1);
        }
    }

    // Clear the index file.
    let _ = Vfs::remove(INDEX_FILE);

    crate::serial_println!("[trash] Emptied recycle bin ({} items deleted)", count);

    match errors {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Permanently delete a single item from the recycle bin.
///
/// `trash_name` is the filename as it appears in `/_TRASH/`.
pub fn purge_one(trash_name: impl AsRef<Path>) -> KernelResult<()> {
    let trash_name = trash_name.as_ref();
    let trash_path = format_trash_path(trash_name);

    // Determine if this is a file or directory.
    let stat = Vfs::stat(&trash_path)?;
    if stat.entry_type == EntryType::Directory {
        recursive_delete(&trash_path)?;
    } else {
        Vfs::remove(&trash_path)?;
    }

    // Best-effort: remove the entry from the index.
    let _ = index_remove(trash_name);

    crate::serial_println!("[trash] Permanently deleted '{}'", trash_name.display());
    Ok(())
}

/// Automatically prune oldest trash items when disk space is low.
///
/// Checks the root filesystem's usage percentage.  If it exceeds
/// [`AUTO_PRUNE_THRESHOLD`], permanently deletes trash items (smallest
/// first, to maximize freed items) until usage drops below
/// [`AUTO_PRUNE_TARGET`] or the trash is empty.
///
/// Called automatically after each `trash()` operation and can also
/// be invoked manually via the `trash --prune` kshell command.
///
/// Returns the number of items pruned, or 0 if no pruning was needed.
#[allow(clippy::arithmetic_side_effects)]
pub fn auto_prune() -> KernelResult<usize> {
    // Check root filesystem usage.
    let info = match Vfs::statvfs("/") {
        Ok(i) => i,
        Err(_) => return Ok(0), // Can't check — skip pruning.
    };

    let usage = info.usage_percent();
    if usage < AUTO_PRUNE_THRESHOLD {
        return Ok(0); // Plenty of space.
    }

    crate::serial_println!(
        "[trash] Disk usage {}% >= {}% threshold, starting auto-prune",
        usage,
        AUTO_PRUNE_THRESHOLD
    );

    // Get all trash items.
    let mut items = list()?;
    if items.is_empty() {
        crate::serial_println!("[trash] Auto-prune: trash is empty, nothing to free");
        return Ok(0);
    }

    // Sort by size ascending — delete smallest items first to maximize
    // the number of items freed per prune cycle.  This heuristic prefers
    // freeing many small items over one large one, which is usually what
    // users expect (they remember the large files they trashed, not the
    // small ones).
    items.sort_by_key(|item| item.size);

    let mut pruned = 0usize;
    for item in &items {
        // Re-check usage after each deletion.
        let current = match Vfs::statvfs("/") {
            Ok(i) => i.usage_percent(),
            Err(_) => break,
        };
        if current < AUTO_PRUNE_TARGET {
            break; // Reached target.
        }

        // Permanently delete this trash item.
        if purge_one(&item.trash_name).is_ok() {
            pruned = pruned.wrapping_add(1);
            crate::serial_println!(
                "[trash] Auto-pruned '{}' ({} bytes, was: {})",
                item.trash_name.display(),
                item.size,
                item.original_path
                    .as_deref()
                    .unwrap_or(Path::new("<unknown>"))
                    .display()
            );
        }
    }

    if pruned > 0 {
        let final_usage = Vfs::statvfs("/").map_or(0, |i| i.usage_percent());
        crate::serial_println!(
            "[trash] Auto-prune complete: {} items deleted, disk usage now {}%",
            pruned,
            final_usage
        );
    }

    Ok(pruned)
}

// ---------------------------------------------------------------------------
// Trash directory management
// ---------------------------------------------------------------------------

/// Ensure the trash directory exists, creating it if necessary.
fn ensure_trash_dir() -> KernelResult<()> {
    match Vfs::stat(TRASH_DIR) {
        Ok(entry) if entry.entry_type == EntryType::Directory => Ok(()),
        Err(KernelError::NotFound) => {
            Vfs::mkdir(TRASH_DIR)?;
            crate::serial_println!("[trash] Created trash directory '{}'", TRASH_DIR);
            Ok(())
        }
        Ok(_) => {
            // Something exists at /_TRASH but it's not a directory.
            Err(KernelError::InvalidArgument)
        }
        Err(e) => Err(e),
    }
}

/// Generate a unique filename in the trash directory.
///
/// If `name` already exists in trash, tries `name_2`, `name_3`, etc.
/// The suffixed names stay within FAT 8.3 limits by shortening the
/// base if necessary.
///
/// Returns the unique name (without path prefix).
#[allow(clippy::arithmetic_side_effects)]
fn unique_trash_name(name: &Path) -> KernelResult<PathBuf> {
    // Check if the name is available.
    let check_path = format_trash_path(name);
    if Vfs::stat(&check_path).is_err() {
        return Ok(name.to_path_buf());
    }

    // Name is taken — try suffixed variants.
    // Split into base and extension for proper suffixing.  This works on raw
    // bytes: a filename has no encoding, so there is no character boundary to
    // respect, and the 8.3 budget below is a byte budget on a FAT volume.
    let bytes = name.as_bytes();
    let (base, ext) = match bytes.iter().rposition(|&b| b == b'.') {
        Some(dot) => (
            bytes.get(..dot).unwrap_or(bytes),
            bytes.get(dot..).unwrap_or(&[]),
        ),
        None => (bytes, &[][..]),
    };

    for i in 2u32..1000 {
        let suffix = format_u32(i);
        let suffix_len = suffix.len().wrapping_add(1); // "_N"

        // Truncate the base to fit within 8 bytes: base + "_" + N.
        let max_base = 8usize.saturating_sub(suffix_len);
        let truncated_base = base.get(..max_base.min(base.len())).unwrap_or(base);

        let mut candidate = PathBuf::with_capacity(
            truncated_base
                .len()
                .saturating_add(suffix_len)
                .saturating_add(ext.len()),
        );
        candidate.extend_bytes(truncated_base);
        candidate.extend_bytes(b"_");
        candidate.extend_bytes(suffix.as_bytes());
        candidate.extend_bytes(ext);

        let check = format_trash_path(&candidate);
        if Vfs::stat(&check).is_err() {
            return Ok(candidate);
        }
    }

    Err(KernelError::AlreadyExists)
}

/// Format the full path to a file in the trash directory.
fn format_trash_path(name: &Path) -> PathBuf {
    Path::new(TRASH_DIR).join(name)
}

/// Recursively delete a directory and all its contents.
///
/// Walks the directory tree depth-first, removing files first, then
/// empty directories.  Returns the first error encountered, but
/// continues trying to delete remaining items.
fn recursive_delete(path: &Path) -> KernelResult<()> {
    let entries = Vfs::readdir(path)?;
    let mut worst_error: Option<KernelError> = None;

    for entry in &entries {
        let child_path = path.join(&entry.name);

        let result = if entry.entry_type == EntryType::Directory {
            recursive_delete(&child_path)
        } else {
            Vfs::remove(&child_path)
        };

        if let Err(e) = result {
            worst_error = Some(e);
        }
    }

    // Now the directory should be empty — remove it.
    if let Err(e) = Vfs::rmdir(path) {
        worst_error = Some(e);
    }

    match worst_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Index file management
// ---------------------------------------------------------------------------
//
// The index file (`/_TRASH/_INDEX`) is a simple line-delimited text
// file mapping trash filenames to their original paths:
//
//     HELLO.TXT=/docs/HELLO.TXT
//     REPORT.TXT=/work/REPORT.TXT
//
// This design keeps all metadata in a single file, avoiding the FAT
// 8.3 naming issue of per-file companion files.

/// Load the full index file contents.
///
/// Returned as raw bytes rather than a `String`: escaping guarantees the file
/// is ASCII, but a truncated or externally-mangled index must degrade to
/// "some records unreadable" rather than to "whole index discarded", which is
/// what the old `from_utf8(..).unwrap_or("")` did.
fn index_load() -> Vec<u8> {
    Vfs::read_file(INDEX_FILE).unwrap_or_default()
}

/// Split one index record into its escaped name and escaped original path.
fn index_split(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let eq = line.iter().position(|&b| b == b'=')?;
    Some((line.get(..eq)?, line.get(eq.wrapping_add(1)..)?))
}

/// Whether an index record names `trash_name`.
///
/// The comparison is case-insensitive because the trash lives on whatever
/// filesystem the deleted file did, and FAT — the case-insensitive one — will
/// happily return `TRTEST.TXT` for a file created as `trtest.txt`.  It runs on
/// the *decoded* name so that escaping cannot make two equal names compare
/// unequal.
fn index_name_matches(escaped_name: &[u8], trash_name: &Path) -> bool {
    unescape_octal(escaped_name)
        .is_some_and(|name| Path::new(name.as_slice()).eq_ignore_ascii_case(trash_name))
}

/// Look up the original path for a trashed filename.
fn index_lookup(index_content: &[u8], trash_name: &Path) -> Option<PathBuf> {
    for line in index_content.split(|&b| b == b'\n') {
        // Each record: "ESCAPED_TRASH_NAME=ESCAPED_ORIGINAL_PATH"
        let Some((name, original)) = index_split(line) else {
            continue;
        };
        if index_name_matches(name, trash_name) {
            // A record whose value will not decode is corrupt; reporting
            // `None` is better than handing back a path that names a
            // different file.
            return unescape_octal(original).map(PathBuf::from);
        }
    }
    None
}

/// Add an entry to the index file.
fn index_add(trash_name: &Path, original_path: &Path) -> KernelResult<()> {
    let mut content = index_load();

    // Append the new entry.  Both fields are escaped, so neither can contain
    // the `=` field separator or the `\n` record separator.
    content.extend_from_slice(escape_octal(trash_name.as_bytes(), INDEX_ESCAPE).as_bytes());
    content.push(b'=');
    content.extend_from_slice(escape_octal(original_path.as_bytes(), INDEX_ESCAPE).as_bytes());
    content.push(b'\n');

    Vfs::write_file(INDEX_FILE, &content)
}

/// Remove an entry from the index file.
fn index_remove(trash_name: &Path) -> KernelResult<()> {
    let content = index_load();
    if content.is_empty() {
        return Ok(());
    }

    // Rebuild without the matching record.
    let mut new_content: Vec<u8> = Vec::with_capacity(content.len());
    for line in content.split(|&b| b == b'\n') {
        // A trailing newline yields a final empty slice; dropping it here is
        // what keeps the file from growing a blank line on every rewrite.
        if line.is_empty() {
            continue;
        }
        if let Some((name, _)) = index_split(line) {
            if index_name_matches(name, trash_name) {
                continue; // Skip this entry.
            }
        }
        new_content.extend_from_slice(line);
        new_content.push(b'\n');
    }

    if new_content.is_empty() {
        // Index is empty — delete the file.
        let _ = Vfs::remove(INDEX_FILE);
        Ok(())
    } else {
        Vfs::write_file(INDEX_FILE, &new_content)
    }
}

/// Format a u32 as a decimal string.
fn format_u32(mut n: u32) -> String {
    if n == 0 {
        return String::from("0");
    }

    let mut digits = [0u8; 10];
    let mut len = 0usize;
    while n > 0 {
        if let Some(slot) = digits.get_mut(len) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        len = len.wrapping_add(1);
    }

    let mut s = String::with_capacity(len);
    for i in (0..len).rev() {
        if let Some(&d) = digits.get(i) {
            s.push(d as char);
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run a self-test of the recycle bin system.
///
/// Creates a test file, trashes it, lists trash, restores it, and
/// verifies the data is intact.  Then trashes it again and empties
/// the bin.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[trash] Running self-test...");

    // Self-skip on a read-only root.
    //
    // Everything below writes to "/": the test file, the `_TRASH` directory and
    // its index.  That precondition used to be enforced from the outside by
    // `if fat_ok` in main.rs, which was never the right question — it asks
    // whether the *FAT* root mounted, not whether the root is writable — and
    // which kept this suite out of CI entirely, where the root is a perfectly
    // writable memfs.  Probing directly is both narrower and actually correct.
    //
    // A probe write rather than a mount-flag check because that is the real
    // precondition: it accounts for a read-only mount, a quota, a file tag and
    // anything else that could stand between here and a successful write.
    //
    // But `.is_err()` is not the right way to read the probe. It reads as
    // "the root is not writable" and means "the write failed for *any* reason"
    // — which includes the permission gate wrongly refusing us, and the trash
    // machinery's own bugs. Under that form the whole suite deleted itself and
    // returned `Ok`. So: ask the mount table whether there is a root at all,
    // and classify the probe's error — only "this system cannot"
    // (`ReadOnlyFilesystem` and friends) is a reason to skip.
    let root_mounted = Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == crate::fs::path::Path::new("/"));
    if !root_mounted {
        crate::serial_println!("[trash]   / is not mounted — skipping self-test.");
        return Ok(());
    }
    let probe = "/_trash_writable_probe";
    match crate::fs::selftest::classify(Vfs::write_file(probe, b"")) {
        crate::fs::selftest::Setup::Ready => {}
        crate::fs::selftest::Setup::Unsupported(_) => {
            crate::serial_println!("[trash]   Root is not writable — skipping self-test.");
            return Ok(());
        }
        crate::fs::selftest::Setup::Failed(e) => {
            crate::serial_println!(
                "[trash]   FAIL: / is mounted but writing {} failed: {:?} — that is not a \
                 missing feature, so it is a defect rather than a reason to skip",
                probe,
                e
            );
            return Err(KernelError::InternalError);
        }
    }
    // Absence is the expected end state, so a failure here is nothing to report.
    let _ = Vfs::remove(probe);

    // Clean up any leftover from previous runs.
    let _ = Vfs::remove("/_TRASH/_INDEX");
    let _ = Vfs::remove("/_TRASH/TRTEST.TXT");
    let _ = Vfs::remove("/TRTEST.TXT");
    let _ = Vfs::rmdir("/_TRASH");

    // Create a test file.
    let test_data = b"Recycle bin self-test data: 0123456789 ABCDEFGHIJ\n";
    Vfs::write_file("/TRTEST.TXT", test_data)?;

    // Trash it.
    trash("/TRTEST.TXT")?;

    // Verify the file is gone from its original location.
    match Vfs::stat("/TRTEST.TXT") {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[trash]   File removed from original location ✓");
        }
        Ok(_) => {
            crate::serial_println!("[trash]   FAIL: file still exists at original path");
            return Err(KernelError::InternalError);
        }
        Err(e) => return Err(e),
    }

    // List trash — should contain our file.
    let items = list()?;
    crate::serial_println!("[trash]   Trash contains {} item(s)", items.len());
    let found = items
        .iter()
        .find(|i| i.trash_name.eq_ignore_ascii_case("TRTEST.TXT"));
    if found.is_none() {
        crate::serial_println!("[trash]   FAIL: TRTEST.TXT not found in trash listing");
        return Err(KernelError::InternalError);
    }
    let item = found.expect("checked above");
    crate::serial_println!(
        "[trash]   Found: '{}' from '{:?}' ({} bytes) ✓",
        item.trash_name.display(),
        item.original_path,
        item.size
    );

    // Verify the index records the original path.
    if item.original_path.as_deref() != Some(Path::new("/TRTEST.TXT")) {
        crate::serial_println!(
            "[trash]   FAIL: original path is '{:?}', expected '/TRTEST.TXT'",
            item.original_path
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[trash]   Origin path correct ✓");

    // Restore the file.
    let restored_path = restore("TRTEST.TXT")?;
    if restored_path.as_path() != Path::new("/TRTEST.TXT") {
        crate::serial_println!(
            "[trash]   FAIL: restored to '{}', not '/TRTEST.TXT'",
            restored_path.display()
        );
        return Err(KernelError::InternalError);
    }

    // Verify the file data is intact.
    let readback = Vfs::read_file("/TRTEST.TXT")?;
    if readback.as_slice() != test_data.as_slice() {
        crate::serial_println!(
            "[trash]   FAIL: restored data mismatch ({} vs {} bytes)",
            readback.len(),
            test_data.len()
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[trash]   Restored data verified ({} bytes) ✓",
        readback.len()
    );

    // Trash it again to test empty().
    trash("/TRTEST.TXT")?;
    let items_before = list()?;
    crate::serial_println!(
        "[trash]   Trash has {} item(s) before empty",
        items_before.len()
    );

    empty()?;

    let items_after = list()?;
    if !items_after.is_empty() {
        crate::serial_println!(
            "[trash]   FAIL: trash not empty after empty() ({} items)",
            items_after.len()
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[trash]   Trash empty after empty() ✓");

    // --- The index is total over legal filenames ---
    //
    // A path is an uninterpreted byte string, so a filename may legally
    // contain the index's own delimiters (`=`, `\n`) and bytes that are not
    // UTF-8 at all.  Before escaping, such a name either round-tripped as a
    // *different* name or split its record in two — which silently rewrote an
    // unrelated entry's original path and so made a file restorable to an
    // attacker-chosen location.
    //
    // This exercises the index directly rather than by trashing a real file,
    // which keeps the case reachable on every root.  On a FAT root the
    // filesystem physically cannot store such a name (its long names are
    // UCS-2), so it would reject it long before the index saw it; on a memfs or
    // ext4 root — which is what CI actually boots — the name stores fine and a
    // round-trip through a real file would work, but it would then be testing
    // the filesystem as much as the index.  Going straight at the index tests
    // the thing that was broken, on any root.
    {
        ensure_trash_dir()?;
        let hostile_name = Path::new(b"tr\xffk=y\nname.txt".as_slice());
        let hostile_orig = Path::new(b"/a b/\xfe=q\nr/tr\xffk=y\nname.txt".as_slice());
        // A second, ordinary record that a split of the hostile one would
        // corrupt — the bug is only visible when there is a neighbour to hurt.
        let plain_name = Path::new("PLAIN.TXT");
        let plain_orig = Path::new("/PLAIN.TXT");

        index_add(hostile_name, hostile_orig)?;
        index_add(plain_name, plain_orig)?;

        let index = index_load();
        // Escaping is what lets the index be parsed at all: every record must
        // be one line with exactly one `=`.
        if !index.is_ascii() {
            crate::serial_println!("[trash]   FAIL: index is not pure ASCII after escaping");
            return Err(KernelError::InternalError);
        }
        let records = index
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .count();
        if records != 2 {
            crate::serial_println!(
                "[trash]   FAIL: index has {} records, expected 2 (record split?)",
                records
            );
            return Err(KernelError::InternalError);
        }

        if index_lookup(&index, hostile_name).as_deref() != Some(hostile_orig) {
            crate::serial_println!("[trash]   FAIL: hostile name did not round-trip the index");
            return Err(KernelError::InternalError);
        }
        if index_lookup(&index, plain_name).as_deref() != Some(plain_orig) {
            crate::serial_println!("[trash]   FAIL: hostile record corrupted its neighbour");
            return Err(KernelError::InternalError);
        }

        // Removal must match the same name it stored, and leave the neighbour.
        index_remove(hostile_name)?;
        let index = index_load();
        if index_lookup(&index, hostile_name).is_some() {
            crate::serial_println!("[trash]   FAIL: hostile record survived removal");
            return Err(KernelError::InternalError);
        }
        if index_lookup(&index, plain_name).as_deref() != Some(plain_orig) {
            crate::serial_println!("[trash]   FAIL: removal took the neighbour with it");
            return Err(KernelError::InternalError);
        }
        index_remove(plain_name)?;
        crate::serial_println!("[trash]   Index total over non-UTF-8 / delimiter names ✓");
    }

    // Clean up the trash directory itself.
    let _ = Vfs::rmdir(TRASH_DIR);

    crate::serial_println!("[trash] Self-test passed.");
    Ok(())
}
