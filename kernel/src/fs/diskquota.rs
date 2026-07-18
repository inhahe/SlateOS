//! Disk Quota Management — per-user/group storage limits.
//!
//! Enforces storage quotas with soft/hard limits, grace periods,
//! and usage tracking per user and per group.
//!
//! ## Architecture
//!
//! ```text
//! File write
//!   → diskquota::check_quota(user, bytes) → allow/deny
//!   → diskquota::update_usage(user, delta) → track change
//!
//! Administration
//!   → diskquota::set_quota(user, soft, hard) → configure limits
//!   → diskquota::get_report() → usage report
//!
//! Integration:
//!   → quota (filesystem quota)
//!   → useracct (user accounts)
//!   → storageclean (cleanup)
//!   → notifcenter (warnings)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Quota target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaTarget {
    User,
    Group,
}

impl QuotaTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Group => "Group",
        }
    }
}

/// Quota status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaStatus {
    Ok,
    SoftExceeded,
    HardExceeded,
    GracePeriod,
}

impl QuotaStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::SoftExceeded => "Soft Exceeded",
            Self::HardExceeded => "Hard Exceeded",
            Self::GracePeriod => "Grace Period",
        }
    }
}

/// A quota entry for a user or group.
#[derive(Debug, Clone)]
pub struct QuotaEntry {
    pub id: u32,
    pub name: String,
    pub target_type: QuotaTarget,
    pub bytes_used: u64,
    pub file_count: u64,
    pub soft_limit_bytes: u64,
    pub hard_limit_bytes: u64,
    pub soft_limit_files: u64,
    pub hard_limit_files: u64,
    pub grace_start_ns: Option<u64>,
    pub grace_period_ns: u64,
}

impl QuotaEntry {
    /// Compute current status based on usage vs limits.
    pub fn status(&self) -> QuotaStatus {
        if self.bytes_used >= self.hard_limit_bytes {
            QuotaStatus::HardExceeded
        } else if self.bytes_used >= self.soft_limit_bytes {
            if self.grace_start_ns.is_some() {
                QuotaStatus::GracePeriod
            } else {
                QuotaStatus::SoftExceeded
            }
        } else {
            QuotaStatus::Ok
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENTRIES: usize = 200;
const DEFAULT_GRACE_NS: u64 = 7 * 24 * 60 * 60 * 1_000_000_000; // 7 days.

struct State {
    entries: Vec<QuotaEntry>,
    next_id: u32,
    enabled: bool,
    total_checks: u64,
    total_denials: u64,
    total_warnings: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        entries: Vec::new(),
        next_id: 1,
        enabled: true,
        total_checks: 0,
        total_denials: 0,
        total_warnings: 0,
        ops: 0,
    });
}

/// Enable/disable quota enforcement.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Set quota for a user or group.
pub fn set_quota(name: &str, target: QuotaTarget, soft_bytes: u64, hard_bytes: u64) -> KernelResult<u32> {
    with_state(|state| {
        if let Some(e) = state.entries.iter_mut().find(|e| e.name == name && e.target_type == target) {
            e.soft_limit_bytes = soft_bytes;
            e.hard_limit_bytes = hard_bytes;
            Ok(e.id)
        } else {
            if state.entries.len() >= MAX_ENTRIES {
                return Err(KernelError::ResourceExhausted);
            }
            let id = state.next_id;
            state.next_id += 1;
            state.entries.push(QuotaEntry {
                id, name: String::from(name), target_type: target,
                bytes_used: 0, file_count: 0,
                soft_limit_bytes: soft_bytes, hard_limit_bytes: hard_bytes,
                soft_limit_files: u64::MAX, hard_limit_files: u64::MAX,
                grace_start_ns: None, grace_period_ns: DEFAULT_GRACE_NS,
            });
            Ok(id)
        }
    })
}

/// Set file count limits.
pub fn set_file_limits(name: &str, target: QuotaTarget, soft_files: u64, hard_files: u64) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut()
            .find(|e| e.name == name && e.target_type == target)
            .ok_or(KernelError::NotFound)?;
        entry.soft_limit_files = soft_files;
        entry.hard_limit_files = hard_files;
        Ok(())
    })
}

