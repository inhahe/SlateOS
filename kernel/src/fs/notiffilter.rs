//! Notification Filters — notification filtering and routing rules.
//!
//! Defines rules for filtering, prioritizing, and routing
//! notifications based on app, category, time, and keywords.
//!
//! ## Architecture
//!
//! ```text
//! Notification arrives
//!   → notiffilter::evaluate(notif) → allow/silence/redirect
//!   → notiffilter::log_decision → track filtering stats
//!
//! Integration:
//!   → notifcenter (notification center)
//!   → notifprefs (notification preferences)
//!   → notifgroup (notification grouping)
//!   → focusassist (focus assist/DnD)
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

/// Filter action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAction {
    Allow,
    Silence,
    Block,
    Redirect,
    Defer,
}

impl FilterAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Silence => "Silence",
            Self::Block => "Block",
            Self::Redirect => "Redirect",
            Self::Defer => "Defer",
        }
    }
}

/// Match condition type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    AppName,
    Category,
    Title,
    Body,
    Priority,
}

impl MatchField {
    pub fn label(self) -> &'static str {
        match self {
            Self::AppName => "App",
            Self::Category => "Category",
            Self::Title => "Title",
            Self::Body => "Body",
            Self::Priority => "Priority",
        }
    }
}

/// A filter rule.
#[derive(Debug, Clone)]
pub struct FilterRule {
    pub id: u32,
    pub name: String,
    pub field: MatchField,
    pub pattern: String,
    pub action: FilterAction,
    pub enabled: bool,
    pub hit_count: u64,
}

/// A notification for evaluation.
#[derive(Debug, Clone)]
pub struct NotifData {
    pub app_name: String,
    pub category: String,
    pub title: String,
    pub body: String,
    pub priority: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 100;

struct State {
    rules: Vec<FilterRule>,
    next_id: u32,
    total_evaluated: u64,
    total_allowed: u64,
    total_blocked: u64,
    total_silenced: u64,
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

fn field_value(notif: &NotifData, field: MatchField) -> &str {
    match field {
        MatchField::AppName => &notif.app_name,
        MatchField::Category => &notif.category,
        MatchField::Title => &notif.title,
        MatchField::Body => &notif.body,
        MatchField::Priority => &notif.priority,
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
            FilterRule { id: 1, name: String::from("Block spam"), field: MatchField::Title, pattern: String::from("spam"), action: FilterAction::Block, enabled: true, hit_count: 0 },
            FilterRule { id: 2, name: String::from("Silence ads"), field: MatchField::Category, pattern: String::from("advertisement"), action: FilterAction::Silence, enabled: true, hit_count: 0 },
        ],
        next_id: 3,
        total_evaluated: 0,
        total_allowed: 0,
        total_blocked: 0,
        total_silenced: 0,
        ops: 0,
    });
}

/// Add a filter rule.
pub fn add_rule(name: &str, field: MatchField, pattern: &str, action: FilterAction) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(FilterRule {
            id, name: String::from(name), field,
            pattern: String::from(pattern), action,
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

/// Enable/disable a rule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = enabled;
        Ok(())
    })
}

/// Evaluate a notification against all rules.
pub fn evaluate(notif: &NotifData) -> KernelResult<FilterAction> {
    with_state(|state| {
        state.total_evaluated += 1;
        for rule in &mut state.rules {
            if !rule.enabled { continue; }
            let value = field_value(notif, rule.field).to_lowercase();
            let pattern = rule.pattern.to_lowercase();
            if value.contains(&pattern) {
                rule.hit_count += 1;
                match rule.action {
                    FilterAction::Block => state.total_blocked += 1,
                    FilterAction::Silence => state.total_silenced += 1,
                    FilterAction::Allow => state.total_allowed += 1,
                    _ => {}
                }
                return Ok(rule.action);
            }
        }
        state.total_allowed += 1;
        Ok(FilterAction::Allow) // Default: allow.
    })
}

/// List all rules.
pub fn list_rules() -> Vec<FilterRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Statistics: (rule_count, total_evaluated, total_allowed, total_blocked, total_silenced, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.total_evaluated, s.total_allowed, s.total_blocked, s.total_silenced, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("notiffilter::self_test() — running tests...");
    init_defaults();

    // 1: Default rules.
    assert_eq!(list_rules().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Normal notification allowed.
    let notif = NotifData { app_name: String::from("email"), category: String::from("message"), title: String::from("New message"), body: String::from("Hello"), priority: String::from("normal") };
    let action = evaluate(&notif).expect("eval1");
    assert_eq!(action, FilterAction::Allow);
    crate::serial_println!("  [2/8] allow: OK");

    // 3: Spam blocked.
    let spam = NotifData { app_name: String::from("store"), category: String::from("promo"), title: String::from("Buy spam products now!"), body: String::from("Sale"), priority: String::from("low") };
    let action = evaluate(&spam).expect("eval2");
    assert_eq!(action, FilterAction::Block);
    crate::serial_println!("  [3/8] block: OK");

    // 4: Ad silenced.
    let ad = NotifData { app_name: String::from("game"), category: String::from("advertisement"), title: String::from("Special offer"), body: String::from(""), priority: String::from("low") };
    let action = evaluate(&ad).expect("eval3");
    assert_eq!(action, FilterAction::Silence);
    crate::serial_println!("  [4/8] silence: OK");

    // 5: Add custom rule.
    let rid = add_rule("Defer work emails", MatchField::AppName, "work", FilterAction::Defer).expect("add");
    let work = NotifData { app_name: String::from("work-email"), category: String::from("email"), title: String::from("Meeting"), body: String::from(""), priority: String::from("normal") };
    let action = evaluate(&work).expect("eval4");
    assert_eq!(action, FilterAction::Defer);
    crate::serial_println!("  [5/8] custom rule: OK");

    // 6: Disable rule.
    set_enabled(rid, false).expect("disable");
    let action = evaluate(&work).expect("eval5");
    assert_eq!(action, FilterAction::Allow); // Rule disabled, falls through.
    crate::serial_println!("  [6/8] disable: OK");

    // 7: Remove rule.
    remove_rule(rid).expect("remove");
    assert_eq!(list_rules().len(), 2);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (rules, evaluated, allowed, blocked, silenced, ops) = stats();
    assert_eq!(rules, 2);
    assert!(evaluated >= 5);
    assert!(allowed >= 2);
    assert!(blocked >= 1);
    assert!(silenced >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("notiffilter::self_test() — all 8 tests passed");
}
