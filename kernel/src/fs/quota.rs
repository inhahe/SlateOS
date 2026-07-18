//! Filesystem quotas — per-user and per-group disk usage limits.
//!
//! Quotas prevent any single user (or group) from consuming all available
//! disk space, enabling fair sharing on multi-user systems.
//!
//! ## Design
//!
//! ```text
//! VFS write_file / mkdir / link
//!          ↓
//!   quota::check_quota(uid, bytes)
//!          ↓  (fail fast if over limit)
//!   actual filesystem write
//!          ↓
//!   quota::charge(uid, bytes)
//!          ↓
//!   quota::release(uid, bytes)  ← on delete
//! ```
//!
//! ## Architecture
//!
//! - **Soft limit**: user gets a warning but writes proceed.  A grace
//!   period (default 7 days) allows temporary excess before enforcement.
//! - **Hard limit**: writes are rejected immediately when exceeded.
//! - **Inode (file count) limit**: limits the number of files a user
//!   can create, preventing inode exhaustion attacks.
//! - **In-memory tracking**: usage counters are maintained in memory
//!   for fast hot-path checking.  Persistent quota storage (to survive
//!   reboots) is deferred to when we have a config file format.
//!
//! ## Performance
//!
//! The `check_quota()` function is on the VFS write hot path.  When no
//! quotas are configured (the common early case), it returns immediately.
//! With quotas active, it's a single BTreeMap lookup — O(log n) in the
//! number of users with quotas.
//!
//! ## Reference
//!
//! design.txt: capability-based security, multi-user operation.
//! Linux: `man quota`, `man edquota`, `man repquota`.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Identifies a quota subject — either a user or a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuotaSubject {
    /// Per-user quota, identified by UID.
    User(u32),
    /// Per-group quota, identified by GID.
    Group(u32),
}

/// Quota limits for a single subject.
#[derive(Debug, Clone, Copy)]
pub struct QuotaLimits {
    /// Soft limit on bytes (0 = unlimited).
    pub soft_bytes: u64,
    /// Hard limit on bytes (0 = unlimited).
    pub hard_bytes: u64,
    /// Soft limit on file count (0 = unlimited).
    pub soft_inodes: u64,
    /// Hard limit on file count (0 = unlimited).
    pub hard_inodes: u64,
    /// Grace period in seconds for soft limit violations.
    /// After this period, the soft limit is enforced as hard.
    pub grace_seconds: u64,
}

impl Default for QuotaLimits {
    fn default() -> Self {
        Self {
            soft_bytes: 0,
            hard_bytes: 0,
            soft_inodes: 0,
            hard_inodes: 0,
            grace_seconds: 7 * 24 * 3600, // 7 days
        }
    }
}

/// Current usage tracked for a quota subject.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuotaUsage {
    /// Current bytes used.
    pub bytes_used: u64,
    /// Current file count.
    pub inodes_used: u64,
    /// Timestamp (seconds since boot) when soft byte limit was first
    /// exceeded.  0 means not in violation.
    pub soft_bytes_exceeded_at: u64,
    /// Timestamp when soft inode limit was first exceeded.
    pub soft_inodes_exceeded_at: u64,
}

/// Combined quota info (limits + usage) for reporting.
#[derive(Debug, Clone)]
pub struct QuotaInfo {
    /// The quota subject.
    pub subject: QuotaSubject,
    /// Configured limits.
    pub limits: QuotaLimits,
    /// Current usage.
    pub usage: QuotaUsage,
    /// Whether the subject is currently over the soft byte limit.
    pub over_soft_bytes: bool,
    /// Whether the subject is currently over the hard byte limit.
    pub over_hard_bytes: bool,
    /// Whether the subject is currently over the soft inode limit.
    pub over_soft_inodes: bool,
    /// Whether the subject is currently over the hard inode limit.
    pub over_hard_inodes: bool,
}

/// Result of a quota check — whether the operation is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaCheckResult {
    /// Within limits, proceed.
    Allowed,
    /// Over soft limit but within grace period — warn but allow.
    SoftWarning,
    /// Rejected: over hard limit or past grace period.
    Denied,
}