/// Check if a write of `bytes` would be allowed.
pub fn check_quota(name: &str, target: QuotaTarget, bytes: u64) -> KernelResult<bool> {
    with_state(|state| {
        state.total_checks += 1;
        if !state.enabled {
            return Ok(true);
        }
        let entry = match state.entries.iter().find(|e| e.name == name && e.target_type == target) {
            Some(e) => e,
            None => return Ok(true), // No quota set → allow.
        };
        let new_usage = entry.bytes_used.saturating_add(bytes);
        if new_usage > entry.hard_limit_bytes {
            state.total_denials += 1;
            Ok(false)
        } else if new_usage > entry.soft_limit_bytes {
            state.total_warnings += 1;
            Ok(true) // Soft limit: warn but allow.
        } else {
            Ok(true)
        }
    })
}

/// Update usage after a write/delete.
pub fn update_usage(name: &str, target: QuotaTarget, bytes_delta: i64, file_delta: i64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let entry = state.entries.iter_mut()
            .find(|e| e.name == name && e.target_type == target)
            .ok_or(KernelError::NotFound)?;
        if bytes_delta >= 0 {
            entry.bytes_used = entry.bytes_used.saturating_add(bytes_delta as u64);
        } else {
            entry.bytes_used = entry.bytes_used.saturating_sub((-bytes_delta) as u64);
        }
        if file_delta >= 0 {
            entry.file_count = entry.file_count.saturating_add(file_delta as u64);
        } else {
            entry.file_count = entry.file_count.saturating_sub((-file_delta) as u64);
        }
        // Start grace period if crossing soft limit.
        if entry.bytes_used >= entry.soft_limit_bytes && entry.grace_start_ns.is_none() {
            entry.grace_start_ns = Some(now);
        } else if entry.bytes_used < entry.soft_limit_bytes {
            entry.grace_start_ns = None;
        }
        Ok(())
    })
}

/// Remove a quota entry.
pub fn remove_quota(name: &str, target: QuotaTarget) -> KernelResult<()> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| !(e.name == name && e.target_type == target));
        if state.entries.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all quota entries.
pub fn list_quotas() -> Vec<QuotaEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.entries.clone())
}

/// Get quota for a specific user/group.
pub fn get_quota(name: &str, target: QuotaTarget) -> Option<QuotaEntry> {
    STATE.lock().as_ref().and_then(|s| {
        s.entries.iter().find(|e| e.name == name && e.target_type == target).cloned()
    })
}

/// Statistics: (entry_count, total_checks, total_denials, total_warnings, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.entries.len(), s.total_checks, s.total_denials, s.total_warnings, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("diskquota::self_test() — running tests...");
    init_defaults();

    // 1: No quotas initially.
    assert!(list_quotas().is_empty());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Set user quota.
    let id = set_quota("alice", QuotaTarget::User, 1_000_000, 2_000_000).expect("set");
    assert!(id > 0);
    assert_eq!(list_quotas().len(), 1);
    crate::serial_println!("  [2/8] set quota: OK");

    // 3: Check within limit.
    let ok = check_quota("alice", QuotaTarget::User, 500_000).expect("check");
    assert!(ok);
    crate::serial_println!("  [3/8] within limit: OK");

    // 4: Update usage and check hard limit.
    update_usage("alice", QuotaTarget::User, 1_500_000, 10).expect("update");
    let ok = check_quota("alice", QuotaTarget::User, 600_000).expect("check2");
    assert!(!ok); // 1_500_000 + 600_000 > 2_000_000 hard limit.
    crate::serial_println!("  [4/8] hard limit: OK");

    // 5: Soft limit triggers grace period.
    let q = get_quota("alice", QuotaTarget::User).expect("get");
    assert_eq!(q.status(), QuotaStatus::GracePeriod); // 1_500_000 > 1_000_000 soft.
    crate::serial_println!("  [5/8] grace period: OK");

    // 6: Group quota.
    set_quota("devs", QuotaTarget::Group, 5_000_000, 10_000_000).expect("group");
    let ok = check_quota("devs", QuotaTarget::Group, 4_000_000).expect("check3");
    assert!(ok);
    crate::serial_println!("  [6/8] group quota: OK");

    // 7: Remove quota.
    remove_quota("devs", QuotaTarget::Group).expect("remove");
    assert_eq!(list_quotas().len(), 1);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (entries, checks, denials, _warnings, ops) = stats();
    assert_eq!(entries, 1);
    assert!(checks >= 3);
    assert!(denials >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("diskquota::self_test() — all 8 tests passed");
}
