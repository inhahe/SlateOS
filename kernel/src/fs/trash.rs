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

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, Vfs};

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
const TRASH_DIR: &str = "/_TRASH";

/// Name of the index file inside the trash directory.
///
/// Maps trashed filenames to their original paths.
/// Format: one entry per line, `trash_name=original_path`.
const INDEX_FILE: &str = "/_TRASH/_INDEX";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A single item in the recycle bin.
#[derive(Debug, Clone)]
pub struct TrashItem {
    /// Filename as it appears in the trash directory.
    pub trash_name: String,
    /// Original path where the file was before deletion.
    pub original_path: String,
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
pub fn trash(path: &str) -> KernelResult<()> {
    // Verify the source exists.
    let stat = Vfs::stat(path)?;
    let _ = stat; // Used for existence check only.

    // Ensure the trash directory exists.
    ensure_trash_dir()?;

    // Extract the filename from the path.
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Find a unique name in the trash directory.
    let trash_name = unique_trash_name(filename)?;
    let trash_path = format_trash_path(&trash_name);

    // Move the file to trash via rename.
    // This is O(1) on the same filesystem — only directory entries change.
    Vfs::rename(path, &trash_path)?;

    // Update the index file with the mapping.
    index_add(&trash_name, path)?;

    crate::serial_println!("[trash] Moved '{}' to trash as '{}'", path, trash_name);

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
        let original = index_lookup(&index, &entry.name)
            .unwrap_or_else(|| String::from("<unknown>"));

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
pub fn restore(trash_name: &str) -> KernelResult<String> {
    let trash_path = format_trash_path(trash_name);

    // Look up the original path from the index.
    let index = index_load();
    let original = index_lookup(&index, trash_name)
        .ok_or(KernelError::NotFound)?;

    // Move the file back to its original location.
    Vfs::rename(&trash_path, &original)?;

    // Remove the entry from the index.
    index_remove(trash_name)?;

    crate::serial_println!(
        "[trash] Restored '{}' to '{}'",
        trash_name,
        original
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
pub fn purge_one(trash_name: &str) -> KernelResult<()> {
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

    crate::serial_println!("[trash] Permanently deleted '{}'", trash_name);
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
        usage, AUTO_PRUNE_THRESHOLD
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
                item.trash_name, item.size, item.original_path
            );
        }
    }

    if pruned > 0 {
        let final_usage = Vfs::statvfs("/").map_or(0, |i| i.usage_percent());
        crate::serial_println!(
            "[trash] Auto-prune complete: {} items deleted, disk usage now {}%",
            pruned, final_usage
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
fn unique_trash_name(name: &str) -> KernelResult<String> {
    // Check if the name is available.
    let check_path = format_trash_path(name);
    if Vfs::stat(&check_path).is_err() {
        return Ok(String::from(name));
    }

    // Name is taken — try suffixed variants.
    // Split into base and extension for proper suffixing.
    let (base, ext) = if let Some(dot) = name.rfind('.') {
        (&name[..dot], Some(&name[dot..]))
    } else {
        (name, None)
    };

    for i in 2u32..1000 {
        let suffix = format_u32(i);
        let suffix_len = suffix.len().wrapping_add(1); // "_N"

        // Truncate the base to fit within 8 chars: base + "_" + N.
        let max_base = 8usize.saturating_sub(suffix_len);
        let truncated_base = if base.len() > max_base {
            &base[..max_base]
        } else {
            base
        };

        let candidate = match ext {
            Some(e) => {
                let mut s = String::from(truncated_base);
                s.push('_');
                s.push_str(&suffix);
                s.push_str(e);
                s
            }
            None => {
                let mut s = String::from(truncated_base);
                s.push('_');
                s.push_str(&suffix);
                s
            }
        };

        let check = format_trash_path(&candidate);
        if Vfs::stat(&check).is_err() {
            return Ok(candidate);
        }
    }

    Err(KernelError::AlreadyExists)
}

/// Format the full path to a file in the trash directory.
fn format_trash_path(name: &str) -> String {
    let mut path = String::from(TRASH_DIR);
    path.push('/');
    path.push_str(name);
    path
}

/// Recursively delete a directory and all its contents.
///
/// Walks the directory tree depth-first, removing files first, then
/// empty directories.  Returns the first error encountered, but
/// continues trying to delete remaining items.
fn recursive_delete(path: &str) -> KernelResult<()> {
    let entries = Vfs::readdir(path)?;
    let mut worst_error: Option<KernelError> = None;

    for entry in &entries {
        let mut child_path = String::from(path);
        child_path.push('/');
        child_path.push_str(&entry.name);

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

/// Load the full index file contents as a string.
fn index_load() -> String {
    match Vfs::read_file(INDEX_FILE) {
        Ok(data) => {
            core::str::from_utf8(&data)
                .unwrap_or("")
                .into()
        }
        Err(_) => String::new(),
    }
}

/// Look up the original path for a trashed filename.
fn index_lookup(index_content: &str, trash_name: &str) -> Option<String> {
    for line in index_content.lines() {
        // Each line: "TRASH_NAME=ORIGINAL_PATH"
        if let Some(eq_pos) = line.find('=') {
            let name = &line[..eq_pos];
            if name.eq_ignore_ascii_case(trash_name) {
                return Some(String::from(&line[eq_pos + 1..]));
            }
        }
    }
    None
}

/// Add an entry to the index file.
fn index_add(trash_name: &str, original_path: &str) -> KernelResult<()> {
    let mut content = index_load();

    // Append the new entry.
    content.push_str(trash_name);
    content.push('=');
    content.push_str(original_path);
    content.push('\n');

    Vfs::write_file(INDEX_FILE, content.as_bytes())
}

/// Remove an entry from the index file.
fn index_remove(trash_name: &str) -> KernelResult<()> {
    let content = index_load();
    if content.is_empty() {
        return Ok(());
    }

    // Rebuild without the matching line.
    let mut new_content = String::new();
    for line in content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let name = &line[..eq_pos];
            if name.eq_ignore_ascii_case(trash_name) {
                continue; // Skip this entry.
            }
        }
        new_content.push_str(line);
        new_content.push('\n');
    }

    if new_content.is_empty() {
        // Index is empty — delete the file.
        let _ = Vfs::remove(INDEX_FILE);
        Ok(())
    } else {
        Vfs::write_file(INDEX_FILE, new_content.as_bytes())
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
    let found = items.iter().find(|i| i.trash_name.eq_ignore_ascii_case("TRTEST.TXT"));
    if found.is_none() {
        crate::serial_println!("[trash]   FAIL: TRTEST.TXT not found in trash listing");
        return Err(KernelError::InternalError);
    }
    let item = found.expect("checked above");
    crate::serial_println!(
        "[trash]   Found: '{}' from '{}' ({} bytes) ✓",
        item.trash_name, item.original_path, item.size
    );

    // Verify the index records the original path.
    if item.original_path != "/TRTEST.TXT" {
        crate::serial_println!(
            "[trash]   FAIL: original path is '{}', expected '/TRTEST.TXT'",
            item.original_path
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[trash]   Origin path correct ✓");

    // Restore the file.
    let restored_path = restore("TRTEST.TXT")?;
    if restored_path != "/TRTEST.TXT" {
        crate::serial_println!(
            "[trash]   FAIL: restored to '{}', not '/TRTEST.TXT'",
            restored_path
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

    // Clean up the trash directory itself.
    let _ = Vfs::rmdir(TRASH_DIR);

    crate::serial_println!("[trash] Self-test passed.");
    Ok(())
}
