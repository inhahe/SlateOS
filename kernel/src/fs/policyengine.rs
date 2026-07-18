//! Policy Engine — system-wide policy enforcement.
//!
//! Centralized engine for defining and enforcing system policies
//! covering security, privacy, network, application, and hardware rules.
//!
//! ## Architecture
//!
//! ```text
//! Action request
//!   → policyengine::evaluate(subject, action, resource) → allow/deny
//!   → policyengine::audit_log → record decision
//!
//! Integration:
//!   → usbpolicy (USB device policies)
//!   → apppermissions (app permissions)
//!   → fwsettings (firewall)
//!   → parental (parental controls)
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

/// Policy category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyCategory {
    Security,
    Privacy,
    Network,
    Application,
    Hardware,
    Storage,
    System,
}

impl PolicyCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Security => "Security",
            Self::Privacy => "Privacy",
            Self::Network => "Network",
            Self::Application => "Application",
            Self::Hardware => "Hardware",
            Self::Storage => "Storage",
            Self::System => "System",
        }
    }
}

/// Policy effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
    Audit,
    AllowWithAudit,
}

impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Audit => "Audit",
            Self::AllowWithAudit => "Allow+Audit",
        }
    }
}

/// A policy rule.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub id: u32,
    pub name: String,
    pub category: PolicyCategory,
    pub subject: String,    // Who (user, app, group, or "*" for any).
    pub action: String,     // What (e.g., "install", "exec", "network_access").
    pub resource: String,   // On what (e.g., "/usr/bin/*", "usb:*", "*").
    pub effect: Effect,
    pub priority: u32,
    pub enabled: bool,
    pub hit_count: u64,
}

/// An audit log entry.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub rule_id: Option<u32>,
    pub subject: String,
    pub action: String,
    pub resource: String,
    pub effect: Effect,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 200;
const MAX_AUDIT: usize = 500;

struct State {
    rules: Vec<PolicyRule>,
    audit_log: Vec<AuditEntry>,
    next_id: u32,
    default_effect: Effect,
    enforcement_enabled: bool,
    total_evaluations: u64,
    total_denials: u64,
    total_audits: u64,
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

fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" { return true; }
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        rules: alloc::vec![
            PolicyRule { id: 1, name: String::from("Allow all user actions"), category: PolicyCategory::System, subject: String::from("*"), action: String::from("*"), resource: String::from("*"), effect: Effect::Allow, priority: 0, enabled: true, hit_count: 0 },
            PolicyRule { id: 2, name: String::from("Deny exec from /tmp"), category: PolicyCategory::Security, subject: String::from("*"), action: String::from("exec"), resource: String::from("/tmp/*"), effect: Effect::Deny, priority: 100, enabled: true, hit_count: 0 },
            PolicyRule { id: 3, name: String::from("Audit USB storage"), category: PolicyCategory::Hardware, subject: String::from("*"), action: String::from("usb_connect"), resource: String::from("usb:storage:*"), effect: Effect::AllowWithAudit, priority: 50, enabled: true, hit_count: 0 },
        ],
        audit_log: Vec::new(),
        next_id: 4,
        default_effect: Effect::Allow,
        enforcement_enabled: true,
        total_evaluations: 0,
        total_denials: 0,
        total_audits: 0,
        ops: 0,
    });
}

