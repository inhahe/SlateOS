//! Disk Quota Management — per-user/group storage limits.
//!
//! Enforces storage quotas with soft/hard limits, grace periods,
//! and usage tracking per user and per group.
//!
//! ## Architecture
//!
//! ```text
//! File write
//!   → diskquota::check_quota(user, bytes, files) → verdict (allow/warn/deny)
//!   → diskquota::update_usage(user, bytes_delta, file_delta) → track change
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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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

    /// Severity rank, for combining the byte verdict with the file verdict.
    ///
    /// `SoftExceeded` and `GracePeriod` rank equal: they describe the same
    /// condition (over soft, under hard) and differ only in whether the grace
    /// clock has been started, which is a single per-entry fact rather than a
    /// per-dimension one.  Ranking them equal is what lets the tie-break below
    /// prefer the byte verdict, so adding file enforcement can only ever make
    /// a status *worse*, never merely different at the same tier.
    fn rank(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::SoftExceeded | Self::GracePeriod => 1,
            Self::HardExceeded => 2,
        }
    }

    /// The worse of two verdicts; on a tie, `self` wins.
    fn worse(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// The outcome of a [`check_quota`] call.
///
/// This is richer than the `bool` it replaced because a denial that cannot say
/// *which* limit fired is close to useless to the user who hits it: it sends
/// someone hunting for a large file to delete when their actual problem is two
/// hundred small ones.  The caller cannot reconstruct the reason afterwards
/// either — re-reading the entry to compare is a race against every other
/// writer, and would report the state *after* whatever happened next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaVerdict {
    /// Under both soft limits.
    Allowed,
    /// Over a soft limit but under both hard limits: permitted, with a warning.
    Warned { bytes: bool, files: bool },
    /// Over a hard limit: refused.
    Denied { bytes: bool, files: bool },
}

impl QuotaVerdict {
    /// Whether the operation may proceed.  A warning still allows.
    pub fn allowed(self) -> bool {
        !matches!(self, Self::Denied { .. })
    }

    /// Human-readable reason, naming the limit(s) that fired.
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Warned {
                bytes: true,
                files: true,
            } => "soft limit exceeded (bytes and files)",
            Self::Warned { bytes: true, .. } => "soft byte limit exceeded",
            Self::Warned { .. } => "soft file limit exceeded",
            Self::Denied {
                bytes: true,
                files: true,
            } => "hard limit exceeded (bytes and files)",
            Self::Denied { bytes: true, .. } => "hard byte limit exceeded",
            Self::Denied { .. } => "hard file limit exceeded",
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
    /// Status of one dimension (bytes, or files) against its own two limits.
    fn dimension_status(used: u64, soft: u64, hard: u64, in_grace: bool) -> QuotaStatus {
        if used >= hard {
            QuotaStatus::HardExceeded
        } else if used >= soft {
            if in_grace {
                QuotaStatus::GracePeriod
            } else {
                QuotaStatus::SoftExceeded
            }
        } else {
            QuotaStatus::Ok
        }
    }

    /// Compute current status based on usage vs limits.
    ///
    /// Returns the **worse** of the byte verdict and the file verdict.  Both
    /// halves are limits the administrator set and the module confirmed, so
    /// reporting only the byte half made `soft_limit_files`/`hard_limit_files`
    /// decorative — a user at 400 files against a 200-file limit read `OK`.
    pub fn status(&self) -> QuotaStatus {
        let in_grace = self.grace_start_ns.is_some();
        let bytes = Self::dimension_status(
            self.bytes_used,
            self.soft_limit_bytes,
            self.hard_limit_bytes,
            in_grace,
        );
        let files = Self::dimension_status(
            self.file_count,
            self.soft_limit_files,
            self.hard_limit_files,
            in_grace,
        );
        bytes.worse(files)
    }

    /// Whether usage is at or over *either* soft limit.
    ///
    /// There is one `grace_start_ns` per entry, not one per dimension, so the
    /// clock must be driven by the union: keying it on bytes alone would clear
    /// a grace period that the file count still justifies the moment the user
    /// deleted a large file.
    fn over_soft(&self) -> bool {
        self.bytes_used >= self.soft_limit_bytes || self.file_count >= self.soft_limit_files
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
    if guard.is_some() {
        return;
    }
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
pub fn set_quota(
    name: &str,
    target: QuotaTarget,
    soft_bytes: u64,
    hard_bytes: u64,
) -> KernelResult<u32> {
    with_state(|state| {
        if let Some(e) = state
            .entries
            .iter_mut()
            .find(|e| e.name == name && e.target_type == target)
        {
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
                id,
                name: String::from(name),
                target_type: target,
                bytes_used: 0,
                file_count: 0,
                soft_limit_bytes: soft_bytes,
                hard_limit_bytes: hard_bytes,
                soft_limit_files: u64::MAX,
                hard_limit_files: u64::MAX,
                grace_start_ns: None,
                grace_period_ns: DEFAULT_GRACE_NS,
            });
            Ok(id)
        }
    })
}