/// Summary statistics about the quota system.
#[derive(Debug, Clone, Copy)]
pub struct QuotaStats {
    /// Whether quotas are globally enabled.
    pub enabled: bool,
    /// Number of configured quota entries.
    pub entries: usize,
    /// Number of user quotas.
    pub user_quotas: usize,
    /// Number of group quotas.
    pub group_quotas: usize,
    /// Number of subjects currently over soft limit.
    pub over_soft: usize,
    /// Number of subjects currently over hard limit.
    pub over_hard: usize,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct QuotaEntry {
    limits: QuotaLimits,
    usage: QuotaUsage,
}

struct QuotaInner {
    /// Whether quota enforcement is active.
    enabled: bool,
    /// Per-subject quota entries.
    entries: BTreeMap<QuotaSubject, QuotaEntry>,
}

static QUOTAS: Mutex<QuotaInner> = Mutex::new(QuotaInner {
    enabled: false,
    entries: BTreeMap::new(),
});

// ---------------------------------------------------------------------------
// Public API — configuration
// ---------------------------------------------------------------------------

/// Enable or disable quota enforcement globally.
///
/// When disabled, `check_quota()` always returns `Allowed` and no
/// usage tracking occurs.  Existing limits and usage data are preserved.
pub fn set_enabled(enabled: bool) {
    QUOTAS.lock().enabled = enabled;
    serial_println!("[quota] Enforcement {}", if enabled { "enabled" } else { "disabled" });
}

/// Check whether quotas are globally enabled.
pub fn is_enabled() -> bool {
    QUOTAS.lock().enabled
}

/// Set quota limits for a subject.
///
/// Creates a new entry if one doesn't exist.  Preserves existing usage
/// data.  Setting all limits to 0 effectively removes the quota (but
/// usage tracking continues).
pub fn set_limits(subject: QuotaSubject, limits: QuotaLimits) {
    let mut inner = QUOTAS.lock();
    let entry = inner.entries.entry(subject).or_insert(QuotaEntry {
        limits: QuotaLimits::default(),
        usage: QuotaUsage::default(),
    });
    entry.limits = limits;
}

/// Remove quota limits and usage tracking for a subject.
pub fn remove(subject: QuotaSubject) -> bool {
    QUOTAS.lock().entries.remove(&subject).is_some()
}

/// Get the current quota info for a subject.
pub fn get_info(subject: QuotaSubject) -> Option<QuotaInfo> {
    let inner = QUOTAS.lock();
    inner.entries.get(&subject).map(|entry| {
        build_info(subject, entry, current_time_secs())
    })
}

/// List all configured quotas.
pub fn list_all() -> Vec<QuotaInfo> {
    let inner = QUOTAS.lock();
    let now = current_time_secs();
    inner
        .entries
        .iter()
        .map(|(&subject, entry)| build_info(subject, entry, now))
        .collect()
}

/// Get summary statistics.
pub fn stats() -> QuotaStats {
    let inner = QUOTAS.lock();
    let now = current_time_secs();
    let mut user_count = 0usize;
    let mut group_count = 0usize;
    let mut over_soft = 0usize;
    let mut over_hard = 0usize;

    for (&subject, entry) in &inner.entries {
        match subject {
            QuotaSubject::User(_) => {
                user_count = user_count.saturating_add(1);
            }
            QuotaSubject::Group(_) => {
                group_count = group_count.saturating_add(1);
            }
        }
        let info = build_info(subject, entry, now);
        if info.over_soft_bytes || info.over_soft_inodes {
            over_soft = over_soft.saturating_add(1);
        }
        if info.over_hard_bytes || info.over_hard_inodes {
            over_hard = over_hard.saturating_add(1);
        }
    }

    QuotaStats {
        enabled: inner.enabled,
        entries: inner.entries.len(),
        user_quotas: user_count,
        group_quotas: group_count,
        over_soft,
        over_hard,
    }
}

/// Clear all quota entries.
#[allow(dead_code)]
pub fn clear() {
    let mut inner = QUOTAS.lock();
    inner.entries.clear();
}

// ---------------------------------------------------------------------------
// Public API — hot-path checking and usage tracking
// ---------------------------------------------------------------------------

/// Check whether a write of `additional_bytes` is allowed for the given
/// user and group.
///
/// This is the **hot-path** function called before every VFS write.
/// When quotas are disabled, it returns immediately.
///
/// Returns `Allowed` if within limits, `SoftWarning` if over soft limit
/// but within grace period, or `Denied` if over hard limit or past
/// grace period.
pub fn check_write(uid: u32, gid: u32, additional_bytes: u64) -> QuotaCheckResult {
    let inner = QUOTAS.lock();
    if !inner.enabled {
        return QuotaCheckResult::Allowed;
    }

    let now = current_time_secs();

    // Check user quota.
    if let Some(entry) = inner.entries.get(&QuotaSubject::User(uid)) {
        match check_bytes(entry, additional_bytes, now) {
            QuotaCheckResult::Denied => return QuotaCheckResult::Denied,
            QuotaCheckResult::SoftWarning => {
                // Continue to check group — group Denied overrides user SoftWarning.
            }
            QuotaCheckResult::Allowed => {}
        }
    }

    // Check group quota.
    if let Some(entry) = inner.entries.get(&QuotaSubject::Group(gid)) {
        match check_bytes(entry, additional_bytes, now) {
            QuotaCheckResult::Denied => return QuotaCheckResult::Denied,
            r => return r,
        }
    }

    // Check user result again for SoftWarning.
    if let Some(entry) = inner.entries.get(&QuotaSubject::User(uid)) {
        let result = check_bytes(entry, additional_bytes, now);
        if result != QuotaCheckResult::Allowed {
            return result;
        }
    }

    QuotaCheckResult::Allowed
}

/// Check whether creating a new file (inode) is allowed.
///
/// Similar to `check_write()` but checks inode limits instead of bytes.
pub fn check_create(uid: u32, gid: u32) -> QuotaCheckResult {
    let inner = QUOTAS.lock();
    if !inner.enabled {
        return QuotaCheckResult::Allowed;
    }

    let now = current_time_secs();

    // Check user inode quota.
    if let Some(entry) = inner.entries.get(&QuotaSubject::User(uid)) {
        if check_inodes(entry, now) == QuotaCheckResult::Denied { return QuotaCheckResult::Denied }
    }

    // Check group inode quota.
    if let Some(entry) = inner.entries.get(&QuotaSubject::Group(gid)) {
        match check_inodes(entry, now) {
            QuotaCheckResult::Denied => return QuotaCheckResult::Denied,
            r => return r,
        }
    }

    // Re-check user for SoftWarning.
    if let Some(entry) = inner.entries.get(&QuotaSubject::User(uid)) {
        let result = check_inodes(entry, now);
        if result != QuotaCheckResult::Allowed {
            return result;
        }
    }

    QuotaCheckResult::Allowed
}

/// Charge bytes to a user and group after a successful write.
///
/// Call this AFTER the write succeeds.  If the file is new, also call
/// `charge_inode()`.
pub fn charge_bytes(uid: u32, gid: u32, bytes: u64) {
    let mut inner = QUOTAS.lock();
    if !inner.enabled {
        return;
    }

    let now = current_time_secs();

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::User(uid)) {
        entry.usage.bytes_used = entry.usage.bytes_used.saturating_add(bytes);
        update_soft_timestamp_bytes(entry, now);
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::Group(gid)) {
        entry.usage.bytes_used = entry.usage.bytes_used.saturating_add(bytes);
        update_soft_timestamp_bytes(entry, now);
    }
}

