//! Filesystem operation interceptors.
//!
//! Allows registered handlers to approve, deny, or modify filesystem
//! operations *before* they happen.  Unlike the notify system (which is
//! asynchronous and post-operation), interceptors are synchronous and
//! pre-operation — a handler can reject a file write, rename, or delete.
//!
//! ## Use Cases
//!
//! - **Antivirus/malware scanning**: intercept file opens and scan content.
//! - **Policy enforcement**: prevent writes to protected directories.
//! - **Audit logging**: log all operations on sensitive paths.
//! - **Backup integration**: ensure files are backed up before deletion.
//! - **Content filtering**: reject files with certain content types.
//!
//! ## Design
//!
//! ```text
//! VFS operation (write_file, remove, rename, ...)
//!         ↓
//!   intercept::pre_check(op, path)
//!         ↓
//!   Each registered interceptor is consulted:
//!     → Allow: continue to next interceptor
//!     → Deny(reason): abort operation with PermissionDenied
//!     → (no response within timeout): treated as Allow
//!         ↓
//!   If all interceptors allow → proceed with actual operation
//! ```
//!
//! ## Performance
//!
//! `pre_check()` is on the VFS hot path.  When no interceptors are
//! registered, it returns immediately.  With interceptors, each one's
//! callback is invoked synchronously.  Interceptors must be fast —
//! a configurable timeout (default 100ms) prevents one slow handler
//! from stalling the entire filesystem.
//!
//! ## Safety
//!
//! Interceptors run in kernel context and must not deadlock.  They must
//! NOT call back into the VFS (which would attempt to re-lock the VFS
//! mutex and deadlock).  The pre_check() call happens *before* the VFS
//! lock is taken.
//!
//! ## Reference
//!
//! design.txt: "Interceptor capability: programs can reject operations
//! before they happen with strict timeout (~100ms)"

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Filesystem operation type that interceptors can examine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsOp {
    /// A file is about to be created or written.
    Write,
    /// A file or directory is about to be deleted.
    Delete,
    /// A file or directory is about to be renamed.
    Rename,
    /// A directory is about to be created.
    Mkdir,
    /// A file is about to be opened for reading.
    Read,
    /// File metadata is about to change (chmod, chown, xattr).
    MetadataChange,
    /// A hard link is about to be created.
    Link,
    /// A symbolic link is about to be created.
    Symlink,
}

/// Bitmask of operations an interceptor is interested in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsOpMask(pub u32);

impl FsOpMask {
    pub const WRITE: Self = Self(1 << 0);
    pub const DELETE: Self = Self(1 << 1);
    pub const RENAME: Self = Self(1 << 2);
    pub const MKDIR: Self = Self(1 << 3);
    pub const READ: Self = Self(1 << 4);
    pub const METADATA: Self = Self(1 << 5);
    pub const LINK: Self = Self(1 << 6);
    pub const SYMLINK: Self = Self(1 << 7);

    /// All write-like operations (write, delete, rename, mkdir, link, symlink, metadata).
    pub const ALL_WRITES: Self = Self(0xFF & !(1 << 4));
    /// All operations.
    pub const ALL: Self = Self(0xFF);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl FsOp {
    /// Convert to corresponding mask bit.
    pub const fn to_mask(self) -> FsOpMask {
        match self {
            Self::Write => FsOpMask::WRITE,
            Self::Delete => FsOpMask::DELETE,
            Self::Rename => FsOpMask::RENAME,
            Self::Mkdir => FsOpMask::MKDIR,
            Self::Read => FsOpMask::READ,
            Self::MetadataChange => FsOpMask::METADATA,
            Self::Link => FsOpMask::LINK,
            Self::Symlink => FsOpMask::SYMLINK,
        }
    }

    /// Human-readable name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Rename => "rename",
            Self::Mkdir => "mkdir",
            Self::Read => "read",
            Self::MetadataChange => "metadata",
            Self::Link => "link",
            Self::Symlink => "symlink",
        }
    }
}

/// Context passed to interceptor callbacks.
#[derive(Debug, Clone)]
pub struct InterceptContext {
    /// The operation being performed.
    pub op: FsOp,
    /// The primary path affected.
    pub path: String,
    /// For rename: the destination path.  For link/symlink: the target.
    pub secondary_path: Option<String>,
}