/// Set file count limits.
pub fn set_file_limits(
    name: &str,
    target: QuotaTarget,
    soft_files: u64,
    hard_files: u64,
) -> KernelResult<()> {
    with_state(|state| {
        let entry = state
            .entries
            .iter_mut()
            .find(|e| e.name == name && e.target_type == target)
            .ok_or(KernelError::NotFound)?;
        entry.soft_limit_files = soft_files;
        entry.hard_limit_files = hard_files;
        Ok(())
    })
}

/// Check whether an operation adding `bytes` of data and `files` new files
/// would be allowed.
///
/// Both dimensions are tested in **one** call, under one acquisition of the
/// table lock, so the verdict is a single consistent snapshot and a caller
/// cannot test one limit and forget the other — which is precisely how the
/// file limits came to be stored and never compared.  Pass `files = 0` for a
/// write that extends an existing file.
pub fn check_quota(
    name: &str,
    target: QuotaTarget,
    bytes: u64,
    files: u64,
) -> KernelResult<QuotaVerdict> {
    with_state(|state| {
        state.total_checks += 1;
        if !state.enabled {
            return Ok(QuotaVerdict::Allowed);
        }
        let entry = match state
            .entries
            .iter()
            .find(|e| e.name == name && e.target_type == target)
        {
            Some(e) => e,
            None => return Ok(QuotaVerdict::Allowed), // No quota set → allow.
        };
        let new_bytes = entry.bytes_used.saturating_add(bytes);
        let new_files = entry.file_count.saturating_add(files);

        let deny_bytes = new_bytes > entry.hard_limit_bytes;
        let deny_files = new_files > entry.hard_limit_files;
        if deny_bytes || deny_files {
            state.total_denials += 1;
            return Ok(QuotaVerdict::Denied {
                bytes: deny_bytes,
                files: deny_files,
            });
        }

        let warn_bytes = new_bytes > entry.soft_limit_bytes;
        let warn_files = new_files > entry.soft_limit_files;
        if warn_bytes || warn_files {
            state.total_warnings += 1;
            // Soft limit: warn but allow.
            return Ok(QuotaVerdict::Warned {
                bytes: warn_bytes,
                files: warn_files,
            });
        }

        Ok(QuotaVerdict::Allowed)
    })
}

/// Update usage after a write/delete.
pub fn update_usage(
    name: &str,
    target: QuotaTarget,
    bytes_delta: i64,
    file_delta: i64,
) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let entry = state
            .entries
            .iter_mut()
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
        // Start grace period if crossing *either* soft limit, and clear it only
        // once both are back under.  See `QuotaEntry::over_soft`.
        if entry.over_soft() {
            if entry.grace_start_ns.is_none() {
                entry.grace_start_ns = Some(now);
            }
        } else {
            entry.grace_start_ns = None;
        }
        Ok(())
    })
}

