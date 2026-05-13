//! File sealing — immutability contracts for shared files.
//!
//! File sealing (inspired by Linux's `memfd_seal`) provides a mechanism
//! to place irrevocable restrictions on file operations. Once a seal
//! is applied, it cannot be removed. This enables:
//!
//! - **Shared memory safety**: a process sharing a file can seal it to
//!   prevent the other side from growing, shrinking, or writing it.
//! - **Config integrity**: seal configuration files after loading to
//!   prevent runtime tampering.
//! - **IPC buffers**: seal message buffers so the receiver knows the
//!   content won't change mid-processing.
//!
//! ## Seal Types
//!
//! | Seal          | Effect                                     |
//! |---------------|--------------------------------------------|
//! | SealShrink    | Prevents truncation (file cannot get smaller)|
//! | SealGrow      | Prevents extension (file cannot get larger) |
//! | SealWrite     | Prevents all writes (content is immutable)  |
//! | SealSeal      | Prevents adding more seals                  |
//! | SealExec      | Prevents execute permission changes         |
//!
//! ## Design Notes
//!
//! - Seals are per-path (not per-fd) in our VFS model.
//! - Seals are additive and irrevocable — once set, a seal can never
//!   be removed (except by deleting the file).
//! - SealSeal must be applied last; after it, no further seals can
//!   be added.
//! - VFS integration: `check_seals(path, operation)` returns whether
//!   the operation is permitted given current seals.
//! - Maximum tracked sealed files: 512.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum sealed files tracked.
const MAX_SEALED_FILES: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Seal flags (bitfield).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealFlags(u32);

impl SealFlags {
    /// No seals.
    pub const NONE: Self = Self(0);
    /// Cannot shrink (truncate to smaller size).
    pub const SHRINK: Self = Self(1 << 0);
    /// Cannot grow (write/truncate to larger size).
    pub const GROW: Self = Self(1 << 1);
    /// Cannot write any data.
    pub const WRITE: Self = Self(1 << 2);
    /// Cannot add more seals.
    pub const SEAL: Self = Self(1 << 3);
    /// Cannot change execute permissions.
    pub const EXEC: Self = Self(1 << 4);

    /// All seals combined.
    pub const ALL: Self = Self(0x1F);

    /// Check if a specific seal is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Combine seals.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Check if no seals are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Human-readable representation of active seals.
    pub fn label(self) -> String {
        if self.is_empty() {
            return String::from("none");
        }
        let mut parts = Vec::new();
        if self.contains(Self::SHRINK) { parts.push("shrink"); }
        if self.contains(Self::GROW) { parts.push("grow"); }
        if self.contains(Self::WRITE) { parts.push("write"); }
        if self.contains(Self::SEAL) { parts.push("seal"); }
        if self.contains(Self::EXEC) { parts.push("exec"); }
        let joined = parts.join("+");
        String::from(joined.as_str())
    }

    /// Parse from a comma/plus-separated string.
    pub fn from_str(s: &str) -> Self {
        let mut flags = Self::NONE;
        for part in s.split(&[',', '+', '|']) {
            match part.trim() {
                "shrink" | "noshrink" => flags = flags.union(Self::SHRINK),
                "grow" | "nogrow" => flags = flags.union(Self::GROW),
                "write" | "nowrite" | "immutable" => flags = flags.union(Self::WRITE),
                "seal" | "noseal" => flags = flags.union(Self::SEAL),
                "exec" | "noexec" => flags = flags.union(Self::EXEC),
                "all" => flags = Self::ALL,
                _ => {}
            }
        }
        flags
    }
}

/// Operation type for seal checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealOp {
    /// Writing data to the file.
    Write,
    /// Truncating to a smaller size.
    Shrink,
    /// Extending to a larger size.
    Grow,
    /// Adding new seals.
    AddSeal,
    /// Changing execute permission.
    ChangeExec,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A sealed file entry.