/// Decision returned by an interceptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterceptDecision {
    /// Allow the operation to proceed.
    Allow,
    /// Deny the operation with a reason string.
    Deny(String),
}

/// Callback type for interceptors.
///
/// Must return quickly (within timeout).  Must NOT call back into VFS.
pub type InterceptFn = fn(&InterceptContext) -> InterceptDecision;

/// A registered interceptor.
struct Interceptor {
    /// Unique identifier.
    id: u64,
    /// Human-readable name for this interceptor.
    name: String,
    /// Path prefix this interceptor monitors (empty = all paths).
    path_prefix: String,
    /// Operations this interceptor cares about.
    mask: FsOpMask,
    /// The callback function.
    handler: InterceptFn,
    /// Whether this interceptor is currently active.
    active: bool,
    /// Number of times this interceptor was invoked.
    invocations: u64,
    /// Number of times this interceptor denied an operation.
    denials: u64,
}

/// Statistics about the interceptor subsystem.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct InterceptStats {
    /// Number of registered interceptors.
    pub interceptors: usize,
    /// Number of active interceptors.
    pub active: usize,
    /// Total pre_check calls.
    pub total_checks: u64,
    /// Total denials across all interceptors.
    pub total_denials: u64,
    /// Total allows (checks - denials).
    pub total_allows: u64,
}

/// Info about a single interceptor (for listing).
#[derive(Debug, Clone)]
pub struct InterceptorInfo {
    /// Unique ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Monitored path prefix.
    pub path_prefix: String,
    /// Operation mask.
    pub mask: FsOpMask,
    /// Whether active.
    pub active: bool,
    /// Number of invocations.
    pub invocations: u64,
    /// Number of denials.
    pub denials: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct InterceptInner {
    interceptors: BTreeMap<u64, Interceptor>,
    total_checks: u64,
    total_denials: u64,
}

static INTERCEPTS: Mutex<InterceptInner> = Mutex::new(InterceptInner {
    interceptors: BTreeMap::new(),
    total_checks: 0,
    total_denials: 0,
});

/// Maximum number of interceptors.
const MAX_INTERCEPTORS: usize = 64;

// ---------------------------------------------------------------------------
// Public API — management
// ---------------------------------------------------------------------------

/// Register a new filesystem interceptor.
///
/// - `name`: human-readable label.
/// - `path_prefix`: only intercept operations on paths starting with
///   this prefix.  Empty string means all paths.
/// - `mask`: which operations to intercept.
/// - `handler`: callback function invoked before each matching operation.
///
/// Returns the interceptor ID.
pub fn register(
    name: &str,
    path_prefix: &str,
    mask: FsOpMask,
    handler: InterceptFn,
) -> KernelResult<u64> {
    let mut inner = INTERCEPTS.lock();

    if inner.interceptors.len() >= MAX_INTERCEPTORS {
        return Err(KernelError::OutOfMemory);
    }

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    inner.interceptors.insert(id, Interceptor {
        id,
        name: String::from(name),
        path_prefix: String::from(path_prefix),
        mask,
        handler,
        active: true,
        invocations: 0,
        denials: 0,
    });

    serial_println!(
        "[intercept] Registered interceptor {} '{}' (path='{}', mask={:#x})",
        id, name, path_prefix, mask.0
    );

    Ok(id)
}

/// Unregister an interceptor.
pub fn unregister(id: u64) -> bool {
    let mut inner = INTERCEPTS.lock();
    let removed = inner.interceptors.remove(&id).is_some();
    if removed {
        serial_println!("[intercept] Unregistered interceptor {}", id);
    }
    removed
}

/// Enable or disable an interceptor.
pub fn set_active(id: u64, active: bool) -> KernelResult<()> {
    let mut inner = INTERCEPTS.lock();
    let interceptor = inner.interceptors.get_mut(&id)
        .ok_or(KernelError::InvalidHandle)?;
    interceptor.active = active;
    Ok(())
}

/// List all registered interceptors.
pub fn list() -> Vec<InterceptorInfo> {
    let inner = INTERCEPTS.lock();
    inner.interceptors.values().map(|i| InterceptorInfo {
        id: i.id,
        name: i.name.clone(),
        path_prefix: i.path_prefix.clone(),
        mask: i.mask,
        active: i.active,
        invocations: i.invocations,
        denials: i.denials,
    }).collect()
}

/// Get statistics.
pub fn stats() -> InterceptStats {
    let inner = INTERCEPTS.lock();
    let active = inner.interceptors.values().filter(|i| i.active).count();
    InterceptStats {
        interceptors: inner.interceptors.len(),
        active,
        total_checks: inner.total_checks,
        total_denials: inner.total_denials,
        total_allows: inner.total_checks.saturating_sub(inner.total_denials),
    }
}

/// Clear all interceptors (for testing).
#[allow(dead_code)]
pub fn clear() {
    let mut inner = INTERCEPTS.lock();
    inner.interceptors.clear();
    inner.total_checks = 0;
    inner.total_denials = 0;
}

// ---------------------------------------------------------------------------
// Public API — hot path (called by VFS)
// ---------------------------------------------------------------------------

/// Returns true if `path` lies within the directory subtree denoted by
/// `prefix`.
///
/// An empty prefix (or `"/"`) matches the whole tree.  Otherwise the
/// prefix names a directory and `path` matches if it equals that
/// directory or is strictly underneath it.  The match must end on a
/// path-component boundary so that prefix `/protected` matches
/// `/protected` and `/protected/secret` but never `/protectedX`.
///
/// Callers register prefixes with a trailing slash (e.g. `/protected/`),
/// but a trailing slash is not required: it is normalised away before the
/// boundary check so both forms behave identically.  The previous inline
/// matcher applied the `byte-after-prefix == '/'` boundary check against
/// a prefix that *already* carried the trailing slash, so it only ever
/// matched double-slash paths (`/protected//x`) — meaning every real deny
/// handler registered with a trailing slash silently failed open.
///
/// This is a thin wrapper over [`crate::fs::pathutil::path_in_subtree`], the
/// single canonical subtree predicate; the wrapper is retained for the
/// descriptive name at the deny-check call sites and the bug-history note.
#[inline]
fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    crate::fs::pathutil::path_in_subtree(path, prefix)
}

