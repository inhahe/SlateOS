//! Filesystem audit logging.
//!
//! Records a structured audit trail of filesystem operations for
//! security monitoring and forensic analysis.  Each audit entry
//! captures who performed what operation on which path, when, and
//! the result (success/failure).
//!
//! ## Architecture
//!
//! Audit events are stored in a bounded ring buffer (default 4096
//! entries).  When the buffer is full, the oldest entries are
//! discarded.  The buffer is lock-protected and designed for
//! low overhead on the hot path.
//!
//! ## Event types
//!
//! The audit system tracks:
//! - File reads, writes, and deletes
//! - Directory creation and removal
//! - Permission and ownership changes
//! - Symlink and hardlink operations
//! - Mount/unmount operations
//! - Attribute changes (xattrs, ACLs)
//!
//! ## Filtering
//!
//! Events can be filtered by:
//! - Operation type (bitmask)
//! - Path prefix (watch specific directories)
//! - UID (track specific users)
//! - Success/failure only
//!
//! ## Performance
//!
//! Audit logging is optional and disabled by default.  When enabled,
//! each audited operation adds ~100ns overhead (one lock acquisition
//! + ring buffer write).  Path-prefix filtering avoids logging
//!   operations on uninteresting paths.
//!
//! ## Reference
//!
//! Linux audit(8), auditd(8), auditctl(8), ausearch(8)

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Categories of auditable filesystem operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AuditOp {
    Read = 1,
    Write = 2,
    Delete = 4,
    Mkdir = 8,
    Rmdir = 16,
    Rename = 32,
    Chmod = 64,
    Chown = 128,
    Link = 256,
    Symlink = 512,
    Mount = 1024,
    Unmount = 2048,
    XattrSet = 4096,
    XattrRemove = 8192,
    Open = 16384,
    Close = 32768,
    Truncate = 65536,
    Exec = 131072,
}

impl AuditOp {
    /// Human-readable name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "READ",
            Self::Write => "WRITE",
            Self::Delete => "DELETE",
            Self::Mkdir => "MKDIR",
            Self::Rmdir => "RMDIR",
            Self::Rename => "RENAME",
            Self::Chmod => "CHMOD",
            Self::Chown => "CHOWN",
            Self::Link => "LINK",
            Self::Symlink => "SYMLINK",
            Self::Mount => "MOUNT",
            Self::Unmount => "UNMOUNT",
            Self::XattrSet => "XATTR_SET",
            Self::XattrRemove => "XATTR_RM",
            Self::Open => "OPEN",
            Self::Close => "CLOSE",
            Self::Truncate => "TRUNCATE",
            Self::Exec => "EXEC",
        }
    }

    /// Convert from raw bitmask value.
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            4 => Some(Self::Delete),
            8 => Some(Self::Mkdir),
            16 => Some(Self::Rmdir),
            32 => Some(Self::Rename),
            64 => Some(Self::Chmod),
            128 => Some(Self::Chown),
            256 => Some(Self::Link),
            512 => Some(Self::Symlink),
            1024 => Some(Self::Mount),
            2048 => Some(Self::Unmount),
            4096 => Some(Self::XattrSet),
            8192 => Some(Self::XattrRemove),
            16384 => Some(Self::Open),
            32768 => Some(Self::Close),
            65536 => Some(Self::Truncate),
            131072 => Some(Self::Exec),
            _ => None,
        }
    }
}

/// Bitmask of operation types to audit.
#[derive(Debug, Clone, Copy)]
pub struct AuditMask(pub u32);

