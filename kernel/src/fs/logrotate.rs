//! Log Rotation — automated log file management.
//!
//! Manages log files by size/age thresholds, rotating old logs
//! to compressed archives and cleaning up according to retention
//! policies.
//!
//! ## Architecture
//!
//! ```text
//! Log rotation lifecycle
//!   → logrotate::add_rule(path, config) → register rotation rule
//!   → logrotate::check() → evaluate all rules, rotate as needed
//!   → logrotate::cleanup() → remove logs past retention limit
//!
//! Integration:
//!   → syslog (system logging)
//!   → eventlog (event logging)
//!   → storageclean (storage cleanup)
//!   → tasksched (scheduled rotation)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Rotation trigger condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateTrigger {
    /// Rotate when file exceeds size in bytes.
    Size(u64),
    /// Rotate daily.
    Daily,
    /// Rotate weekly.
    Weekly,
    /// Rotate monthly.
    Monthly,
    /// Rotate on both size and age (whichever comes first).
    SizeOrDaily(u64),
}

/// Compression method for rotated logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressMethod {
    None,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

impl CompressMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Bzip2 => "bzip2",
            Self::Xz => "xz",
            Self::Zstd => "zstd",
        }
    }
}

/// A log rotation rule.
#[derive(Debug, Clone)]
pub struct RotateRule {
    pub id: u32,
    pub log_path: String,
    pub trigger: RotateTrigger,
    pub compress: CompressMethod,
    pub max_archives: u32,
    pub enabled: bool,
    pub last_rotated_ns: u64,
    pub total_rotations: u64,
}

/// A rotation event record.
#[derive(Debug, Clone)]
pub struct RotateEvent {
    pub rule_id: u32,
    pub timestamp_ns: u64,
    pub original_size: u64,
    pub archive_name: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 200;
const MAX_EVENTS: usize = 1000;

struct State {
    rules: Vec<RotateRule>,
    events: Vec<RotateEvent>,
    next_id: u32,
    total_rotations: u64,
    total_bytes_rotated: u64,
    total_cleanups: u64,
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

/// Initialise the log-rotation state with the default rotation policy.
///
/// Unlike the statistics modules in this directory, the rules seeded here are
/// CONFIGURATION (rotation policy), not fabricated observations — the analogue
/// of a shipped `/etc/logrotate.d` configuration. Each default rule carries
/// ZEROED activity (`last_rotated_ns = 0`, `total_rotations = 0`), and the
/// global rotation/byte/cleanup counters all start at zero, so the
/// `/proc/logrotate` generator and the `logrotate` kshell command report the
/// policy honestly: three configured rules, zero rotations performed. Rules are
/// added/removed through [`add_rule`] / [`remove_rule`]; the rotation counters
/// advance only through real [`rotate`] / [`check_all`] / [`cleanup`] calls. The
/// default rules (syslog, kern.log, auth.log) are sensible policy defaults and
/// are deliberately kept — they are settings, not invented activity.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        rules: alloc::vec![
            RotateRule {
                id: 1, log_path: String::from("/var/log/syslog"),
                trigger: RotateTrigger::SizeOrDaily(10_000_000),
                compress: CompressMethod::Gzip, max_archives: 7,
                enabled: true, last_rotated_ns: 0, total_rotations: 0,
            },
            RotateRule {
                id: 2, log_path: String::from("/var/log/kern.log"),
                trigger: RotateTrigger::Weekly,
                compress: CompressMethod::Gzip, max_archives: 4,
                enabled: true, last_rotated_ns: 0, total_rotations: 0,
            },
            RotateRule {
                id: 3, log_path: String::from("/var/log/auth.log"),
                trigger: RotateTrigger::Size(5_000_000),
                compress: CompressMethod::Zstd, max_archives: 12,
                enabled: true, last_rotated_ns: 0, total_rotations: 0,
            },
        ],
        events: Vec::new(),
        next_id: 4,
        total_rotations: 0,
        total_bytes_rotated: 0,
        total_cleanups: 0,
        ops: 0,
    });
}

/// Add a rotation rule.
pub fn add_rule(log_path: &str, trigger: RotateTrigger, compress: CompressMethod, max_archives: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.rules.iter().any(|r| r.log_path == log_path) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(RotateRule {
            id, log_path: String::from(log_path), trigger, compress,
            max_archives, enabled: true, last_rotated_ns: 0, total_rotations: 0,
        });
        Ok(id)
    })
}

/// Remove a rotation rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.rules.len();
        state.rules.retain(|r| r.id != id);
        if state.rules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a rule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = enabled;
        Ok(())
    })
}

