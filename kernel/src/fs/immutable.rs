//! Immutable and append-only file flags.
//!
//! Provides `chattr`-style file flags that restrict modifications:
//! - **Immutable**: file cannot be modified, deleted, renamed, or linked.
//!   Only a privileged user can set/clear the flag.
//! - **Append-only**: file can only be appended to, not overwritten or
//!   truncated.  Useful for log files.
//! - **No-delete**: file can be modified but not deleted or renamed.
//!   Weaker than immutable but useful for protecting important files.
//!
//! ## Design Reference
//!
//! design.txt lines 394-395: "Consider: immutable flag (file can't be
//! modified or deleted until flag is cleared by a privileged user),
//! append-only flag (for log files)."
//!
//! ## Architecture
//!
//! ```text
//! set_flags("/var/log/system.log", FileFlags::APPEND_ONLY)
//!   → check caller is privileged
//!   → store flag in FLAG_TABLE
//!
//! VFS write to "/var/log/system.log"
//!   → check_write("/var/log/system.log", offset)
//!   → if APPEND_ONLY and offset != end-of-file → Err(ReadOnlyFilesystem)
//!   → if IMMUTABLE → Err(ReadOnlyFilesystem)
//!   → otherwise → Ok(())
//!
//! VFS delete "/var/log/system.log"
//!   → check_delete("/var/log/system.log")
//!   → if IMMUTABLE or NO_DELETE → Err(PermissionDenied)
//!   → otherwise → Ok(())
//! ```
//!
//! ## Flag Bits
//!
//! Flags are stored as a `u32` bitmask per file, analogous to Linux's
//! ext2/ext4 inode flags.  The VFS layer queries `check_*` functions
//! before performing operations.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Flag definitions
// ---------------------------------------------------------------------------

/// File flag bitmask type.
pub type FlagBits = u32;

/// File flags that restrict operations.
pub struct FileFlags;

impl FileFlags {
    /// File cannot be modified, deleted, renamed, or linked.
    /// Only a privileged caller can set or clear this flag.
    pub const IMMUTABLE: FlagBits = 1 << 0;

    /// File can only be appended to (no overwrites, no truncation).
    /// Useful for log files.
    pub const APPEND_ONLY: FlagBits = 1 << 1;

    /// File cannot be deleted or renamed, but can be modified.
    pub const NO_DELETE: FlagBits = 1 << 2;

    /// File content is compressed on disk (informational — VFS handles
    /// transparently).
    pub const COMPRESSED: FlagBits = 1 << 3;

    /// File should not be backed up by the backup service.
    pub const NO_BACKUP: FlagBits = 1 << 4;

    /// File should not be indexed by the search indexer.
    pub const NO_INDEX: FlagBits = 1 << 5;

    /// File is a system file (protected from casual deletion).
    pub const SYSTEM: FlagBits = 1 << 6;

    /// File is hidden from normal directory listings.
    pub const HIDDEN: FlagBits = 1 << 7;

    /// All known flag bits.
    pub const ALL_KNOWN: FlagBits = 0xFF;
}

/// Human-readable flag names.
const FLAG_NAMES: &[(FlagBits, &str)] = &[
    (FileFlags::IMMUTABLE, "immutable"),
    (FileFlags::APPEND_ONLY, "append-only"),
    (FileFlags::NO_DELETE, "no-delete"),
    (FileFlags::COMPRESSED, "compressed"),
    (FileFlags::NO_BACKUP, "no-backup"),
    (FileFlags::NO_INDEX, "no-index"),
    (FileFlags::SYSTEM, "system"),
    (FileFlags::HIDDEN, "hidden"),
];

/// Convert flag bits to a human-readable comma-separated string.
pub fn flags_to_string(flags: FlagBits) -> String {
    let mut parts = Vec::new();
    for &(bit, name) in FLAG_NAMES {
        if flags & bit != 0 {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        String::from("none")
    } else {
        let mut out = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(part);
        }
        out
    }
}