/// Release bytes from a user and group after a file deletion or truncation.
pub fn release_bytes(uid: u32, gid: u32, bytes: u64) {
    let mut inner = QUOTAS.lock();
    if !inner.enabled {
        return;
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::User(uid)) {
        entry.usage.bytes_used = entry.usage.bytes_used.saturating_sub(bytes);
        // Clear soft-exceeded timestamp if back under soft limit.
        if entry.limits.soft_bytes > 0 && entry.usage.bytes_used <= entry.limits.soft_bytes {
            entry.usage.soft_bytes_exceeded_at = 0;
        }
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::Group(gid)) {
        entry.usage.bytes_used = entry.usage.bytes_used.saturating_sub(bytes);
        if entry.limits.soft_bytes > 0 && entry.usage.bytes_used <= entry.limits.soft_bytes {
            entry.usage.soft_bytes_exceeded_at = 0;
        }
    }
}

/// Charge one inode (file creation) to a user and group.
pub fn charge_inode(uid: u32, gid: u32) {
    let mut inner = QUOTAS.lock();
    if !inner.enabled {
        return;
    }

    let now = current_time_secs();

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::User(uid)) {
        entry.usage.inodes_used = entry.usage.inodes_used.saturating_add(1);
        update_soft_timestamp_inodes(entry, now);
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::Group(gid)) {
        entry.usage.inodes_used = entry.usage.inodes_used.saturating_add(1);
        update_soft_timestamp_inodes(entry, now);
    }
}