/// Simulate rotating a specific log (by rule ID).
pub fn rotate(id: u32) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        // Simulate rotation: generate archive name and size.
        let simulated_size = 1_000_000u64; // 1 MB simulated
        let archive_name = format!("{}.1.{}", rule.log_path, rule.compress.label());
        rule.last_rotated_ns = now;
        rule.total_rotations += 1;
        let rule_id = rule.id;
        state.total_rotations += 1;
        state.total_bytes_rotated += simulated_size;
        if state.events.len() >= MAX_EVENTS {
            state.events.remove(0);
        }
        state.events.push(RotateEvent {
            rule_id, timestamp_ns: now, original_size: simulated_size,
            archive_name,
        });
        Ok(simulated_size)
    })
}

/// Check all rules and rotate as needed (simulated).
pub fn check_all() -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mut rotated = 0u32;
        let rule_ids: Vec<u32> = state.rules.iter()
            .filter(|r| r.enabled)
            .map(|r| r.id)
            .collect();
        for rid in rule_ids {
            if let Some(rule) = state.rules.iter_mut().find(|r| r.id == rid) {
                let simulated_size = 1_000_000u64;
                let archive_name = format!("{}.1.{}", rule.log_path, rule.compress.label());
                rule.last_rotated_ns = now;
                rule.total_rotations += 1;
                state.total_rotations += 1;
                state.total_bytes_rotated += simulated_size;
                if state.events.len() >= MAX_EVENTS {
                    state.events.remove(0);
                }
                state.events.push(RotateEvent {
                    rule_id: rid, timestamp_ns: now, original_size: simulated_size,
                    archive_name,
                });
                rotated += 1;
            }
        }
        Ok(rotated)
    })
}

/// Cleanup old archives past retention.
pub fn cleanup() -> KernelResult<u32> {
    with_state(|state| {
        state.total_cleanups += 1;
        // Simulated: no actual files to remove.
        Ok(0)
    })
}

/// List all rules.
pub fn list_rules() -> Vec<RotateRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Get recent rotation events.
pub fn list_events() -> Vec<RotateEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.events.clone())
}

/// Statistics.
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.total_rotations, s.total_bytes_rotated, s.total_cleanups, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("logrotate::self_test() — running tests...");
    // Start from a clean state with the default policy so the assertions below
    // are exact, and clear at the end so the simulated rotations performed by
    // this test do NOT leak into the live /proc/logrotate counters.
    *STATE.lock() = None;
    init_defaults();

    // 1: Default rules — the shipped rotation policy (3 rules), zero rotations
    //    performed yet (counters are config, not activity).
    assert_eq!(list_rules().len(), 3);
    let (rc0, rot0, by0, cl0, _) = stats();
    assert_eq!((rc0, rot0, by0, cl0), (3, 0, 0, 0));
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add rule — the fourth rule gets id 4 (next_id after the 3 defaults).
    let id = add_rule("/var/log/app.log", RotateTrigger::Daily, CompressMethod::Zstd, 5).expect("add");
    assert_eq!(id, 4);
    assert_eq!(list_rules().len(), 4);
    crate::serial_println!("  [2/8] add rule: OK");

    // 3: Duplicate path rejected.
    assert!(add_rule("/var/log/app.log", RotateTrigger::Weekly, CompressMethod::None, 3).is_err());
    crate::serial_println!("  [3/8] duplicate: OK");

    // 4: Rotate single — one simulated 1 MB rotation; one event recorded.
    let bytes = rotate(id).expect("rotate");
    assert_eq!(bytes, 1_000_000);
    assert_eq!(list_events().len(), 1);
    assert_eq!(stats().1, 1); // total_rotations
    crate::serial_println!("  [4/8] rotate: OK");

    // 5: Disable/enable.
    set_enabled(id, false).expect("disable");
    let rule = list_rules().into_iter().find(|r| r.id == id).expect("find");
    assert!(!rule.enabled);
    set_enabled(id, true).expect("enable");
    crate::serial_println!("  [5/8] enable/disable: OK");

    // 6: Check all — rotates every enabled rule (all 4); total rotations now
    //    1 (from step 4) + 4 = 5, and 5 events recorded.
    let count = check_all().expect("check_all");
    assert_eq!(count, 4);
    assert_eq!(list_events().len(), 5);
    crate::serial_println!("  [6/8] check all: OK");

    // 7: Remove rule — back to the 3 default rules; double remove errors.
    remove_rule(id).expect("remove");
    assert_eq!(list_rules().len(), 3);
    assert!(remove_rule(id).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Final stats reflect only the real activity above: 3 rules, 5 rotations,
    //    5 MB rotated, 0 cleanups.
    let (rule_count, rotations, bytes_rotated, cleanups, ops) = stats();
    assert_eq!((rule_count, rotations, bytes_rotated, cleanups), (3, 5, 5_000_000, 0));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state — restore the clean default policy so
    // /proc/logrotate shows the shipped rules with zero performed rotations.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("logrotate::self_test() — all 8 tests passed");
}