impl AuditMask {
    /// Audit nothing.
    pub const NONE: Self = Self(0);
    /// Audit all operations.
    pub const ALL: Self = Self(0x0003_FFFF);
    /// Audit write-like operations only (write, delete, mkdir, rmdir, rename, chmod, chown, link, symlink, truncate).
    pub const WRITES: Self = Self(
        AuditOp::Write as u32 | AuditOp::Delete as u32 | AuditOp::Mkdir as u32
        | AuditOp::Rmdir as u32 | AuditOp::Rename as u32 | AuditOp::Chmod as u32
        | AuditOp::Chown as u32 | AuditOp::Link as u32 | AuditOp::Symlink as u32
        | AuditOp::Truncate as u32
    );
    /// Audit security-relevant operations (chmod, chown, mount, unmount, xattr, exec).
    pub const SECURITY: Self = Self(
        AuditOp::Chmod as u32 | AuditOp::Chown as u32 | AuditOp::Mount as u32
        | AuditOp::Unmount as u32 | AuditOp::XattrSet as u32 | AuditOp::XattrRemove as u32
        | AuditOp::Exec as u32
    );

    /// Check if a specific operation is in this mask.
    pub fn contains(self, op: AuditOp) -> bool {
        self.0 & (op as u32) != 0
    }
}

/// A single audit log entry.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// Timestamp (seconds since epoch, 0 if unavailable).
    pub timestamp: u64,
    /// Operation type.
    pub op: AuditOp,
    /// UID that performed the operation (0 = root/kernel).
    pub uid: u32,
    /// Primary path (subject of the operation).
    pub path: String,
    /// Secondary path (destination for rename/link, target for symlink).
    pub path2: Option<String>,
    /// Whether the operation succeeded.
    pub success: bool,
    /// Error code if the operation failed.
    pub error_code: Option<i32>,
    /// Additional context (e.g., new permissions for chmod).
    pub detail: Option<String>,
}

/// An audit rule — defines what to audit.
#[derive(Debug, Clone)]
pub struct AuditRule {
    /// Unique rule ID.
    pub id: u64,
    /// Path prefix to match (empty = match all).
    pub path_prefix: String,
    /// Operation mask.
    pub mask: AuditMask,
    /// UID filter (None = all users).
    pub uid: Option<u32>,
    /// Whether to log only failures.
    pub failures_only: bool,
    /// Whether this rule is enabled.
    pub enabled: bool,
}