/// Release one inode (file deletion) from a user and group.
pub fn release_inode(uid: u32, gid: u32) {
    let mut inner = QUOTAS.lock();
    if !inner.enabled {
        return;
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::User(uid)) {
        entry.usage.inodes_used = entry.usage.inodes_used.saturating_sub(1);
        if entry.limits.soft_inodes > 0 && entry.usage.inodes_used <= entry.limits.soft_inodes {
            entry.usage.soft_inodes_exceeded_at = 0;
        }
    }

    if let Some(entry) = inner.entries.get_mut(&QuotaSubject::Group(gid)) {
        entry.usage.inodes_used = entry.usage.inodes_used.saturating_sub(1);
        if entry.limits.soft_inodes > 0 && entry.usage.inodes_used <= entry.limits.soft_inodes {
            entry.usage.soft_inodes_exceeded_at = 0;
        }
    }
}

/// Manually set usage values for a subject (used during filesystem scan
/// to initialize usage counters from actual on-disk state).
pub fn set_usage(subject: QuotaSubject, bytes: u64, inodes: u64) {
    let mut inner = QUOTAS.lock();
    let entry = inner.entries.entry(subject).or_insert(QuotaEntry {
        limits: QuotaLimits::default(),
        usage: QuotaUsage::default(),
    });
    entry.usage.bytes_used = bytes;
    entry.usage.inodes_used = inodes;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check whether adding `additional` bytes would violate the quota.
fn check_bytes(entry: &QuotaEntry, additional: u64, now: u64) -> QuotaCheckResult {
    let new_total = entry.usage.bytes_used.saturating_add(additional);

    // Check hard limit.
    if entry.limits.hard_bytes > 0 && new_total > entry.limits.hard_bytes {
        return QuotaCheckResult::Denied;
    }

    // Check soft limit.
    if entry.limits.soft_bytes > 0 && new_total > entry.limits.soft_bytes {
        // Check grace period.
        if entry.usage.soft_bytes_exceeded_at > 0 {
            let elapsed = now.saturating_sub(entry.usage.soft_bytes_exceeded_at);
            if elapsed >= entry.limits.grace_seconds {
                return QuotaCheckResult::Denied;
            }
        }
        return QuotaCheckResult::SoftWarning;
    }

    QuotaCheckResult::Allowed
}

/// Check whether creating one more inode would violate the quota.
fn check_inodes(entry: &QuotaEntry, now: u64) -> QuotaCheckResult {
    let new_total = entry.usage.inodes_used.saturating_add(1);

    // Check hard limit.
    if entry.limits.hard_inodes > 0 && new_total > entry.limits.hard_inodes {
        return QuotaCheckResult::Denied;
    }

    // Check soft limit.
    if entry.limits.soft_inodes > 0 && new_total > entry.limits.soft_inodes {
        if entry.usage.soft_inodes_exceeded_at > 0 {
            let elapsed = now.saturating_sub(entry.usage.soft_inodes_exceeded_at);
            if elapsed >= entry.limits.grace_seconds {
                return QuotaCheckResult::Denied;
            }
        }
        return QuotaCheckResult::SoftWarning;
    }

    QuotaCheckResult::Allowed
}

/// Update the soft-exceeded timestamp for bytes if newly exceeded.
fn update_soft_timestamp_bytes(entry: &mut QuotaEntry, now: u64) {
    if entry.limits.soft_bytes > 0
        && entry.usage.bytes_used > entry.limits.soft_bytes
        && entry.usage.soft_bytes_exceeded_at == 0
    {
        entry.usage.soft_bytes_exceeded_at = now;
    }
}

/// Update the soft-exceeded timestamp for inodes if newly exceeded.
fn update_soft_timestamp_inodes(entry: &mut QuotaEntry, now: u64) {
    if entry.limits.soft_inodes > 0
        && entry.usage.inodes_used > entry.limits.soft_inodes
        && entry.usage.soft_inodes_exceeded_at == 0
    {
        entry.usage.soft_inodes_exceeded_at = now;
    }
}

/// Build a QuotaInfo from an entry.
fn build_info(subject: QuotaSubject, entry: &QuotaEntry, now: u64) -> QuotaInfo {
    let over_soft_bytes = entry.limits.soft_bytes > 0
        && entry.usage.bytes_used > entry.limits.soft_bytes;
    let over_hard_bytes = entry.limits.hard_bytes > 0
        && entry.usage.bytes_used > entry.limits.hard_bytes;
    let over_soft_inodes = entry.limits.soft_inodes > 0
        && entry.usage.inodes_used > entry.limits.soft_inodes;
    let over_hard_inodes = entry.limits.hard_inodes > 0
        && entry.usage.inodes_used > entry.limits.hard_inodes;

    let _ = now; // Reserved for grace period calculations in future.

    QuotaInfo {
        subject,
        limits: entry.limits,
        usage: entry.usage,
        over_soft_bytes,
        over_hard_bytes,
        over_soft_inodes,
        over_hard_inodes,
    }
}

/// Get the current time in seconds since boot.
///
/// Uses HPET if available, falls back to a rough estimate.
fn current_time_secs() -> u64 {
    // Use the hrtimer subsystem if available.
    let ns = crate::hrtimer::now_ns();
    ns / 1_000_000_000
}

/// Format a byte count as a human-readable string (e.g., "1.5 MiB").
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        alloc::format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}.{} KiB", bytes / 1024, (bytes % 1024) * 10 / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        let mib = bytes / (1024 * 1024);
        let frac = (bytes % (1024 * 1024)) * 10 / (1024 * 1024);
        alloc::format!("{}.{} MiB", mib, frac)
    } else {
        let gib = bytes / (1024 * 1024 * 1024);
        let frac = (bytes % (1024 * 1024 * 1024)) * 10 / (1024 * 1024 * 1024);
        alloc::format!("{}.{} GiB", gib, frac)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the filesystem quota system.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[quota] Running self-test...");

    // Save previous state.
    let was_enabled = is_enabled();

    // --- Test 1: basic enable/disable ---
    {
        set_enabled(false);
        if is_enabled() {
            serial_println!("[quota]   ERROR: expected disabled");
            return Err(KernelError::InternalError);
        }
        set_enabled(true);
        if !is_enabled() {
            serial_println!("[quota]   ERROR: expected enabled");
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   enable/disable OK");
    }

    let test_uid = 9999;
    let test_gid = 8888;
    let test_user = QuotaSubject::User(test_uid);
    let test_group = QuotaSubject::Group(test_gid);

    // --- Test 2: set limits and check within limits ---
    {
        set_limits(test_user, QuotaLimits {
            soft_bytes: 10_000,
            hard_bytes: 20_000,
            soft_inodes: 100,
            hard_inodes: 200,
            grace_seconds: 3600,
        });

        // Manually set usage below limits.
        set_usage(test_user, 5_000, 50);

        let result = check_write(test_uid, test_gid, 1_000);
        if result != QuotaCheckResult::Allowed {
            serial_println!("[quota]   ERROR: expected Allowed, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   within-limits check OK");
    }

    // --- Test 3: soft limit violation ---
    {
        set_usage(test_user, 9_000, 50);

        // Adding 2000 → 11000, over soft (10000) but under hard (20000).
        let result = check_write(test_uid, test_gid, 2_000);
        if result != QuotaCheckResult::SoftWarning {
            serial_println!("[quota]   ERROR: expected SoftWarning, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   soft-limit warning OK");
    }

    // --- Test 4: hard limit violation ---
    {
        set_usage(test_user, 15_000, 50);

        // Adding 6000 → 21000, over hard (20000).
        let result = check_write(test_uid, test_gid, 6_000);
        if result != QuotaCheckResult::Denied {
            serial_println!("[quota]   ERROR: expected Denied, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   hard-limit denial OK");
    }

    // --- Test 5: inode limits (soft warning + hard denial) ---
    // Limits in force from Test 2: soft_inodes = 100, hard_inodes = 200.
    // check_create() probes inodes_used + 1 against those, mirroring how
    // Tests 2-4 probe byte usage. The three bands must behave like bytes:
    // under-soft → Allowed, over-soft (within grace) → SoftWarning, at/over
    // hard → Denied.
    {
        // Band 1: under the soft limit (50 + 1 = 51 ≤ 100) → Allowed.
        set_usage(test_user, 1_000, 50);
        let result = check_create(test_uid, test_gid);
        if result != QuotaCheckResult::Allowed {
            serial_println!("[quota]   ERROR: expected Allowed under soft inode limit, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }

        // Band 2: over soft (150 + 1 = 151 > 100) but under hard (< 200),
        // grace not yet enforced → SoftWarning.
        set_usage(test_user, 1_000, 150);
        let result = check_create(test_uid, test_gid);
        if result != QuotaCheckResult::SoftWarning {
            serial_println!("[quota]   ERROR: expected SoftWarning over soft inode limit, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }

        // Band 3: at the hard limit (200 + 1 = 201 > 200) → Denied.
        set_usage(test_user, 1_000, 200);
        let result = check_create(test_uid, test_gid);
        if result != QuotaCheckResult::Denied {
            serial_println!("[quota]   ERROR: expected Denied at hard inode limit, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   inode limit OK");
    }

    // --- Test 6: charge and release ---
    {
        set_usage(test_user, 0, 0);

        charge_bytes(test_uid, test_gid, 5_000);
        charge_inode(test_uid, test_gid);

        let info = get_info(test_user);
        if let Some(info) = info {
            if info.usage.bytes_used != 5_000 {
                serial_println!("[quota]   ERROR: expected 5000 bytes, got {}", info.usage.bytes_used);
                cleanup_test(test_user, test_group, was_enabled);
                return Err(KernelError::InternalError);
            }
            if info.usage.inodes_used != 1 {
                serial_println!("[quota]   ERROR: expected 1 inode, got {}", info.usage.inodes_used);
                cleanup_test(test_user, test_group, was_enabled);
                return Err(KernelError::InternalError);
            }
        } else {
            serial_println!("[quota]   ERROR: get_info returned None");
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }

        release_bytes(test_uid, test_gid, 3_000);
        release_inode(test_uid, test_gid);

        let info = get_info(test_user).unwrap_or_else(|| panic!("missing entry"));
        if info.usage.bytes_used != 2_000 || info.usage.inodes_used != 0 {
            serial_println!("[quota]   ERROR: release incorrect ({}, {})", info.usage.bytes_used, info.usage.inodes_used);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   charge/release OK");
    }

    // --- Test 7: group quota enforcement ---
    {
        set_limits(test_group, QuotaLimits {
            soft_bytes: 0,
            hard_bytes: 5_000,
            soft_inodes: 0,
            hard_inodes: 0,
            grace_seconds: 3600,
        });
        set_usage(test_group, 4_000, 0);

        // User is under their limit but group would exceed.
        set_usage(test_user, 0, 0);
        let result = check_write(test_uid, test_gid, 2_000);
        if result != QuotaCheckResult::Denied {
            serial_println!("[quota]   ERROR: group denial expected, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   group enforcement OK");
    }

    // --- Test 8: disabled quotas ---
    {
        set_enabled(false);
        set_usage(test_user, 19_000, 199);
        // Would normally be denied, but quotas are disabled.
        let result = check_write(test_uid, test_gid, 5_000);
        if result != QuotaCheckResult::Allowed {
            serial_println!("[quota]   ERROR: disabled should allow, got {:?}", result);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        set_enabled(true);
        serial_println!("[quota]   disabled bypass OK");
    }

    // --- Test 9: stats ---
    {
        let st = stats();
        if st.entries < 2 {
            serial_println!("[quota]   ERROR: expected >= 2 entries, got {}", st.entries);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        if st.user_quotas < 1 || st.group_quotas < 1 {
            serial_println!("[quota]   ERROR: expected user and group quotas");
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   stats OK (entries={}, users={}, groups={})", st.entries, st.user_quotas, st.group_quotas);
    }

    // --- Test 10: format_bytes ---
    {
        if format_bytes(512) != "512 B" {
            serial_println!("[quota]   ERROR: format_bytes(512) = '{}'", format_bytes(512));
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        let mib = format_bytes(1024 * 1024);
        if !mib.contains("MiB") {
            serial_println!("[quota]   ERROR: format_bytes(1M) = '{}'", mib);
            cleanup_test(test_user, test_group, was_enabled);
            return Err(KernelError::InternalError);
        }
        serial_println!("[quota]   format_bytes OK");
    }

    // Cleanup.
    cleanup_test(test_user, test_group, was_enabled);

    serial_println!("[quota] Self-test passed (10 tests).");
    Ok(())
}

/// Helper to clean up test quota entries.
fn cleanup_test(user: QuotaSubject, group: QuotaSubject, was_enabled: bool) {
    remove(user);
    remove(group);
    set_enabled(was_enabled);
}