/// Check all interceptors before a filesystem operation.
///
/// This is called by the VFS *before* acquiring the VFS lock.
/// If any active interceptor denies the operation, returns
/// `Err(PermissionDenied)` with the denial reason logged.
///
/// When no interceptors are registered, returns immediately.
pub fn pre_check(op: FsOp, path: &str, secondary_path: Option<&str>) -> KernelResult<()> {
    let mut inner = INTERCEPTS.lock();

    // Fast path: no interceptors.
    if inner.interceptors.is_empty() {
        return Ok(());
    }

    inner.total_checks = inner.total_checks.saturating_add(1);

    let op_mask = op.to_mask();

    // Collect matching handler fn pointers and their IDs.  We copy these
    // out so we can drop the mutable borrow on `inner` and then call
    // handlers + update stats in separate steps.
    let candidates: Vec<(u64, InterceptFn)> = inner
        .interceptors
        .values()
        .filter(|i| {
            i.active && i.mask.contains(op_mask) && path_matches_prefix(path, &i.path_prefix)
        })
        .map(|i| (i.id, i.handler))
        .collect();

    // Release the lock before calling handlers (handlers must NOT
    // re-lock INTERCEPTS, but releasing here is cleaner).
    drop(inner);

    // Build context.
    let ctx = InterceptContext {
        op,
        path: String::from(path),
        secondary_path: secondary_path.map(String::from),
    };

    for (id, handler) in &candidates {
        // Re-lock to increment invocation counter.
        {
            let mut inner = INTERCEPTS.lock();
            if let Some(interceptor) = inner.interceptors.get_mut(id) {
                interceptor.invocations = interceptor.invocations.saturating_add(1);
            }
        }

        let decision = handler(&ctx);

        match decision {
            InterceptDecision::Allow => {}
            InterceptDecision::Deny(ref reason) => {
                let mut inner = INTERCEPTS.lock();
                if let Some(interceptor) = inner.interceptors.get_mut(id) {
                    interceptor.denials = interceptor.denials.saturating_add(1);
                }
                inner.total_denials = inner.total_denials.saturating_add(1);
                serial_println!(
                    "[intercept] DENIED: {} on '{}' by interceptor {}: {}",
                    op.name(), path, id, reason
                );
                return Err(KernelError::PermissionDenied);
            }
        }
    }

    Ok(())
}