/// Summary statistics.
#[derive(Debug, Clone, Copy)]
pub struct AuditStats {
    pub total_events: u64,
    pub dropped_events: u64,
    pub rules_count: usize,
    pub buffer_size: usize,
    pub buffer_used: usize,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// Default audit buffer capacity.
const DEFAULT_BUFFER_SIZE: usize = 4096;

struct AuditBuffer {
    entries: Vec<Option<AuditEntry>>,
    write_pos: usize,
    count: usize,
}

impl AuditBuffer {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            write_pos: 0,
            count: 0,
        }
    }

    fn init(&mut self, capacity: usize) {
        if self.entries.is_empty() {
            self.entries.resize_with(capacity, || None);
        }
    }

    fn push(&mut self, entry: AuditEntry) {
        if self.entries.is_empty() {
            self.init(DEFAULT_BUFFER_SIZE);
        }
        let cap = self.entries.len();
        self.entries[self.write_pos] = Some(entry);
        self.write_pos = (self.write_pos + 1) % cap;
        if self.count < cap {
            self.count += 1;
        }
    }

    /// Read entries from newest to oldest, up to `max`.
    fn recent(&self, max: usize) -> Vec<AuditEntry> {
        let to_read = max.min(self.count);
        let cap = self.entries.len();
        let mut result = Vec::with_capacity(to_read);

        for i in 0..to_read {
            let idx = (self.write_pos + cap - 1 - i) % cap;
            if let Some(entry) = &self.entries[idx] {
                result.push(entry.clone());
            }
        }

        result
    }

    /// Get all entries matching a filter, newest first.
    fn search<F>(&self, max: usize, predicate: F) -> Vec<AuditEntry>
    where
        F: Fn(&AuditEntry) -> bool,
    {
        let cap = self.entries.len();
        let mut result = Vec::new();

        for i in 0..self.count {
            if result.len() >= max {
                break;
            }
            let idx = (self.write_pos + cap - 1 - i) % cap;
            if let Some(entry) = &self.entries[idx] {
                if predicate(entry) {
                    result.push(entry.clone());
                }
            }
        }

        result
    }

    fn clear(&mut self) {
        for slot in &mut self.entries {
            *slot = None;
        }
        self.write_pos = 0;
        self.count = 0;
    }

    fn used(&self) -> usize {
        self.count
    }

    fn capacity(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct AuditInner {
    buffer: AuditBuffer,
    rules: BTreeMap<u64, AuditRule>,
    next_rule_id: u64,
    next_seq: u64,
    total_events: u64,
    dropped_events: u64,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

static AUDIT: Mutex<AuditInner> = Mutex::new(AuditInner {
    buffer: AuditBuffer::new(),
    rules: BTreeMap::new(),
    next_rule_id: 1,
    next_seq: 1,
    total_events: 0,
    dropped_events: 0,
});

// ---------------------------------------------------------------------------
// Public API — configuration
// ---------------------------------------------------------------------------

/// Enable audit logging.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
    AUDIT.lock().buffer.init(DEFAULT_BUFFER_SIZE);
}

/// Disable audit logging.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Check if audit is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Add an audit rule.  Returns the rule ID.
pub fn add_rule(
    path_prefix: &str,
    mask: AuditMask,
    uid: Option<u32>,
    failures_only: bool,
) -> u64 {
    let mut inner = AUDIT.lock();
    let id = inner.next_rule_id;
    inner.next_rule_id = inner.next_rule_id.wrapping_add(1);

    inner.rules.insert(id, AuditRule {
        id,
        path_prefix: path_prefix.into(),
        mask,
        uid,
        failures_only,
        enabled: true,
    });

    id
}

/// Remove an audit rule.
pub fn remove_rule(id: u64) -> bool {
    AUDIT.lock().rules.remove(&id).is_some()
}

/// Enable/disable a specific rule.
pub fn set_rule_enabled(id: u64, enabled: bool) -> KernelResult<()> {
    let mut inner = AUDIT.lock();
    let rule = inner.rules.get_mut(&id).ok_or(KernelError::NotFound)?;
    rule.enabled = enabled;
    Ok(())
}

/// List all rules.
pub fn list_rules() -> Vec<AuditRule> {
    AUDIT.lock().rules.values().cloned().collect()
}

/// Clear the audit log buffer.
pub fn clear() {
    let mut inner = AUDIT.lock();
    inner.buffer.clear();
}

/// Get statistics.
pub fn stats() -> AuditStats {
    let inner = AUDIT.lock();
    AuditStats {
        total_events: inner.total_events,
        dropped_events: inner.dropped_events,
        rules_count: inner.rules.len(),
        buffer_size: inner.buffer.capacity(),
        buffer_used: inner.buffer.used(),
        enabled: is_enabled(),
    }
}

// ---------------------------------------------------------------------------
// Public API — event recording
// ---------------------------------------------------------------------------

/// Record an audit event.
///
/// This is the hot-path function called from VFS operations.  It
/// checks the global enable flag (atomic, no lock) and returns
/// immediately if disabled.
pub fn log_event(
    op: AuditOp,
    uid: u32,
    path: &str,
    path2: Option<&str>,
    success: bool,
    error_code: Option<i32>,
    detail: Option<&str>,
) {
    // Fast path: check atomic flag without locking.
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);

    let mut inner = AUDIT.lock();

    // Check if any rule matches.
    let mut matched = false;
    for rule in inner.rules.values() {
        if !rule.enabled {
            continue;
        }
        if !rule.mask.contains(op) {
            continue;
        }
        if !rule.path_prefix.is_empty() && !path.starts_with(&rule.path_prefix) {
            continue;
        }
        if let Some(rule_uid) = rule.uid {
            if rule_uid != uid {
                continue;
            }
        }
        if rule.failures_only && success {
            continue;
        }
        matched = true;
        break;
    }

    if !matched {
        return;
    }

    let seq = inner.next_seq;
    inner.next_seq = inner.next_seq.wrapping_add(1);
    inner.total_events = inner.total_events.saturating_add(1);

    // Get timestamp.
    let timestamp = crate::timekeeping::clock_realtime() / 1_000_000_000;

    inner.buffer.push(AuditEntry {
        seq,
        timestamp,
        op,
        uid,
        path: path.into(),
        path2: path2.map(Into::into),
        success,
        error_code,
        detail: detail.map(Into::into),
    });
}