/// Remove a quota entry.
pub fn remove_quota(name: &str, target: QuotaTarget) -> KernelResult<()> {
    with_state(|state| {
        let before = state.entries.len();
        state
            .entries
            .retain(|e| !(e.name == name && e.target_type == target));
        if state.entries.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// List all quota entries.
pub fn list_quotas() -> Vec<QuotaEntry> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.entries.clone())
}

/// Get quota for a specific user/group.
pub fn get_quota(name: &str, target: QuotaTarget) -> Option<QuotaEntry> {
    STATE.lock().as_ref().and_then(|s| {
        s.entries
            .iter()
            .find(|e| e.name == name && e.target_type == target)
            .cloned()
    })
}

/// Statistics: (entry_count, total_checks, total_denials, total_warnings, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.entries.len(),
            s.total_checks,
            s.total_denials,
            s.total_warnings,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("diskquota::self_test() — running tests...");
    init_defaults();

    // 1: No quotas initially.
    assert!(list_quotas().is_empty());
    crate::serial_println!("  [1/14] empty: OK");

    // 2: Set user quota.
    let id = set_quota("alice", QuotaTarget::User, 1_000_000, 2_000_000).expect("set");
    assert!(id > 0);
    assert_eq!(list_quotas().len(), 1);
    crate::serial_println!("  [2/14] set quota: OK");

    // 3: Check within limit.
    let v = check_quota("alice", QuotaTarget::User, 500_000, 0).expect("check");
    assert_eq!(v, QuotaVerdict::Allowed);
    crate::serial_println!("  [3/14] within limit: OK");

    // 4: Update usage and check hard limit.
    update_usage("alice", QuotaTarget::User, 1_500_000, 10).expect("update");
    let v = check_quota("alice", QuotaTarget::User, 600_000, 0).expect("check2");
    // 1_500_000 + 600_000 > 2_000_000 hard limit — and the denial names bytes,
    // not files, which are unlimited on this entry.
    assert_eq!(
        v,
        QuotaVerdict::Denied {
            bytes: true,
            files: false
        }
    );
    assert!(!v.allowed());
    crate::serial_println!("  [4/14] hard byte limit: OK");

    // 5: Soft limit triggers grace period.
    let q = get_quota("alice", QuotaTarget::User).expect("get");
    assert_eq!(q.status(), QuotaStatus::GracePeriod); // 1_500_000 > 1_000_000 soft.
    crate::serial_println!("  [5/14] grace period: OK");

    // 6: Group quota.
    set_quota("devs", QuotaTarget::Group, 5_000_000, 10_000_000).expect("group");
    let v = check_quota("devs", QuotaTarget::Group, 4_000_000, 0).expect("check3");
    assert_eq!(v, QuotaVerdict::Allowed);
    crate::serial_println!("  [6/14] group quota: OK");

    // 7: Remove quota.
    remove_quota("devs", QuotaTarget::Group).expect("remove");
    assert_eq!(list_quotas().len(), 1);
    crate::serial_println!("  [7/14] remove: OK");

    // 8: Stats.
    let (entries, checks, denials, _warnings, ops) = stats();
    assert_eq!(entries, 1);
    assert!(checks >= 3);
    assert!(denials >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/14] stats: OK");

    // --- File-count limits: stored, and now compared. -----------------------
    // A fresh entry with generous byte limits, so every verdict below is
    // attributable to the file half alone.
    set_quota("bob", QuotaTarget::User, u64::MAX, u64::MAX).expect("bob");
    set_file_limits("bob", QuotaTarget::User, 100, 200).expect("bob files");

    // 9: A file-creating check under both file limits is allowed.
    let v = check_quota("bob", QuotaTarget::User, 0, 50).expect("files ok");
    assert_eq!(v, QuotaVerdict::Allowed);
    crate::serial_println!("  [9/14] file count within limit: OK");

    // 10: Over the *soft* file limit warns but still allows.
    let v = check_quota("bob", QuotaTarget::User, 0, 150).expect("files warn");
    assert_eq!(
        v,
        QuotaVerdict::Warned {
            bytes: false,
            files: true
        }
    );
    assert!(v.allowed());
    crate::serial_println!("  [10/14] soft file limit warns: OK");

    // 11: Over the *hard* file limit is refused. This is the regression the
    // whole change exists for: before enforcement this returned "allowed", so
    // a 200-file limit stopped nothing.
    let v = check_quota("bob", QuotaTarget::User, 0, 201).expect("files deny");
    assert_eq!(
        v,
        QuotaVerdict::Denied {
            bytes: false,
            files: true
        }
    );
    assert!(!v.allowed());
    crate::serial_println!("  [11/14] hard file limit denies: OK");

    // 12: The file count alone starts the grace clock and drives `status()`,
    // with zero bytes used.
    update_usage("bob", QuotaTarget::User, 0, 120).expect("bob usage");
    let q = get_quota("bob", QuotaTarget::User).expect("get bob");
    assert_eq!(q.bytes_used, 0);
    assert_eq!(q.file_count, 120);
    assert!(q.grace_start_ns.is_some());
    assert_eq!(q.status(), QuotaStatus::GracePeriod); // 120 >= 100 soft files.
    crate::serial_println!("  [12/14] grace from file count alone: OK");

    // 13: Past the hard file limit, `status()` reports the worse verdict even
    // though the byte half is comfortably `Ok`.
    update_usage("bob", QuotaTarget::User, 0, 100).expect("bob usage 2");
    let q = get_quota("bob", QuotaTarget::User).expect("get bob 2");
    assert_eq!(q.file_count, 220);
    assert_eq!(q.status(), QuotaStatus::HardExceeded);
    crate::serial_println!("  [13/14] status takes the worse verdict: OK");

    // 14: Dropping back under the soft file limit clears the grace clock —
    // the union condition releasing once *both* dimensions are under.
    update_usage("bob", QuotaTarget::User, 0, -200).expect("bob usage 3");
    let q = get_quota("bob", QuotaTarget::User).expect("get bob 3");
    assert_eq!(q.file_count, 20);
    assert!(q.grace_start_ns.is_none());
    assert_eq!(q.status(), QuotaStatus::Ok);
    crate::serial_println!("  [14/14] grace clears when file count drops: OK");

    remove_quota("bob", QuotaTarget::User).expect("remove bob");

    crate::serial_println!("diskquota::self_test() — all 14 tests passed");
}