#[derive(Debug, Clone)]
struct SealEntry {
    path: String,
    flags: SealFlags,
    sealed_at_ns: u64,
}

/// Sealed file table.
static SEAL_TABLE: spin::Mutex<Vec<SealEntry>> = spin::Mutex::new(Vec::new());

/// Statistics.
static SEAL_OPS: AtomicU64 = AtomicU64::new(0);
static CHECK_OPS: AtomicU64 = AtomicU64::new(0);
static DENIED_OPS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Add seals to a file.
///
/// Seals are additive — new seals are OR'd with existing ones.
/// Returns an error if SealSeal is already set (no more seals allowed).
pub fn add_seals(path: &str, new_seals: SealFlags) -> KernelResult<SealFlags> {
    if path.is_empty() || new_seals.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    SEAL_OPS.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let mut table = SEAL_TABLE.lock();

    // Check existing entry.
    if let Some(entry) = table.iter_mut().find(|e| e.path == path) {
        // Cannot add seals if SEAL is already set.
        if entry.flags.contains(SealFlags::SEAL) {
            DENIED_OPS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::PermissionDenied);
        }
        entry.flags = entry.flags.union(new_seals);
        return Ok(entry.flags);
    }

    // New entry.
    if table.len() >= MAX_SEALED_FILES {
        return Err(KernelError::ResourceExhausted);
    }

    let flags = new_seals;
    table.push(SealEntry {
        path: String::from(path),
        flags,
        sealed_at_ns: now,
    });

    Ok(flags)
}

/// Get current seals for a file.
pub fn get_seals(path: &str) -> SealFlags {
    let table = SEAL_TABLE.lock();
    table.iter()
        .find(|e| e.path == path)
        .map_or(SealFlags::NONE, |e| e.flags)
}

/// Check if an operation is permitted given current seals.
///
/// Returns Ok(()) if the operation is allowed, or Err if a seal
/// blocks it. This is the VFS integration point.
pub fn check_seals(path: &str, op: SealOp) -> KernelResult<()> {
    CHECK_OPS.fetch_add(1, Ordering::Relaxed);

    let seals = get_seals(path);
    if seals.is_empty() {
        return Ok(()); // No seals, everything allowed.
    }

    let denied = match op {
        SealOp::Write => seals.contains(SealFlags::WRITE),
        SealOp::Shrink => seals.contains(SealFlags::SHRINK) || seals.contains(SealFlags::WRITE),
        SealOp::Grow => seals.contains(SealFlags::GROW) || seals.contains(SealFlags::WRITE),
        SealOp::AddSeal => seals.contains(SealFlags::SEAL),
        SealOp::ChangeExec => seals.contains(SealFlags::EXEC),
    };

    if denied {
        DENIED_OPS.fetch_add(1, Ordering::Relaxed);
        Err(KernelError::PermissionDenied)
    } else {
        Ok(())
    }
}

/// Remove all seals for a file (used when the file is deleted).
///
/// This is the only way to remove seals — by deleting the file itself.
/// Regular unseal operations are not supported by design.
pub fn remove_on_delete(path: &str) {
    let mut table = SEAL_TABLE.lock();
    table.retain(|e| e.path != path);
}

/// List all sealed files.
pub fn list_sealed() -> Vec<(String, SealFlags)> {
    let table = SEAL_TABLE.lock();
    table.iter().map(|e| (e.path.clone(), e.flags)).collect()
}