/// Evaluate a policy request.
pub fn evaluate(subject: &str, action: &str, resource: &str) -> KernelResult<Effect> {
    with_state(|state| {
        state.total_evaluations += 1;
        let now = crate::hpet::elapsed_ns();

        if !state.enforcement_enabled {
            return Ok(Effect::Allow);
        }

        // Find the highest-priority matching rule.
        let mut best: Option<&mut PolicyRule> = None;
        let mut best_priority = 0u32;

        for rule in &mut state.rules {
            if !rule.enabled { continue; }
            if !matches_pattern(&rule.subject, subject) { continue; }
            if !matches_pattern(&rule.action, action) { continue; }
            if !matches_pattern(&rule.resource, resource) { continue; }
            if best.is_none() || rule.priority > best_priority {
                best_priority = rule.priority;
                best = Some(rule);
            }
        }

        let (effect, rule_id) = if let Some(rule) = best {
            rule.hit_count += 1;
            (rule.effect, Some(rule.id))
        } else {
            (state.default_effect, None)
        };

        match effect {
            Effect::Deny => state.total_denials += 1,
            Effect::Audit | Effect::AllowWithAudit => state.total_audits += 1,
            Effect::Allow => {}
        }

        // Audit log for deny and audit effects.
        if effect != Effect::Allow {
            if state.audit_log.len() >= MAX_AUDIT { state.audit_log.remove(0); }
            state.audit_log.push(AuditEntry {
                rule_id,
                subject: String::from(subject),
                action: String::from(action),
                resource: String::from(resource),
                effect, timestamp_ns: now,
            });
        }

        Ok(effect)
    })
}

/// Add a policy rule.
pub fn add_rule(name: &str, category: PolicyCategory, subject: &str, action: &str, resource: &str, effect: Effect, priority: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(PolicyRule {
            id, name: String::from(name), category,
            subject: String::from(subject), action: String::from(action),
            resource: String::from(resource), effect, priority,
            enabled: true, hit_count: 0,
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

/// Enable/disable enforcement.
pub fn set_enforcement(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enforcement_enabled = enabled;
        Ok(())
    })
}

/// Set default effect for unmatched requests.
pub fn set_default(effect: Effect) -> KernelResult<()> {
    with_state(|state| {
        state.default_effect = effect;
        Ok(())
    })
}

/// List all rules.
pub fn list_rules() -> Vec<PolicyRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Get audit log.
pub fn get_audit_log(max: usize) -> Vec<AuditEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut log = s.audit_log.clone();
        log.reverse();
        log.truncate(max);
        log
    })
}

/// Statistics: (rule_count, audit_size, total_evaluations, total_denials, total_audits, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.audit_log.len(), s.total_evaluations, s.total_denials, s.total_audits, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("policyengine::self_test() — running tests...");
    init_defaults();

    // 1: Default rules.
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Allow by default.
    let e = evaluate("user1", "read", "/home/file.txt").expect("eval1");
    assert_eq!(e, Effect::Allow);
    crate::serial_println!("  [2/8] default allow: OK");

    // 3: Deny exec from /tmp.
    let e = evaluate("user1", "exec", "/tmp/malware").expect("eval2");
    assert_eq!(e, Effect::Deny);
    crate::serial_println!("  [3/8] deny: OK");

    // 4: Audit USB storage.
    let e = evaluate("user1", "usb_connect", "usb:storage:flash").expect("eval3");
    assert_eq!(e, Effect::AllowWithAudit);
    crate::serial_println!("  [4/8] audit: OK");

    // 5: Add custom rule.
    let rid = add_rule("Deny net for app_x", PolicyCategory::Network, "app_x", "network_access", "*", Effect::Deny, 200).expect("add");
    let e = evaluate("app_x", "network_access", "tcp:80").expect("eval4");
    assert_eq!(e, Effect::Deny);
    crate::serial_println!("  [5/8] custom rule: OK");

    // 6: Audit log populated.
    let log = get_audit_log(10);
    assert!(log.len() >= 3); // deny + audit + deny.
    crate::serial_println!("  [6/8] audit log: OK");

    // 7: Remove rule.
    remove_rule(rid).expect("remove");
    let e = evaluate("app_x", "network_access", "tcp:80").expect("eval5");
    assert_eq!(e, Effect::Allow); // Falls through to default allow-all.
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (rules, _audit, evals, denials, audits, ops) = stats();
    assert_eq!(rules, 3);
    assert!(evals >= 5);
    assert!(denials >= 2);
    assert!(audits >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("policyengine::self_test() — all 8 tests passed");
}