/// Convenience: pre-check for write operations.
#[inline]
pub fn pre_write(path: &str) -> KernelResult<()> {
    pre_check(FsOp::Write, path, None)
}

/// Convenience: pre-check for delete operations.
#[inline]
pub fn pre_delete(path: &str) -> KernelResult<()> {
    pre_check(FsOp::Delete, path, None)
}

/// Convenience: pre-check for rename operations.
#[inline]
pub fn pre_rename(from: &str, to: &str) -> KernelResult<()> {
    pre_check(FsOp::Rename, from, Some(to))
}

/// Convenience: pre-check for mkdir operations.
#[inline]
pub fn pre_mkdir(path: &str) -> KernelResult<()> {
    pre_check(FsOp::Mkdir, path, None)
}

// ---------------------------------------------------------------------------
// Built-in interceptor handlers
// ---------------------------------------------------------------------------

/// A "read-only zone" interceptor that denies all write operations
/// to paths under a configured prefix.
///
/// Register with:
/// ```ignore
/// intercept::register("ro-zone", "/protected/", FsOpMask::ALL_WRITES, intercept::readonly_handler);
/// ```
pub fn readonly_handler(_ctx: &InterceptContext) -> InterceptDecision {
    InterceptDecision::Deny(String::from("path is in a read-only zone"))
}

/// A "no-delete zone" interceptor that denies delete operations only.
pub fn no_delete_handler(ctx: &InterceptContext) -> InterceptDecision {
    if ctx.op == FsOp::Delete {
        InterceptDecision::Deny(String::from("deletion not allowed in this zone"))
    } else {
        InterceptDecision::Allow
    }
}