/// Get statistics.
pub fn stats() -> (u64, u64, u64, usize) {
    let sealed_count = SEAL_TABLE.lock().len();
    (
        SEAL_OPS.load(Ordering::Relaxed),
        CHECK_OPS.load(Ordering::Relaxed),
        DENIED_OPS.load(Ordering::Relaxed),
        sealed_count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    SEAL_OPS.store(0, Ordering::Relaxed);
    CHECK_OPS.store(0, Ordering::Relaxed);
    DENIED_OPS.store(0, Ordering::Relaxed);
}

/// Clear all seal entries (for testing).
pub fn clear_all() {
    SEAL_TABLE.lock().clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[sealing] Running self-test...");

    test_add_get_seals();
    test_seal_check();
    test_seal_seal();
    test_write_implies_shrink_grow();
    test_flags_parse();
    test_remove_on_delete();

    serial_println!("[sealing] Self-test passed (6 tests).");
    Ok(())
}

fn test_add_get_seals() {
    let path = "/test/seal_basic";

    // No seals initially.
    assert!(get_seals(path).is_empty());

    // Add SHRINK seal.
    let result = add_seals(path, SealFlags::SHRINK).unwrap();
    assert!(result.contains(SealFlags::SHRINK));
    assert!(!result.contains(SealFlags::GROW));

    // Add GROW seal (additive).
    let result2 = add_seals(path, SealFlags::GROW).unwrap();
    assert!(result2.contains(SealFlags::SHRINK));
    assert!(result2.contains(SealFlags::GROW));

    // Query.
    let seals = get_seals(path);
    assert!(seals.contains(SealFlags::SHRINK));
    assert!(seals.contains(SealFlags::GROW));

    remove_on_delete(path);
    serial_println!("[sealing]   add_get_seals: ok");
}

fn test_seal_check() {
    let path = "/test/seal_check";

    add_seals(path, SealFlags::SHRINK).unwrap();

    // Shrink denied.
    assert!(check_seals(path, SealOp::Shrink).is_err());

    // Write allowed (no WRITE seal).
    assert!(check_seals(path, SealOp::Write).is_ok());

    // Grow allowed (no GROW seal).
    assert!(check_seals(path, SealOp::Grow).is_ok());

    remove_on_delete(path);
    serial_println!("[sealing]   seal_check: ok");
}

fn test_seal_seal() {
    let path = "/test/seal_seal";

    add_seals(path, SealFlags::SHRINK).unwrap();
    add_seals(path, SealFlags::SEAL).unwrap();

    // Cannot add more seals.
    assert!(add_seals(path, SealFlags::GROW).is_err());

    // Existing seals still enforced.
    assert!(check_seals(path, SealOp::Shrink).is_err());

    remove_on_delete(path);
    serial_println!("[sealing]   seal_seal: ok");
}

fn test_write_implies_shrink_grow() {
    let path = "/test/seal_write";

    add_seals(path, SealFlags::WRITE).unwrap();

    // WRITE seal blocks write, shrink, AND grow.
    assert!(check_seals(path, SealOp::Write).is_err());
    assert!(check_seals(path, SealOp::Shrink).is_err());
    assert!(check_seals(path, SealOp::Grow).is_err());

    // But AddSeal and ChangeExec still allowed.
    assert!(check_seals(path, SealOp::AddSeal).is_ok());
    assert!(check_seals(path, SealOp::ChangeExec).is_ok());

    remove_on_delete(path);
    serial_println!("[sealing]   write_implies: ok");
}

fn test_flags_parse() {
    let flags = SealFlags::from_str("shrink+grow+write");
    assert!(flags.contains(SealFlags::SHRINK));
    assert!(flags.contains(SealFlags::GROW));
    assert!(flags.contains(SealFlags::WRITE));
    assert!(!flags.contains(SealFlags::SEAL));

    let flags2 = SealFlags::from_str("all");
    assert!(flags2.contains(SealFlags::ALL));

    let label = SealFlags::SHRINK.union(SealFlags::WRITE).label();
    assert!(label.contains("shrink"));
    assert!(label.contains("write"));

    assert_eq!(SealFlags::NONE.label(), "none");

    serial_println!("[sealing]   flags_parse: ok");
}

fn test_remove_on_delete() {
    let path = "/test/seal_delete";

    add_seals(path, SealFlags::ALL).unwrap();
    assert!(!get_seals(path).is_empty());

    remove_on_delete(path);
    assert!(get_seals(path).is_empty());

    serial_println!("[sealing]   remove_on_delete: ok");
}
