//! Security Policy — mandatory access control policy engine.
//!
//! Manages security labels, access rules, and enforcement modes.
//! Supports label assignment to processes and files, with rule-based
//! access decisions (allow/deny/audit).
//!
//! ## Architecture
//!
//! ```text
//! Security policy
//!   → secpolicy::check(subject, object, action) → access decision
//!   → secpolicy::add_rule(rule) → add policy rule
//!   → secpolicy::set_label(entity, label) → assign label
//!   → secpolicy::set_mode(mode) → enforcing/permissive/disabled
//!
//! Integration:
//!   → acl (access control lists)
//!   → apppermissions (app permissions)
//!   → audit (audit logging)
//!   → secureboot (secure boot)
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

/// Enforcement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforceMode {
    Disabled,
    Permissive,   // Log but don't deny.
    Enforcing,
}

impl EnforceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Permissive => "Permissive",
            Self::Enforcing => "Enforcing",
        }
    }
}

/// Access action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
    Execute,
    Create,
    Delete,
    Network,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Execute => "execute",
            Self::Create => "create",
            Self::Delete => "delete",
            Self::Network => "network",
        }
    }
}

/// Rule decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Audit,   // Allow but log.
}

impl Decision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Audit => "audit",
        }
    }
}

/// A security label.
#[derive(Debug, Clone)]
pub struct SecurityLabel {
    pub entity_id: u32,
    pub entity_type: String,  // "process", "file", "socket"
    pub label: String,
}

/// A policy rule.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub id: u32,
    pub subject_label: String,
    pub object_label: String,
    pub action: Action,
    pub decision: Decision,
    pub priority: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 256;
const MAX_LABELS: usize = 1024;

struct State {
    mode: EnforceMode,
    rules: Vec<PolicyRule>,
    labels: Vec<SecurityLabel>,
    next_rule_id: u32,
    total_checks: u64,
    total_denied: u64,
    total_allowed: u64,
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
        mode: EnforceMode::Permissive,
        rules: alloc::vec![
            PolicyRule { id: 1, subject_label: String::from("system"), object_label: String::from("system"),
                action: Action::Read, decision: Decision::Allow, priority: 100 },
            PolicyRule { id: 2, subject_label: String::from("user"), object_label: String::from("user"),
                action: Action::Read, decision: Decision::Allow, priority: 50 },
            PolicyRule { id: 3, subject_label: String::from("user"), object_label: String::from("system"),
                action: Action::Write, decision: Decision::Deny, priority: 90 },
        ],
        labels: Vec::new(),
        next_rule_id: 4,
        total_checks: 0,
        total_denied: 0,
        total_allowed: 0,
        ops: 0,
    });
}

/// Check access.
pub fn check_access(subject_label: &str, object_label: &str, action: Action) -> KernelResult<Decision> {
    with_state(|state| {
        state.total_checks += 1;
        if state.mode == EnforceMode::Disabled {
            state.total_allowed += 1;
            return Ok(Decision::Allow);
        }
        // Find highest-priority matching rule.
        let rule = state.rules.iter()
            .filter(|r| r.subject_label == subject_label && r.object_label == object_label && r.action == action)
            .max_by_key(|r| r.priority);
        let decision = rule.map_or(Decision::Deny, |r| r.decision);
        match decision {
            Decision::Allow | Decision::Audit => state.total_allowed += 1,
            Decision::Deny => state.total_denied += 1,
        }
        if state.mode == EnforceMode::Permissive && decision == Decision::Deny {
            // Log but allow.
            state.total_allowed += 1;
            state.total_denied -= 1;
            return Ok(Decision::Audit);
        }
        Ok(decision)
    })
}

/// Add a policy rule.
pub fn add_rule(subject: &str, object: &str, action: Action, decision: Decision, priority: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_rule_id;
        state.next_rule_id += 1;
        state.rules.push(PolicyRule {
            id, subject_label: String::from(subject), object_label: String::from(object),
            action, decision, priority,
        });
        Ok(id)
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.rules.len();
        state.rules.retain(|r| r.id != id);
        if state.rules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List rules.
pub fn list_rules() -> Vec<PolicyRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Set enforcement mode.
pub fn set_mode(mode: EnforceMode) -> KernelResult<()> {
    with_state(|state| { state.mode = mode; Ok(()) })
}

/// Get enforcement mode.
pub fn get_mode() -> EnforceMode {
    STATE.lock().as_ref().map_or(EnforceMode::Disabled, |s| s.mode)
}

/// Assign a label.
pub fn set_label(entity_id: u32, entity_type: &str, label: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.labels.len() >= MAX_LABELS { return Err(KernelError::ResourceExhausted); }
        if let Some(existing) = state.labels.iter_mut().find(|l| l.entity_id == entity_id && l.entity_type == entity_type) {
            existing.label = String::from(label);
        } else {
            state.labels.push(SecurityLabel {
                entity_id, entity_type: String::from(entity_type), label: String::from(label),
            });
        }
        Ok(())
    })
}

/// Get label for entity.
pub fn get_label(entity_id: u32, entity_type: &str) -> Option<String> {
    STATE.lock().as_ref().and_then(|s| {
        s.labels.iter().find(|l| l.entity_id == entity_id && l.entity_type == entity_type)
            .map(|l| l.label.clone())
    })
}

/// Statistics: (rule_count, total_checks, total_allowed, total_denied, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.total_checks, s.total_allowed, s.total_denied, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("secpolicy::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_rules().len(), 3);
    assert_eq!(get_mode(), EnforceMode::Permissive);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Check access (allowed).
    let d = check_access("system", "system", Action::Read).expect("check1");
    assert_eq!(d, Decision::Allow);
    crate::serial_println!("  [2/8] allow: OK");

    // 3: Check denied (permissive → audit).
    let d = check_access("user", "system", Action::Write).expect("check2");
    assert_eq!(d, Decision::Audit); // Permissive mode converts deny to audit.
    crate::serial_println!("  [3/8] permissive deny: OK");

    // 4: Enforcing mode.
    set_mode(EnforceMode::Enforcing).expect("mode");
    let d = check_access("user", "system", Action::Write).expect("check3");
    assert_eq!(d, Decision::Deny);
    crate::serial_println!("  [4/8] enforcing: OK");

    // 5: Add rule.
    let id = add_rule("user", "system", Action::Write, Decision::Allow, 100).expect("add");
    let d = check_access("user", "system", Action::Write).expect("check4");
    assert_eq!(d, Decision::Allow); // New rule has higher priority.
    crate::serial_println!("  [5/8] add rule: OK");

    // 6: Remove rule.
    remove_rule(id).expect("remove");
    let d = check_access("user", "system", Action::Write).expect("check5");
    assert_eq!(d, Decision::Deny);
    crate::serial_println!("  [6/8] remove rule: OK");

    // 7: Labels.
    set_label(100, "process", "user").expect("label");
    let lbl = get_label(100, "process").expect("get_label");
    assert_eq!(lbl, "user");
    crate::serial_println!("  [7/8] labels: OK");

    // 8: Stats.
    let (rules, checks, allowed, denied, ops) = stats();
    assert_eq!(rules, 3);
    assert!(checks >= 5);
    assert!(allowed > 0);
    assert!(denied > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("secpolicy::self_test() — all 8 tests passed");
}