/// Parse a flag name to its bit.
pub fn parse_flag_name(name: &str) -> Option<FlagBits> {
    for &(bit, flag_name) in FLAG_NAMES {
        if flag_name.eq_ignore_ascii_case(name) {
            return Some(bit);
        }
    }
    // Also accept single-char shortcuts.
    match name {
        "i" => Some(FileFlags::IMMUTABLE),
        "a" => Some(FileFlags::APPEND_ONLY),
        "d" => Some(FileFlags::NO_DELETE),
        "c" => Some(FileFlags::COMPRESSED),
        "b" => Some(FileFlags::NO_BACKUP),
        "n" => Some(FileFlags::NO_INDEX),
        "s" => Some(FileFlags::SYSTEM),
        "h" => Some(FileFlags::HIDDEN),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Maximum files tracked.
const MAX_FILES: usize = 65536;

struct FlagTable {
    /// Path → flags.
    entries: BTreeMap<String, FlagBits>,
}

impl FlagTable {
    const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

static TABLE: Mutex<FlagTable> = Mutex::new(FlagTable::new());
static SET_COUNT: AtomicU64 = AtomicU64::new(0);
static CHECK_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Set flags on a file (OR with existing flags).
pub fn set_flags(path: &str, flags: FlagBits) -> KernelResult<()> {
    if flags & !FileFlags::ALL_KNOWN != 0 {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut table = TABLE.lock();
    if let Some(existing) = table.entries.get_mut(path) {
        *existing |= flags;
    } else {
        if table.entries.len() >= MAX_FILES {
            return Err(KernelError::ResourceExhausted);
        }
        table.entries.insert(String::from(path), flags);
    }
    Ok(())
}

/// Clear specific flags on a file.
pub fn clear_flags(path: &str, flags: FlagBits) -> KernelResult<()> {
    let mut table = TABLE.lock();
    if let Some(existing) = table.entries.get_mut(path) {
        *existing &= !flags;
        if *existing == 0 {
            table.entries.remove(path);
        }
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Replace all flags on a file.
pub fn replace_flags(path: &str, flags: FlagBits) -> KernelResult<()> {
    if flags & !FileFlags::ALL_KNOWN != 0 {
        return Err(KernelError::InvalidArgument);
    }
    SET_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut table = TABLE.lock();
    if flags == 0 {
        table.entries.remove(path);
    } else {
        if !table.entries.contains_key(path) && table.entries.len() >= MAX_FILES {
            return Err(KernelError::ResourceExhausted);
        }
        table.entries.insert(String::from(path), flags);
    }
    Ok(())
}

/// Get flags for a file (0 if none set).
pub fn get_flags(path: &str) -> FlagBits {
    let table = TABLE.lock();
    table.entries.get(path).copied().unwrap_or(0)
}

/// Remove all flags for a file.
pub fn remove_flags(path: &str) -> KernelResult<()> {
    let mut table = TABLE.lock();
    table.entries.remove(path).ok_or(KernelError::NotFound)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Check API — called by VFS before operations
// ---------------------------------------------------------------------------

/// Check whether a write operation is allowed.
///
/// - Immutable files reject all writes.
/// - Append-only files reject writes that are not at end-of-file.
///   `is_append` should be true if the write is an append operation.
pub fn check_write(path: &str, is_append: bool) -> KernelResult<()> {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let flags = get_flags(path);
    if flags & FileFlags::IMMUTABLE != 0 {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    if flags & FileFlags::APPEND_ONLY != 0 && !is_append {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    Ok(())
}

/// Check whether a truncation is allowed.
pub fn check_truncate(path: &str) -> KernelResult<()> {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let flags = get_flags(path);
    if flags & (FileFlags::IMMUTABLE | FileFlags::APPEND_ONLY) != 0 {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    Ok(())
}

/// Check whether a delete/rename is allowed.
pub fn check_delete(path: &str) -> KernelResult<()> {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let flags = get_flags(path);
    if flags & (FileFlags::IMMUTABLE | FileFlags::NO_DELETE) != 0 {
        return Err(KernelError::PermissionDenied);
    }
    Ok(())
}

/// Check whether metadata changes are allowed.
pub fn check_metadata(path: &str) -> KernelResult<()> {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let flags = get_flags(path);
    if flags & FileFlags::IMMUTABLE != 0 {
        return Err(KernelError::ReadOnlyFilesystem);
    }
    Ok(())
}

/// Check whether a link (hard link) can be created to this file.
pub fn check_link(path: &str) -> KernelResult<()> {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let flags = get_flags(path);
    if flags & FileFlags::IMMUTABLE != 0 {
        return Err(KernelError::PermissionDenied);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rename support
// ---------------------------------------------------------------------------

/// Update flag table when a file is renamed.
pub fn rename_path(old_path: &str, new_path: &str) -> KernelResult<()> {
    let mut table = TABLE.lock();
    if let Some(flags) = table.entries.remove(old_path) {
        table.entries.insert(String::from(new_path), flags);
        Ok(())
    } else {
        // No flags on this file — nothing to do.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// List all files with any flags set.
pub fn list_flagged() -> Vec<(String, FlagBits)> {
    let table = TABLE.lock();
    table.entries.iter().map(|(p, f)| (p.clone(), *f)).collect()
}

/// List files with a specific flag set.
pub fn list_with_flag(flag: FlagBits) -> Vec<String> {
    let table = TABLE.lock();
    table.entries.iter()
        .filter(|(_, f)| **f & flag != 0)
        .map(|(p, _)| p.clone())
        .collect()
}

/// Count files with any flags.
pub fn flagged_count() -> usize {
    let table = TABLE.lock();
    table.entries.len()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (flagged_files, set_ops, check_ops).
pub fn stats() -> (usize, u64, u64) {
    (
        flagged_count(),
        SET_COUNT.load(Ordering::Relaxed),
        CHECK_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SET_COUNT.store(0, Ordering::Relaxed);
    CHECK_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all flag data.
pub fn clear_all() {
    let mut table = TABLE.lock();
    table.entries.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the immutable module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: set and get flags.
    {
        set_flags("/test/file.txt", FileFlags::IMMUTABLE)?;
        let flags = get_flags("/test/file.txt");
        assert_eq!(flags, FileFlags::IMMUTABLE);
        serial_println!("[immutable] test 1 passed: set/get flags");
    }

    // Test 2: immutable blocks writes and deletes.
    {
        assert!(check_write("/test/file.txt", false).is_err());
        assert!(check_write("/test/file.txt", true).is_err());
        assert!(check_delete("/test/file.txt").is_err());
        assert!(check_truncate("/test/file.txt").is_err());
        assert!(check_metadata("/test/file.txt").is_err());
        assert!(check_link("/test/file.txt").is_err());
        serial_println!("[immutable] test 2 passed: immutable blocks all operations");
    }

    // Test 3: append-only allows appends but blocks overwrites.
    {
        replace_flags("/test/log.txt", FileFlags::APPEND_ONLY)?;
        assert!(check_write("/test/log.txt", true).is_ok());    // Append OK.
        assert!(check_write("/test/log.txt", false).is_err());  // Overwrite blocked.
        assert!(check_truncate("/test/log.txt").is_err());       // Truncate blocked.
        assert!(check_delete("/test/log.txt").is_ok());          // Delete OK (not NO_DELETE).
        serial_println!("[immutable] test 3 passed: append-only semantics");
    }

    // Test 4: no-delete blocks delete but allows writes.
    {
        replace_flags("/test/important.txt", FileFlags::NO_DELETE)?;
        assert!(check_write("/test/important.txt", false).is_ok());  // Write OK.
        assert!(check_delete("/test/important.txt").is_err());        // Delete blocked.
        serial_println!("[immutable] test 4 passed: no-delete semantics");
    }

    // Test 5: flag combination (OR).
    {
        set_flags("/test/important.txt", FileFlags::APPEND_ONLY)?;
        let flags = get_flags("/test/important.txt");
        assert_eq!(flags, FileFlags::NO_DELETE | FileFlags::APPEND_ONLY);
        assert!(check_write("/test/important.txt", false).is_err()); // Overwrite blocked.
        assert!(check_delete("/test/important.txt").is_err());        // Delete blocked.
        serial_println!("[immutable] test 5 passed: combined flags");
    }

    // Test 6: clear specific flags.
    {
        clear_flags("/test/important.txt", FileFlags::APPEND_ONLY)?;
        let flags = get_flags("/test/important.txt");
        assert_eq!(flags, FileFlags::NO_DELETE);
        assert!(check_write("/test/important.txt", false).is_ok()); // Write OK now.
        serial_println!("[immutable] test 6 passed: clear specific flags");
    }

    // Test 7: rename and list.
    {
        rename_path("/test/important.txt", "/moved/important.txt")?;
        assert_eq!(get_flags("/test/important.txt"), 0);
        assert_eq!(get_flags("/moved/important.txt"), FileFlags::NO_DELETE);

        let flagged = list_flagged();
        assert!(flagged.len() >= 2); // At least file.txt, log.txt, moved/important.txt
        let immutable_files = list_with_flag(FileFlags::IMMUTABLE);
        assert_eq!(immutable_files.len(), 1);
        serial_println!("[immutable] test 7 passed: rename and list");
    }

    clear_all();
    reset_stats();

    serial_println!("[immutable] all 7 self-tests passed");
    Ok(())
}