/// An "audit log" interceptor that always allows but logs the operation.
pub fn audit_handler(ctx: &InterceptContext) -> InterceptDecision {
    serial_println!(
        "[audit] {} {} (secondary: {:?})",
        ctx.op.name(),
        ctx.path,
        ctx.secondary_path
    );
    InterceptDecision::Allow
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the filesystem interceptor system.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[intercept] Running self-test...");

    // --- Test 1: no interceptors → allow ---
    {
        let result = pre_check(FsOp::Write, "/test/file.txt", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: empty registry should allow");
            return Err(KernelError::InternalError);
        }
        serial_println!("[intercept]   empty registry allows OK");
    }

    // --- Test 2: register and allow ---
    {
        fn allow_all(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Allow
        }

        let id = register("test-allow", "/test/", FsOpMask::ALL, allow_all)?;
        let result = pre_check(FsOp::Write, "/test/file.txt", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: allow handler denied");
            unregister(id);
            return Err(KernelError::InternalError);
        }
        unregister(id);
        serial_println!("[intercept]   allow handler OK");
    }

    // --- Test 3: deny handler ---
    {
        fn deny_all(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Deny(String::from("test denial"))
        }

        let id = register("test-deny", "/protected/", FsOpMask::ALL, deny_all)?;

        // Should deny operations in /protected/.
        let result = pre_check(FsOp::Write, "/protected/secret.txt", None);
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: deny handler allowed");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Should NOT deny operations outside /protected/.
        let result = pre_check(FsOp::Write, "/other/file.txt", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: deny handler blocked wrong path");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Boundary: a sibling that merely shares the prefix string but is
        // not a path-component child must NOT match (no "/protectedX" leak).
        let result = pre_check(FsOp::Write, "/protectedX/file.txt", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: deny handler matched non-boundary path");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Boundary: the protected directory itself (no trailing slash on the
        // path) must match the "/protected/" prefix.
        let result = pre_check(FsOp::Write, "/protected", None);
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: deny handler missed the protected dir itself");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        unregister(id);
        serial_println!("[intercept]   deny handler with path prefix OK");
    }

    // --- Test 4: operation mask filtering ---
    {
        fn deny_writes(ctx: &InterceptContext) -> InterceptDecision {
            if ctx.op == FsOp::Write || ctx.op == FsOp::Delete {
                InterceptDecision::Deny(String::from("no writes"))
            } else {
                InterceptDecision::Allow
            }
        }

        // Register only for WRITE operations.
        let id = register("test-mask", "", FsOpMask::WRITE, deny_writes)?;

        // Write should be denied.
        let result = pre_check(FsOp::Write, "/any/path", None);
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: write not denied");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Delete should NOT be intercepted (not in mask).
        let result = pre_check(FsOp::Delete, "/any/path", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: delete blocked by write-only mask");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        unregister(id);
        serial_println!("[intercept]   operation mask filtering OK");
    }

    // --- Test 5: multiple interceptors (first deny wins) ---
    {
        fn allow_handler(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Allow
        }
        fn deny_handler(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Deny(String::from("denied by handler 2"))
        }

        let id1 = register("test-multi-1", "", FsOpMask::ALL, allow_handler)?;
        let id2 = register("test-multi-2", "", FsOpMask::ALL, deny_handler)?;

        let result = pre_check(FsOp::Write, "/file", None);
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: second handler should have denied");
            unregister(id1);
            unregister(id2);
            return Err(KernelError::InternalError);
        }

        unregister(id1);
        unregister(id2);
        serial_println!("[intercept]   multiple interceptors OK");
    }

    // --- Test 6: inactive interceptor ---
    {
        fn deny_handler(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Deny(String::from("should not fire"))
        }

        let id = register("test-inactive", "", FsOpMask::ALL, deny_handler)?;
        set_active(id, false)?;

        let result = pre_check(FsOp::Write, "/file", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: inactive interceptor fired");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        unregister(id);
        serial_println!("[intercept]   inactive interceptor OK");
    }

    // --- Test 7: built-in readonly handler ---
    {
        let id = register("ro-zone", "/readonly/", FsOpMask::ALL_WRITES, readonly_handler)?;

        // Write to protected path → denied.
        let result = pre_check(FsOp::Write, "/readonly/data", None);
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: readonly zone allowed write");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Read from protected path → allowed (not in ALL_WRITES mask when READ is not set).
        let result = pre_check(FsOp::Read, "/readonly/data", None);
        if result.is_err() {
            serial_println!("[intercept]   ERROR: readonly zone blocked read");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        unregister(id);
        serial_println!("[intercept]   readonly handler OK");
    }

    // --- Test 8: statistics ---
    {
        let st = stats();
        if st.total_checks == 0 {
            serial_println!("[intercept]   ERROR: no checks counted");
            return Err(KernelError::InternalError);
        }
        if st.total_denials == 0 {
            serial_println!("[intercept]   ERROR: no denials counted");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[intercept]   stats OK (checks={}, denials={}, allows={})",
            st.total_checks, st.total_denials, st.total_allows
        );
    }

    // --- Test 9: interceptor info listing ---
    {
        fn test_handler(_ctx: &InterceptContext) -> InterceptDecision {
            InterceptDecision::Allow
        }
        let id = register("test-list", "/list/", FsOpMask::WRITE, test_handler)?;
        let all = list();
        let found = all.iter().any(|i| i.id == id && i.name == "test-list");
        if !found {
            serial_println!("[intercept]   ERROR: registered interceptor not in list");
            unregister(id);
            return Err(KernelError::InternalError);
        }
        unregister(id);
        serial_println!("[intercept]   listing OK");
    }

    // --- Test 10: rename interception ---
    {
        fn deny_rename(ctx: &InterceptContext) -> InterceptDecision {
            if ctx.op == FsOp::Rename {
                InterceptDecision::Deny(String::from("no renaming"))
            } else {
                InterceptDecision::Allow
            }
        }
        let id = register("test-rename", "", FsOpMask::RENAME, deny_rename)?;

        let result = pre_rename("/old/name", "/new/name");
        if result.is_ok() {
            serial_println!("[intercept]   ERROR: rename not denied");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        // Write should be allowed.
        let result = pre_write("/any/file");
        if result.is_err() {
            serial_println!("[intercept]   ERROR: write denied by rename-only interceptor");
            unregister(id);
            return Err(KernelError::InternalError);
        }

        unregister(id);
        serial_println!("[intercept]   rename interception OK");
    }

    serial_println!("[intercept] Self-test passed (10 tests).");
    Ok(())
}