/// Convenience: log a successful operation.
pub fn log_ok(op: AuditOp, uid: u32, path: &str) {
    log_event(op, uid, path, None, true, None, None);
}

/// Convenience: log a failed operation.
pub fn log_err(op: AuditOp, uid: u32, path: &str, err: KernelError) {
    log_event(op, uid, path, None, false, Some(err.code()), None);
}

/// Convenience: log a two-path operation (rename, link).
pub fn log_two_path(op: AuditOp, uid: u32, from: &str, to: &str, success: bool) {
    log_event(op, uid, from, Some(to), success, None, None);
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Get the most recent `n` audit entries.
pub fn recent(n: usize) -> Vec<AuditEntry> {
    AUDIT.lock().buffer.recent(n)
}

/// Search for entries matching criteria.
pub fn search(
    max: usize,
    op_filter: Option<AuditOp>,
    path_prefix: Option<&str>,
    uid_filter: Option<u32>,
    failures_only: bool,
) -> Vec<AuditEntry> {
    let inner = AUDIT.lock();
    inner.buffer.search(max, |entry| {
        if let Some(op) = op_filter {
            if entry.op != op {
                return false;
            }
        }
        if let Some(prefix) = path_prefix {
            if !entry.path.starts_with(prefix) {
                return false;
            }
        }
        if let Some(uid) = uid_filter {
            if entry.uid != uid {
                return false;
            }
        }
        if failures_only && entry.success {
            return false;
        }
        true
    })
}

/// Get entries since a specific sequence number.
pub fn since(seq: u64, max: usize) -> Vec<AuditEntry> {
    let inner = AUDIT.lock();
    inner.buffer.search(max, |entry| entry.seq >= seq)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the audit logging module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[audit] Running self-test...");

    // Save and restore enabled state.
    let was_enabled = is_enabled();

    // --- Test 1: Enable/disable ---
    {
        enable();
        if !is_enabled() {
            serial_println!("[audit]   ERROR: not enabled after enable()");
            return Err(KernelError::InternalError);
        }
        disable();
        if is_enabled() {
            serial_println!("[audit]   ERROR: still enabled after disable()");
            return Err(KernelError::InternalError);
        }
        serial_println!("[audit]   enable/disable: OK");
    }

    // --- Test 2: Events ignored when disabled ---
    {
        disable();
        clear();
        log_ok(AuditOp::Read, 0, "/test");
        let entries = recent(10);
        if !entries.is_empty() {
            serial_println!("[audit]   ERROR: event logged while disabled");
            return Err(KernelError::InternalError);
        }
        serial_println!("[audit]   disabled ignores events: OK");
    }

    // --- Test 3: Add rule and log matching event ---
    {
        enable();
        clear();

        // Remove any leftover rules.
        for rule in list_rules() {
            remove_rule(rule.id);
        }

        let rule_id = add_rule("/test", AuditMask::ALL, None, false);

        log_ok(AuditOp::Write, 1000, "/test/file.txt");

        let entries = recent(10);
        if entries.is_empty() {
            serial_println!("[audit]   ERROR: matching event not logged");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        let e = &entries[0];
        if e.op != AuditOp::Write || e.uid != 1000 || e.path != "/test/file.txt" || !e.success {
            serial_println!("[audit]   ERROR: event data mismatch");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   rule match + log: OK");
    }

    // --- Test 4: Path prefix filtering ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("/important", AuditMask::ALL, None, false);

        log_ok(AuditOp::Read, 0, "/important/secret.txt");
        log_ok(AuditOp::Read, 0, "/tmp/scratch.txt");  // Should not match.

        let entries = recent(10);
        if entries.len() != 1 {
            serial_println!("[audit]   ERROR: expected 1 entry, got {}", entries.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }
        if entries[0].path != "/important/secret.txt" {
            serial_println!("[audit]   ERROR: wrong path logged");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   path prefix filter: OK");
    }

    // --- Test 5: Operation mask filtering ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("", AuditMask::WRITES, None, false);

        log_ok(AuditOp::Read, 0, "/foo");   // Should NOT be logged (read not in WRITES).
        log_ok(AuditOp::Write, 0, "/bar");  // Should be logged.
        log_ok(AuditOp::Delete, 0, "/baz"); // Should be logged.

        let entries = recent(10);
        if entries.len() != 2 {
            serial_println!("[audit]   ERROR: expected 2 entries, got {}", entries.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   op mask filter: OK");
    }

    // --- Test 6: UID filtering ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("", AuditMask::ALL, Some(1000), false);

        log_ok(AuditOp::Read, 1000, "/user/file");  // Matches.
        log_ok(AuditOp::Read, 0, "/root/file");      // Doesn't match.

        let entries = recent(10);
        if entries.len() != 1 {
            serial_println!("[audit]   ERROR: expected 1 entry, got {}", entries.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   UID filter: OK");
    }

    // --- Test 7: Failures-only filter ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("", AuditMask::ALL, None, true);

        log_ok(AuditOp::Read, 0, "/success");
        log_err(AuditOp::Write, 0, "/failure", KernelError::PermissionDenied);

        let entries = recent(10);
        if entries.len() != 1 {
            serial_println!("[audit]   ERROR: expected 1 entry (failure only), got {}", entries.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }
        if entries[0].success {
            serial_println!("[audit]   ERROR: success event logged with failures_only");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   failures-only filter: OK");
    }

    // --- Test 8: Two-path logging ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("", AuditMask::ALL, None, false);

        log_two_path(AuditOp::Rename, 0, "/old/path", "/new/path", true);

        let entries = recent(10);
        if entries.is_empty() {
            serial_println!("[audit]   ERROR: rename not logged");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }
        let e = &entries[0];
        if e.path2.as_deref() != Some("/new/path") {
            serial_println!("[audit]   ERROR: path2 not recorded");
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   two-path logging: OK");
    }

    // --- Test 9: Search function ---
    {
        clear();
        for rule in list_rules() { remove_rule(rule.id); }

        let rule_id = add_rule("", AuditMask::ALL, None, false);

        log_ok(AuditOp::Read, 0, "/a");
        log_ok(AuditOp::Write, 0, "/b");
        log_ok(AuditOp::Read, 1000, "/c");

        let reads = search(10, Some(AuditOp::Read), None, None, false);
        if reads.len() != 2 {
            serial_println!("[audit]   ERROR: search found {} reads, expected 2", reads.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        let user_events = search(10, None, None, Some(1000), false);
        if user_events.len() != 1 {
            serial_println!("[audit]   ERROR: search found {} user events, expected 1", user_events.len());
            remove_rule(rule_id);
            return Err(KernelError::InternalError);
        }

        remove_rule(rule_id);
        serial_println!("[audit]   search: OK");
    }

    // --- Test 10: Stats ---
    {
        let s = stats();
        if s.total_events == 0 {
            serial_println!("[audit]   ERROR: total events is 0");
            return Err(KernelError::InternalError);
        }
        serial_println!("[audit]   stats: OK (total={} buffer={}/{})",
            s.total_events, s.buffer_used, s.buffer_size);
    }

    // --- Cleanup ---
    clear();
    for rule in list_rules() { remove_rule(rule.id); }
    if was_enabled { enable(); } else { disable(); }

    serial_println!("[audit] Self-test passed (10 tests).");
    Ok(())
}
